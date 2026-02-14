//! Tests for search_messages tool

use crate::mcp::server::McpServer;
use crate::mcp::tools::SearchRequest;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{
    ChannelId, ChannelName, MediaFilter, MediaType, Message, MessageId, QueryMetadata,
    SearchResult, Username,
};
use rmcp::handler::server::wrapper::Parameters;
use std::sync::Arc;

#[tokio::test]
async fn search_messages_returns_results() {
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
            media_type: MediaType::None,
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
        media_filter: None,
    };

    let result = server.search_messages(Parameters(request)).await;

    // Then: Returns search results
    assert!(result.is_ok());
    let response: SearchResult = serde_json::from_str(&result.unwrap()).unwrap();
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
        media_filter: None, // no filter either = error
    };

    // When: Search messages
    let result = server.search_messages(Parameters(request)).await;

    // Then: Returns error (empty query AND no media_filter)
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
        media_filter: None,
    };

    // When: Search messages
    let result = server.search_messages(Parameters(request)).await;

    // Then: Returns rate limit error
    assert!(result.is_err());
    if let Err(error_msg) = result {
        assert!(error_msg.contains("rate limit"));
    }
}

#[tokio::test]
async fn search_messages_with_channel_filter() {
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
        media_filter: None,
    };

    let result = server.search_messages(Parameters(request)).await;

    // Then: Success
    assert!(result.is_ok());
}

#[tokio::test]
async fn search_messages_applies_limits() {
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
        media_filter: None,
    };

    let result = server.search_messages(Parameters(request)).await;

    // Then: Success (limits applied internally)
    assert!(result.is_ok());
}

#[tokio::test]
async fn search_allows_empty_query_with_media_filter() {
    // Given: Mock client returning search results
    let mut mock_client = MockTelegramClientTrait::new();
    let expected_result = SearchResult {
        messages: vec![Message {
            id: MessageId::new(1).unwrap(),
            channel_id: ChannelId::new(123).unwrap(),
            channel_name: ChannelName::new("Test Channel").unwrap(),
            channel_username: Username::new("testchannel").unwrap(),
            text: "".to_string(), // document with no caption
            timestamp: chrono::Utc::now(),
            sender_id: None,
            sender_name: None,
            has_media: true,
            media_type: MediaType::Document,
        }],
        total_found: 1,
        search_time_ms: 100,
        query_metadata: QueryMetadata {
            query: "".to_string(),
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

    // When: Search with empty query but media_filter set
    let request = SearchRequest {
        query: "".to_string(), // empty query is OK
        channel_id: None,
        hours_back: None,
        limit: None,
        media_filter: Some(MediaFilter::Document), // filter by documents
    };

    let result = server.search_messages(Parameters(request)).await;

    // Then: Success (empty query allowed with media_filter)
    assert!(result.is_ok());
    let response: SearchResult = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(response.total_found, 1);
}

#[tokio::test]
async fn search_passes_media_filter_to_params() {
    // Given: Mock client that verifies media_filter is passed
    let mut mock_client = MockTelegramClientTrait::new();
    let expected_result = SearchResult {
        messages: vec![],
        total_found: 0,
        search_time_ms: 50,
        query_metadata: QueryMetadata {
            query: "AI news".to_string(),
            hours_back: 48,
            channels_searched: 1,
        },
    };
    let expected = expected_result.clone();

    mock_client
        .expect_search_messages()
        .returning(move |params| {
            // Verify media_filter is passed correctly
            assert_eq!(params.media_filter, Some(MediaFilter::Photo));
            Ok(expected.clone())
        });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Search with media_filter
    let request = SearchRequest {
        query: "AI news".to_string(),
        channel_id: None,
        hours_back: None,
        limit: None,
        media_filter: Some(MediaFilter::Photo),
    };

    let result = server.search_messages(Parameters(request)).await;

    // Then: Success and media_filter was passed to client
    assert!(result.is_ok());
}
