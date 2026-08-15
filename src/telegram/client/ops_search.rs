//! search_messages operation.
//!
//! Unit of `client` (LM-2).

use super::raw_pager::{RawChannelSearchPager, RawGlobalSearchPager};
use super::search_budget::SearchBudget;
use super::*;
use crate::telegram::albums::PageAccumulator;
use tracing::Instrument;

impl TelegramClient {
    pub(super) async fn search_messages_impl(
        &self,
        params: &SearchParams,
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
        let (page, channels_scanned, budget) = if let Some(channel_id) = &params.channel_id {
            with_timeout(
                "search_messages_channel",
                self.timeouts.search_secs,
                async {
                    let mut page =
                        PageAccumulator::new(params.collapse_albums, params.limit as usize);
                    let mut channels_scanned = 0u32;
                    let mut budget = SearchBudget::new(self.search_deadline_secs);
                    // Find the channel in our dialogs
                    let mut dialogs = self.client.iter_dialogs();

                    while let Some(dialog) = dialogs.next().await.map_err(|e| {
                        Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
                    })? {
                        // The dialog walk is on the deadline too: a large dialog list, or
                        // a `channel_id` the account has not joined (which walks every
                        // dialog before concluding), would otherwise run past the budget
                        // into `search_secs` and error — the exact outcome the deadline
                        // exists to replace with a graceful partial. Checking here also
                        // latches `timed_out`, so a slow walk is reported rather than
                        // looking like a fast empty result.
                        if budget.expired() {
                            break;
                        }
                        let peer = dialog.peer();
                        if peer.id().bare_id() == Some(channel_id.get()) {
                            channels_scanned += 1;

                            // Search in this specific channel via the raw
                            // messages.Search pager: same request as grammers'
                            // search_messages, but the response envelope is
                            // kept so forwards get attributed (zero extra calls).
                            let peer_ref = peer_to_ref(peer).await?;
                            let mut pager = RawChannelSearchPager::new(&self.client, peer_ref)
                                .query(&params.query);
                            if let Some(before) = before_offset {
                                pager = pager.offset_id(before);
                            }

                            // Apply media filter if specified
                            if let Some(ref media_filter) = params.media_filter {
                                pager = pager.filter(convert_media_filter(media_filter));
                            }

                            loop {
                                if budget.expired() {
                                    break;
                                }
                                let next = pager.next().await.map_err(|e| {
                                    Error::TelegramApi(format!("Search failed: {}", e))
                                })?;
                                // Before the `else break`: a round trip that came back
                                // empty still cost the caller latency, which is what
                                // the field reports.
                                if let Some(page_size) = pager.take_last_page_size() {
                                    budget.record_page(page_size);
                                }
                                let Some((raw_msg, entities)) = next else {
                                    break;
                                };
                                if let Some(to) = params.to_date
                                    && timestamp_from_raw(&raw_msg).is_some_and(|t| t > to)
                                {
                                    continue; // newer than the requested window; keep iterating toward it
                                }
                                if timestamp_from_raw(&raw_msg).is_none_or(|t| t < cutoff_time) {
                                    break; // reverse chronological order
                                }
                                // Exclusive lower cursor bound: everything from here on
                                // is older (reverse chronological), so stop (A8).
                                if let Some(after) = after_bound
                                    && raw_msg.id() <= after
                                {
                                    break;
                                }
                                if let Some(converted) =
                                    convert_raw_message(&raw_msg, peer, &entities)
                                    && !page.push(converted)
                                {
                                    break;
                                }
                            }
                            break;
                        }
                    }
                    Ok((page, channels_scanned, budget))
                },
            )
            .await
            .map(|(page, channels_scanned, budget)| (page, Some(channels_scanned), budget))?
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

            // Search all channels using global search
            let span = tracing::debug_span!(
                "search_global",
                query = %params.query,
                media_filter = ?params.media_filter,
                window_from = %cutoff_time,
            );
            let (page, budget) = with_timeout(
                "search_all_messages",
                self.timeouts.search_secs,
                async move {
                    let mut page =
                        PageAccumulator::new(params.collapse_albums, params.limit as usize);
                    let mut budget = SearchBudget::new(self.search_deadline_secs);
                    // Global search via the raw messages.SearchGlobal pager:
                    // same request as grammers' search_all_messages, but the
                    // response envelope is kept so forwards get attributed
                    // (zero extra calls). The pager also yields each result's
                    // own chat peer, built from that same envelope.
                    // The window is bound server-side by construction. The client-side
                    // window checks below are retained as defense in depth: they cost
                    // nothing once the server honors those bounds, and keep the result
                    // correct if it ever does not.
                    let mut pager =
                        RawGlobalSearchPager::new(&self.client, cutoff_time, params.to_date)
                            .query(&params.query);

                    if let Some(ref media_filter) = params.media_filter {
                        pager = pager.filter(convert_media_filter(media_filter));
                    }

                    let mut mtproto_nanos: u128 = 0;

                    loop {
                        if budget.expired() {
                            break;
                        }
                        let fetch_start = Instant::now();
                        let next = pager
                            .next()
                            .await
                            .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?;
                        mtproto_nanos += fetch_start.elapsed().as_nanos();
                        // Before the `else break`: a round trip that came back empty
                        // still cost the caller latency, which is what the field reports.
                        if let Some(page_size) = pager.take_last_page_size() {
                            budget.record_page(page_size);
                            tracing::debug!(
                                page = budget.pages_fetched(),
                                messages_in_page = page_size,
                                messages_scanned = budget.messages_scanned(),
                                kept = page.len(),
                                "Global search page fetched"
                            );
                        }
                        let Some((raw_msg, entities, chat_peer)) = next else {
                            break;
                        };
                        if let Some(to) = params.to_date
                            && timestamp_from_raw(&raw_msg).is_some_and(|t| t > to)
                        {
                            continue; // newer than the requested window; keep iterating toward it
                        }
                        if timestamp_from_raw(&raw_msg).is_none_or(|t| t < cutoff_time) {
                            continue; // Skip old messages but keep searching
                        }
                        if let Some(peer) = chat_peer.as_ref()
                            && let Some(converted) = convert_raw_message(&raw_msg, peer, &entities)
                            && !page.push(converted)
                        {
                            break;
                        }
                    }

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
            .await?;

            // server-side global search: scan scope unknowable
            (page, None, budget)
        };

        let has_more = page.has_more();
        let mut messages = page.into_messages();

        // Sort by timestamp (newest first)
        messages.sort_by_key(|b| std::cmp::Reverse(b.timestamp));

        let channels_in_results = {
            let unique: std::collections::HashSet<_> =
                messages.iter().map(|m| m.channel_id.get()).collect();
            unique.len() as u32
        };
        let search_time_ms = start_time.elapsed().as_millis() as u64;
        let returned = messages.len() as u64;

        tracing::info!(
            query = %params.query,
            media_filter = ?params.media_filter,
            results = returned,
            channels_scanned = ?channels_scanned,
            channels_in_results,
            duration_ms = search_time_ms,
            pages_fetched = budget.pages_fetched(),
            messages_scanned = budget.messages_scanned(),
            timed_out = budget.timed_out(),
            "Search completed"
        );

        Ok(SearchResult {
            returned,
            has_more,
            search_time_ms,
            query_metadata: QueryMetadata {
                query: params.query.clone(),
                window_from: cutoff_time,
                window_to: params.to_date,
                channels_scanned,
                channels_in_results,
                timed_out: budget.timed_out(),
                // Deliberately paired with `timed_out`, and deliberately *not* with
                // `has_more`: expiry stopped the walk without proving anything lies
                // beyond the page, and the global path has no cursor to resume from
                // anyway (see the `before_id`/`after_id` rejection above).
                //
                // Can be a conservative false positive: the deadline is checked at the
                // top of an iteration, so expiry landing on what would have been the
                // terminal iteration reports a complete result set as partial. Avoiding
                // that needs lookahead the loop does not have, and the error is in the
                // safe direction — it is not a bug to go fix later.
                partial: budget.timed_out(),
                pages_fetched: budget.pages_fetched(),
                messages_scanned: budget.messages_scanned(),
            },
            messages,
        })
    }
}
