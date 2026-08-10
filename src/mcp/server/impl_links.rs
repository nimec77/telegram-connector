//! `McpServer` inherent `*_impl` methods: Message-link generation & open tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::*;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn generate_message_link_impl(
        &self,
        request: GenerateLinkRequest,
    ) -> Result<String, String> {
        let message_id = parse_message_id(request.message_id)?;

        // One rate-limited peer resolution: the username is required to emit
        // the shareable t.me/<username> form for public channels (B2), and
        // it lets channel_id be a username too (D9).
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        let identity = self
            .telegram_client
            .resolve_channel_identity(&request.channel_id)
            .await
            .map_err(|e| e.to_string())?;

        let link = MessageLink::new(identity.id, message_id, identity.username.as_deref());
        let include_tg = request.include_tg_protocol.unwrap_or(true);

        let response = MessageLinkResponse {
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

        json_response(&response)
    }

    pub(super) async fn open_message_in_telegram_impl(
        &self,
        request: OpenMessageRequest,
    ) -> Result<String, String> {
        let message_id = parse_message_id(request.message_id)?;

        #[cfg(not(target_os = "macos"))]
        {
            let _ = message_id;
            return Err("open_message_in_telegram is only supported on macOS".to_string());
        }

        #[cfg(target_os = "macos")]
        {
            self.rate_limiter
                .acquire(1)
                .await
                .map_err(|e| e.to_string())?;

            let identity = self
                .telegram_client
                .resolve_channel_identity(&request.channel_id)
                .await
                .map_err(|e| e.to_string())?;

            let link = MessageLink::new(identity.id, message_id, identity.username.as_deref());

            // Choose link type (defaults to tg:// protocol)
            let use_tg = request.use_tg_protocol.unwrap_or(true);
            let link_to_open = if use_tg {
                &link.tg_protocol_link
            } else {
                &link.https_link
            };

            // Execute open command (macOS-specific)
            let result = tokio::process::Command::new("open")
                .arg(link_to_open)
                .output()
                .await;

            let response = match result {
                Ok(output) => {
                    let success = output.status.success();
                    OpenMessageResponse {
                        success,
                        message: if success {
                            "Message opened in Telegram".to_string()
                        } else {
                            format!("Failed to open: {:?}", output.status)
                        },
                        link_used: link_to_open.clone(),
                        app_opened: success,
                    }
                }
                Err(e) => OpenMessageResponse {
                    success: false,
                    message: format!("Failed to execute open command: {}", e),
                    link_used: link_to_open.clone(),
                    app_opened: false,
                },
            };

            json_response(&response)
        }
    }
}
