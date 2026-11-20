//! search_messages operation.
//!
//! Unit of `client` (LM-2).

use super::TelegramClient;
use super::cursor::cursor_wire_bounds;
use super::raw_page::raw_peer_id;
use super::raw_pager::{RawChannelSearchPager, RawGlobalSearchPager};
use super::resolve::{NumericSelection, peer_to_ref};
use super::results::assemble_search_result;
use super::search_budget::SearchBudget;
use super::stream_source::{JoinedChannelKind, stream_rpc_error};
use super::walk::{BelowCutoff, Fetched, Flow, MessageWalk, WalkConfig};
use crate::UtcTimestamp;
use crate::error::Error;
use crate::telegram::albums::{AlbumMode, PageAccumulator};
use crate::telegram::converters::convert_media_filter;
use crate::telegram::timeout::with_timeout;
use crate::telegram::types::{ChannelId, SearchParams, SearchResult};
use grammers_session::types::PeerId;
use std::time::Instant;
use tracing::Instrument;

#[derive(Clone, Copy)]
enum ChannelSearchAccess {
    Dialog,
    Joined,
}

impl TelegramClient {
    pub(super) async fn search_messages_impl(
        &self,
        params: &SearchParams,
    ) -> Result<SearchResult, Error> {
        self.search_messages_with_access(params, ChannelSearchAccess::Dialog)
            .await
    }

    pub(crate) async fn search_joined_channel(
        &self,
        params: &SearchParams,
    ) -> Result<SearchResult, Error> {
        if params.channel_id.is_none() {
            return Err(Error::InvalidInput(
                "Joined-channel search requires a channel ID".into(),
            ));
        }
        self.search_messages_with_access(params, ChannelSearchAccess::Joined)
            .await
    }

    async fn search_messages_with_access(
        &self,
        params: &SearchParams,
        access: ChannelSearchAccess,
    ) -> Result<SearchResult, Error> {
        // Validate parameters
        // Empty query is allowed when media_filter is set (search for media type only)
        if params.query.is_empty() && params.media_filter.is_none() {
            return Err(Error::InvalidInput(
                "Search query cannot be empty (unless media_filter is specified)".to_string(),
            ));
        }

        if params.limit == 0 {
            return Err(Error::InvalidInput(
                "Search limit must be greater than 0".to_string(),
            ));
        }

        let start_time = Instant::now();
        let cutoff_time = params.window_start();

        // Convert cursor bounds once, outside the timeout closures, so `?` maps
        // through the existing error path (A8).
        let (before_offset, after_bound) = cursor_wire_bounds(params.before_id, params.after_id)?;

        // If channel_id is specified, search only that channel
        let (page, channels_scanned, budget) = if let Some(channel_id) = params.channel_id {
            let (page, scanned, budget) = self
                .search_in_channel(
                    params,
                    channel_id,
                    cutoff_time,
                    before_offset,
                    after_bound,
                    access,
                )
                .await?;
            (page, Some(scanned), budget)
        } else {
            // Cursors are single-channel only (decision 2): global search has no
            // per-channel offset_id to ride, and no way to bound it client-side
            // without scanning every channel's history.
            if params.before_id.is_some() || params.after_id.is_some() {
                return Err(Error::InvalidInput(
                    "before_id/after_id require channel_id: cursor pagination is per-channel"
                        .to_string(),
                ));
            }
            let (page, budget) = self.search_global(params, cutoff_time).await?;
            // server-side global search: scan scope unknowable
            (page, None, budget)
        };

        let has_more = page.has_more();
        let mut messages = page.into_messages();

        // Sort by timestamp (newest first). Stays here, not in
        // `assemble_search_result`: history relies on its pager's already
        // reverse-chronological order and must not be sorted.
        messages.sort_by_key(|b| std::cmp::Reverse(b.timestamp));

        let search_time_ms = start_time.elapsed().as_millis() as u64;

        // The global path has no cursor to resume from anyway (see the
        // `before_id`/`after_id` rejection above), so `partial`/`timed_out` —
        // computed by `assemble_search_result`, deliberately paired together and
        // never with `has_more` — are the only truncation signal it can give.
        let result = assemble_search_result(
            messages,
            &budget,
            has_more,
            params.query.clone(),
            cutoff_time,
            params.to_date,
            channels_scanned,
            search_time_ms,
        );

        tracing::info!(
            query = %params.query,
            media_filter = ?params.media_filter,
            results = result.returned,
            channels_scanned = ?channels_scanned,
            channels_in_results = result.query_metadata.channels_in_results,
            duration_ms = search_time_ms,
            pages_fetched = budget.pages_fetched(),
            messages_scanned = budget.messages_scanned(),
            timed_out = budget.timed_out(),
            "Search completed"
        );

        Ok(result)
    }

