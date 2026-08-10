//! get_recent_messages operation.
//!
//! Unit of `client` (LM-2).

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

        let messages = with_timeout("iter_messages", self.timeouts.history_secs, async {
            let mut messages = Vec::new();
            let mut messages_iter = self.client.iter_messages(peer_ref);

            while let Some(msg) = messages_iter
                .next()
                .await
                .map_err(|e| Error::TelegramApi(format!("Failed to iterate messages: {}", e)))?
            {
                if let Some(to) = params.to_date
                    && message_timestamp(&msg).is_some_and(|t| t > to)
                {
                    continue; // newer than the requested window; keep iterating toward it
                }

                // Check time filter - messages are in reverse chronological order
                if message_timestamp(&msg).is_none_or(|t| t < cutoff_time) {
                    break;
                }

                // Apply media filter client-side (iter_messages doesn't support server-side filtering)
                if params
                    .media_filter
                    .as_ref()
                    .is_some_and(|filter| !matches_media_filter(&msg, filter))
                {
                    continue;
                }

                if let Some(converted) = convert_message(&msg, &peer) {
                    messages.push(converted);
                    if messages.len() >= params.limit as usize {
                        break;
                    }
                }
            }
            Ok(messages)
        })
        .await?;

        let search_time_ms = start_time.elapsed().as_millis() as u64;
        let total_found = messages.len() as u64;

        tracing::info!(
            channel_id = resolved_channel_id,
            identifier = ?params.channel_identifier,
            media_filter = ?params.media_filter,
            results = total_found,
            hours_back = params.hours_back,
            duration_ms = search_time_ms,
            "Get recent messages completed"
        );

        Ok(SearchResult {
            messages,
            total_found,
            search_time_ms,
            query_metadata: QueryMetadata {
                query: String::new(), // No query for history retrieval
                hours_back: params.hours_back,
                channels_searched: 1,
            },
        })
    }
}
