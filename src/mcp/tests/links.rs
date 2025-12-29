//! Tests for generate_message_link and open_message_in_telegram tools

use crate::mcp::server::McpServer;
use crate::mcp::tools::{GenerateLinkRequest, OpenMessageRequest};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use rmcp::handler::server::wrapper::Parameters;
use std::sync::Arc;

// ============================================================================
// generate_message_link tests
// ============================================================================

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
    let result = server.generate_message_link(Parameters(request)).await;

    // Then: Returns both link formats
    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.channel_id, "123456789");
    assert_eq!(response.message_id, 42);
    assert_eq!(response.https_link, "https://t.me/c/123456789/42?single");
    assert!(response.tg_protocol_link.is_some());
    assert_eq!(
        response.tg_protocol_link.unwrap(),
        "tg://privatepost?channel=123456789&post=42&single"
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
    let result = server.generate_message_link(Parameters(request)).await;

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
    let result = server.generate_message_link(Parameters(request)).await;

    // Then: Returns error
    assert!(result.is_err());
    if let Err(error_msg) = result {
        assert!(error_msg.contains("Invalid channel_id"));
    }
}

// ============================================================================
// open_message_in_telegram tests
// ============================================================================

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
    let result = server.open_message_in_telegram(Parameters(request)).await;

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
    let result = server.open_message_in_telegram(Parameters(request)).await;

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
    let result = server.open_message_in_telegram(Parameters(request)).await;

    // Then: Returns response with https:// link
    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(response.link_used.starts_with("https://"));
}
