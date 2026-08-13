//! Multi-channel fan-out tests for get_recent_messages (work-order A3) and
//! search_messages (work-order A6).

use crate::error::Error;
use crate::mcp::server::McpServer;
use crate::mcp::tools::types::requests::{GetRecentMessagesRequest, ResponseFormat, SearchRequest};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{QueryMetadata, SearchResult};
use crate::test_helpers::create_test_message;
use std::sync::Arc;

/// A single-message `SearchResult` for `channel`, mirroring
/// `create_test_search_result` but with a channel-derived message id so
/// per-channel mock arms are easy to tell apart in assertions.
fn single_message_result(channel: i64) -> SearchResult {
    SearchResult {
        messages: vec![create_test_message(channel * 1000, "text", channel)],
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
    }
}

/// A `GetRecentMessagesRequest` in fan-out mode (channel_ids set, channel_id absent).
fn multi_request(channel_ids: Vec<&str>) -> GetRecentMessagesRequest {
    GetRecentMessagesRequest {
        channel_id: None,
        channel_ids: Some(channel_ids.into_iter().map(String::from).collect()),
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
    }
}

/// A `SearchRequest` in fan-out mode (channel_ids set, channel_id absent).
fn multi_search_request(channel_ids: Vec<&str>) -> SearchRequest {
    SearchRequest {
        query: "тест".to_string(),
        channel_ids: Some(channel_ids.into_iter().map(String::from).collect()),
        ..Default::default()
    }
}

#[tokio::test]
async fn recent_multi_fans_out_and_merges() {
    let mut telegram = MockTelegramClientTrait::new();
    // Two channels, distinguished by the HistoryParams identifier/id the
    // impl builds per entry; return one message each.
    telegram
        .expect_get_recent_messages()
        .times(2)
        .returning(|params| {
            let channel = params.channel_id.map(|c| c.get()).unwrap_or(555);
            Ok(single_message_result(channel)) // local helper over create_test_message
        });
    let mut limiter = MockRateLimiterTrait::new();
    // 1 token per channel, acquired once.
    limiter
        .expect_acquire()
        .with(mockall::predicate::eq(2))
        .times(1)
        .returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(telegram), Arc::new(limiter));

    let out = server
        .get_recent_messages_impl(multi_request(vec!["111", "222"]))
        .await
        .expect("ok");
    let json: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(json["returned"], 2);
    assert_eq!(json["query_metadata"]["channels_scanned"], 2);
}

#[tokio::test]
async fn recent_multi_rejects_both_and_neither_and_cursors() {
    let server = McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    );

    // channel_id + channel_ids together: rejected, no client/limiter call.
    let mut both = multi_request(vec!["111"]);
    both.channel_id = Some("222".to_string());
    let err = server
        .get_recent_messages_impl(both)
        .await
        .expect_err("both set must be rejected");
    assert!(err.contains("not both"), "got: {err}");

    // Neither channel_id nor channel_ids: history has no global mode.
    let mut neither = multi_request(vec!["111"]);
    neither.channel_ids = None;
    let err = server
        .get_recent_messages_impl(neither)
        .await
        .expect_err("neither set must be rejected");
    assert!(err.contains("required"), "got: {err}");

    // channel_ids + before_id: cursors are per-channel, incompatible with fan-out.
    let mut with_cursor = multi_request(vec!["111", "222"]);
    with_cursor.before_id = Some(100);
    let err = server
        .get_recent_messages_impl(with_cursor)
        .await
        .expect_err("cursor with channel_ids must be rejected");
    assert!(err.contains("single channel"), "got: {err}");
}

#[tokio::test]
async fn recent_multi_partial_failure_lands_in_channel_errors() {
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_get_recent_messages()
        .withf(|p| p.channel_id.map(|c| c.get()) == Some(111))
        .times(1)
        .returning(|_| Ok(single_message_result(111)));
    telegram
        .expect_get_recent_messages()
        .withf(|p| p.channel_id.map(|c| c.get()) == Some(222))
        .times(1)
        .returning(|_| Err(Error::InvalidInput("Channel not found: 222".to_string())));

    let mut limiter = MockRateLimiterTrait::new();
    limiter
        .expect_acquire()
        .with(mockall::predicate::eq(2))
        .times(1)
        .returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(telegram), Arc::new(limiter));

    let out = server
        .get_recent_messages_impl(multi_request(vec!["111", "222"]))
        .await
        .expect("partial failure must still return the surviving channel's results");
    let json: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(json["returned"], 1);
    let errors = json["channel_errors"]
        .as_array()
        .expect("channel_errors present");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["channel"], "222");
    assert!(
        errors[0]["error"]
            .as_str()
            .expect("error string")
            .contains("not found"),
        "got: {}",
        errors[0]["error"]
    );
}