    /// Channel-scoped search: walk dialogs to the target channel, then page
    /// the raw messages.Search pager under the search timeout. Returns the
    /// accumulated page, the channels-scanned count, and the budget counters.
    async fn search_in_channel(
        &self,
        params: &SearchParams,
        channel_id: ChannelId,
        cutoff_time: UtcTimestamp,
        before_offset: Option<i32>,
        after_bound: Option<i32>,
        access: ChannelSearchAccess,
    ) -> Result<(PageAccumulator, u32, SearchBudget), Error> {
        let album_mode = AlbumMode::from_collapse_albums(params.collapse_albums);
        with_timeout(
            "search_messages_channel",
            self.timeouts.search_secs,
            async {
                let cfg = WalkConfig {
                    cutoff_time,
                    to_date: params.to_date,
                    after_bound,
                    // messages.Search filters server-side; no client-side pass.
                    media_filter: None,
                    below_cutoff: BelowCutoff::Stop,
                };
                let mut walk = MessageWalk::new(
                    cfg,
                    album_mode,
                    params.limit as usize,
                    self.search_deadline_secs,
                );
                let mut channels_scanned = 0u32;
                let resolved = match access {
                    ChannelSearchAccess::Joined => {
                        let channel = self
                            .joined_channel_peers(
                                &[channel_id],
                                JoinedChannelKind::BroadcastOrSupergroup,
                            )
                            .await?
                            .remove(&channel_id)
                            .ok_or_else(|| Error::ChannelNotFound(channel_id.to_string()))?;
                        Some((channel.peer, channel.reference))
                    }
                    ChannelSearchAccess::Dialog => {
                        // Complete this existing dialog traversal before trusting a numeric match.
                        // A namespace retained from username resolution narrows the same pass;
                        // a bare numeric input remains fail-closed across eligible namespaces.
                        let mut dialogs = self.client.iter_dialogs();
                        let mut selected = NumericSelection::with_namespace(
                            channel_id.get(),
                            params.channel_namespace,
                        );
                        while let Some(dialog) = dialogs.next().await.map_err(|e| {
                            Error::TelegramApi(format!("Failed to iterate dialogs: {e}"))
                        })? {
                            if walk.expired() {
                                break;
                            }
                            let peer = dialog.peer();
                            selected.consider(
                                crate::telegram::converters::peer_namespace(peer),
                                peer.id().bare_id(),
                                || peer.clone(),
                            )?;
                        }
                        // Expiry makes a provisional match unusable.
                        if !walk.expired()
                            && let Some(peer) = selected.finish()
                        {
                            let reference = peer_to_ref(&peer).await?;
                            Some((peer, reference))
                        } else {
                            None
                        }
                    }
                };
                if !walk.expired()
                    && let Some((peer, peer_ref)) = resolved
                {
                    channels_scanned += 1;
                    let mut pager =
                        RawChannelSearchPager::new(&self.client, peer_ref).query(&params.query);
                    if let Some(before) = before_offset {
                        pager = pager.offset_id(before);
                    }
                    if let Some(ref filter) = params.media_filter {
                        pager = pager.filter(convert_media_filter(filter));
                    }
                    loop {
                        if walk.expired() {
                            break;
                        }
                        let next = pager.next().await.map_err(|e| match access {
                            ChannelSearchAccess::Joined => stream_rpc_error(e),
                            ChannelSearchAccess::Dialog => {
                                Error::TelegramApi(format!("Search failed: {e}"))
                            }
                        })?;
                        if matches!(access, ChannelSearchAccess::Joined)
                            && next
                                .as_ref()
                                .and_then(|(raw, _)| raw_peer_id(raw))
                                .is_some_and(|source| PeerId::from(source.clone()) != peer.id())
                        {
                            return Err(Error::UnsupportedPeer);
                        }
                        let page_size = pager.take_last_page_size();
                        let fetched = next.map(|(raw, entities)| Fetched {
                            raw,
                            entities,
                            peer: Some(&peer),
                        });
                        if walk.step(fetched, page_size) == Flow::Stop {
                            break;
                        }
                    }
                }
                let (page, budget) = walk.into_parts();
                Ok((page, channels_scanned, budget))
            },
        )
        .await
    }

