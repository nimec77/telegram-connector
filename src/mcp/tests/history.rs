//! Tests for get_recent_messages tool

use crate::mcp::server::McpServer;
use crate::mcp::tools::GetRecentMessagesRequest;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{
    Channel, ChannelId, ChannelName, MediaFilter, MediaType, Message, MessageId, QueryMetadata,
    SearchResult, Username,
};
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

fn create_test_message(id: i64, text: &str, channel_id: i64) -> Message {
    Message {
        id: MessageId::new(id).unwrap(),
        channel_id: ChannelId::new(channel_id).unwrap(),
        channel_name: ChannelName::new("Test Channel").unwrap(),
        channel_username: Username::new("testchannel").unwrap(),
        text: text.to_string(),
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
    }
}

fn create_test_channel(id: i64, username: &str) -> Channel {
    Channel {
        id: ChannelId::new(id).unwrap(),
        name: ChannelName::new("Test Channel").unwrap(),
        username: Username::new(username).unwrap(),
        description: None,
        member_count: 1000,
        is_verified: false,
        is_public: true,
        is_subscribed: true,
        last_message_date: None,
    }
}

#[tokio::test]
async fn get_recent_messages_returns_results() {
    // Given: Mock client returning messages
    let mut mock_client = MockTelegramClientTrait::new();
    let expected_result = SearchResult {
        messages: vec![
            create_test_message(1, "Recent message 1", 123),
            create_test_message(2, "Recent message 2", 123),
        ],
        total_found: 2,
        search_time_ms: 50,
        query_metadata: QueryMetadata {
            query: String::new(),
            hours_back: 48,
            channels_searched: 1,
        },
    };
    let expected = expected_result.clone();

    mock_client
        .expect_get_recent_messages()
        .returning(move |_| Ok(expected.clone()));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Get recent messages
    let request = GetRecentMessagesRequest {
        channel_id: "123".to_string(),
        hours_back: None,
        limit: None,
        media_filter: None,
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns messages
    assert!(result.is_ok());
    let response: SearchResult = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(response.total_found, 2);
    assert_eq!(response.messages.len(), 2);
}

#[tokio::test]
async fn get_recent_messages_empty_channel_id_fails() {
    // Given: Server and empty channel_id
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: "   ".to_string(), // whitespace only
        hours_back: None,
        limit: None,
        media_filter: None,
    };

    // When: Get recent messages
    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Fails with error
    assert!(result.is_err());
    let error = result.err().unwrap();
    assert!(error.contains("channel_id is required"));
}

#[tokio::test]
async fn get_recent_messages_with_media_filter() {
    // Given: Mock client with media filter
    let mut mock_client = MockTelegramClientTrait::new();
    let expected_result = SearchResult {
        messages: vec![create_test_message(1, "Photo message", 123)],
        total_found: 1,
        search_time_ms: 30,
        query_metadata: QueryMetadata {
            query: String::new(),
            hours_back: 24,
            channels_searched: 1,
        },
    };
    let expected = expected_result.clone();

    mock_client
        .expect_get_recent_messages()
        .withf(|params| params.media_filter == Some(MediaFilter::Photo))
        .returning(move |_| Ok(expected.clone()));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Get recent messages with photo filter
    let request = GetRecentMessagesRequest {
        channel_id: "123".to_string(),
        hours_back: Some(24),
        limit: None,
        media_filter: Some(MediaFilter::Photo),
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns filtered results
    assert!(result.is_ok());
    let response: SearchResult = serde_json::from_str(&result.unwrap()).unwrap();
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
        total_found: 3,
        search_time_ms: 40,
        query_metadata: QueryMetadata {
            query: String::new(),
            hours_back: 72,
            channels_searched: 1,
        },
    };
    let expected = expected_result.clone();

    mock_client
        .expect_get_recent_messages()
        .withf(|params| params.limit == 3 && params.hours_back == 72)
        .returning(move |_| Ok(expected.clone()));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Get recent messages with custom limits
    let request = GetRecentMessagesRequest {
        channel_id: "123".to_string(),
        hours_back: Some(72),
        limit: Some(3),
        media_filter: None,
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns results with correct limits applied
    assert!(result.is_ok());
    let response: SearchResult = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(response.messages.len(), 3);
}

#[tokio::test]
async fn get_recent_messages_with_username_resolves_channel() {
    // Given: Mock client that resolves username to channel
    let mut mock_client = MockTelegramClientTrait::new();

    // First, get_channel_info is called to resolve username
    mock_client
        .expect_get_channel_info()
        .withf(|id| id == "tech_news")
        .returning(|_| Ok(create_test_channel(456, "tech_news")));

    // Then, get_recent_messages is called with resolved channel_id
    let expected_result = SearchResult {
        messages: vec![create_test_message(1, "News update", 456)],
        total_found: 1,
        search_time_ms: 60,
        query_metadata: QueryMetadata {
            query: String::new(),
            hours_back: 48,
            channels_searched: 1,
        },
    };
    let expected = expected_result.clone();

    mock_client
        .expect_get_recent_messages()
        .withf(|params| params.channel_id.get() == 456)
        .returning(move |_| Ok(expected.clone()));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Get recent messages using username
    let request = GetRecentMessagesRequest {
        channel_id: "tech_news".to_string(), // username, not numeric ID
        hours_back: None,
        limit: None,
        media_filter: None,
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Username is resolved and messages are returned
    assert!(result.is_ok());
    let response: SearchResult = serde_json::from_str(&result.unwrap()).unwrap();
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
        })
    });

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: "123".to_string(),
        hours_back: None,
        limit: None,
        media_filter: None,
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