#[tokio::test]
async fn recent_multi_compact_returns_channels_map() {
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_get_recent_messages()
        .withf(|p| p.channel_id.map(|c| c.get()) == Some(111))
        .times(1)
        .returning(|_| Ok(single_message_result(111)));
    telegram
        .expect_get_recent_messages()
        .withf(|p| p.channel_id.map(|c| c.get()) == Some(222))
        .times(1)
        .returning(|_| Ok(single_message_result(222)));

    let mut limiter = MockRateLimiterTrait::new();
    limiter
        .expect_acquire()
        .with(mockall::predicate::eq(2))
        .times(1)
        .returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(telegram), Arc::new(limiter));

    let mut request = multi_request(vec!["111", "222"]);
    request.format = Some(ResponseFormat::Compact);
    let out = server.get_recent_messages_impl(request).await.expect("ok");
    let json: serde_json::Value = serde_json::from_str(&out).expect("json");

    let channels = json["channels"].as_object().expect("channels map present");
    assert!(channels.contains_key("111"), "got: {channels:?}");
    assert!(channels.contains_key("222"), "got: {channels:?}");

    for m in json["messages"].as_array().expect("messages") {
        assert!(
            m.get("channel_id").is_some(),
            "channel_id survives in multi compact"
        );
        assert!(
            m.get("channel_name").is_none(),
            "channel_name must be hoisted into the channels map"
        );
        assert!(
            m.get("channel_username").is_none(),
            "channel_username must be hoisted into the channels map"
        );
    }
}

#[tokio::test]
async fn search_multi_fans_out_and_merges() {
    let mut telegram = MockTelegramClientTrait::new();
    // Two channels, distinguished by the SearchParams channel_id the impl
    // builds per entry; return one message each.
    telegram
        .expect_search_messages()
        .times(2)
        .returning(|params| {
            let channel = params.channel_id.map(|c| c.get()).unwrap_or(555);
            Ok(single_message_result(channel)) // shared helper over create_test_message
        });
    let mut limiter = MockRateLimiterTrait::new();
    // 1 token per channel, acquired once.
    limiter
        .expect_acquire()
        .with(mockall::predicate::eq(2))
        .times(1)
        .returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(telegram), Arc::new(limiter));

    let out = server
        .search_messages_impl(multi_search_request(vec!["111", "222"]))
        .await
        .expect("ok");
    let json: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(json["returned"], 2);
    assert_eq!(json["query_metadata"]["channels_scanned"], 2);
    assert_eq!(
        json["query_metadata"]["query"], "тест",
        "the request query threads through to the merged response"
    );
}

#[tokio::test]
async fn search_multi_rejects_both_and_cursors() {
    let server = McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    );

    // channel_id + channel_ids together: rejected, no client/limiter call.
    let mut both = multi_search_request(vec!["111"]);
    both.channel_id = Some("222".to_string());
    let err = server
        .search_messages_impl(both)
        .await
        .expect_err("both set must be rejected");
    assert!(err.contains("not both"), "got: {err}");

    // channel_ids + before_id: cursors are per-channel, incompatible with fan-out.
    let mut with_cursor = multi_search_request(vec!["111", "222"]);
    with_cursor.before_id = Some(100);
    let err = server
        .search_messages_impl(with_cursor)
        .await
        .expect_err("cursor with channel_ids must be rejected");
    assert!(err.contains("single channel"), "got: {err}");
}

#[tokio::test]
async fn search_multi_partial_failure_lands_in_channel_errors() {
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_search_messages()
        .withf(|p| p.channel_id.map(|c| c.get()) == Some(111))
        .times(1)
        .returning(|_| Ok(single_message_result(111)));
    telegram
        .expect_search_messages()
        .withf(|p| p.channel_id.map(|c| c.get()) == Some(222))
        .times(1)
        .returning(|_| Err(Error::InvalidInput("Channel not found: 222".to_string())));

    let mut limiter = MockRateLimiterTrait::new();
    limiter
        .expect_acquire()
        .with(mockall::predicate::eq(2))
        .times(1)
        .returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(telegram), Arc::new(limiter));

    let out = server
        .search_messages_impl(multi_search_request(vec!["111", "222"]))
        .await
        .expect("partial failure must still return the surviving channel's results");
    let json: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(json["returned"], 1);
    let errors = json["channel_errors"]
        .as_array()
        .expect("channel_errors present");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["channel"], "222");
    assert!(
        errors[0]["error"]
            .as_str()
            .expect("error string")
            .contains("not found"),
        "got: {}",
        errors[0]["error"]
    );
}

