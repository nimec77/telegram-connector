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
            peer_namespace: Some(crate::telegram::types::PeerNamespace::Channel),
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

#[tokio::test]
async fn global_search_keeps_user_group_and_channel_namespaces_without_a_cursor() {
    use crate::config::OutputFormat;
    use crate::telegram::types::PeerNamespace;

    for mode in [OutputFormat::JsonV1, OutputFormat::Legacy] {
        let messages = [
            PeerNamespace::User,
            PeerNamespace::Chat,
            PeerNamespace::Channel,
        ]
        .into_iter()
        .map(|namespace| {
            let mut message = create_test_message(9, "global hit", 11);
            message.peer_namespace = Some(namespace);
            message
        })
        .collect();
        let mut upstream = create_test_search_result(messages, "q", 3);
        upstream.has_more = true;
        let mut client = MockTelegramClientTrait::new();
        client
            .expect_search_messages()
            .withf(|params| params.channel_id.is_none())
            .returning(move |_| Ok(upstream.clone()));
        let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
        let response = server
            .search_messages_data(SearchRequest {
                query: "q".into(),
                ..Default::default()
            })
            .await;
        let rendered = crate::mcp::tools::types::contract::render(mode, response);
        let wire = serde_json::to_value(rendered).expect("wire result");
        let data = if mode == OutputFormat::JsonV1 {
            wire["structuredContent"]["data"].clone()
        } else {
            serde_json::from_str(wire["content"][0]["text"].as_str().expect("legacy text"))
                .expect("legacy JSON")
        };
        assert_eq!(data["returned"], 3);
        assert_eq!(data["has_more"], true);
        assert!(
            data.get("next_cursor").is_none(),
            "global IDs cannot form a cursor"
        );
        let messages = data["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 3);
        if mode == OutputFormat::JsonV1 {
            assert_eq!(
                messages
                    .iter()
                    .map(|message| message["peer_namespace"].as_str().expect("namespace"))
                    .collect::<Vec<_>>(),
                ["user", "chat", "channel"]
            );
        } else {
            assert!(
                messages
                    .iter()
                    .all(|message| message.get("peer_namespace").is_none()),
                "legacy keeps its established field shape"
            );
        }
    }
}

#[tokio::test]
async fn global_search_renders_converted_namespaced_albums_in_both_modes() {
    use crate::config::OutputFormat;
    use crate::telegram::albums::{AlbumMode, PageAccumulator};
    use crate::telegram::converters::convert_raw_message;
    use crate::telegram::envelope::EntityLookup;
    use crate::test_helpers::{raw_tl_channel, raw_tl_message_grouped, raw_tl_user};
    use grammers_client::peer::{Peer, User};
    use grammers_client::{Client, tl};
    use grammers_mtsender::SenderPool;
    use grammers_session::storages::MemorySession;

    let SenderPool { handle, .. } = SenderPool::new(Arc::new(MemorySession::default()), 1);
    let client = Client::new(handle);
    let users = [
        tl::enums::User::User(raw_tl_user(11, Some("User author"), None, None)),
        tl::enums::User::User(raw_tl_user(44, None, None, None)),
    ];
    let chats = [
        tl::enums::Chat::Forbidden(tl::types::ChatForbidden {
            id: 11,
            title: "Chat author".into(),
        }),
        tl::enums::Chat::Channel(raw_tl_channel(11, "Channel author", None)),
    ];
    let entities = EntityLookup::from_envelope(&chats, &users);
    let peers = [
        Peer::User(User::from_raw(&client, users[0].clone())),
        Peer::from_raw(&client, chats[0].clone()),
        Peer::from_raw(&client, chats[1].clone()),
    ];
    let mut page = PageAccumulator::new(AlbumMode::Collapse, 3);
    for id in [12, 11] {
        for peer in &peers {
            let mut raw = raw_tl_message_grouped(id, 2_000, 11, 77);
            if let tl::enums::Message::Message(message) = &mut raw {
                message.peer_id = peer.id().into();
                message.from_id = Some(peer.id().into());
                message.message = if id == 12 {
                    "caption".into()
                } else {
                    String::new()
                };
            }
            assert!(
                page.push(convert_raw_message(&raw, peer, &entities).expect("real TL conversion"))
            );
        }
    }
    let mut messages = page.into_messages();
    // A missing entity and an entity with no names both preserve the user ID;
    // absent from_id remains absent even when the source is a resolved user.
    for (id, sender_id) in [(30, Some(43)), (31, Some(44)), (32, None)] {
        let mut raw = raw_tl_message_grouped(id, 2_000, 11, 77);
        if let tl::enums::Message::Message(message) = &mut raw {
            message.peer_id = peers[0].id().into();
            message.from_id =
                sender_id.map(|user_id| tl::enums::Peer::User(tl::types::PeerUser { user_id }));
            message.grouped_id = None;
        }
        messages
            .push(convert_raw_message(&raw, &peers[0], &entities).expect("sender absence fixture"));
    }
    for mode in [OutputFormat::JsonV1, OutputFormat::Legacy] {
        let upstream = create_test_search_result(messages.clone(), "q", 6);
        let mut client = MockTelegramClientTrait::new();
        client
            .expect_search_messages()
            .withf(|params| params.channel_id.is_none())
            .returning(move |_| Ok(upstream.clone()));
        let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
        let response = server
            .search_messages_data(SearchRequest {
                query: "q".into(),
                ..Default::default()
            })
            .await;
        let wire = serde_json::to_value(crate::mcp::tools::types::contract::render(mode, response))
            .expect("wire result");
        let data = if mode == OutputFormat::JsonV1 {
            let text: serde_json::Value =
                serde_json::from_str(wire["content"][0]["text"].as_str().expect("fallback text"))
                    .expect("fallback JSON");
            assert_eq!(text, wire["structuredContent"]);
            wire["structuredContent"]["data"].clone()
        } else {
            serde_json::from_str(wire["content"][0]["text"].as_str().expect("legacy text"))
                .expect("legacy JSON")
        };
        assert_eq!(data["returned"], 6);
        assert_eq!(data["has_more"], false);
        assert!(data.get("next_cursor").is_none());
        let messages = data["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 6);
        for (index, (namespace, name)) in [
            ("user", "User author"),
            ("chat", "Chat author"),
            ("channel", "Channel author"),
        ]
        .into_iter()
        .enumerate()
        {
            let message = &messages[index];
            assert_eq!(message["id"], 11);
            assert_eq!(message["text"], "caption");
            assert_eq!(message["album"]["media_count"], 2);
            assert_eq!(message["album"]["message_ids"], serde_json::json!([11, 12]));
            assert_eq!(message["sender_name"], name);
            let sender = if index == 0 {
                if mode == OutputFormat::JsonV1 {
                    serde_json::json!("11")
                } else {
                    serde_json::json!(11)
                }
            } else {
                serde_json::Value::Null
            };
            assert_eq!(message["sender_id"], sender);
            if mode == OutputFormat::JsonV1 {
                assert_eq!(message["peer_namespace"], namespace);
            } else {
                assert!(message.get("peer_namespace").is_none());
            }
        }
        for (message, (id, sender)) in
            messages[3..]
                .iter()
                .zip([(30, Some(43)), (31, Some(44)), (32, None)])
        {
            assert_eq!(message["id"], id);
            assert!(
                message
                    .get("sender_name")
                    .expect("nullable sender name")
                    .is_null()
            );
            let expected = match sender {
                Some(id) if mode == OutputFormat::JsonV1 => serde_json::json!(id.to_string()),
                Some(id) => serde_json::json!(id),
                None => serde_json::Value::Null,
            };
            assert_eq!(message["sender_id"], expected);
        }
    }
}

#[tokio::test]
async fn fanout_search_renders_unique_namespaced_observations_in_both_modes() {
    assert_fanout_namespaced_rendering(true).await;
}

#[tokio::test]
async fn fanout_history_renders_unique_namespaced_observations_in_both_modes() {
    assert_fanout_namespaced_rendering(false).await;
}

async fn assert_fanout_namespaced_rendering(search: bool) {
    use crate::config::OutputFormat;
    use crate::mcp::tools::GetRecentMessagesRequest;
    use crate::telegram::types::PeerNamespace;
    for mode in [OutputFormat::JsonV1, OutputFormat::Legacy] {
        for upstream_has_more in [false, true] {
            let mut first = create_test_message(9, "first observation", 123);
            first.peer_namespace = Some(PeerNamespace::Chat);
            let mut duplicate = first.clone();
            duplicate.text = "discard later observation".into();
            let mut distinct = first.clone();
            distinct.peer_namespace = Some(PeerNamespace::Channel);
            let make_result = move |second: bool| {
                let mut result = create_test_search_result(
                    if second {
                        vec![duplicate.clone(), distinct.clone()]
                    } else {
                        vec![first.clone()]
                    },
                    "q",
                    1,
                );
                result.has_more = second && upstream_has_more;
                result
            };
            let mut client = MockTelegramClientTrait::new();
            if search {
                client
                    .expect_search_messages()
                    .times(2)
                    .returning(move |params| {
                        Ok(make_result(params.channel_id.unwrap().get() == 222))
                    });
            } else {
                client
                    .expect_get_recent_messages()
                    .times(2)
                    .returning(move |params| {
                        Ok(make_result(params.channel_id.unwrap().get() == 222))
                    });
            }
            let mut limiter = MockRateLimiterTrait::new();
            limiter
                .expect_acquire()
                .with(mockall::predicate::eq(2))
                .times(1)
                .returning(|_| Ok(()));
            let server = McpServer::new(Arc::new(client), Arc::new(limiter));
            let response = if search {
                server
                    .search_messages_data(SearchRequest {
                        query: "q".into(),
                        channel_ids: Some(vec!["111".into(), "222".into()]),
                        limit: Some(2),
                        ..Default::default()
                    })
                    .await
            } else {
                server
                    .get_recent_messages_data(GetRecentMessagesRequest {
                        channel_id: None,
                        channel_ids: Some(vec!["111".into(), "222".into()]),
                        hours_back: None,
                        limit: Some(2),
                        media_filter: None,
                        from_date: None,
                        to_date: None,
                        collapse_albums: None,
                        before_id: None,
                        after_id: None,
                        max_text_length: None,
                        format: None,
                    })
                    .await
            };
            let rendered = crate::mcp::tools::types::contract::render(mode, response);
            let wire = serde_json::to_value(rendered).unwrap();
            let data = if mode == OutputFormat::JsonV1 {
                wire["structuredContent"]["data"].clone()
            } else {
                serde_json::from_str(wire["content"][0]["text"].as_str().unwrap()).unwrap()
            };
            assert_eq!(data["returned"], 2);
            assert_eq!(data["has_more"], upstream_has_more);
            assert_eq!(data["query_metadata"]["channels_scanned"], 2);
            assert_eq!(data["query_metadata"]["channels_in_results"], 2);
            assert!(data.get("next_cursor").is_none());
            let messages = data["messages"].as_array().unwrap();
            assert_eq!(messages.len(), 2);
            assert!(messages.iter().all(|m| m["text"] == "first observation"));
            if mode == OutputFormat::JsonV1 {
                assert_eq!(messages[0]["peer_namespace"], "chat");
                assert_eq!(messages[1]["peer_namespace"], "channel");
            }
        }
    }
}
