//! Tests for search_public_channels tool

use crate::mcp::server::McpServer;
use crate::mcp::tools::{ChannelsResponse, SearchPublicChannelsRequest};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::test_helpers::{create_test_channel_named, permissive_limiter};
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

#[tokio::test]
async fn search_public_channels_returns_channels_response() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_search_public_channels()
        .withf(|q, limit| q == "rust" && *limit == 10)
        .return_once(|_, _| Ok(vec![create_test_channel_named(42, "Rust News", false)]));

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
    assert_eq!(response.returned, 1);
    assert!(
        response.total.is_none(),
        "contacts.Search has no global match count"
    );
    assert_eq!(
        response.has_more,
        Some(false),
        "1 result under the limit of 10 ⇒ known, not full"
    );
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

#[tokio::test]
async fn search_public_channels_truncates_to_requested_limit() {
    // `contacts.search` bounds only its global `results` set; the `chats` it
    // returns also carries the caller's own dialog matches, so the converted list
    // can overshoot. The response must never exceed the requested limit, and
    // `total` must match what is actually returned.
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_search_public_channels()
        .withf(|q, limit| q == "rust" && *limit == 3)
        .return_once(|_, _| {
            Ok((0..25)
                .map(|i| create_test_channel_named(1000 + i, &format!("Channel {i}"), false))
                .collect())
        });

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchPublicChannelsRequest {
        query: "rust".to_string(),
        limit: Some(3),
    };

    let result = server
        .search_public_channels(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .expect("tool call should succeed");

    let response: ChannelsResponse = serde_json::from_str(&result).unwrap();
    assert_eq!(
        response.channels.len(),
        3,
        "must not exceed requested limit"
    );
    assert_eq!(
        response.returned, 3,
        "returned must match the truncated list"
    );
    assert!(
        response.total.is_none(),
        "contacts.Search has no global match count"
    );
    assert!(
        response.has_more.is_none(),
        "a full page says nothing about what lies beyond it (D10)"
    );
}

#[tokio::test]
async fn discovery_has_more_is_unknown_at_limit() {
    // A full page (returned == limit) means the caller cannot tell whether more
    // results exist behind it; has_more must be null, not falsely `false` (D10).
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_search_public_channels()
        .withf(|q, limit| q == "rust" && *limit == 1)
        .return_once(|_, _| Ok(vec![create_test_channel_named(42, "Rust News", false)]));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchPublicChannelsRequest {
        query: "rust".to_string(),
        limit: Some(1),
    };

    let result = server
        .search_public_channels(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .expect("tool call should succeed");

    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["returned"], 1);
    assert!(
        json["total"].is_null(),
        "contacts.Search has no global match count"
    );
    assert!(
        json["has_more"].is_null(),
        "full page ⇒ unknown, not false (D10)"
    );
}

#[tokio::test]
async fn discovery_has_more_false_under_limit() {
    // Fewer results than the requested limit came back: that genuinely means no
    // more exist, so has_more is a known `false`, not null.
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_search_public_channels()
        .withf(|q, limit| q == "rust" && *limit == 10)
        .return_once(|_, _| Ok(vec![create_test_channel_named(42, "Rust News", false)]));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchPublicChannelsRequest {
        query: "rust".to_string(),
        limit: Some(10),
    };

    let result = server
        .search_public_channels(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .expect("tool call should succeed");

    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["has_more"], false);
}
