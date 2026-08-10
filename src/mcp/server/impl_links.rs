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
        // Parse and validate IDs using helpers
        let channel_id = parse_channel_id(&request.channel_id)?;
        let message_id = parse_message_id(request.message_id)?;

        // Generate links using existing MessageLink from link.rs
        let link = MessageLink::new(channel_id, message_id, None);

        // Build response based on include_tg_protocol flag (defaults to true)
        let include_tg = request.include_tg_protocol.unwrap_or(true);

        let response = MessageLinkResponse {
            channel_id: request.channel_id,
            message_id: request.message_id,
            https_link: link.https_link,
            tg_protocol_link: if include_tg {
                Some(link.tg_protocol_link)
            } else {
                None
            },
        };

        json_response(&response)
    }

    pub(super) async fn open_message_in_telegram_impl(
        &self,
        request: OpenMessageRequest,
    ) -> Result<String, String> {
        // Parse and validate IDs using helpers
        let channel_id = parse_channel_id(&request.channel_id)?;
        let message_id = parse_message_id(request.message_id)?;

        // Generate links
        let link = MessageLink::new(channel_id, message_id, None);

        // Choose link type (defaults to tg:// protocol)
        let use_tg = request.use_tg_protocol.unwrap_or(true);
        let link_to_open = if use_tg {
            &link.tg_protocol_link
        } else {
            &link.https_link
        };

        // Execute open command (macOS-specific)
        #[cfg(target_os = "macos")]
        let result = tokio::process::Command::new("open")
            .arg(link_to_open)
            .output()
            .await;

        #[cfg(not(target_os = "macos"))]
        let result: Result<std::process::Output, std::io::Error> = Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "open_message_in_telegram is only supported on macOS",
        ));

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
