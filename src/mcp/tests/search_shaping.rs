//! search_messages: response shaping — serialization, cursors, compact format, degradation flags.

use crate::mcp::server::McpServer;
use crate::mcp::tools::{ResponseFormat, SearchRequest};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{
    ChannelId, ChannelName, MediaType, Message, MessageId, QueryMetadata, SearchResult, Username,
};
use crate::test_helpers::{create_test_message, create_test_search_result, permissive_limiter};
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

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

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "x".to_string(),
        ..Default::default()
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

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "x".to_string(),
        ..Default::default()
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
async fn search_response_reports_window_and_returned() {
    // The response must report the executed window and an honest "returned"
    // count (page size), not an overloaded total-match count (B6/B7).
    let mut mock_client = MockTelegramClientTrait::new();
    let msg = crate::test_helpers::create_test_message(1, "hi", 123);
    mock_client
        .expect_search_messages()
        .returning(move |_| Ok(create_test_search_result(vec![msg.clone()], "q", 1)));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "q".to_string(),
        ..Default::default()
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
        before_id: Some(100),
        ..Default::default()
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
        format: Some(ResponseFormat::Compact),
        ..Default::default()
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

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: channel-scoped search requests the compact format.
    let request = SearchRequest {
        query: "тест".to_string(),
        channel_id: Some("123".to_string()),
        limit: Some(3),
        format: Some(ResponseFormat::Compact),
        ..Default::default()
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

#[tokio::test]
async fn timed_out_search_returns_partial_results_not_an_error() {
    let mut mock_client = MockTelegramClientTrait::new();
    let mut degraded =
        create_test_search_result(vec![create_test_message(1, "partial hit", 123)], "rare", 1);
    degraded.query_metadata.timed_out = true;
    degraded.query_metadata.partial = true;
    degraded.query_metadata.pages_fetched = 41;
    degraded.query_metadata.messages_scanned = 4100;

    mock_client
        .expect_search_messages()
        .returning(move |_| Ok(degraded.clone()));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let result = server
        .search_messages(
            Parameters(SearchRequest {
                query: "rare".to_string(),
                ..Default::default()
            }),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let body = result.expect("a slow-but-working search must not surface as an error");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(parsed["query_metadata"]["timed_out"], true);
    assert_eq!(parsed["query_metadata"]["partial"], true);
    assert_eq!(parsed["query_metadata"]["pages_fetched"], 41);
    assert_eq!(parsed["query_metadata"]["messages_scanned"], 4100);
    // The whole point: results survive the deadline.
    assert_eq!(parsed["returned"], 1);
}

#[tokio::test]
async fn healthy_search_omits_the_degradation_flags() {
    let mut mock_client = MockTelegramClientTrait::new();
    let expected = create_test_search_result(vec![create_test_message(1, "hit", 123)], "common", 1);
    mock_client
        .expect_search_messages()
        .returning(move |_| Ok(expected.clone()));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let result = server
        .search_messages(
            Parameters(SearchRequest {
                query: "common".to_string(),
                ..Default::default()
            }),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let body = result.expect("search succeeds");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert!(parsed["query_metadata"].get("timed_out").is_none());
    assert!(parsed["query_metadata"].get("partial").is_none());
}