#[tokio::test]
async fn search_multi_compact_returns_channels_map() {
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_search_messages()
        .withf(|p| p.channel_id.map(|c| c.get()) == Some(111))
        .times(1)
        .returning(|_| Ok(single_message_result(111)));
    telegram
        .expect_search_messages()
        .withf(|p| p.channel_id.map(|c| c.get()) == Some(222))
        .times(1)
        .returning(|_| Ok(single_message_result(222)));

    let mut limiter = MockRateLimiterTrait::new();
    limiter
        .expect_acquire()
        .with(mockall::predicate::eq(2))
        .times(1)
        .returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(telegram), Arc::new(limiter));

    let mut request = multi_search_request(vec!["111", "222"]);
    request.format = Some(ResponseFormat::Compact);
    let out = server.search_messages_impl(request).await.expect("ok");
    let json: serde_json::Value = serde_json::from_str(&out).expect("json");

    let channels = json["channels"].as_object().expect("channels map present");
    assert!(channels.contains_key("111"), "got: {channels:?}");
    assert!(channels.contains_key("222"), "got: {channels:?}");

    for m in json["messages"].as_array().expect("messages") {
        assert!(
            m.get("channel_id").is_some(),
            "channel_id survives in multi compact"
        );
        assert!(
            m.get("channel_name").is_none(),
            "channel_name must be hoisted into the channels map"
        );
        assert!(
            m.get("channel_username").is_none(),
            "channel_username must be hoisted into the channels map"
        );
    }
}

#[tokio::test]
async fn search_without_any_channel_scope_stays_global() {
    // channel_id: None, channel_ids: None -> expect_search_messages with
    // params.channel_id == None fires exactly once (no fan-out).
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_search_messages()
        .withf(|p| p.channel_id.is_none())
        .times(1)
        .returning(|_| {
            Ok(SearchResult {
                messages: vec![],
                returned: 0,
                has_more: false,
                search_time_ms: 1,
                query_metadata: QueryMetadata {
                    query: "тест".to_string(),
                    window_from: chrono::Utc::now() - chrono::Duration::hours(48),
                    window_to: None,
                    channels_scanned: Some(0),
                    channels_in_results: 0,
                    timed_out: false,
                    partial: false,
                    pages_fetched: 0,
                    messages_scanned: 0,
                },
            })
        });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(telegram), Arc::new(limiter));

    let request = SearchRequest {
        query: "тест".to_string(),
        ..Default::default()
    };
    let out = server
        .search_messages_impl(request)
        .await
        .expect("global search with no channel scope must work");
    let json: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(json["returned"], 0);
}

#[test]
fn fanout_merge_sums_counters_and_ors_flags() {
    use crate::mcp::tools::fanout::{ChannelFetchOutcome, merge_results};
    use crate::test_helpers::create_test_search_result;
    use chrono::Utc;

    let mut clean = create_test_search_result(vec![], "q", 0);
    clean.query_metadata.pages_fetched = 2;
    clean.query_metadata.messages_scanned = 150;

    let mut degraded = create_test_search_result(vec![], "q", 0);
    degraded.query_metadata.pages_fetched = 5;
    degraded.query_metadata.messages_scanned = 500;
    degraded.query_metadata.timed_out = true;
    degraded.query_metadata.partial = true;

    let merged = merge_results(
        // Degraded first, deliberately: with the `true` flags last, a
        // last-channel-wins overwrite (`=` where `|=` is meant) would pass.
        vec![
            ChannelFetchOutcome {
                channel: "a".into(),
                result: Ok(degraded),
            },
            ChannelFetchOutcome {
                channel: "b".into(),
                result: Ok(clean),
            },
        ],
        20,
        "q".to_string(),
        Utc::now(),
        None,
    )
    .expect("merge succeeds");

    // Summed, not dropped — a caller must see the whole fan-out's cost.
    assert_eq!(merged.query_metadata.pages_fetched, 7);
    assert_eq!(merged.query_metadata.messages_scanned, 650);
    // One degraded channel degrades the merged result.
    assert!(merged.query_metadata.timed_out);
    assert!(merged.query_metadata.partial);
}
