//! `McpServer` inherent `*_impl` methods: Channel listing & lookup tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::*;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn get_subscribed_channels_impl(
        &self,
        request: GetChannelsRequest,
    ) -> Result<String, String> {
        let limit = request.limit.unwrap_or(20);
        let offset = request.offset.unwrap_or(0);

        let channels = self
            .telegram_client
            .get_subscribed_channels(limit, offset)
            .await
            .map_err(|e| e.to_string())?;

        let total = channels.len();
        let has_more = total >= limit as usize;

        let response = ChannelsResponse {
            channels,
            total,
            has_more,
        };

        serde_json::to_string(&response).map_err(|e| e.to_string())
    }

    pub(super) async fn get_channel_info_impl(
        &self,
        request: GetChannelInfoRequest,
    ) -> Result<String, String> {
        let channel = self
            .telegram_client
            .get_channel_info(&request.channel_identifier)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::to_string(&channel).map_err(|e| e.to_string())
    }
}
