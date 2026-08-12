//! get_recent_messages operation.
//!
//! Unit of `client` (LM-2).

use super::raw_pager::RawHistoryPager;
use super::*;
use crate::telegram::albums::{PostCounter, album_key, collapse_albums};

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
                let id = params.channel_id.ok_or_else(|| {
                    let reference = params.channel_identifier.as_deref().unwrap_or("");
                    tracing::warn!(reference, "Channel not found: username did not resolve");
                    Error::InvalidInput(format!("Channel not found: {}", reference))
                })?;
                self.find_dialog_peer(id.get()).await?.ok_or_else(|| {
                    tracing::warn!(channel_id = %id, "Channel not found in dialogs");
                    Error::InvalidInput(format!("Channel not found: {}", id))
                })?
            }
        };

        // Numeric id of the resolved channel, for logging (derived from the peer
        // rather than pre-resolved in the server — AD-2).
        let resolved_channel_id = peer.id().bare_id();

        // Use iter_messages to get message history (no search query)
        let peer_ref = peer_to_ref(&peer).await?;

        // Convert cursor bounds once, outside the timeout closure, so `?` maps
        // through the existing error path (A8).
        let before_offset = match params.before_id {
            Some(id) => Some(id.as_i32().ok_or_else(|| {
                Error::InvalidInput(format!(
                    "before_id {} exceeds Telegram's message id range",
                    id.get()
                ))
            })?),
            None => None,
        };
        let after_bound = match params.after_id {
            Some(id) => Some(id.as_i32().ok_or_else(|| {
                Error::InvalidInput(format!(
                    "after_id {} exceeds Telegram's message id range",
                    id.get()
                ))
            })?),
            None => None,
        };

        let (messages, has_more) =
            with_timeout("iter_messages", self.timeouts.history_secs, async {
                let mut messages = Vec::new();
                let mut has_more = false;
                let mut counter = PostCounter::default();
                // Raw GetHistory pager instead of grammers' iter_messages: same
                // request, but it keeps the response envelope so forwards get
                // attributed from data already in hand (zero extra calls).
                let mut pager = RawHistoryPager::new(&self.client, peer_ref);
                if let Some(before) = before_offset {
                    pager = pager.offset_id(before);
                }

                while let Some((raw_msg, entities)) = pager
                    .next()
                    .await
                    .map_err(|e| Error::TelegramApi(format!("Failed to iterate messages: {}", e)))?
                {
                    if let Some(to) = params.to_date
                        && timestamp_from_raw(&raw_msg).is_some_and(|t| t > to)
                    {
                        continue; // newer than the requested window; keep iterating toward it
                    }

                    // Check time filter - messages are in reverse chronological order
                    if timestamp_from_raw(&raw_msg).is_none_or(|t| t < cutoff_time) {
                        break;
                    }

                    // Exclusive lower cursor bound: everything from here on
                    // is older (reverse chronological), so stop (A8).
                    if let Some(after) = after_bound
                        && raw_msg.id() <= after
                    {
                        break;
                    }

                    // Apply media filter client-side (GetHistory has no server-side filtering)
                    if params
                        .media_filter
                        .as_ref()
                        .is_some_and(|filter| !matches_media_filter_raw(&raw_msg, filter))
                    {
                        continue;
                    }

                    if let Some(converted) = convert_raw_message(&raw_msg, &peer, &entities) {
                        if params.collapse_albums {
                            // Post-level limit: stop only when a NEW post would
                            // overflow; trailing siblings of admitted albums pass.
                            if !counter.admit(album_key(&converted), params.limit as usize) {
                                has_more = true;
                                break;
                            }
                            messages.push(converted);
                        } else {
                            // Refuse the overflow message instead of pushing the
                            // limit-th and breaking blind: refusing proves a
                            // qualifying message exists beyond the page (A8).
                            if messages.len() >= params.limit as usize {
                                has_more = true;
                                break;
                            }
                            messages.push(converted);
                        }
                    }
                }
                Ok((messages, has_more))
            })
            .await?;

        let messages = if params.collapse_albums {
            collapse_albums(messages)
        } else {
            messages
        };

        let search_time_ms = start_time.elapsed().as_millis() as u64;
        let returned = messages.len() as u64;
        let channels_in_results = if messages.is_empty() { 0 } else { 1 };

        tracing::info!(
            channel_id = resolved_channel_id,
            identifier = ?params.channel_identifier,
            media_filter = ?params.media_filter,
            results = returned,
            hours_back = params.hours_back,
            duration_ms = search_time_ms,
            "Get recent messages completed"
        );

        Ok(SearchResult {
            returned,
            has_more,
            search_time_ms,
            query_metadata: QueryMetadata {
                query: String::new(), // No query for history retrieval
                window_from: cutoff_time,
                window_to: params.to_date,
                channels_scanned: Some(1),
                channels_in_results,
            },
            messages,
        })
    }
}
