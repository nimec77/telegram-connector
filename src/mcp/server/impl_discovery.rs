//! `McpServer` inherent `*_impl` method: public channel discovery.

use super::*;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn search_public_channels_impl(
        &self,
        request: SearchPublicChannelsRequest,
    ) -> Result<String, String> {
        if request.query.trim().is_empty() {
            return Err("Search query cannot be empty".to_string());
        }

        let limit = request.limit.unwrap_or(10).clamp(1, 50);

        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        let channels = self
            .telegram_client
            .search_public_channels(&request.query, limit)
            .await
            .map_err(|e| e.to_string())?;

        let total = channels.len();
        let response = ChannelsResponse {
            channels,
            total,
            has_more: false,
        };
        json_response(&response)
    }
}
