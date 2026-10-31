//! `McpServer` inherent `*_impl` methods: Message-link generation & open tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::McpServer;
use crate::error::Error;
use crate::link::MessageLink;
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

        // One rate-limited peer resolution: the username is required to emit
        // the shareable t.me/<username> form for public channels (B2), and
        // it lets channel_id be a username too (D9).
        crate::mcp::tools::validation::validate_channel_reference(&request.channel_id)?;
        self.rate_limiter.acquire(1).await?;

        let identity = self
            .telegram_client
            .resolve_channel_identity(&request.channel_id)
            .await?;

        let link = MessageLink::new(identity.id, message_id, identity.username.as_deref());
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
        crate::mcp::tools::validation::validate_channel_reference(&request.channel_id)?;
        self.rate_limiter.acquire(1).await?;
        let identity = self
            .telegram_client
            .resolve_channel_identity(&request.channel_id)
            .await?;
        let link = MessageLink::new(identity.id, message_id, identity.username.as_deref());
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
