use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::client::TelegramClientTrait;
use rmcp::model::{Implementation, InitializeResult, ProtocolVersion};
use rmcp::{ServerHandler, ServiceExt};
use std::sync::Arc;

pub struct McpServer<T: TelegramClientTrait, R: RateLimiterTrait> {
    // These fields will be used in Phase 11 when implementing MCP tools
    #[allow(dead_code)]
    telegram_client: Arc<T>,
    #[allow(dead_code)]
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
}
