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

        let mut channels = self
            .telegram_client
            .search_public_channels(&request.query, limit)
            .await
            .map_err(|e| e.to_string())?;

        // `contacts.search`'s `limit` bounds only its global `results` set, while
        // the `chats` it returns also carries the caller's own dialog matches — so
        // the converted list can overshoot. The client truncates too; this keeps
        // the MCP contract ("at most `limit` results") true for any client impl.
        channels.truncate(limit as usize);
        let returned = channels.len();
        let response = ChannelsResponse {
            channels,
            returned,
            total: None, // contacts.Search reports no global match count
            // A full page says nothing about what lies beyond it (D10).
            has_more: if returned as u32 == limit {
                None
            } else {
                Some(false)
            },
        };
        json_response(&response)
    }
}
