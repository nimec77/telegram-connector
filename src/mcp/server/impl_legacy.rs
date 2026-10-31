//! Legacy direct adapters. Registered canonical routes call typed operations directly.
use super::McpServer;
use crate::mcp::tools;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn check_mcp_status_impl(&self) -> Result<String, String> {
        let value = self
            .check_mcp_status_data()
            .await
            .map_err(|error| error.to_string())?;
        tools::json_response(&value)
    }
    pub(super) async fn get_last_responses_impl(
        &self,
        request: tools::GetLastResponsesRequest,
    ) -> Result<String, String> {
        let value = self
            .get_last_responses_data(request)
            .await
            .map_err(|error| error.to_string())?;
        tools::json_response(&value)
    }
    pub(super) async fn get_subscribed_channels_impl(
        &self,
        request: tools::GetChannelsRequest,
    ) -> Result<String, String> {
        let value = self
            .get_subscribed_channels_data(request)
            .await
            .map_err(|error| error.to_string())?;
        tools::json_response(&value)
    }
    pub(super) async fn get_channel_info_impl(
        &self,
        request: tools::GetChannelInfoRequest,
    ) -> Result<String, String> {
        let value = self
            .get_channel_info_data(request)
            .await
            .map_err(|error| error.to_string())?;
        tools::json_response(&value)
    }
    pub(super) async fn generate_message_link_impl(
        &self,
        request: tools::GenerateLinkRequest,
    ) -> Result<String, String> {
        let value = self
            .generate_message_link_data(request)
            .await
            .map_err(|error| error.to_string())?;
        tools::json_response(&value)
    }
    pub(super) async fn open_message_in_telegram_impl(
        &self,
        request: tools::OpenMessageRequest,
    ) -> Result<String, String> {
        let value = match self.open_message_in_telegram_data(request).await {
            Ok(value) => value,
            Err(crate::error::Error::DesktopOpenFailed { message, link }) => {
                tools::OpenMessageResponse {
                    success: false,
                    message,
                    link_used: link,
                    app_opened: false,
                }
            }
            Err(error) => return Err(error.to_string()),
        };
        tools::json_response(&value)
    }
    pub(super) async fn search_messages_impl(
        &self,
        request: tools::SearchRequest,
    ) -> Result<String, String> {
        let value = self
            .search_messages_data(request)
            .await
            .map_err(|error| error.to_string())?;
        tools::json_response(&value)
    }
    pub(super) async fn get_recent_messages_impl(
        &self,
        request: tools::GetRecentMessagesRequest,
    ) -> Result<String, String> {
        let value = self
            .get_recent_messages_data(request)
            .await
            .map_err(|error| error.to_string())?;
        tools::json_response(&value)
    }
    pub(super) async fn get_message_by_link_impl(
        &self,
        request: tools::GetMessageByLinkRequest,
    ) -> Result<String, String> {
        let value = self
            .get_message_by_link_data(request)
            .await
            .map_err(|error| error.to_string())?;
        tools::json_response(&value)
    }
    pub(super) async fn resolve_channels_impl(
        &self,
        request: tools::ResolveChannelsRequest,
    ) -> Result<String, String> {
        let value = self
            .resolve_channels_data(request)
            .await
            .map_err(|error| error.to_string())?;
        tools::json_response(&value)
    }
    pub(super) async fn get_channel_stats_impl(
        &self,
        request: tools::GetChannelStatsRequest,
    ) -> Result<String, String> {
        let value = self
            .get_channel_stats_data(request)
            .await
            .map_err(|error| error.to_string())?;
        tools::json_response(&value)
    }
    pub(super) async fn get_messages_batch_impl(
        &self,
        request: tools::GetMessagesBatchRequest,
    ) -> Result<String, String> {
        let value = self
            .get_messages_batch_data(request)
            .await
            .map_err(|error| error.to_string())?;
        tools::json_response(&value)
    }
    pub(super) async fn search_public_channels_impl(
        &self,
        request: tools::SearchPublicChannelsRequest,
    ) -> Result<String, String> {
        let value = self
            .search_public_channels_data(request)
            .await
            .map_err(|error| error.to_string())?;
        tools::json_response(&value)
    }
    pub(super) async fn get_message_media_impl(
        &self,
        request: tools::GetMessageMediaRequest,
    ) -> Result<rmcp::model::CallToolResult, String> {
        use crate::mcp::tools::types::contract::WireData as _;
        self.get_message_media_data(request)
            .await
            .and_then(|data| data.legacy())
            .map_err(|e| e.to_string())
    }
    pub(super) async fn get_messages_media_batch_impl(
        &self,
        request: tools::GetMessagesMediaBatchRequest,
    ) -> Result<rmcp::model::CallToolResult, String> {
        use crate::mcp::tools::types::contract::WireData as _;
        self.get_messages_media_batch_data(request)
            .await
            .and_then(|data| data.legacy())
            .map_err(|e| e.to_string())
    }
    pub(super) async fn transcribe_voice_message_impl(
        &self,
        request: tools::TranscribeVoiceMessageRequest,
    ) -> Result<String, String> {
        let data = self
            .transcribe_voice_message_data(request)
            .await
            .map_err(|e| e.to_string())?;
        tools::json_response(&data)
    }
}
