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

        // Acquire rate limiter tokens (1 token per search)
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        // Build search params
        let params = SearchParams {
            query: request.query,
            channel_id,
            hours_back,
            limit,
            media_filter: request.media_filter,
        };

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
            total_found = result.total_found,
            messages_returned = message_ids.len(),
            message_ids = ?message_ids,
            search_time_ms = result.search_time_ms,
            channels_searched = result.query_metadata.channels_searched,
            "Search results"
        );

        serde_json::to_string(&SearchResponse::from(result)).map_err(|e| e.to_string())
    }

    pub(super) async fn get_recent_messages_impl(
        &self,
        request: GetRecentMessagesRequest,
    ) -> Result<String, String> {
        // Validate channel_id is provided
        if request.channel_id.trim().is_empty() {
            return Err("channel_id is required".to_string());
        }

        // Parse channel_id (can be numeric ID or username)
        let original_identifier = request.channel_id.clone();
        let (channel_id, channel_identifier) =
            if request.channel_id.chars().all(|c| c.is_ascii_digit()) {
                // Numeric ID - use helper for validation, no identifier needed
                (parse_channel_id(&request.channel_id)?, None)
            } else {
                // Username provided - resolve via get_channel_info and pass identifier
                let channel = self
                    .telegram_client
                    .get_channel_info(&request.channel_id)
                    .await
                    .map_err(|e| format!("Channel not found: {}", e))?;
                (channel.id, Some(original_identifier))
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

        // Acquire rate limiter tokens (1 token per request)
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        // Build history params
        let params = HistoryParams {
            channel_id,
            channel_identifier,
            hours_back,
            limit,
            media_filter: request.media_filter,
        };

        // Execute history retrieval
        let result = self
            .telegram_client
            .get_recent_messages(&params)
            .await
            .map_err(|e| e.to_string())?;

        // Log results (IDs only, not message text - for privacy and log size)
        let message_ids: Vec<i64> = result.messages.iter().map(|m| m.id.get()).collect();
        tracing::info!(
            channel_id = %params.channel_id,
            media_filter = ?params.media_filter,
            hours_back = params.hours_back,
            limit = params.limit,
            total_found = result.total_found,
            messages_returned = message_ids.len(),
            message_ids = ?message_ids,
            search_time_ms = result.search_time_ms,
            "Recent messages results"
        );

        serde_json::to_string(&SearchResponse::from(result)).map_err(|e| e.to_string())
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

        serde_json::to_string(&MessageResponse::from(message)).map_err(|e| e.to_string())
    }
}
