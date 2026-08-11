//! Tests for get_recent_messages tool

use crate::mcp::server::McpServer;
use crate::mcp::tools::GetRecentMessagesRequest;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{
    ChannelId, ChannelName, MediaFilter, MediaType, Message, MessageId, QueryMetadata,
    SearchResult, Username,
};
use crate::test_helpers::create_test_search_result;
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

fn create_test_message(id: i64, text: &str, channel_id: i64) -> Message {
    Message {
        id: MessageId::new(id).unwrap(),
        channel_id: ChannelId::new(channel_id).unwrap(),
        channel_name: ChannelName::new("Test Channel").unwrap(),
        channel_username: Some(Username::new("testchannel").unwrap()),
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
        from_date: None,
        to_date: None,
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
        from_date: None,
        to_date: None,
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
        from_date: None,
        to_date: None,
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
        from_date: None,
        to_date: None,
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
async fn get_recent_messages_with_username_passes_identifier_without_pre_resolving() {
    // Given: Mock client. AD-2: the server must NOT pre-resolve the username via
    // get_channel_info — the client owns resolution. No expect_get_channel_info()
    // is set, so any such call makes mockall panic and fails this test.
    let mut mock_client = MockTelegramClientTrait::new();

    // get_recent_messages receives the raw username as the identifier and a None
    // numeric channel_id (the client resolves and derives the id from the peer).
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
        .withf(|params| {
            params.channel_identifier.as_deref() == Some("tech_news") && params.channel_id.is_none()
        })
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
        from_date: None,
        to_date: None,
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
        from_date: None,
        to_date: None,
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
async fn get_recent_messages_passes_date_range_to_client() {
    // Given: Mock client that verifies from_date/to_date are parsed and passed through
    let mut mock_client = MockTelegramClientTrait::new();

    mock_client
        .expect_get_recent_messages()
        .withf(|p| {
            p.from_date == Some("2026-08-01T00:00:00Z".parse().unwrap())
                && p.to_date == Some("2026-08-05T00:00:00Z".parse().unwrap())
        })
        .returning(move |_| Ok(create_test_search_result(vec![], "", 1)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Get recent messages with an explicit date range
    let request = GetRecentMessagesRequest {
        channel_id: "123".to_string(),
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: Some("2026-08-01T00:00:00Z".to_string()),
        to_date: Some("2026-08-05T00:00:00Z".to_string()),
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Success, and the client received the parsed date range
    assert!(result.is_ok());
}

#[tokio::test]
async fn get_recent_messages_accepts_equal_from_and_to_date() {
    // Both bounds are documented as inclusive: from_date == to_date is a
    // single-instant window, not an inverted range.
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_get_recent_messages()
        .withf(|p| {
            let instant: chrono::DateTime<chrono::Utc> = "2026-08-01T00:00:00Z".parse().unwrap();
            p.from_date == Some(instant) && p.to_date == Some(instant)
        })
        .returning(move |_| Ok(create_test_search_result(vec![], "", 1)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: "123".to_string(),
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: Some("2026-08-01T00:00:00Z".to_string()),
        to_date: Some("2026-08-01T00:00:00Z".to_string()),
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(
        result.is_ok(),
        "equal inclusive bounds must be accepted, got {:?}",
        result.err()
    );
}

#[tokio::test]
async fn get_recent_messages_rejects_inverted_range() {
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: "123".to_string(),
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: Some("2026-08-05T00:00:00Z".to_string()),
        to_date: Some("2026-08-01T00:00:00Z".to_string()),
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .contains("from_date must be earlier than to_date")
    );
}

#[tokio::test]
async fn get_recent_messages_rejects_to_date_older_than_hours_back_window() {
    // to_date alone, older than `now - hours_back`, is a structurally empty
    // window: the history walk would silently return []. Reject it instead,
    // without spending a client call or a rate-limiter token.
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let long_ago = chrono::Utc::now() - chrono::Duration::days(30);

    let request = GetRecentMessagesRequest {
        channel_id: "123".to_string(),
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: None,
        to_date: Some(long_ago.to_rfc3339()),
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("from_date"),
        "error must tell the caller to supply from_date, got: {err}"
    );
}

#[tokio::test]
async fn get_recent_messages_accepts_to_date_inside_hours_back_window() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_get_recent_messages()
        .returning(move |_| Ok(create_test_search_result(vec![], "", 1)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let recent = chrono::Utc::now() - chrono::Duration::hours(1);

    let request = GetRecentMessagesRequest {
        channel_id: "123".to_string(),
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: None,
        to_date: Some(recent.to_rfc3339()),
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_ok(), "got {:?}", result.err());
}

#[tokio::test]
async fn get_recent_messages_rejects_blank_to_date() {
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: "123".to_string(),
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: None,
        to_date: Some("".to_string()),
    };

    let result = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid to_date"));
}
