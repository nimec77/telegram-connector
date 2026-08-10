//! Tests for search_public_channels tool

use crate::mcp::server::McpServer;
use crate::mcp::tools::{ChannelsResponse, SearchPublicChannelsRequest};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::Username;
use crate::telegram::{Channel, ChannelId, ChannelName};
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

/// Helper to create test channel (mirrors `channels.rs`'s local helper).
fn create_test_channel(id: i64, name: &str) -> Channel {
    Channel {
        id: ChannelId::new(id).unwrap(),
        name: ChannelName::new(name).unwrap(),
        username: Username::new("testchannel").unwrap(),
        description: Some("Test channel".to_string()),
        member_count: Some(1000),
        is_verified: false,
        is_public: true,
        is_subscribed: false,
        last_message_date: None,
    }
}

#[tokio::test]
async fn search_public_channels_returns_channels_response() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_search_public_channels()
        .withf(|q, limit| q == "rust" && *limit == 10)
        .return_once(|_, _| Ok(vec![create_test_channel(42, "Rust News")]));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter
        .expect_acquire()
        .with(mockall::predicate::eq(1))
        .returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchPublicChannelsRequest {
        query: "rust".to_string(),
        limit: None,
    };

    let result = server
        .search_public_channels(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_ok());
    let response: ChannelsResponse = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(response.total, 1);
    assert!(!response.has_more);
    assert!(!response.channels[0].is_subscribed);
}

#[tokio::test]
async fn search_public_channels_rejects_empty_query() {
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchPublicChannelsRequest {
        query: "".to_string(),
        limit: None,
    };

    let result = server
        .search_public_channels(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_err());
    if let Err(error_msg) = result {
        assert!(error_msg.contains("query cannot be empty"));
    }
}
