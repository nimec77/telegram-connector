use crate::link::MessageLink;
use crate::mcp::tools::{
    ChannelsResponse, GenerateLinkRequest, GetChannelInfoRequest, GetChannelsRequest,
    MessageLinkResponse, StatusResponse,
};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::client::TelegramClientTrait;
use crate::telegram::types::{ChannelId, MessageId};
use crate::telegram::Channel;
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
}
