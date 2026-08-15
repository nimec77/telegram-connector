//! get_recent_messages: core behavior (identifiers, filters, limits, album collapsing).

use crate::mcp::server::McpServer;
use crate::mcp::tools::{GetRecentMessagesRequest, SearchResponse};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{MediaFilter, QueryMetadata, SearchResult};
use crate::test_helpers::{create_test_message, create_test_search_result, permissive_limiter};
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

#[tokio::test]
async fn get_recent_messages_returns_results() {
    // Given: Mock client returning messages
    let mut mock_client = MockTelegramClientTrait::new();
    let expected_result = SearchResult {
        messages: vec![
            create_test_message(1, "Recent message 1", 123),
            create_test_message(2, "Recent message 2", 123),
        ],
        returned: 2,
        has_more: false,
        search_time_ms: 50,
        query_metadata: QueryMetadata {
            query: String::new(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(48),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
            timed_out: false,
            partial: false,
            pages_fetched: 0,
            messages_scanned: 0,
        },
    };
    let expected = expected_result.clone();

    mock_client
        .expect_get_recent_messages()
        .returning(move |_| Ok(expected.clone()));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Get recent messages
    let request = GetRecentMessagesRequest {
        channel_id: Some("123".to_string()),
        ..Default::default()
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns messages
    assert!(result.is_ok());
    let response: SearchResponse = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(response.returned, 2);
    assert_eq!(response.messages.len(), 2);
}

#[tokio::test]
async fn get_recent_messages_missing_channel_id_fails() {
    // Given: Server, and neither channel_id nor channel_ids set. (Wire clients
    // that send a blank/whitespace-only channel_id never reach this point:
    // flexible_opt_string already collapses it to None at the deserialization
    // boundary, so "missing" is the only shape this validation needs to catch.)
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: None,
        ..Default::default()
    };

    // When: Get recent messages
    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Fails with error
    assert!(result.is_err());
    let error = result.err().unwrap();
    assert!(error.contains("required"), "got: {error}");
}

#[tokio::test]
async fn get_recent_messages_with_media_filter() {
    // Given: Mock client with media filter
    let mut mock_client = MockTelegramClientTrait::new();
    let expected_result = SearchResult {
        messages: vec![create_test_message(1, "Photo message", 123)],
        returned: 1,
        has_more: false,
        search_time_ms: 30,
        query_metadata: QueryMetadata {
            query: String::new(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(24),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
            timed_out: false,
            partial: false,
            pages_fetched: 0,
            messages_scanned: 0,
        },
    };
    let expected = expected_result.clone();

    mock_client
        .expect_get_recent_messages()
        .withf(|params| params.media_filter == Some(MediaFilter::Photo))
        .returning(move |_| Ok(expected.clone()));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Get recent messages with photo filter
    let request = GetRecentMessagesRequest {
        channel_id: Some("123".to_string()),
        hours_back: Some(24),
        media_filter: Some(MediaFilter::Photo),
        ..Default::default()
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns filtered results
    assert!(result.is_ok());
    let response: SearchResponse = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(response.messages.len(), 1);
}

#[tokio::test]
async fn get_recent_messages_applies_limits() {
    // Given: Mock client respecting limits
    let mut mock_client = MockTelegramClientTrait::new();
    let expected_result = SearchResult {
        messages: vec![
            create_test_message(1, "Message 1", 123),
            create_test_message(2, "Message 2", 123),
            create_test_message(3, "Message 3", 123),
        ],
        returned: 3,
        has_more: false,
        search_time_ms: 40,
        query_metadata: QueryMetadata {
            query: String::new(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(72),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
            timed_out: false,
            partial: false,
            pages_fetched: 0,
            messages_scanned: 0,
        },
    };
    let expected = expected_result.clone();

    mock_client
        .expect_get_recent_messages()
        .withf(|params| params.limit == 3 && params.hours_back == 72)
        .returning(move |_| Ok(expected.clone()));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Get recent messages with custom limits
    let request = GetRecentMessagesRequest {
        channel_id: Some("123".to_string()),
        hours_back: Some(72),
        limit: Some(3),
        ..Default::default()
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns results with correct limits applied
    assert!(result.is_ok());
    let response: SearchResponse = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(response.messages.len(), 3);
}

#[tokio::test]
async fn get_recent_messages_with_username_passes_identifier_without_pre_resolving() {
    // Given: Mock client. AD-2: the server must NOT pre-resolve the username via
    // get_channel_info — the client owns resolution. No expect_get_channel_info()
    // is set, so any such call makes mockall panic and fails this test.
    let mut mock_client = MockTelegramClientTrait::new();

    // get_recent_messages receives the raw username as the identifier and a None
    // numeric channel_id (the client resolves and derives the id from the peer).
    let expected_result = SearchResult {
        messages: vec![create_test_message(1, "News update", 456)],
        returned: 1,
        has_more: false,
        search_time_ms: 60,
        query_metadata: QueryMetadata {
            query: String::new(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(48),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
            timed_out: false,
            partial: false,
            pages_fetched: 0,
            messages_scanned: 0,
        },
    };
    let expected = expected_result.clone();

    mock_client
        .expect_get_recent_messages()
        .withf(|params| {
            params.channel_identifier.as_deref() == Some("tech_news") && params.channel_id.is_none()
        })
        .returning(move |_| Ok(expected.clone()));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Get recent messages using username
    let request = GetRecentMessagesRequest {
        channel_id: Some("tech_news".to_string()), // username, not numeric ID
        ..Default::default()
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Username is resolved and messages are returned
    assert!(result.is_ok());
    let response: SearchResponse = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(response.messages.len(), 1);
}

#[tokio::test]
async fn get_recent_messages_rate_limited() {
    // Given: Rate limiter that rejects
    let mock_client = MockTelegramClientTrait::new();
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| {
        Err(crate::error::Error::RateLimit {
            retry_after_seconds: 5,
            detail: String::new(),
        })
    });

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: Some("123".to_string()),
        ..Default::default()
    };

    // When: Get recent messages when rate limited
    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns rate limit error
    assert!(result.is_err());
    let error = result.err().unwrap();
    assert!(error.contains("rate limit"));
}

#[tokio::test]
async fn collapse_albums_flag_reaches_params() {
    // collapse_albums: Some(false) must reach HistoryParams.collapse_albums == false.
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_get_recent_messages()
        .withf(|p| !p.collapse_albums)
        .returning(move |_| Ok(create_test_search_result(vec![], "", 0)));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: Some("123".to_string()),
        collapse_albums: Some(false),
        ..Default::default()
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_ok(), "got {:?}", result.err());
}

#[tokio::test]
async fn collapse_albums_defaults_to_true() {
    // Field left None: HistoryParams.collapse_albums must default true.
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_get_recent_messages()
        .withf(|p| p.collapse_albums)
        .returning(move |_| Ok(create_test_search_result(vec![], "", 0)));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: Some("123".to_string()),
        ..Default::default()
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_ok(), "got {:?}", result.err());
}