    /// Global search via the raw messages.SearchGlobal pager with server-side
    /// window bounds. Cursor rejection happens in the dispatcher — this path
    /// has no per-channel offset to ride.
    async fn search_global(
        &self,
        params: &SearchParams,
        cutoff_time: UtcTimestamp,
    ) -> Result<(PageAccumulator, SearchBudget), Error> {
        let album_mode = AlbumMode::from_collapse_albums(params.collapse_albums);
        let start_time = Instant::now();
        let span = tracing::debug_span!(
            "search_global",
            query = %params.query,
            media_filter = ?params.media_filter,
            window_from = %cutoff_time,
        );
        with_timeout(
            "search_all_messages",
            self.timeouts.search_secs,
            async move {
                let cfg = WalkConfig {
                    cutoff_time,
                    to_date: params.to_date,
                    // Cursors are per-channel; the dispatcher rejects them here.
                    after_bound: None,
                    // SearchGlobal filters server-side.
                    media_filter: None,
                    // Relevance-ordered across channels: one old result says
                    // nothing about the next, so skip rather than stop.
                    below_cutoff: BelowCutoff::Skip,
                };
                let mut walk = MessageWalk::new(
                    cfg,
                    album_mode,
                    params.limit as usize,
                    self.search_deadline_secs,
                );
                // Global search via the raw messages.SearchGlobal pager:
                // same request as grammers' search_all_messages, but the
                // response envelope is kept so forwards get attributed
                // (zero extra calls). The pager also yields each result's
                // own chat peer, built from that same envelope.
                // The window is bound server-side by construction. The client-side
                // window checks `MessageWalk` applies are retained as defense in
                // depth: they cost nothing once the server honors those bounds, and
                // keep the result correct if it ever does not.
                let mut pager =
                    RawGlobalSearchPager::new(&self.client, cutoff_time, params.to_date)
                        .query(&params.query);

                if let Some(ref media_filter) = params.media_filter {
                    pager = pager.filter(convert_media_filter(media_filter));
                }

                let mut mtproto_nanos: u128 = 0;

                loop {
                    if walk.expired() {
                        break;
                    }
                    let fetch_start = Instant::now();
                    let next = pager
                        .next()
                        .await
                        .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?;
                    mtproto_nanos += fetch_start.elapsed().as_nanos();
                    let page_size = pager.take_last_page_size();
                    // Read before the fold: `kept` reports messages admitted
                    // *before* this page, which is what the pre-refactor log
                    // meant and the cleaner progress reading at fetch time.
                    let kept_before = walk.kept();
                    // Destructured before `Fetched` so `chat_peer` outlives the
                    // borrow taken by `peer`. Moving `raw`/`entities` in keeps this
                    // hot loop clone-free.
                    let flow = match next {
                        Some((raw, entities, chat_peer)) => walk.step(
                            Some(Fetched {
                                raw,
                                entities,
                                peer: chat_peer.as_ref(),
                            }),
                            page_size,
                        ),
                        None => walk.step(None, page_size),
                    };
                    // Logged after `step`, which owns `record_page` — so the page
                    // counters read the same as they did when the log sat ahead of
                    // the fold, and `kept_before` was captured ahead of it. Every
                    // logged value is identical to the pre-refactor ordering.
                    // `page_size` is `Some` even when the round trip came back
                    // empty: it still cost the caller latency, which is what the
                    // field reports.
                    if let Some(size) = page_size {
                        tracing::debug!(
                            page_no = walk.pages_fetched(),
                            messages_in_page = size,
                            messages_scanned = walk.messages_scanned(),
                            kept = kept_before,
                            "Global search page fetched"
                        );
                    }
                    if flow == Flow::Stop {
                        break;
                    }
                }

                let (page, budget) = walk.into_parts();
                tracing::debug!(
                    pages_fetched = budget.pages_fetched(),
                    messages_scanned = budget.messages_scanned(),
                    mtproto_ms = (mtproto_nanos / 1_000_000) as u64,
                    duration_ms = start_time.elapsed().as_millis() as u64,
                    "Global search finished"
                );

                Ok((page, budget))
            }
            .instrument(span),
        )
        .await
    }
}
