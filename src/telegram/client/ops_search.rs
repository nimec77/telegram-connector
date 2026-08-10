//! search_messages operation.
//!
//! Unit of `client` (LM-2).

use super::*;

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
        let cutoff_time = Utc::now() - Duration::hours(params.hours_back as i64);

        // If channel_id is specified, search only that channel
        let (mut messages, channels_searched) = if let Some(channel_id) = &params.channel_id {
            with_timeout(
                "search_messages_channel",
                self.timeouts.search_secs,
                async {
                    let mut messages = Vec::new();
                    let mut channels_searched = 0u32;
                    // Find the channel in our dialogs
                    let mut dialogs = self.client.iter_dialogs();

                    while let Some(dialog) = dialogs.next().await.map_err(|e| {
                        Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
                    })? {
                        let peer = dialog.peer();
                        if peer.id().bare_id() == Some(channel_id.get()) {
                            channels_searched += 1;

                            // Search in this specific channel
                            let peer_ref = peer_to_ref(peer).await?;
                            let mut search_iter =
                                self.client.search_messages(peer_ref).query(&params.query);

                            // Apply media filter if specified
                            if let Some(ref media_filter) = params.media_filter {
                                search_iter =
                                    search_iter.filter(convert_media_filter(media_filter));
                            }

                            while let Some(msg) = search_iter
                                .next()
                                .await
                                .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?
                            {
                                if message_timestamp(&msg).is_none_or(|t| t < cutoff_time) {
                                    break; // reverse chronological order
                                }
                                if let Some(converted) = convert_message(&msg, peer) {
                                    messages.push(converted);
                                    if messages.len() >= params.limit as usize {
                                        break;
                                    }
                                }
                            }
                            break;
                        }
                    }
                    Ok((messages, channels_searched))
                },
            )
            .await?
        } else {
            // Search all channels using global search
            let collected = with_timeout("search_all_messages", self.timeouts.search_secs, async {
                let mut messages = Vec::new();
                let mut search_iter = self.client.search_all_messages().query(&params.query);

                if let Some(ref media_filter) = params.media_filter {
                    search_iter = search_iter.filter(convert_media_filter(media_filter));
                }

                while let Some(msg) = search_iter
                    .next()
                    .await
                    .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?
                {
                    if message_timestamp(&msg).is_none_or(|t| t < cutoff_time) {
                        continue; // Skip old messages but keep searching
                    }
                    if let Some(peer) = msg.peer()
                        && let Some(converted) = convert_message(&msg, peer)
                    {
                        messages.push(converted);
                        if messages.len() >= params.limit as usize {
                            break;
                        }
                    }
                }
                Ok(messages)
            })
            .await?;

            // Count unique channels in results
            let unique_channels: std::collections::HashSet<_> =
                collected.iter().map(|m| m.channel_id.get()).collect();
            (collected, unique_channels.len() as u32)
        };

        // Sort by timestamp (newest first)
        messages.sort_by_key(|b| std::cmp::Reverse(b.timestamp));

        let search_time_ms = start_time.elapsed().as_millis() as u64;
        let total_found = messages.len() as u64;

        tracing::info!(
            query = %params.query,
            media_filter = ?params.media_filter,
            results = total_found,
            channels = channels_searched,
            duration_ms = search_time_ms,
            "Search completed"
        );

        Ok(SearchResult {
            messages,
            total_found,
            search_time_ms,
            query_metadata: QueryMetadata {
                query: params.query.clone(),
                hours_back: params.hours_back,
                channels_searched,
            },
        })
    }
}
