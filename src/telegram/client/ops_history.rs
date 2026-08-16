//! get_recent_messages operation.
//!
//! Unit of `client` (LM-2).

use super::raw_pager::RawHistoryPager;
use super::walk::{BelowCutoff, Fetched, Flow, MessageWalk, WalkConfig};
use super::*;

impl TelegramClient {
    pub(super) async fn get_recent_messages_impl(
        &self,
        params: &HistoryParams,
    ) -> Result<SearchResult, Error> {
        // Validate limit
        if params.limit == 0 {
            return Err(Error::InvalidInput(
                "Limit must be greater than 0".to_string(),
            ));
        }

        let start_time = Instant::now();
        let cutoff_time = params.window_start();

        // Resolve the channel: prefer a best-effort username lookup (doesn't require
        // subscription), then fall back to a dialog walk by the known numeric id. A
        // username miss or RPC failure is non-fatal — it falls through to the dialog
        // walk, preserving the original "prefer username, fall back to dialog by id"
        // behaviour (AD-1).
        let resolved_peer = match params
            .channel_identifier
            .as_deref()
            .and_then(username_to_resolve)
        {
            Some(identifier) => match self.resolve_username_peer(identifier).await {
                Ok(Some(peer)) => Some(peer),
                Ok(None) => {
                    tracing::warn!(
                        identifier = %identifier,
                        "Username not found, falling back to dialog search"
                    );
                    None
                }
                Err(e) => {
                    tracing::warn!(
                        identifier = %identifier,
                        error = %e,
                        "Failed to resolve username, falling back to dialog search"
                    );
                    None
                }
            },
            None => None,
        };

        // Use the resolved peer, or fall back to a dialog walk by numeric id. A
        // username reference carries no numeric id (`channel_id == None`), so a
        // username that fails to resolve hard-errors here rather than walking
        // dialogs by an id we never had (AD-2).
        let peer = match resolved_peer {
            Some(peer) => peer,
            None => {
                let id = dialog_fallback_target(
                    params.channel_id,
                    params.channel_identifier.as_deref(),
                )?;
                self.find_dialog_peer(id).await?.ok_or_else(|| {
                    tracing::warn!(channel_id = id, "Channel not found in dialogs");
                    Error::InvalidInput(format!("Channel not found: {}", id))
                })?
            }
        };

        // Numeric id of the resolved channel, for logging (derived from the peer
        // rather than pre-resolved in the server — AD-2).
        let resolved_channel_id = peer.id().bare_id();

        // Use iter_messages to get message history (no search query)
        let peer_ref = peer_to_ref(&peer).await?;

        // Convert cursor bounds once, outside the timeout closures, so `?` maps
        // through the existing error path (A8).
        let (before_offset, after_bound) = cursor_wire_bounds(params.before_id, params.after_id)?;

        let (page, budget) = with_timeout("iter_messages", self.timeouts.history_secs, async {
            let cfg = WalkConfig {
                cutoff_time,
                to_date: params.to_date,
                after_bound,
                media_filter: params.media_filter.as_ref(),
                below_cutoff: BelowCutoff::Stop,
            };
            // Deadline 0: the spec scopes the search deadline to search, so
            // history's budget carries counters only and never expires.
            let mut walk = MessageWalk::new(cfg, params.collapse_albums, params.limit as usize, 0);
            // Raw GetHistory pager instead of grammers' iter_messages: same
            // request, but it keeps the response envelope so forwards get
            // attributed from data already in hand (zero extra calls).
            let mut pager = RawHistoryPager::new(&self.client, peer_ref);
            if let Some(before) = before_offset {
                pager = pager.offset_id(before);
            }

            loop {
                if walk.expired() {
                    break;
                }
                let next = pager.next().await.map_err(|e| {
                    Error::TelegramApi(format!("Failed to iterate messages: {}", e))
                })?;
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
            Ok(walk.into_parts())
        })
        .await?;

        let has_more = page.has_more();
        let messages = page.into_messages();

        let search_time_ms = start_time.elapsed().as_millis() as u64;
        let returned = messages.len() as u64;

        tracing::info!(
            channel_id = resolved_channel_id,
            identifier = ?params.channel_identifier,
            media_filter = ?params.media_filter,
            results = returned,
            hours_back = params.hours_back,
            duration_ms = search_time_ms,
            "Get recent messages completed"
        );

        // History has no deadline (spec scopes it to search), so `budget` can
        // never actually expire here — `assemble_search_result` reads
        // `timed_out`/`partial` off it anyway rather than hardcoding `false`:
        // identical today (the walk loop calls `expired()` every iteration,
        // but `SearchBudget::new(0)` never latches it) and self-maintaining
        // if a deadline is ever extended to history.
        Ok(assemble_search_result(
            messages,
            &budget,
            has_more,
            String::new(), // no query for history retrieval
            cutoff_time,
            params.to_date,
            Some(1),
            search_time_ms,
        ))
    }
}

/// The numeric id to walk dialogs by when username resolution did not produce
/// a peer.
///
/// A username reference carries no numeric id (`channel_id == None`), so a
/// username that fails to resolve hard-errors here rather than walking dialogs
/// by an id we never had (AD-2).
fn dialog_fallback_target(
    channel_id: Option<ChannelId>,
    identifier: Option<&str>,
) -> Result<i64, Error> {
    channel_id.map(|id| id.get()).ok_or_else(|| {
        let reference = identifier.unwrap_or("");
        tracing::warn!(reference, "Channel not found: username did not resolve");
        Error::InvalidInput(format!("Channel not found: {}", reference))
    })
}

#[cfg(test)]
#[path = "tests/ops_history_tests.rs"]
mod tests;
