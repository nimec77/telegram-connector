//! Tests for search_messages tool

use crate::mcp::server::McpServer;
use crate::mcp::tools::{ResponseFormat, SearchRequest, SearchResponse};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{
    ChannelId, ChannelIdentity, ChannelName, MediaFilter, MediaType, Message, MessageId,
    QueryMetadata, SearchResult, Username,
};
use crate::test_helpers::{create_test_message, create_test_search_result};
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

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Search messages
    let request = SearchRequest {
        query: "AI".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: None,
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
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
        query: "   ".to_string(), // whitespace only
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None, // no filter either = error
        from_date: None,
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
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
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: None,
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
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

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Search with channel filter
    let request = SearchRequest {
        query: "test".to_string(),
        channel_id: Some("999".to_string()),
        channel_ids: None,
        hours_back: Some(24),
        limit: Some(50),
        media_filter: None,
        from_date: None,
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
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

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Search with values exceeding limits
    let request = SearchRequest {
        query: "test".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: Some(1000), // exceeds MAX_HOURS_BACK (72)
        limit: Some(500),       // exceeds MAX_LIMIT (100)
        media_filter: None,
        from_date: None,
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
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

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Search with empty query but media_filter set
    let request = SearchRequest {
        query: "".to_string(), // empty query is OK
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: Some(MediaFilter::Document), // filter by documents
        from_date: None,
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
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

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Search with media_filter
    let request = SearchRequest {
        query: "AI news".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: Some(MediaFilter::Photo),
        from_date: None,
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Success and media_filter was passed to client
    assert!(result.is_ok());
}

#[tokio::test]
async fn search_messages_serializes_enrichment_fields() {
    use crate::telegram::types::{ForwardInfo, LinkPreview};

    let mut mock_client = MockTelegramClientTrait::new();
    let enriched = SearchResult {
        messages: vec![Message {
            id: MessageId::new(1).unwrap(),
            channel_id: ChannelId::new(123).unwrap(),
            channel_name: ChannelName::new("Test Channel").unwrap(),
            channel_username: Some(Username::new("testchannel").unwrap()),
            text: "forwarded post".to_string(),
            timestamp: chrono::Utc::now(),
            sender_id: None,
            sender_name: None,
            has_media: false,
            media_type: MediaType::None,
            forwarded_from: Some(ForwardInfo {
                channel_id: Some(ChannelId::new(555).unwrap()),
                channel_name: None,
                channel_username: None,
                sender_name: None,
                post_author: None,
                original_date: None,
                original_message_id: Some(MessageId::new(42).unwrap()),
            }),
            link_preview: Some(LinkPreview {
                url: "https://example.com".to_string(),
                site_name: Some("Example".to_string()),
                title: Some("Title".to_string()),
                description: Some("Desc".to_string()),
            }),
            views: Some(999),
            forwards: Some(12),
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
        search_time_ms: 10,
        query_metadata: QueryMetadata {
            query: "x".to_string(),
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

    mock_client
        .expect_search_messages()
        .returning(move |_| Ok(enriched.clone()));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "x".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: None,
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .unwrap();

    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    let msg = &json["messages"][0];
    assert_eq!(msg["views"], 999);
    assert_eq!(msg["forwards"], 12);
    assert_eq!(msg["forwarded_from"]["channel_id"], 555);
    assert_eq!(msg["forwarded_from"]["original_message_id"], 42);
    assert!(msg["forwarded_from"].get("channel_name").is_none());
    assert_eq!(msg["link_preview"]["url"], "https://example.com");
    // Plain-message backward compat: sender_id still present as null.
    assert!(msg["sender_id"].is_null());
}

#[tokio::test]
async fn search_messages_serializes_enriched_forward_without_resolve_calls() {
    let mut mock_client = MockTelegramClientTrait::new();
    let enriched = create_test_search_result(
        vec![
            crate::test_helpers::create_test_message_with_enriched_forward(
                1,
                "переслано",
                123,
                1783384254,
            ),
        ],
        "x",
        0,
    );

    mock_client
        .expect_search_messages()
        .times(1)
        .returning(move |_| Ok(enriched.clone()));
    // No expectation on resolve_channels / resolve_channel_identity /
    // get_channel_info: mockall panics if any of them is called — the
    // zero-resolve guarantee for the enrichment path.

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "x".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: None,
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .unwrap();

    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    let fwd = &json["messages"][0]["forwarded_from"];
    assert_eq!(fwd["channel_id"], 1783384254);
    assert_eq!(fwd["channel_name"], "Военкор");
    assert_eq!(fwd["channel_username"], "voenkor_ru");
    assert_eq!(fwd["post_author"], "И. Петров");
    assert_eq!(fwd["original_message_id"], 1863);
    // Absent enrichment fields stay skipped, not null.
    assert!(fwd.get("sender_name").is_none());
}

#[tokio::test]
async fn search_passes_date_range_to_client() {
    // Given: Mock client that verifies from_date/to_date are parsed and passed through
    let mut mock_client = MockTelegramClientTrait::new();

    mock_client
        .expect_search_messages()
        .withf(|p| {
            p.from_date == Some("2026-08-01T00:00:00Z".parse().unwrap())
                && p.to_date == Some("2026-08-05T00:00:00Z".parse().unwrap())
        })
        .returning(move |_| Ok(create_test_search_result(vec![], "q", 0)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Search with an explicit date range
    let request = SearchRequest {
        query: "q".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: Some("2026-08-01T00:00:00Z".to_string()),
        to_date: Some("2026-08-05T00:00:00Z".to_string()),
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Success, and the client received the parsed date range
    assert!(result.is_ok());
}

#[tokio::test]
async fn search_rejects_invalid_from_date() {
    // Given: Server (no client call is expected - parsing fails before dispatch)
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "q".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: Some("not-a-date".to_string()),
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
    };

    // When: Search with a malformed from_date
    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns a parse error
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid from_date"));
}

#[tokio::test]
async fn search_rejects_inverted_range() {
    // Given: Server (no client call is expected - validation fails before dispatch)
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "q".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: Some("2026-08-05T00:00:00Z".to_string()),
        to_date: Some("2026-08-01T00:00:00Z".to_string()),
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
    };

    // When: Search with from_date after to_date
    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns an inverted-range error
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .contains("from_date must be earlier than to_date")
    );
}

#[tokio::test]
async fn search_accepts_equal_from_and_to_date() {
    // Both bounds are documented as inclusive, so from_date == to_date is a
    // single-instant window, not an inverted range: it must reach the client.
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_search_messages()
        .withf(|p| {
            let instant: chrono::DateTime<chrono::Utc> = "2026-08-01T00:00:00Z".parse().unwrap();
            p.from_date == Some(instant) && p.to_date == Some(instant)
        })
        .returning(move |_| Ok(create_test_search_result(vec![], "q", 0)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "q".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: Some("2026-08-01T00:00:00Z".to_string()),
        to_date: Some("2026-08-01T00:00:00Z".to_string()),
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(
        result.is_ok(),
        "equal inclusive bounds must be accepted, got {:?}",
        result.err()
    );
}

#[tokio::test]
async fn search_rejects_to_date_older_than_hours_back_window() {
    // to_date alone, older than `now - hours_back`, describes a structurally
    // empty window: the search would burn its whole timeout budget for nothing.
    // No client call and no rate-limiter token may be spent.
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let long_ago = chrono::Utc::now() - chrono::Duration::days(30);

    let request = SearchRequest {
        query: "q".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: None,
        to_date: Some(long_ago.to_rfc3339()),
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("from_date"),
        "error must tell the caller to supply from_date, got: {err}"
    );
}

#[tokio::test]
async fn search_accepts_to_date_inside_hours_back_window() {
    // A to_date that still overlaps the hours_back window is a real window.
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_search_messages()
        .returning(move |_| Ok(create_test_search_result(vec![], "q", 0)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let recent = chrono::Utc::now() - chrono::Duration::hours(1);

    let request = SearchRequest {
        query: "q".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: None,
        to_date: Some(recent.to_rfc3339()),
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_ok(), "got {:?}", result.err());
}

#[tokio::test]
async fn search_rejects_blank_from_date() {
    // A present-but-blank date is a caller mistake; silently degrading it to
    // "no filter" would quietly return a different window than asked for.
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "q".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: Some("   ".to_string()),
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid from_date"));
}

#[tokio::test]
async fn search_accepts_padded_from_date() {
    // A valid date with surrounding whitespace parses rather than erroring.
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_search_messages()
        .withf(|p| p.from_date == Some("2026-08-01T00:00:00Z".parse().unwrap()))
        .returning(move |_| Ok(create_test_search_result(vec![], "q", 0)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "q".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: Some(" 2026-08-01T00:00:00Z ".to_string()),
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_ok(), "got {:?}", result.err());
}

#[tokio::test]
async fn search_response_reports_window_and_returned() {
    // The response must report the executed window and an honest "returned"
    // count (page size), not an overloaded total-match count (B6/B7).
    let mut mock_client = MockTelegramClientTrait::new();
    let msg = crate::test_helpers::create_test_message(1, "hi", 123);
    mock_client
        .expect_search_messages()
        .returning(move |_| Ok(create_test_search_result(vec![msg.clone()], "q", 1)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "q".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: None,
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
    };

    let result_string = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .unwrap();

    let json: serde_json::Value = serde_json::from_str(&result_string).expect("valid JSON");
    assert_eq!(json["returned"], 1);
    assert!(
        json.get("total_found").is_none(),
        "total_found must be renamed"
    );
    let meta = &json["query_metadata"];
    assert!(
        meta.get("hours_back").is_none(),
        "hours_back echo removed (B7)"
    );
    assert!(
        meta["window_from"].is_string(),
        "executed window start present"
    );
    assert_eq!(meta["channels_in_results"], 1);
}

#[tokio::test]
async fn search_messages_rejects_cursors_without_channel() {
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "новости".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: None,
        to_date: None,
        collapse_albums: None,
        before_id: Some(100),
        after_id: None,
        max_text_length: None,
        format: None,
    };
    let out = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;
    let err = out.expect_err("must reject");
    assert!(
        err.contains("channel_id"),
        "error should name the remedy: {err}"
    );
}

#[tokio::test]
async fn search_messages_rejects_compact_without_channel() {
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "тест".to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: None,
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: Some(ResponseFormat::Compact),
    };
    let out = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;
    let err = out.expect_err("must reject");
    assert!(
        err.contains("channel_id or channel_ids"),
        "error should name both remedies now that compact supports multi scope: {err}"
    );
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
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().returning(|_| Ok(()));
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

#[tokio::test]
async fn search_messages_shapes_response_end_to_end_for_single_channel() {
    // Given: single-channel search results with more pages available.
    let mut mock_client = MockTelegramClientTrait::new();
    let result = SearchResult {
        messages: vec![
            create_test_message(30, "third", 123),
            create_test_message(20, "second", 123),
            create_test_message(10, "first", 123),
        ],
        returned: 3,
        has_more: true,
        search_time_ms: 5,
        query_metadata: QueryMetadata {
            query: "тест".to_string(),
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
    mock_client
        .expect_search_messages()
        .returning(move |_| Ok(result.clone()));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: channel-scoped search requests the compact format.
    let request = SearchRequest {
        query: "тест".to_string(),
        channel_id: Some("123".to_string()),
        channel_ids: None,
        hours_back: None,
        limit: Some(3),
        media_filter: None,
        from_date: None,
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: Some(ResponseFormat::Compact),
    };

    let out = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .expect("ok");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");

    // Then: the shared shaping pipeline ran end to end - cursor emitted from
    // the last (oldest) message, compact header hoisted, per-message channel
    // fields stripped.
    assert_eq!(v["has_more"], serde_json::Value::Bool(true));
    assert_eq!(v["next_cursor"]["before_id"], serde_json::json!(10));
    assert_eq!(v["channel"]["id"], serde_json::json!(123));
    for m in v["messages"].as_array().expect("messages array") {
        assert!(m.get("channel_id").is_none());
    }
}
