//! search_messages operation.
//!
//! Unit of `client` (LM-2).

use super::raw_pager::{RawChannelSearchPager, RawGlobalSearchPager};
use super::*;
use crate::telegram::albums::{PostCounter, album_key, collapse_albums};

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

        // If channel_id is specified, search only that channel
        let (messages, channels_scanned, has_more) = if let Some(channel_id) = &params.channel_id {
            with_timeout(
                "search_messages_channel",
                self.timeouts.search_secs,
                async {
                    let mut messages = Vec::new();
                    let mut channels_scanned = 0u32;
                    let mut has_more = false;
                    let mut counter = PostCounter::default();
                    // Find the channel in our dialogs
                    let mut dialogs = self.client.iter_dialogs();

                    while let Some(dialog) = dialogs.next().await.map_err(|e| {
                        Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
                    })? {
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

                            while let Some((raw_msg, entities)) = pager
                                .next()
                                .await
                                .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?
                            {
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
                                {
                                    if params.collapse_albums {
                                        // Post-level limit: stop only when a NEW post
                                        // would overflow; trailing siblings of admitted
                                        // albums pass.
                                        if !counter
                                            .admit(album_key(&converted), params.limit as usize)
                                        {
                                            has_more = true;
                                            break;
                                        }
                                        messages.push(converted);
                                    } else {
                                        // Refuse the overflow message instead of pushing
                                        // the limit-th and breaking blind: refusing
                                        // proves a qualifying message exists beyond the
                                        // page (A8).
                                        if messages.len() >= params.limit as usize {
                                            has_more = true;
                                            break;
                                        }
                                        messages.push(converted);
                                    }
                                }
                            }
                            break;
                        }
                    }
                    Ok((messages, channels_scanned, has_more))
                },
            )
            .await
            .map(|(messages, channels_scanned, has_more)| {
                (messages, Some(channels_scanned), has_more)
            })?
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
            let (collected, has_more) =
                with_timeout("search_all_messages", self.timeouts.search_secs, async {
                    let mut messages = Vec::new();
                    let mut has_more = false;
                    let mut counter = PostCounter::default();
                    // Global search via the raw messages.SearchGlobal pager:
                    // same request as grammers' search_all_messages, but the
                    // response envelope is kept so forwards get attributed
                    // (zero extra calls). The pager also yields each result's
                    // own chat peer, built from that same envelope.
                    // Bound the search server-side. The client-side window checks below are
                    // retained as defense in depth: they cost nothing once the server honors
                    // these bounds, and keep the result correct if it ever does not.
                    let mut pager = RawGlobalSearchPager::new(&self.client)
                        .query(&params.query)
                        .window(cutoff_time, params.to_date);

                    if let Some(ref media_filter) = params.media_filter {
                        pager = pager.filter(convert_media_filter(media_filter));
                    }

                    while let Some((raw_msg, entities, chat_peer)) = pager
                        .next()
                        .await
                        .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?
                    {
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
                        {
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

            (collected, None, has_more) // server-side global search: scan scope unknowable
        };

        let mut messages = if params.collapse_albums {
            collapse_albums(messages)
        } else {
            messages
        };

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
                timed_out: false,
                partial: false,
                pages_fetched: 0,
                messages_scanned: 0,
            },
            messages,
        })
    }
}
