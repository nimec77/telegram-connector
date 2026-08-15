//! get_recent_messages: cursors, byte budget, and response shaping.

use crate::mcp::server::McpServer;
use crate::mcp::tools::{GetRecentMessagesRequest, ResponseFormat};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{Message, QueryMetadata, SearchResult};
use crate::test_helpers::{create_test_message, create_test_search_result, permissive_limiter};
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

#[tokio::test]
async fn get_recent_messages_emits_next_cursor_when_limit_truncates() {
    // Given: client reports has_more (limit refused a qualifying message)
    let mut mock_client = MockTelegramClientTrait::new();
    let result = SearchResult {
        messages: vec![
            create_test_message(20, "newest", 123),
            create_test_message(10, "oldest included", 123),
        ],
        returned: 2,
        has_more: true,
        search_time_ms: 5,
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
    mock_client
        .expect_get_recent_messages()
        .withf(|p| p.before_id.is_none() && p.after_id.is_none())
        .returning(move |_| Ok(result.clone()));
    let mock_limiter = permissive_limiter();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: Some("123".to_string()),
        ..Default::default()
    };
    let out = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .expect("tool ok");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");

    // Then: has_more is surfaced and the cursor points at the oldest included id
    assert_eq!(v["has_more"], serde_json::Value::Bool(true));
    assert_eq!(v["next_cursor"]["before_id"], serde_json::json!(10));
}

#[tokio::test]
async fn get_recent_messages_passes_cursor_params_to_client() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_get_recent_messages()
        .withf(|p| {
            p.before_id.map(|id| id.get()) == Some(610_119)
                && p.after_id.map(|id| id.get()) == Some(600_000)
        })
        .returning(move |_| Ok(create_test_search_result(vec![], "", 1)));
    let mock_limiter = permissive_limiter();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: Some("123".to_string()),
        before_id: Some(610_119),
        after_id: Some(600_000),
        ..Default::default()
    };
    let out = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;
    assert!(out.is_ok());
}

#[tokio::test]
async fn get_recent_messages_rejects_inverted_cursor_range() {
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: Some("123".to_string()),
        before_id: Some(100),
        after_id: Some(100),
        ..Default::default()
    };
    let out = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;
    let err = out.expect_err("must reject");
    assert!(
        err.contains("before_id"),
        "error should name the field: {err}"
    );
}

#[tokio::test]
async fn get_recent_messages_truncates_long_text() {
    let mut mock_client = MockTelegramClientTrait::new();
    let long_text = "б".repeat(3000);
    let msg = create_test_message(1, &long_text, 123);
    let result = SearchResult {
        messages: vec![msg],
        returned: 1,
        has_more: false,
        search_time_ms: 5,
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
    mock_client
        .expect_get_recent_messages()
        .returning(move |_| Ok(result.clone()));
    let mock_limiter = permissive_limiter();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: Some("123".to_string()),
        // max_text_length left at default (2000)
        ..Default::default()
    };
    let out = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .expect("ok");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    let m = &v["messages"][0];
    assert_eq!(m["text"].as_str().expect("text").chars().count(), 2000);
    assert_eq!(m["text_truncated"], serde_json::Value::Bool(true));
    assert_eq!(m["text_full_length"], serde_json::json!(3000));
}

#[tokio::test]
async fn get_recent_messages_compact_hoists_channel_header() {
    let mut mock_client = MockTelegramClientTrait::new();
    let result = SearchResult {
        messages: vec![
            create_test_message(2, "второе", 123),
            create_test_message(1, "первое", 123),
        ],
        returned: 2,
        has_more: false,
        search_time_ms: 5,
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
    mock_client
        .expect_get_recent_messages()
        .returning(move |_| Ok(result.clone()));
    let mock_limiter = permissive_limiter();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: Some("123".to_string()),
        format: Some(ResponseFormat::Compact),
        ..Default::default()
    };
    let out = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .expect("ok");
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");

    // Then: one response-level header, no per-message channel fields
    assert_eq!(v["channel"]["id"], serde_json::json!(123));
    assert_eq!(v["channel"]["name"], serde_json::json!("Test Channel"));
    assert_eq!(v["channel"]["username"], serde_json::json!("testchannel"));
    let m = &v["messages"][0];
    assert!(m.get("channel_id").is_none());
    assert!(m.get("channel_name").is_none());
    assert!(m.get("channel_username").is_none());
}

#[tokio::test]
async fn get_recent_messages_oversized_page_stays_under_budget() {
    // Given: 100 messages × ~900 chars ≈ 90 KB serialized (the audit's B4 case)
    let mut mock_client = MockTelegramClientTrait::new();
    let messages: Vec<Message> = (1..=100)
        .rev()
        .map(|i| create_test_message(i, &"д".repeat(900), 123))
        .collect();
    let result = SearchResult {
        returned: messages.len() as u64,
        messages,
        has_more: false,
        search_time_ms: 5,
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
    mock_client
        .expect_get_recent_messages()
        .returning(move |_| Ok(result.clone()));
    let mock_limiter = permissive_limiter();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetRecentMessagesRequest {
        channel_id: Some("123".to_string()),
        limit: Some(100),
        ..Default::default()
    };
    let out = server
        .get_recent_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .expect("ok");
    assert!(
        out.len() <= 40_000,
        "default budget must cap the page, got {}",
        out.len()
    );
    let v: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["has_more"], serde_json::Value::Bool(true));
    assert!(
        v["next_cursor"]["before_id"].is_i64(),
        "cursor must be present"
    );
}
