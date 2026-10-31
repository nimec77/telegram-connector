//! `McpServer` inherent `*_impl` methods: Channel listing & lookup tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::McpServer;
use crate::error::Error;
use crate::mcp::tools::validation::validate_channel_reference;
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
        if !(1..=500).contains(&limit) {
            return Err(Error::InvalidInput(
                "limit must be between 1 and 500".to_string(),
            ));
        }
        self.rate_limiter.acquire(1).await?;

        let page = self
            .telegram_client
            .get_subscribed_channels(limit, offset)
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
        validate_channel_reference(&request.channel_identifier)?;
        // One operation charge covers resolution and optional full enrichment.
        self.rate_limiter.acquire(1).await?;
        let channel = if request.include_full.unwrap_or(false) {
            self.telegram_client
                .get_full_channel_info(&request.channel_identifier)
                .await
        } else {
            self.telegram_client
                .get_channel_info(&request.channel_identifier)
                .await
        }?;

        Ok(channel)
    }
}
