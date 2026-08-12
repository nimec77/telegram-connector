//! `McpServer` inherent `*_impl` methods: Search & history retrieval tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::*;

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

        // Parse optional channel_id using helper
        let channel_id = parse_optional_channel_id(&request.channel_id)?;

        if channel_id.is_none() && (request.before_id.is_some() || request.after_id.is_some()) {
            return Err(
                "before_id/after_id require channel_id: cursor pagination is per-channel; \
                 global search cannot page by message id"
                    .to_string(),
            );
        }

        let format = request.format.unwrap_or_default();
        if channel_id.is_none() && format == ResponseFormat::Compact {
            return Err(
                "format=compact requires channel_id: the compact header describes one channel; \
                 use full format for global search"
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
                "before_id ({}) must be greater than after_id ({}): the page covers after_id < id < before_id",
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

        // Build search params
        let params = SearchParams {
            query: request.query,
            channel_id,
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
        validate_date_window(
            params.from_date,
            params.to_date,
            params.window_start(),
            params.hours_back,
        )?;

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
        for msg in &mut response.messages {
            shaping::truncate_text(msg, max_text_length);
        }
        if response.has_more
            && cursor_eligible
            && let Some(last) = response.messages.last()
        {
            response.next_cursor = Some(NextCursor { before_id: last.id });
        }
        if format == ResponseFormat::Compact {
            shaping::compact_response(&mut response);
        }
        json_response(&response)
    }

    pub(super) async fn get_recent_messages_impl(
        &self,
        request: GetRecentMessagesRequest,
    ) -> Result<String, String> {
        // Validate channel_id is provided
        if request.channel_id.trim().is_empty() {
            return Err("channel_id is required".to_string());
        }

        // Parse channel_id (can be numeric ID or username). The client owns
        // resolution; the server no longer pre-resolves usernames via
        // get_channel_info (AD-2) — that second resolve was redundant with the
        // client's own resolve_username_peer.
        let (channel_id, channel_identifier) =
            if request.channel_id.chars().all(|c| c.is_ascii_digit()) {
                // Numeric ID - validate now; the client walks dialogs by it.
                (Some(parse_channel_id(&request.channel_id)?), None)
            } else {
                // Username - hand the raw reference to the client, which resolves
                // it (and derives the numeric id from the resolved peer).
                (None, Some(request.channel_id.clone()))
            };

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
                "before_id ({}) must be greater than after_id ({}): the page covers after_id < id < before_id",
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

        // Build history params
        let params = HistoryParams {
            channel_id,
            channel_identifier,
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
        validate_date_window(
            params.from_date,
            params.to_date,
            params.window_start(),
            params.hours_back,
        )?;

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
        for msg in &mut response.messages {
            shaping::truncate_text(msg, max_text_length);
        }
        if response.has_more
            && cursor_eligible
            && let Some(last) = response.messages.last()
        {
            response.next_cursor = Some(NextCursor { before_id: last.id });
        }
        if format == ResponseFormat::Compact {
            shaping::compact_response(&mut response);
        }
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

        // Acquire rate limiter token
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        // Fetch the message
        let message = self
            .telegram_client
            .get_message_by_id(&channel_identifier, message_id.get() as i32)
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
