//! `McpServer` inherent `*_impl` methods: Channel listing & lookup tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::McpServer;
use crate::error::Error;
use crate::mcp::tools::{ChannelsResponse, GetChannelInfoRequest, GetChannelsRequest};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn get_subscribed_channels_data(
        &self,
        request: GetChannelsRequest,
    ) -> Result<ChannelsResponse, Error> {
        let limit = request.limit.unwrap_or(20);
        let offset = request.offset.unwrap_or(0);
        let operations = self.account_operations();
        let context = operations.history_context(crate::mcp::runtime::operation_cancellation())?;
        let page = operations
            .subscribed_channels(limit, offset, &context)
            .await?;

        let returned = page.channels.len();
        let response = ChannelsResponse {
            channels: page.channels,
            returned,
            total: Some(page.total),
            has_more: Some(offset as usize + returned < page.total),
        };

        Ok(response)
    }

    pub(super) async fn get_channel_info_data(
        &self,
        request: GetChannelInfoRequest,
    ) -> Result<crate::telegram::types::Channel, Error> {
        let operations = self.account_operations();
        let context = operations.history_context(crate::mcp::runtime::operation_cancellation())?;
        operations
            .channel_info_by_reference(
                &request.channel_identifier,
                request.include_full.unwrap_or(false),
                &context,
            )
            .await
    }
}
