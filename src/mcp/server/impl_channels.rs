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

        let page = self
            .telegram_client
            .get_subscribed_channels(limit, offset)
            .await
            .map_err(|e| e.to_string())?;

        let returned = page.channels.len();
        let response = ChannelsResponse {
            channels: page.channels,
            returned,
            total: Some(page.total),
            has_more: Some(offset as usize + returned < page.total),
        };

        json_response(&response)
    }

    pub(super) async fn get_channel_info_impl(
        &self,
        request: GetChannelInfoRequest,
    ) -> Result<String, String> {
        let channel = if request.include_full.unwrap_or(false) {
            // The full path issues an extra `channels.getFullChannel` RPC on top of
            // resolution, so it is metered like every other RPC-issuing tool
            // (search / history / link / discovery / media all acquire before the
            // call). The basic path is left unmetered, as it is today — changing
            // that is a separate decision about the whole channels module.
            self.rate_limiter
                .acquire(1)
                .await
                .map_err(|e| e.to_string())?;

            self.telegram_client
                .get_full_channel_info(&request.channel_identifier)
                .await
        } else {
            self.telegram_client
                .get_channel_info(&request.channel_identifier)
                .await
        }
        .map_err(|e| e.to_string())?;

        json_response(&channel)
    }
}
