//! search_messages: core behavior (queries, filters, limits, rate limiting).

use crate::mcp::server::McpServer;
use crate::mcp::tools::{SearchRequest, SearchResponse};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{
    ChannelId, ChannelIdentity, ChannelName, MediaFilter, MediaType, Message, MessageId,
    QueryMetadata, SearchResult, Username,
};
use crate::test_helpers::{create_test_search_result, permissive_limiter};
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
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
            channel_username: Some(Username::new("testchannel").unwrap()),
            text: "Test message about AI".to_string(),
            timestamp: chrono::Utc::now(),
            sender_id: None,
            sender_name: None,
            has_media: false,
            media_type: MediaType::None,
            forwarded_from: None,
            link_preview: None,
            views: None,
            forwards: None,
            reply_to_message_id: None,
            video_info: None,
            audio_info: None,
            document_info: None,
            poll_info: None,
            grouped_id: None,
            link: "https://t.me/testchannel/1".to_string(),
            reactions: None,
            reactions_total: None,
            album: None,
        }],
        returned: 1,
        has_more: false,
        search_time_ms: 100,
        query_metadata: QueryMetadata {
            query: "AI".to_string(),
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
        .expect_search_messages()
        .returning(move |_| Ok(expected.clone()));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Search messages
    let request = SearchRequest {
        query: "AI".to_string(),
        ..Default::default()
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns search results
    assert!(result.is_ok());
    let response: SearchResponse = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(response.returned, 1);
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
        query: "   ".to_string(), // whitespace only; no media_filter either = error
        ..Default::default()
    };

    // When: Search messages
    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

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
            detail: String::new(),
        })
    });

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "test".to_string(),
        ..Default::default()
    };

    // When: Search messages
    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

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
        returned: 0,
        has_more: false,
        search_time_ms: 50,
        query_metadata: QueryMetadata {
            query: "test".to_string(),
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
        .expect_search_messages()
        .returning(move |params| {
            // Verify channel_id is passed correctly
            assert!(params.channel_id.is_some());
            assert_eq!(params.channel_id.unwrap().get(), 999);
            Ok(expected.clone())
        });

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Search with channel filter
    let request = SearchRequest {
        query: "test".to_string(),
        channel_id: Some("999".to_string()),
        hours_back: Some(24),
        limit: Some(50),
        ..Default::default()
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Success
    assert!(result.is_ok());
}

#[tokio::test]
async fn search_messages_applies_limits() {
    // Given: Mock client that verifies params
    let mut mock_client = MockTelegramClientTrait::new();
    let expected_result = SearchResult {
        messages: vec![],
        returned: 0,
        has_more: false,
        search_time_ms: 50,
        query_metadata: QueryMetadata {
            query: "test".to_string(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(72), // capped to MAX_HOURS_BACK
            window_to: None,
            channels_scanned: Some(0),
            channels_in_results: 0,
            timed_out: false,
            partial: false,
            pages_fetched: 0,
            messages_scanned: 0,
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

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Search with values exceeding limits
    let request = SearchRequest {
        query: "test".to_string(),
        hours_back: Some(1000), // exceeds MAX_HOURS_BACK (72)
        limit: Some(500),       // exceeds MAX_LIMIT (100)
        ..Default::default()
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

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
            channel_username: Some(Username::new("testchannel").unwrap()),
            text: "".to_string(), // document with no caption
            timestamp: chrono::Utc::now(),
            sender_id: None,
            sender_name: None,
            has_media: true,
            media_type: MediaType::Document,
            forwarded_from: None,
            link_preview: None,
            views: None,
            forwards: None,
            reply_to_message_id: None,
            video_info: None,
            audio_info: None,
            document_info: None,
            poll_info: None,
            grouped_id: None,
            link: "https://t.me/testchannel/1".to_string(),
            reactions: None,
            reactions_total: None,
            album: None,
        }],
        returned: 1,
        has_more: false,
        search_time_ms: 100,
        query_metadata: QueryMetadata {
            query: "".to_string(),
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
        .expect_search_messages()
        .returning(move |_| Ok(expected.clone()));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Search with empty query but media_filter set
    let request = SearchRequest {
        query: "".to_string(),                     // empty query is OK
        media_filter: Some(MediaFilter::Document), // filter by documents
        ..Default::default()
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Success (empty query allowed with media_filter)
    assert!(result.is_ok());
    let response: SearchResponse = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(response.returned, 1);
}

#[tokio::test]
async fn search_passes_media_filter_to_params() {
    // Given: Mock client that verifies media_filter is passed
    let mut mock_client = MockTelegramClientTrait::new();
    let expected_result = SearchResult {
        messages: vec![],
        returned: 0,
        has_more: false,
        search_time_ms: 50,
        query_metadata: QueryMetadata {
            query: "AI news".to_string(),
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
        .expect_search_messages()
        .returning(move |params| {
            // Verify media_filter is passed correctly
            assert_eq!(params.media_filter, Some(MediaFilter::Photo));
            Ok(expected.clone())
        });

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Search with media_filter
    let request = SearchRequest {
        query: "AI news".to_string(),
        media_filter: Some(MediaFilter::Photo),
        ..Default::default()
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Success and media_filter was passed to client
    assert!(result.is_ok());
}

#[tokio::test]
async fn search_accepts_username_channel_id() {
    // §1.3 restoration: search_messages must accept a username channel_id,
    // not just a numeric one. The username is resolved to a ChannelId via
    // one cheap resolve_channel_identity call before the search itself.
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_resolve_channel_identity()
        .withf(|r| r == "@swodki")
        .returning(|_| {
            Ok(ChannelIdentity {
                id: ChannelId::new(1144180066).expect("id"),
                username: Some("swodki".into()),
            })
        });
    telegram
        .expect_search_messages()
        .withf(|p| p.channel_id.map(|c| c.get()) == Some(1144180066))
        .returning(|_| Ok(create_test_search_result(vec![], "тест", 0)));
    let limiter = permissive_limiter();
    let server = McpServer::new(Arc::new(telegram), Arc::new(limiter));

    let request = SearchRequest {
        query: "тест".to_string(),
        channel_id: Some("@swodki".to_string()),
        ..Default::default()
    };
    server
        .search_messages_impl(request)
        .await
        .expect("username channel_id must work (§1.3)");
}
