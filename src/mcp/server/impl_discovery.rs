//! `McpServer` inherent `*_impl` method: public channel discovery.

use super::McpServer;
use crate::error::Error;
use crate::mcp::tools::{ChannelsResponse, SearchPublicChannelsRequest};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn search_public_channels_data(
        &self,
        request: SearchPublicChannelsRequest,
    ) -> Result<ChannelsResponse, Error> {
        let limit = request.limit.unwrap_or(10).clamp(1, 50);
        let operations = self.account_operations();
        let context = operations.search_context(crate::mcp::runtime::operation_cancellation())?;
        let mut channels = operations
            .public_channels(&request.query, limit, &context)
            .await?;

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
        Ok(response)
    }
}
