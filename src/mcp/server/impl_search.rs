//! `McpServer` inherent `*_impl` methods: Search & history retrieval tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::*;
use crate::mcp::tools::helpers::wire_message_id;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn search_messages_impl(
        &self,
        request: SearchRequest,
    ) -> Result<String, String> {
        // Validate: query required unless media_filter is set
        if request.query.trim().is_empty() && request.media_filter.is_none() {
            return Err(
                "Search query cannot be empty (unless media_filter is set to filter by media type)"
                    .to_string(),
            );
        }

        // `Some(list)` = fan-out; `None` covers both the single-channel_id path
        // and, when channel_id is also absent, the global-search path (A6).
        let scope = fanout::validate_channel_scope(&request.channel_id, &request.channel_ids)?;
        let channel_scoped = request.channel_id.is_some() || scope.is_some();

        if !channel_scoped && (request.before_id.is_some() || request.after_id.is_some()) {
            return Err(
                "before_id/after_id require channel_id: cursor pagination is per-channel; \
                 global search cannot page by message id"
                    .to_string(),
            );
        }

        let format = request.format.unwrap_or_default();
        if !channel_scoped && format == ResponseFormat::Compact {
            return Err(
                "format=compact requires channel_id or channel_ids: the compact header \
                 describes channel scope; use full format for global search"
                    .to_string(),
            );
        }

        // Apply defaults and limits
        let hours_back = request
            .hours_back
            .unwrap_or(SearchParams::DEFAULT_HOURS_BACK)
            .min(SearchParams::MAX_HOURS_BACK);

        let limit = request
            .limit
            .unwrap_or(SearchParams::DEFAULT_LIMIT)
            .min(SearchParams::MAX_LIMIT);

        // Validate limit is greater than 0
        if limit == 0 {
            return Err("Search limit must be greater than 0".to_string());
        }

        // Parse the optional date range
        let from_date = parse_optional_utc("from_date", &request.from_date)?;
        let to_date = parse_optional_utc("to_date", &request.to_date)?;

        // Parse and cross-validate the cursor bounds (A8).
        let before_id = request
            .before_id
            .map(parse_message_id)
            .transpose()
            .map_err(|e| format!("before_id: {}", e))?;
        let after_id = request
            .after_id
            .map(parse_message_id)
            .transpose()
            .map_err(|e| format!("after_id: {}", e))?;
        if let (Some(before), Some(after)) = (before_id, after_id)
            && before.get() <= after.get()
        {
            return Err(format!(
                "before_id ({}) must be greater than after_id ({}): the page covers after_id \
                 < id < before_id",
                before.get(),
                after.get()
            ));
        }

        let max_text_length = request
            .max_text_length
            .unwrap_or(shaping::DEFAULT_MAX_TEXT_LENGTH);
        if max_text_length == 0 {
            return Err("max_text_length must be greater than 0".to_string());
        }

        // Shared parameter template; the channel target (channel_id) is filled
        // in per-path below: once for the single-channel/global path, once per
        // entry for the fan-out path.
        let params_template = SearchParams {
            query: request.query.clone(),
            channel_id: None,
            hours_back,
            limit,
            media_filter: request.media_filter,
            from_date,
            to_date,
            collapse_albums: request.collapse_albums.unwrap_or(true),
            before_id,
            after_id,
        };

        // Reject an empty window before spending a token or a network round-trip.
        let window_from = params_template.window_start();
        validate_date_window(
            params_template.from_date,
            params_template.to_date,
            window_from,
            params_template.hours_back,
        )?;

        if let Some(list) = scope {
            if before_id.is_some() || after_id.is_some() {
                return Err(
                    "before_id/after_id require a single channel_id: cursor pagination is \
                     per-channel"
                        .to_string(),
                );
            }

            // Rate cost for fan-out: one atomic acquire for the deduped channel
            // count, so the D5 deficit message stays accurate.
            self.rate_limiter
                .acquire(list.len() as u32)
                .await
                .map_err(|e| e.to_string())?;

            let outcomes = futures::stream::iter(list.into_iter().map(|reference| {
                let base = params_template.clone(); // SearchParams minus channel_id
                async move {
                    let result = match self.search_channel_id(&reference).await {
                        Ok(channel_id) => {
                            let params = SearchParams {
                                channel_id: Some(channel_id),
                                ..base
                            };
                            self.telegram_client
                                .search_messages(&params)
                                .await
                                .map_err(|e| e.to_string())
                        }
                        Err(e) => Err(e),
                    };
                    fanout::ChannelFetchOutcome {
                        channel: reference,
                        result,
                    }
                }
            }))
            .buffered(fanout::FANOUT_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;

            let mut response = fanout::merge_results(
                outcomes,
                limit as usize,
                request.query.clone(),
                window_from,
                to_date,
            )?;
            shaping::shape_response(
                &mut response,
                format,
                max_text_length,
                /* cursor_eligible */ false,
                self.response_byte_budget,
                shaping::CompactScope::Multi,
            )?;
            return json_response(&response);
        }

        // Single-channel/global path. Numeric refs parse locally; a username
        // ref spends one resolve RPC before the search itself (§1.3).
        let channel_id = match &request.channel_id {
            Some(reference) => Some(self.search_channel_id(reference).await?),
            None => None,
        };

        // Build search params
        let params = SearchParams {
            channel_id,
            ..params_template
        };

        // Acquire rate limiter tokens (1 token per search)
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        // Execute search
        let result = self
            .telegram_client
            .search_messages(&params)
            .await
            .map_err(|e| e.to_string())?;

        // Log search results (IDs only, not message text - for privacy and log size)
        let message_ids: Vec<i64> = result.messages.iter().map(|m| m.id.get()).collect();
        tracing::info!(
            query = %params.query,
            channel_id = ?params.channel_id.map(|c| c.get()),
            media_filter = ?params.media_filter,
            hours_back = params.hours_back,
            limit = params.limit,
            returned = result.returned,
            messages_returned = message_ids.len(),
            message_ids = ?message_ids,
            search_time_ms = result.search_time_ms,
            channels_in_results = result.query_metadata.channels_in_results,
            "Search results"
        );

        let cursor_eligible = params.channel_id.is_some();
        let mut response = SearchResponse::from(result);
        shaping::shape_response(
            &mut response,
            format,
            max_text_length,
            cursor_eligible,
            self.response_byte_budget,
            shaping::CompactScope::Single,
        )?;
        json_response(&response)
    }

    /// Numeric refs parse locally; username refs spend one resolve RPC (§1.3).
    async fn search_channel_id(&self, reference: &str) -> Result<ChannelId, String> {
        if reference.chars().all(|c| c.is_ascii_digit()) {
            parse_channel_id(reference)
        } else {
            self.telegram_client
                .resolve_channel_identity(reference)
                .await
                .map(|identity| identity.id)
                .map_err(|e| e.to_string())
        }
    }

    pub(super) async fn get_recent_messages_impl(
        &self,
        request: GetRecentMessagesRequest,
    ) -> Result<String, String> {
        // Apply defaults and limits
        let hours_back = request
            .hours_back
            .unwrap_or(HistoryParams::DEFAULT_HOURS_BACK)
            .min(HistoryParams::MAX_HOURS_BACK);

        let limit = request
            .limit
            .unwrap_or(HistoryParams::DEFAULT_LIMIT)
            .min(HistoryParams::MAX_LIMIT);

        // Validate limit is greater than 0
        if limit == 0 {
            return Err("Limit must be greater than 0".to_string());
        }

        // Parse the optional date range
        let from_date = parse_optional_utc("from_date", &request.from_date)?;
        let to_date = parse_optional_utc("to_date", &request.to_date)?;

        // Parse and cross-validate the cursor bounds (A8).
        let before_id = request
            .before_id
            .map(parse_message_id)
            .transpose()
            .map_err(|e| format!("before_id: {}", e))?;
        let after_id = request
            .after_id
            .map(parse_message_id)
            .transpose()
            .map_err(|e| format!("after_id: {}", e))?;
        if let (Some(before), Some(after)) = (before_id, after_id)
            && before.get() <= after.get()
        {
            return Err(format!(
                "before_id ({}) must be greater than after_id ({}): the page covers after_id \
                 < id < before_id",
                before.get(),
                after.get()
            ));
        }

        let max_text_length = request
            .max_text_length
            .unwrap_or(shaping::DEFAULT_MAX_TEXT_LENGTH);
        if max_text_length == 0 {
            return Err("max_text_length must be greater than 0".to_string());
        }

        let format = request.format.unwrap_or_default();

        // Shared parameter template; the channel target (channel_id/channel_identifier)
        // is filled in per-path below: once for the single-channel path, once per
        // entry for the fan-out path.
        let params_template = HistoryParams {
            channel_id: None,
            channel_identifier: None,
            hours_back,
            limit,
            media_filter: request.media_filter,
            from_date,
            to_date,
            collapse_albums: request.collapse_albums.unwrap_or(true),
            before_id,
            after_id,
        };

        // Reject an empty window before spending a token or a network round-trip.
        let window_from = params_template.window_start();
        validate_date_window(
            params_template.from_date,
            params_template.to_date,
            window_from,
            params_template.hours_back,
        )?;

        let channels = fanout::validate_channel_scope(&request.channel_id, &request.channel_ids)?;
        // channels: Option<Vec<String>> — Some(list) means fan-out.
        if let Some(list) = channels {
            if before_id.is_some() || after_id.is_some() {
                return Err("before_id/after_id require a single channel_id: cursor \
                            pagination is per-channel"
                    .to_string());
            }

            // Rate cost for fan-out: one atomic acquire for the deduped channel
            // count, so the D5 deficit message stays accurate.
            self.rate_limiter
                .acquire(list.len() as u32)
                .await
                .map_err(|e| e.to_string())?;

            let outcomes = futures::stream::iter(list.into_iter().map(|reference| {
                let client = Arc::clone(&self.telegram_client);
                let base = params_template.clone(); // HistoryParams minus target
                async move {
                    let result = match history_target(&reference) {
                        Ok((channel_id, channel_identifier)) => {
                            let params = HistoryParams {
                                channel_id,
                                channel_identifier,
                                ..base
                            };
                            client
                                .get_recent_messages(&params)
                                .await
                                .map_err(|e| e.to_string())
                        }
                        Err(e) => Err(e),
                    };
                    fanout::ChannelFetchOutcome {
                        channel: reference,
                        result,
                    }
                }
            }))
            .buffered(fanout::FANOUT_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;

            let mut response = fanout::merge_results(
                outcomes,
                limit as usize,
                String::new(),
                window_from,
                to_date,
            )?;
            shaping::shape_response(
                &mut response,
                format,
                max_text_length,
                /* cursor_eligible */ false,
                self.response_byte_budget,
                shaping::CompactScope::Multi,
            )?;
            return json_response(&response);
        }

        // Single-channel path. The client owns resolution; the server no longer
        // pre-resolves usernames via get_channel_info (AD-2) — that second
        // resolve was redundant with the client's own resolve_username_peer.
        let channel_id_str = request
            .channel_id
            .ok_or_else(|| "channel_id (or channel_ids) is required".to_string())?;
        let (channel_id, channel_identifier) = history_target(&channel_id_str)?;

        // Build history params
        let params = HistoryParams {
            channel_id,
            channel_identifier,
            ..params_template
        };

        // Acquire rate limiter tokens (1 token per request)
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        // Execute history retrieval
        let result = self
            .telegram_client
            .get_recent_messages(&params)
            .await
            .map_err(|e| e.to_string())?;

        // Log results (IDs only, not message text - for privacy and log size)
        let message_ids: Vec<i64> = result.messages.iter().map(|m| m.id.get()).collect();
        tracing::info!(
            channel_id = ?params.channel_id.map(|c| c.get()),
            media_filter = ?params.media_filter,
            hours_back = params.hours_back,
            limit = params.limit,
            returned = result.returned,
            messages_returned = message_ids.len(),
            message_ids = ?message_ids,
            search_time_ms = result.search_time_ms,
            "Recent messages results"
        );

        let cursor_eligible = true; // get_recent_messages: always single-channel
        let mut response = SearchResponse::from(result);
        shaping::shape_response(
            &mut response,
            format,
            max_text_length,
            cursor_eligible,
            self.response_byte_budget,
            shaping::CompactScope::Single,
        )?;
        json_response(&response)
    }

    pub(super) async fn get_message_by_link_impl(
        &self,
        request: GetMessageByLinkRequest,
    ) -> Result<String, String> {
        // Parse the link
        let (channel_ref, message_id) =
            parse_telegram_link(&request.link).map_err(|e| e.to_string())?;

        // Convert ChannelRef to string identifier for the trait method
        let channel_identifier = match &channel_ref {
            ChannelRef::Username(username) => username.clone(),
            ChannelRef::Id(id) => id.get().to_string(),
        };

        let wire_id = wire_message_id(message_id)?;

        // Acquire rate limiter token
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        // Fetch the message
        let message = self
            .telegram_client
            .get_message_by_id(&channel_identifier, wire_id)
            .await
            .map_err(|e| e.to_string())?;

        tracing::info!(
            link = %request.link,
            channel = %channel_identifier,
            message_id = message_id.get(),
            "Message by link results"
        );

        json_response(&MessageResponse::from(message))
    }
}

/// Split a channel reference into HistoryParams' (channel_id, channel_identifier) pair.
fn history_target(reference: &str) -> Result<(Option<ChannelId>, Option<String>), String> {
    if reference.chars().all(|c| c.is_ascii_digit()) {
        Ok((Some(parse_channel_id(reference)?), None))
    } else {
        Ok((None, Some(reference.to_string())))
    }
}
