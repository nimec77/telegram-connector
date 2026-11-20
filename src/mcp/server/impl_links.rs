//! `McpServer` inherent `*_impl` methods: Message-link generation & open tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::McpServer;
use crate::error::Error;
use crate::mcp::tools::validation::parse_message_id;
use crate::mcp::tools::{GenerateLinkRequest, MessageLinkResponse, OpenMessageRequest};
// The desktop result is shared by production launching and the injected offline seam.
use crate::mcp::tools::OpenMessageResponse;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn generate_message_link_data(
        &self,
        request: GenerateLinkRequest,
    ) -> Result<MessageLinkResponse, Error> {
        let message_id = parse_message_id(request.message_id)?;

        let operations = self.account_operations();
        let context = operations.history_context(crate::mcp::runtime::operation_cancellation())?;
        let (identity, link) = operations
            .message_link_by_reference(&request.channel_id, message_id, &context)
            .await?;
        let include_tg = request.include_tg_protocol.unwrap_or(true);

        let response = MessageLinkResponse {
            resolved_channel: Some((identity.id, identity.peer_namespace)),
            channel_id: identity.id.to_string(),
            message_id: request.message_id,
            https_link: link.https_link,
            tg_protocol_link: if include_tg {
                Some(link.tg_protocol_link)
            } else {
                None
            },
            internal_link: link.internal_link,
            is_public: link.is_public,
        };

        Ok(response)
    }

    pub(super) async fn open_message_in_telegram_data(
        &self,
        request: OpenMessageRequest,
    ) -> Result<crate::mcp::tools::OpenMessageResponse, Error> {
        let message_id = parse_message_id(request.message_id)?;

        if !self.desktop_launcher.supported() {
            return Err(Error::UnsupportedPlatform);
        }
        let operations = self.account_operations();
        let context = operations.history_context(crate::mcp::runtime::operation_cancellation())?;
        let (_, link) = operations
            .message_link_by_reference(&request.channel_id, message_id, &context)
            .await?;
        let link_to_open = if request.use_tg_protocol.unwrap_or(true) {
            link.tg_protocol_link
        } else {
            link.https_link
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.desktop_launcher.open(&link_to_open),
        )
        .await
        .map_err(|_| Error::Timeout {
            operation: "desktop_open".into(),
            secs: 10,
        })??;
        Ok(OpenMessageResponse {
            success: true,
            message: "Message opened in Telegram".into(),
            link_used: link_to_open,
            app_opened: true,
        })
    }
}
