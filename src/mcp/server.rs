use crate::link::MessageLink;
use crate::mcp::tools::{
    ChannelsResponse, GenerateLinkRequest, GetChannelInfoRequest, GetChannelsRequest,
    MessageLinkResponse, OpenMessageRequest, OpenMessageResponse, SearchRequest, StatusResponse,
};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::Channel;
use crate::telegram::client::TelegramClientTrait;
use crate::telegram::types::{ChannelId, MessageId, SearchParams, SearchResult};
use rmcp::model::{Implementation, InitializeResult, ProtocolVersion};
use rmcp::{Json, ServerHandler, ServiceExt};
use std::sync::Arc;

pub struct McpServer<T: TelegramClientTrait, R: RateLimiterTrait> {
    telegram_client: Arc<T>,
    rate_limiter: Arc<R>,
}

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub fn new(telegram_client: Arc<T>, rate_limiter: Arc<R>) -> Self {
        Self {
            telegram_client,
            rate_limiter,
        }
    }

    pub async fn run_stdio(self) -> anyhow::Result<()> {
        use tokio::io::{stdin, stdout};

        // Create stdio transport
        let transport = (stdin(), stdout());

        // Start MCP server with stdio transport
        let server = self.serve(transport).await?;

        // Wait for shutdown signal (blocks until server terminates)
        server.waiting().await?;

        Ok(())
    }

    // ========================================================================
    // MCP Tools
    // ========================================================================

    /// Tool 1: check_mcp_status - Health check and diagnostics
    pub async fn check_mcp_status(&self) -> Result<Json<StatusResponse>, String> {
        let connected = self.telegram_client.is_connected().await;
        let tokens = self.rate_limiter.available_tokens();

        Ok(Json(StatusResponse {
            telegram_connected: connected,
            rate_limiter_tokens: tokens,
            server_version: env!("CARGO_PKG_VERSION").to_string(),
        }))
    }

    /// Tool 2: get_subscribed_channels - List user's Telegram channels with pagination
    pub async fn get_subscribed_channels(
        &self,
        request: GetChannelsRequest,
    ) -> Result<Json<ChannelsResponse>, String> {
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

        Ok(Json(response))
    }

    /// Tool 3: get_channel_info - Get detailed information about a Telegram channel
    pub async fn get_channel_info(
        &self,
        request: GetChannelInfoRequest,
    ) -> Result<Json<Channel>, String> {
        let channel = self
            .telegram_client
            .get_channel_info(&request.channel_identifier)
            .await
            .map_err(|e| e.to_string())?;

        Ok(Json(channel))
    }

    /// Tool 4: generate_message_link - Generate deep links for a Telegram message
    pub async fn generate_message_link(
        &self,
        request: GenerateLinkRequest,
    ) -> Result<Json<MessageLinkResponse>, String> {
        // Parse channel_id string to i64
        let channel_id_num: i64 = request.channel_id.parse().map_err(|_| {
            format!(
                "Invalid channel_id: '{}' is not a valid number",
                request.channel_id
            )
        })?;

        // Create type-safe IDs
        let channel_id =
            ChannelId::new(channel_id_num).map_err(|e| format!("Invalid channel_id: {}", e))?;
        let message_id =
            MessageId::new(request.message_id).map_err(|e| format!("Invalid message_id: {}", e))?;

        // Generate links using existing MessageLink from link.rs
        let link = MessageLink::new(channel_id, message_id);

        // Build response based on include_tg_protocol flag (defaults to true)
        let include_tg = request.include_tg_protocol.unwrap_or(true);

        Ok(Json(MessageLinkResponse {
            channel_id: request.channel_id,
            message_id: request.message_id,
            https_link: link.https_link,
            tg_protocol_link: if include_tg {
                Some(link.tg_protocol_link)
            } else {
                None
            },
        }))
    }

    /// Tool 5: open_message_in_telegram - Open message in Telegram Desktop (macOS)
    pub async fn open_message_in_telegram(
        &self,
        request: OpenMessageRequest,
    ) -> Result<Json<OpenMessageResponse>, String> {
        // Parse channel_id string to i64
        let channel_id_num: i64 = request.channel_id.parse().map_err(|_| {
            format!(
                "Invalid channel_id: '{}' is not a valid number",
                request.channel_id
            )
        })?;

        // Create type-safe IDs
        let channel_id =
            ChannelId::new(channel_id_num).map_err(|e| format!("Invalid channel_id: {}", e))?;
        let message_id =
            MessageId::new(request.message_id).map_err(|e| format!("Invalid message_id: {}", e))?;

        // Generate links
        let link = MessageLink::new(channel_id, message_id);

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

        match result {
            Ok(output) => {
                let success = output.status.success();
                Ok(Json(OpenMessageResponse {
                    success,
                    message: if success {
                        "Message opened in Telegram".to_string()
                    } else {
                        format!("Failed to open: {:?}", output.status)
                    },
                    link_used: link_to_open.clone(),
                    app_opened: success,
                }))
            }
            Err(e) => Ok(Json(OpenMessageResponse {
                success: false,
                message: format!("Failed to execute open command: {}", e),
                link_used: link_to_open.clone(),
                app_opened: false,
            })),
        }
    }

    /// Tool 6: search_messages - Search messages across Telegram channels
    pub async fn search_messages(
        &self,
        request: SearchRequest,
    ) -> Result<Json<SearchResult>, String> {
        // Validate query is not empty
        if request.query.trim().is_empty() {
            return Err("Search query cannot be empty".to_string());
        }

        // Parse optional channel_id
        let channel_id = match &request.channel_id {
            Some(id_str) => {
                let id_num: i64 = id_str.parse().map_err(|_| {
                    format!("Invalid channel_id: '{}' is not a valid number", id_str)
                })?;
                Some(ChannelId::new(id_num).map_err(|e| format!("Invalid channel_id: {}", e))?)
            }
            None => None,
        };

        // Apply defaults and limits
        let hours_back = request
            .hours_back
            .unwrap_or(SearchParams::DEFAULT_HOURS_BACK)
            .min(SearchParams::MAX_HOURS_BACK);

        let limit = request
            .limit
            .unwrap_or(SearchParams::DEFAULT_LIMIT)
            .min(SearchParams::MAX_LIMIT);

        // Validate limit is greater than 0
        if limit == 0 {
            return Err("Search limit must be greater than 0".to_string());
        }

        // Acquire rate limiter tokens (1 token per search)
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        // Build search params
        let params = SearchParams {
            query: request.query,
            channel_id,
            hours_back,
            limit,
        };

        // Execute search
        let result = self
            .telegram_client
            .search_messages(&params)
            .await
            .map_err(|e| e.to_string())?;

        Ok(Json(result))
    }
}

