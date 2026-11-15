//! Multi-channel fan-out tests for get_recent_messages (work-order A3) and
//! search_messages (work-order A6).

use crate::error::Error;
use crate::mcp::server::McpServer;
use crate::mcp::tools::types::requests::{GetRecentMessagesRequest, ResponseFormat, SearchRequest};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{QueryMetadata, SearchResult};
use crate::test_helpers::{create_test_message, permissive_limiter};
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
    let limiter = permissive_limiter();
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

// These cases exercise both tool paths against the real merge/shaping logic.
#[tokio::test]
async fn fanout_local_aliases_charge_once_per_kind_and_render_unique_results() {
    use crate::telegram::types::{ChannelId, ChannelIdentity, PeerNamespace};
    for search in [false, true] {
        let mut telegram = MockTelegramClientTrait::new();
        if search {
            telegram
                .expect_resolve_channel_identity()
                .with(mockall::predicate::eq("news"))
                .times(1)
                .returning(|_| {
                    Ok(ChannelIdentity {
                        id: ChannelId::new(456).unwrap(),
                        peer_namespace: PeerNamespace::Channel,
                        username: Some("news".into()),
                    })
                });
            telegram
                .expect_search_messages()
                .times(2)
                .returning(|params| Ok(single_message_result(params.channel_id.unwrap().get())));
        } else {
            telegram
                .expect_get_recent_messages()
                .times(2)
                .returning(|params| {
                    let channel = match (params.channel_id, params.channel_identifier.as_deref()) {
                        (Some(id), None) => {
                            assert_eq!(id.get(), 123);
                            123
                        }
                        (None, Some("news")) => 456,
                        other => panic!("wrong normalized history target: {other:?}"),
                    };
                    Ok(single_message_result(channel))
                });
        }
        let mut limiter = MockRateLimiterTrait::new();
        limiter
            .expect_acquire()
            .with(mockall::predicate::eq(2))
            .times(1)
            .returning(|_| Ok(()));
        let server = McpServer::new(Arc::new(telegram), Arc::new(limiter));
        let aliases = vec![" @News ", "news", "NEWS", "123", "+123", "00123"];
        let output = if search {
            server
                .search_messages_impl(multi_search_request(aliases))
                .await
        } else {
            server
                .get_recent_messages_impl(multi_request(aliases))
                .await
        }
        .expect("normalized fanout");
        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["returned"], 2);
        assert_eq!(json["query_metadata"]["channels_scanned"], 2);
        let mut ids: Vec<_> = json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["channel_id"].as_i64().unwrap())
            .collect();
        ids.sort();
        assert_eq!(ids, vec![123, 456]);
    }
}

#[tokio::test]
async fn fanout_later_invalid_alias_rejects_everything_before_acquire() {
    for search in [false, true] {
        for invalid in ["@@news", "@123", "0", "-123", "bad name", " "] {
            let server = McpServer::new(
                Arc::new(MockTelegramClientTrait::new()),
                Arc::new(MockRateLimiterTrait::new()),
            );
            let aliases = vec!["news", "NEWS", invalid];
            let result = if search {
                server
                    .search_messages_impl(multi_search_request(aliases))
                    .await
            } else {
                server
                    .get_recent_messages_impl(multi_request(aliases))
                    .await
            };
            assert!(result.is_err(), "accepted {invalid:?}");
        }
    }
}

#[tokio::test]
async fn fanout_numeric_and_username_attempts_keep_first_album_and_original_errors() {
    use crate::telegram::types::{
        AlbumInfo, ChannelId, ChannelIdentity, MediaType, MessageId, PeerNamespace,
    };
    for search in [false, true] {
        let make_result = |first: bool| {
            let mut result = single_message_result(123);
            let message = &mut result.messages[0];
            message.text = if first {
                "first caption"
            } else {
                "later caption"
            }
            .into();
            message.album = Some(AlbumInfo {
                media_count: if first { 2 } else { 3 },
                media_types: vec![MediaType::Photo, MediaType::Video],
                message_ids: vec![
                    MessageId::new(123000).unwrap(),
                    MessageId::new(123001).unwrap(),
                ],
            });
            result.query_metadata.pages_fetched = if first { 2 } else { 3 };
            result.query_metadata.messages_scanned = if first { 20 } else { 30 };
            result.query_metadata.partial = !first;
            result.query_metadata.timed_out = !first;
            result
        };
        let first = make_result(true);
        let later = make_result(false);
        let mut telegram = MockTelegramClientTrait::new();
        if search {
            telegram
                .expect_resolve_channel_identity()
                .times(2)
                .returning(|name| {
                    if name == "alias" {
                        Ok(ChannelIdentity {
                            id: ChannelId::new(123).unwrap(),
                            peer_namespace: PeerNamespace::Channel,
                            username: Some("alias".into()),
                        })
                    } else {
                        assert_eq!(name, "missing");
                        Err(Error::InvalidInput("not found".into()))
                    }
                });
            telegram
                .expect_search_messages()
                .times(2)
                .returning(move |params| {
                    Ok(if params.channel_namespace.is_none() {
                        first.clone()
                    } else {
                        later.clone()
                    })
                });
        } else {
            telegram
                .expect_get_recent_messages()
                .times(3)
                .returning(move |params| match params.channel_identifier.as_deref() {
                    None => Ok(first.clone()),
                    Some("alias") => Ok(later.clone()),
                    Some("missing") => Err(Error::InvalidInput("not found".into())),
                    other => panic!("unexpected target: {other:?}"),
                });
        }
        let mut limiter = MockRateLimiterTrait::new();
        limiter
            .expect_acquire()
            .with(mockall::predicate::eq(3))
            .times(1)
            .returning(|_| Ok(()));
        let server = McpServer::new(Arc::new(telegram), Arc::new(limiter));
        let output = if search {
            let mut request = multi_search_request(vec!["123", "alias", " @Missing ", "MISSING"]);
            request.limit = Some(1);
            server.search_messages_impl(request).await
        } else {
            let mut request = multi_request(vec!["123", "alias", " @Missing ", "MISSING"]);
            request.limit = Some(1);
            server.get_recent_messages_impl(request).await
        }
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["returned"], 1);
        assert_eq!(json["has_more"], false);
        assert_eq!(json["messages"][0]["text"], "first caption");
        assert_eq!(json["messages"][0]["album"]["media_count"], 2);
        assert_eq!(json["query_metadata"]["channels_scanned"], 3);
        assert_eq!(json["query_metadata"]["pages_fetched"], 5);
        assert_eq!(json["query_metadata"]["messages_scanned"], 50);
        assert_eq!(json["query_metadata"]["partial"], true);
        assert_eq!(json["query_metadata"]["timed_out"], true);
        assert_eq!(json["channel_errors"].as_array().unwrap().len(), 1);
        assert_eq!(json["channel_errors"][0]["channel"], " @Missing ");
        assert!(json["next_cursor"].is_null());
    }
}
