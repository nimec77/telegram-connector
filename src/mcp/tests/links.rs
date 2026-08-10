//! Tests for generate_message_link and open_message_in_telegram tools

use crate::error::Error;
use crate::mcp::server::McpServer;
use crate::mcp::tools::{
    GenerateLinkRequest, MessageLinkResponse, OpenMessageRequest, OpenMessageResponse,
};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{ChannelId, ChannelIdentity};
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

fn identity(id: i64, username: Option<&str>) -> ChannelIdentity {
    ChannelIdentity {
        id: ChannelId::new(id).expect("valid test id"),
        username: username.map(str::to_string),
    }
}

// ============================================================================
// generate_message_link tests
// ============================================================================

#[tokio::test]
async fn generate_message_link_public_channel_returns_public_forms() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .return_once(|_| Ok(identity(1144180066, Some("swodki"))));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GenerateLinkRequest {
        channel_id: "1144180066".to_string(),
        message_id: 610121,
        include_tg_protocol: None, // defaults to true
    };

    let result = server
        .generate_message_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    let response: MessageLinkResponse =
        serde_json::from_str(&result.expect("tool must succeed")).expect("valid json");
    assert_eq!(response.channel_id, "1144180066");
    assert_eq!(response.message_id, 610121);
    assert_eq!(response.https_link, "https://t.me/swodki/610121");
    assert_eq!(
        response.tg_protocol_link.as_deref(),
        Some("tg://resolve?domain=swodki&post=610121")
    );
    assert_eq!(response.internal_link, "https://t.me/c/1144180066/610121");
    assert!(response.is_public);
}

#[tokio::test]
async fn generate_message_link_accepts_username_input() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .withf(|channel_ref| channel_ref == "swodki")
        .return_once(|_| Ok(identity(1144180066, Some("swodki"))));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GenerateLinkRequest {
        channel_id: "swodki".to_string(),
        message_id: 610121,
        include_tg_protocol: None,
    };

    let result = server
        .generate_message_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    let response: MessageLinkResponse =
        serde_json::from_str(&result.expect("username input must work")).expect("valid json");
    assert_eq!(response.https_link, "https://t.me/swodki/610121");
    assert_eq!(response.channel_id, "1144180066"); // canonical numeric id
}

#[tokio::test]
async fn generate_message_link_private_chat_returns_internal_forms() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .return_once(|_| Ok(identity(521440428, None)));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GenerateLinkRequest {
        channel_id: "521440428".to_string(),
        message_id: 7,
        include_tg_protocol: None,
    };

    let result = server
        .generate_message_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    let response: MessageLinkResponse =
        serde_json::from_str(&result.expect("tool must succeed")).expect("valid json");
    assert_eq!(response.https_link, "https://t.me/c/521440428/7");
    assert_eq!(
        response.tg_protocol_link.as_deref(),
        Some("tg://privatepost?channel=521440428&post=7")
    );
    assert!(!response.is_public);
}

#[tokio::test]
async fn generate_message_link_without_tg_protocol() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .return_once(|_| Ok(identity(999, None)));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GenerateLinkRequest {
        channel_id: "999".to_string(),
        message_id: 111,
        include_tg_protocol: Some(false),
    };

    let result = server
        .generate_message_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    let response: MessageLinkResponse =
        serde_json::from_str(&result.expect("tool must succeed")).expect("valid json");
    assert_eq!(response.https_link, "https://t.me/c/999/111");
    assert!(response.tg_protocol_link.is_none());
}

#[tokio::test]
async fn generate_message_link_unknown_channel_errors() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .return_once(|_| Err(Error::InvalidInput("Channel not found: @nope".to_string())));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GenerateLinkRequest {
        channel_id: "@nope".to_string(),
        message_id: 42,
        include_tg_protocol: None,
    };

    let result = server
        .generate_message_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_err());
    assert!(
        result
            .expect_err("must be error")
            .contains("Channel not found")
    );
}

// ============================================================================
// open_message_in_telegram tests
// ============================================================================

#[tokio::test]
async fn open_message_in_telegram_unknown_channel_errors() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .return_once(|_| {
            Err(Error::InvalidInput(
                "Channel not found: invalid".to_string(),
            ))
        });
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = OpenMessageRequest {
        channel_id: "invalid".to_string(),
        message_id: 42,
        use_tg_protocol: None,
    };

    let result = server
        .open_message_in_telegram(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_err());
    assert!(
        result
            .expect_err("must be error")
            .contains("Channel not found")
    );
}

#[tokio::test]
async fn open_message_in_telegram_uses_public_tg_form_by_default() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .return_once(|_| Ok(identity(1144180066, Some("swodki"))));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = OpenMessageRequest {
        channel_id: "swodki".to_string(),
        message_id: 42,
        use_tg_protocol: None, // defaults to true
    };

    let result = server
        .open_message_in_telegram(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    let response: OpenMessageResponse =
        serde_json::from_str(&result.expect("tool must succeed")).expect("valid json");
    assert_eq!(response.link_used, "tg://resolve?domain=swodki&post=42");
}

#[tokio::test]
async fn open_message_in_telegram_uses_https_when_requested() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .return_once(|_| Ok(identity(123456, None)));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = OpenMessageRequest {
        channel_id: "123456".to_string(),
        message_id: 42,
        use_tg_protocol: Some(false),
    };

    let result = server
        .open_message_in_telegram(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    let response: OpenMessageResponse =
        serde_json::from_str(&result.expect("tool must succeed")).expect("valid json");
    assert_eq!(response.link_used, "https://t.me/c/123456/42");
}
