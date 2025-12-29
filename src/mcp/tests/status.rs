//! Tests for check_mcp_status tool

use crate::mcp::server::McpServer;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use std::sync::Arc;

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