// Implement ServerHandler trait - tool registration will be added in Phase 11
impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> ServerHandler
    for McpServer<T, R>
{
    fn get_info(&self) -> InitializeResult {
        InitializeResult {
            protocol_version: ProtocolVersion::default(),
            capabilities: Default::default(),
            server_info: Implementation {
                name: "telegram-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Telegram MCP Connector - Search Russian Telegram channels".to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rate_limiter::MockRateLimiterTrait;
    use crate::telegram::client::MockTelegramClientTrait;

    #[test]
    fn server_new_creates_instance_with_valid_dependencies() {
        // Given: Mock client and rate limiter
        let mock_client = MockTelegramClientTrait::new();
        let mock_limiter = MockRateLimiterTrait::new();

        let client_arc = Arc::new(mock_client);
        let limiter_arc = Arc::new(mock_limiter);

        // When: Create new server
        let server = McpServer::new(Arc::clone(&client_arc), Arc::clone(&limiter_arc));

        // Then: Server is created successfully
        // Verify Arc refcounts increased (2 refs each: original + server)
        assert_eq!(Arc::strong_count(&client_arc), 2);
        assert_eq!(Arc::strong_count(&limiter_arc), 2);

        // Cleanup
        drop(server);
        assert_eq!(Arc::strong_count(&client_arc), 1);
        assert_eq!(Arc::strong_count(&limiter_arc), 1);
    }

    #[test]
    fn server_handler_provides_server_info() {
        // Given: Server instance with mocks
        let mock_client = MockTelegramClientTrait::new();
        let mock_limiter = MockRateLimiterTrait::new();

        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        // When: Get server info via ServerHandler trait
        use rmcp::ServerHandler;
        let result = server.get_info();

        // Then: InitializeResult contains expected metadata
        assert_eq!(result.protocol_version, ProtocolVersion::default());
        assert_eq!(result.server_info.name, "telegram-mcp");
        assert_eq!(result.server_info.version, env!("CARGO_PKG_VERSION"));
        assert!(result.instructions.is_some());
        assert!(
            result
                .instructions
                .unwrap()
                .contains("Telegram MCP Connector")
        );
    }

    // Manual smoke test for run_stdio() will be done in Phase 12 integration testing

    // ========================================================================
    // Tool Tests
    // ========================================================================

    #[tokio::test]
    async fn check_status_returns_connection_info() {
        // Given: Server with mock client (connected) and rate limiter (tokens available)
        let mut mock_client = MockTelegramClientTrait::new();
        mock_client.expect_is_connected().return_once(|| true);

        let mut mock_limiter = MockRateLimiterTrait::new();
        mock_limiter.expect_available_tokens().return_once(|| 45.5);

        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        // When: Call check_mcp_status
        let result = server.check_mcp_status().await;

        // Then: Returns success with connection info
        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert!(response.telegram_connected);
        assert_eq!(response.rate_limiter_tokens, 45.5);
        assert_eq!(response.server_version, env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn check_status_reports_disconnected() {
        // Given: Server with disconnected client
        let mut mock_client = MockTelegramClientTrait::new();
        mock_client.expect_is_connected().return_once(|| false);

        let mut mock_limiter = MockRateLimiterTrait::new();
        mock_limiter.expect_available_tokens().return_once(|| 0.0);

        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        // When: Call check_mcp_status
        let result = server.check_mcp_status().await;

        // Then: Returns disconnected status
        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert!(!response.telegram_connected);
        assert_eq!(response.rate_limiter_tokens, 0.0);
    }

    #[tokio::test]
    async fn get_subscribed_channels_returns_list() {
        use crate::telegram::types::Username;
        use crate::telegram::{Channel, ChannelId, ChannelName};

        // Helper to create test channel
        fn create_test_channel(id: i64, name: &str) -> Channel {
            Channel {
                id: ChannelId::new(id).unwrap(),
                name: ChannelName::new(name).unwrap(),
                username: Username::new("testchannel").unwrap(),
                description: Some("Test channel".to_string()),
                member_count: 1000,
                is_verified: false,
                is_public: true,
                is_subscribed: true,
                last_message_date: None,
            }
        }

        // Given: Mock client returning test channels
        let mut mock_client = MockTelegramClientTrait::new();
        let test_channels = vec![
            create_test_channel(123, "Channel 1"),
            create_test_channel(456, "Channel 2"),
        ];
        let expected = test_channels.clone();

        mock_client
            .expect_get_subscribed_channels()
            .with(
                mockall::predicate::eq(20), // default limit
                mockall::predicate::eq(0),  // default offset
            )
            .return_once(move |_, _| Ok(expected));

        let mock_limiter = MockRateLimiterTrait::new();
        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        // When: Call get_subscribed_channels with defaults
        let request = GetChannelsRequest {
            limit: None,
            offset: None,
        };

        let result = server.get_subscribed_channels(request).await;

        // Then: Returns success with channel list
        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.channels.len(), 2);
        assert_eq!(response.total, 2);
        assert!(!response.has_more); // 2 channels < 20 limit
    }

    #[tokio::test]
    async fn get_subscribed_channels_respects_pagination() {
        use crate::telegram::types::Username;
        use crate::telegram::{Channel, ChannelId, ChannelName};

        // Helper to create test channel
        fn create_test_channel(id: i64, name: &str) -> Channel {
            Channel {
                id: ChannelId::new(id).unwrap(),
                name: ChannelName::new(name).unwrap(),
                username: Username::new("testchannel").unwrap(),
                description: Some("Test channel".to_string()),
                member_count: 1000,
                is_verified: false,
                is_public: true,
                is_subscribed: true,
                last_message_date: None,
            }
        }

        // Given: Mock client with custom pagination parameters
        let mut mock_client = MockTelegramClientTrait::new();
        let test_channels = vec![create_test_channel(789, "Channel 3")];
        let expected = test_channels.clone();

        mock_client
            .expect_get_subscribed_channels()
            .with(
                mockall::predicate::eq(10), // custom limit
                mockall::predicate::eq(5),  // custom offset
            )
            .return_once(move |_, _| Ok(expected));

        let mock_limiter = MockRateLimiterTrait::new();
        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        // When: Call with custom pagination
        let request = GetChannelsRequest {
            limit: Some(10),
            offset: Some(5),
        };

        let result = server.get_subscribed_channels(request).await;

        // Then: Returns success with correct pagination values
        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.channels.len(), 1);
        assert_eq!(response.total, 1);
        assert!(!response.has_more); // 1 channel < 10 limit
    }

    #[tokio::test]
    async fn get_channel_info_returns_channel_details() {
        use crate::telegram::types::Username;
        use crate::telegram::{Channel, ChannelId, ChannelName};

        // Given: Mock client returning channel details
        let mut mock_client = MockTelegramClientTrait::new();
        let test_channel = Channel {
            id: ChannelId::new(12345).unwrap(),
            name: ChannelName::new("Test Channel").unwrap(),
            username: Username::new("testchannel").unwrap(),
            description: Some("A test channel".to_string()),
            member_count: 5000,
            is_verified: true,
            is_public: true,
            is_subscribed: false,
            last_message_date: None,
        };
        let expected = test_channel.clone();

        mock_client
            .expect_get_channel_info()
            .with(mockall::predicate::eq("testchannel"))
            .return_once(move |_| Ok(expected));

        let mock_limiter = MockRateLimiterTrait::new();
        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        // When: Call get_channel_info
        let request = GetChannelInfoRequest {
            channel_identifier: "testchannel".to_string(),
        };

        let result = server.get_channel_info(request).await;

        // Then: Returns channel details
        assert!(result.is_ok());
        let channel = result.unwrap().0;
        assert_eq!(channel.id, ChannelId::new(12345).unwrap());
        assert_eq!(channel.name.as_str(), "Test Channel");
        assert!(channel.is_verified);
        assert_eq!(channel.member_count, 5000);
    }

    #[tokio::test]
    async fn get_channel_info_handles_error() {
        use crate::error::Error;

        // Given: Mock client returning error
        let mut mock_client = MockTelegramClientTrait::new();
        mock_client
            .expect_get_channel_info()
            .with(mockall::predicate::eq("nonexistent"))
            .return_once(move |_| Err(Error::TelegramApi("Channel not found".to_string())));

        let mock_limiter = MockRateLimiterTrait::new();
        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        // When: Call get_channel_info with nonexistent channel
        let request = GetChannelInfoRequest {
            channel_identifier: "nonexistent".to_string(),
        };

        let result = server.get_channel_info(request).await;

        // Then: Returns error
        assert!(result.is_err());
        if let Err(error_msg) = result {
            assert!(error_msg.contains("Channel not found"));
        }
    }

    // ========================================================================
    // Tool 4: generate_message_link
    // ========================================================================

    #[tokio::test]
    async fn generate_message_link_returns_both_formats() {
        // Given: Server and valid request
        let mock_client = MockTelegramClientTrait::new();
        let mock_limiter = MockRateLimiterTrait::new();
        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        let request = GenerateLinkRequest {
            channel_id: "123456789".to_string(),
            message_id: 42,
            include_tg_protocol: None, // defaults to true
        };

        // When: Generate link
        let result = server.generate_message_link(request).await;

        // Then: Returns both link formats
        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.channel_id, "123456789");
        assert_eq!(response.message_id, 42);
        assert_eq!(response.https_link, "https://t.me/c/123456789/42?single");
        assert!(response.tg_protocol_link.is_some());
        assert_eq!(
            response.tg_protocol_link.unwrap(),
            "tg://resolve?channel=123456789&post=42&single"
        );
    }

    #[tokio::test]
    async fn generate_message_link_without_tg_protocol() {
        // Given: Server and request with include_tg_protocol = false
        let mock_client = MockTelegramClientTrait::new();
        let mock_limiter = MockRateLimiterTrait::new();
        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        let request = GenerateLinkRequest {
            channel_id: "999".to_string(),
            message_id: 111,
            include_tg_protocol: Some(false),
        };

        // When: Generate link
        let result = server.generate_message_link(request).await;

        // Then: Returns only HTTPS link (tg_protocol_link is None)
        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.https_link, "https://t.me/c/999/111?single");
        assert!(response.tg_protocol_link.is_none());
    }

    #[tokio::test]
    async fn generate_message_link_invalid_channel_id() {
        // Given: Server and request with non-numeric channel_id
        let mock_client = MockTelegramClientTrait::new();
        let mock_limiter = MockRateLimiterTrait::new();
        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        let request = GenerateLinkRequest {
            channel_id: "not_a_number".to_string(),
            message_id: 42,
            include_tg_protocol: None,
        };

        // When: Generate link
        let result = server.generate_message_link(request).await;

        // Then: Returns error
        assert!(result.is_err());
        if let Err(error_msg) = result {
            assert!(error_msg.contains("Invalid channel_id"));
        }
    }

    // ========================================================================
    // Tool 5: open_message_in_telegram
    // ========================================================================

    #[tokio::test]
    async fn open_message_in_telegram_invalid_channel_id() {
        // Given: Server and request with non-numeric channel_id
        let mock_client = MockTelegramClientTrait::new();
        let mock_limiter = MockRateLimiterTrait::new();
        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        let request = OpenMessageRequest {
            channel_id: "invalid".to_string(),
            message_id: 42,
            use_tg_protocol: None,
        };

        // When: Try to open message
        let result = server.open_message_in_telegram(request).await;

        // Then: Returns error
        assert!(result.is_err());
        if let Err(error_msg) = result {
            assert!(error_msg.contains("Invalid channel_id"));
        }
    }

    #[tokio::test]
    async fn open_message_in_telegram_uses_tg_protocol_by_default() {
        // Given: Server and request without use_tg_protocol specified
        let mock_client = MockTelegramClientTrait::new();
        let mock_limiter = MockRateLimiterTrait::new();
        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        let request = OpenMessageRequest {
            channel_id: "123456".to_string(),
            message_id: 42,
            use_tg_protocol: None, // defaults to true
        };

        // When: Open message
        let result = server.open_message_in_telegram(request).await;

        // Then: Returns response with tg:// link
        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert!(response.link_used.starts_with("tg://"));
    }

    #[tokio::test]
    async fn open_message_in_telegram_uses_https_when_requested() {
        // Given: Server and request with use_tg_protocol = false
        let mock_client = MockTelegramClientTrait::new();
        let mock_limiter = MockRateLimiterTrait::new();
        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        let request = OpenMessageRequest {
            channel_id: "123456".to_string(),
            message_id: 42,
            use_tg_protocol: Some(false),
        };

        // When: Open message
        let result = server.open_message_in_telegram(request).await;

        // Then: Returns response with https:// link
        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert!(response.link_used.starts_with("https://"));
    }

    // ========================================================================
    // Tool 6: search_messages
    // ========================================================================

    #[tokio::test]
    async fn search_messages_returns_results() {
        use crate::telegram::types::{Message, QueryMetadata, SearchResult, Username};
        use crate::telegram::{ChannelId, ChannelName};

        // Given: Mock client returning search results
        let mut mock_client = MockTelegramClientTrait::new();
        let expected_result = SearchResult {
            messages: vec![Message {
                id: MessageId::new(1).unwrap(),
                channel_id: ChannelId::new(123).unwrap(),
                channel_name: ChannelName::new("Test Channel").unwrap(),
                channel_username: Username::new("testchannel").unwrap(),
                text: "Test message about AI".to_string(),
                timestamp: chrono::Utc::now(),
                sender_id: None,
                sender_name: None,
                has_media: false,
                media_type: crate::telegram::types::MediaType::None,
            }],
            total_found: 1,
            search_time_ms: 100,
            query_metadata: QueryMetadata {
                query: "AI".to_string(),
                hours_back: 48,
                channels_searched: 1,
            },
        };
        let expected = expected_result.clone();

        mock_client
            .expect_search_messages()
            .returning(move |_| Ok(expected.clone()));

        let mut mock_limiter = MockRateLimiterTrait::new();
        mock_limiter.expect_acquire().returning(|_| Ok(()));

        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        // When: Search messages
        let request = SearchRequest {
            query: "AI".to_string(),
            channel_id: None,
            hours_back: None,
            limit: None,
        };

        let result = server.search_messages(request).await;

        // Then: Returns search results
        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.total_found, 1);
        assert_eq!(response.messages.len(), 1);
        assert!(response.messages[0].text.contains("AI"));
    }

    #[tokio::test]
    async fn search_messages_empty_query_fails() {
        // Given: Server and empty query
        let mock_client = MockTelegramClientTrait::new();
        let mock_limiter = MockRateLimiterTrait::new();
        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        let request = SearchRequest {
            query: "   ".to_string(), // whitespace only
            channel_id: None,
            hours_back: None,
            limit: None,
        };

        // When: Search messages
        let result = server.search_messages(request).await;

        // Then: Returns error
        assert!(result.is_err());
        if let Err(error_msg) = result {
            assert!(error_msg.contains("cannot be empty"));
        }
    }

    #[tokio::test]
    async fn search_messages_rate_limited() {
        use crate::error::Error;

        // Given: Rate limiter that denies request
        let mock_client = MockTelegramClientTrait::new();

        let mut mock_limiter = MockRateLimiterTrait::new();
        mock_limiter.expect_acquire().returning(|_| {
            Err(Error::RateLimit {
                retry_after_seconds: 5,
            })
        });

        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        let request = SearchRequest {
            query: "test".to_string(),
            channel_id: None,
            hours_back: None,
            limit: None,
        };

        // When: Search messages
        let result = server.search_messages(request).await;

        // Then: Returns rate limit error
        assert!(result.is_err());
        if let Err(error_msg) = result {
            assert!(error_msg.contains("rate limit"));
        }
    }

    #[tokio::test]
    async fn search_messages_with_channel_filter() {
        use crate::telegram::types::{QueryMetadata, SearchResult};

        // Given: Mock client with channel filter
        let mut mock_client = MockTelegramClientTrait::new();
        let expected_result = SearchResult {
            messages: vec![],
            total_found: 0,
            search_time_ms: 50,
            query_metadata: QueryMetadata {
                query: "test".to_string(),
                hours_back: 24,
                channels_searched: 1,
            },
        };
        let expected = expected_result.clone();

        mock_client
            .expect_search_messages()
            .returning(move |params| {
                // Verify channel_id is passed correctly
                assert!(params.channel_id.is_some());
                assert_eq!(params.channel_id.unwrap().get(), 999);
                Ok(expected.clone())
            });

        let mut mock_limiter = MockRateLimiterTrait::new();
        mock_limiter.expect_acquire().returning(|_| Ok(()));

        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        // When: Search with channel filter
        let request = SearchRequest {
            query: "test".to_string(),
            channel_id: Some("999".to_string()),
            hours_back: Some(24),
            limit: Some(50),
        };

        let result = server.search_messages(request).await;

        // Then: Success
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn search_messages_applies_limits() {
        use crate::telegram::types::{QueryMetadata, SearchResult};

        // Given: Mock client that verifies params
        let mut mock_client = MockTelegramClientTrait::new();
        let expected_result = SearchResult {
            messages: vec![],
            total_found: 0,
            search_time_ms: 50,
            query_metadata: QueryMetadata {
                query: "test".to_string(),
                hours_back: 72, // should be capped to MAX_HOURS_BACK
                channels_searched: 0,
            },
        };
        let expected = expected_result.clone();

        mock_client
            .expect_search_messages()
            .returning(move |params| {
                // Verify limits are applied
                assert_eq!(params.hours_back, 72); // MAX_HOURS_BACK
                assert_eq!(params.limit, 100); // MAX_LIMIT
                Ok(expected.clone())
            });

        let mut mock_limiter = MockRateLimiterTrait::new();
        mock_limiter.expect_acquire().returning(|_| Ok(()));

        let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

        // When: Search with values exceeding limits
        let request = SearchRequest {
            query: "test".to_string(),
            channel_id: None,
            hours_back: Some(1000), // exceeds MAX_HOURS_BACK (72)
            limit: Some(500),       // exceeds MAX_LIMIT (100)
        };

        let result = server.search_messages(request).await;

        // Then: Success (limits applied internally)
        assert!(result.is_ok());
    }
}
