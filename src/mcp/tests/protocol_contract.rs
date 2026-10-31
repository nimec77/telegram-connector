use super::*;

#[tokio::test]
async fn canonical_contract_advertises_every_output_schema() {
    let mut wire = Wire::start();
    wire.send(modern(json!(1), "tools/list")).await;
    let response = wire.recv().await;
    let tools = response["result"]["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 16);
    for tool in tools {
        assert!(
            tool["outputSchema"].is_object(),
            "schema for {}",
            tool["name"]
        );
    }
    wire.close().await;
}

#[tokio::test]
async fn canonical_contract_typed_argument_failure_has_envelope() {
    let mut wire = Wire::start();
    let mut request = modern(json!(1), "tools/call");
    request["params"]["name"] = json!("get_messages_batch");
    request["params"]["arguments"] = json!({"channel_id":"news", "message_ids":true});
    wire.send(request).await;
    let response = wire.recv().await;
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["schema_version"],
        "1"
    );
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        "invalid_input"
    );
    wire.close().await;
}

use crate::config::OutputFormat;
use crate::error::{Error, StateUnavailable};
use crate::telegram::types::{self, ChannelId, PeerNamespace, UserId};
use crate::test_helpers::{
    create_test_channel, create_test_jpeg, create_test_message_with_enriched_forward,
    create_test_search_result, permissive_limiter,
};

const LARGE_ID: i64 = 9_007_199_254_740_993;
fn message() -> types::Message {
    let mut m =
        create_test_message_with_enriched_forward(1, "canonical 🦀", LARGE_ID, LARGE_ID + 2);
    m.sender_id = Some(UserId::new(LARGE_ID + 4).expect("id"));
    m.timestamp = "2026-10-31T01:02:03Z".parse().expect("UTC");
    m
}
fn photo() -> types::MediaDownload {
    let bytes = create_test_jpeg(64, 64);
    types::MediaDownload {
        channel_id: ChannelId::new(LARGE_ID).expect("id"),
        peer_namespace: PeerNamespace::Channel,
        source_size_bytes: bytes.len() as u64,
        bytes,
        media_type: types::MediaType::Photo,
        is_thumbnail: false,
        caption: Some("photo".into()),
        width: Some(64),
        height: Some(64),
        video_info: None,
        largest_width: Some(64),
        largest_height: Some(64),
    }
}
struct FakeDesktop {
    fail: bool,
}
#[async_trait::async_trait]
impl crate::mcp::server::desktop::DesktopLauncher for FakeDesktop {
    fn supported(&self) -> bool {
        true
    }
    async fn open(&self, link: &str) -> Result<(), Error> {
        if self.fail {
            Err(Error::DesktopOpenFailed {
                message: "safe launch failure".into(),
                link: link.into(),
            })
        } else {
            Ok(())
        }
    }
}
fn complete_server(mode: OutputFormat) -> McpServer<MockTelegramClientTrait, MockRateLimiterTrait> {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_cached_account_status()
        .returning(|| types::AccountStatus {
            authorized: true,
            premium: Some(true),
            ..Default::default()
        });
    client.expect_get_subscribed_channels().returning(|_, _| {
        Ok(types::ChannelPage {
            channels: vec![create_test_channel(LARGE_ID, "news")],
            total: 1,
        })
    });
    client
        .expect_get_channel_info()
        .returning(|_| Ok(create_test_channel(LARGE_ID, "news")));
    client.expect_resolve_channel_identity().returning(|_| {
        Ok(types::ChannelIdentity {
            peer_namespace: PeerNamespace::Channel,
            id: ChannelId::new(LARGE_ID).expect("id"),
            username: Some("news".into()),
        })
    });
    client
        .expect_search_messages()
        .returning(|_| Ok(create_test_search_result(vec![message()], "query", 1)));
    client
        .expect_get_recent_messages()
        .returning(|_| Ok(create_test_search_result(vec![message()], "", 1)));
    client
        .expect_get_message_by_id()
        .returning(|_, _| Ok(message()));
    client
        .expect_download_message_media()
        .returning(|_, _, _| Ok(photo()));
    client
        .expect_download_messages_media()
        .returning(|_, _, _| {
            Ok(types::MediaBatch {
                channel_id: ChannelId::new(LARGE_ID).expect("id"),
                peer_namespace: PeerNamespace::Channel,
                outcomes: vec![
                    types::MediaFetchOutcome {
                        message_id: 1,
                        result: Ok(photo()),
                    },
                    types::MediaFetchOutcome {
                        message_id: 2,
                        result: Err(types::MediaFetchError::NotFound),
                    },
                ],
            })
        });
    client.expect_transcribe_audio().returning(|_, _, _| {
        Ok(types::TranscriptionOutcome {
            channel_id: crate::telegram::types::ChannelId::new(LARGE_ID).expect("fixture channel"),
            peer_namespace: crate::telegram::types::PeerNamespace::Channel,
            message_id: crate::telegram::types::MessageId::new(1).expect("fixture message"),
            text: "transcribed".into(),
            partial: false,
            media_type: types::MediaType::Voice,
            duration_seconds: Some(3),
        })
    });
    client
        .expect_search_public_channels()
        .returning(|_, _| Ok(vec![create_test_channel(LARGE_ID, "news")]));
    client.expect_get_messages_batch().returning(|_, _| {
        Ok(types::MessageBatch {
            channel_id: ChannelId::new(LARGE_ID).expect("id"),
            peer_namespace: PeerNamespace::Channel,
            messages: vec![message()],
            missing_ids: vec![2],
        })
    });
    client.expect_resolve_channels().returning(|_| {
        Ok(vec![
            types::ChannelResolution {
                identifier: "news".into(),
                channel: Some(create_test_channel(LARGE_ID, "news")),
                error: None,
            },
            types::ChannelResolution {
                identifier: "missing".into(),
                channel: None,
                error: Some(Error::ChannelNotFound("missing".into())),
            },
        ])
    });
    client.expect_get_channel_stats().returning(|_, _| {
        let now = "2026-10-31T01:02:03Z".parse().expect("UTC");
        Ok(types::stats::compute_stats(
            ChannelId::new(LARGE_ID).expect("id"),
            &[],
            0,
            now,
            now,
            true,
        ))
    });
    let mut limiter = permissive_limiter();
    limiter.expect_try_available_tokens().returning(|| Ok(30.0));
    limiter.expect_capacity().returning(|| 30.0);
    limiter.expect_refill_rate().returning(|| 1.0);
    let mut server = McpServer::new(Arc::new(client), Arc::new(limiter)).with_output_format(mode);
    server.desktop_launcher = Arc::new(FakeDesktop { fail: false });
    server
}
fn cases() -> Vec<(&'static str, Value)> {
    vec![
        ("check_mcp_status", json!({})),
        ("get_subscribed_channels", json!({"limit":1})),
        ("get_channel_info", json!({"channel_identifier":"news"})),
        (
            "generate_message_link",
            json!({"channel_id":"news","message_id":1}),
        ),
        (
            "open_message_in_telegram",
            json!({"channel_id":"news","message_id":1}),
        ),
        (
            "search_messages",
            json!({"query":"query","channel_id":"news"}),
        ),
        (
            "get_recent_messages",
            json!({"channel_id":"news","format":"compact"}),
        ),
        ("get_message_by_link", json!({"link":"https://t.me/news/1"})),
        ("get_last_responses", json!({})),
        (
            "get_message_media",
            json!({"channel_id":"news","message_id":1}),
        ),
        (
            "transcribe_voice_message",
            json!({"channel_id":"news","message_id":1}),
        ),
        ("search_public_channels", json!({"query":"news"})),
        (
            "get_messages_batch",
            json!({"channel_id":"news","message_ids":[1,2]}),
        ),
        (
            "resolve_channels",
            json!({"identifiers":["news","missing"]}),
        ),
        ("get_channel_stats", json!({"channel_id":"news"})),
        (
            "get_messages_media_batch",
            json!({"channel_id":"news","message_ids":[1,2]}),
        ),
    ]
}
async fn start_mode(
    handler: McpServer<MockTelegramClientTrait, MockRateLimiterTrait>,
    modern_mode: bool,
) -> Wire {
    let mut wire = Wire::with_handler(handler);
    if !modern_mode {
        wire.send(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"contract-test","version":"1"}}})).await;
        assert_eq!(wire.recv().await["result"]["protocolVersion"], "2025-11-25");
        wire.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
    }
    wire
}
fn request(id: i64, method: &str, modern_mode: bool) -> Value {
    if modern_mode {
        modern(json!(id), method)
    } else {
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":{}})
    }
}
async fn call(wire: &mut Wire, id: i64, name: &str, args: Value, modern_mode: bool) -> Value {
    let mut req = request(id, "tools/call", modern_mode);
    req["params"]["name"] = json!(name);
    req["params"]["arguments"] = args;
    wire.send(req).await;
    wire.recv().await
}
fn validate(schema: &Value, value: &Value) {
    let validator = jsonschema::draft202012::options()
        .should_validate_formats(true)
        .offline()
        .build(schema)
        .expect("self-contained 2020-12 schema");
    assert!(
        validator.is_valid(value),
        "invalid output: {:?}",
        validator
            .iter_errors(value)
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    );
}
fn envelope(reply: &Value) -> &Value {
    assert!(reply.get("error").is_none(), "protocol error: {reply}");
    let value = &reply["result"]["structuredContent"];
    let fallback: Value = serde_json::from_str(
        reply["result"]["content"][0]["text"]
            .as_str()
            .expect("JSON fallback"),
    )
    .expect("JSON");
    assert_eq!(&fallback, value);
    assert_eq!(value["schema_version"], "1");
    value
}

#[tokio::test]
async fn canonical_contract_all_sixteen_tools_in_both_modes_and_protocols() {
    for mode in [OutputFormat::JsonV1, OutputFormat::Legacy] {
        for modern_mode in [true, false] {
            let mut wire = start_mode(complete_server(mode), modern_mode).await;
            wire.send(request(1, "tools/list", modern_mode)).await;
            let listing = wire.recv().await;
            let tools = listing["result"]["tools"].as_array().expect("tools");
            assert_eq!(tools.len(), 16);
            for (index, (name, args)) in cases().into_iter().enumerate() {
                let tool = tools
                    .iter()
                    .find(|t| t["name"] == name)
                    .expect("listed tool");
                let reply = call(&mut wire, index as i64 + 2, name, args, modern_mode).await;
                assert!(reply.get("error").is_none(), "{name}: {reply}");
                assert_ne!(reply["result"]["isError"], true, "{name}: {reply}");
                if mode == OutputFormat::Legacy {
                    assert!(tool.get("outputSchema").is_none());
                    assert!(reply["result"].get("structuredContent").is_none());
                    if name == "get_message_media" || name == "get_messages_media_batch" {
                        assert_eq!(reply["result"]["content"][0]["type"], "image");
                    }
                    continue;
                }
                let value = envelope(&reply);
                validate(&tool["outputSchema"], value);
                assert!(value["error"].is_null());
                assert!(value["data"].is_object());
                // Every advertised union accepts the same typed failure shape and rejects impossible states.
                let failure = json!({"schema_version":"1","data":null,"error":{"code":"invalid_input","message":"Invalid input","retryable":false}});
                validate(&tool["outputSchema"], &failure);
                let validator = jsonschema::draft202012::options()
                    .offline()
                    .build(&tool["outputSchema"])
                    .expect("schema");
                let mut invalid = value.clone();
                invalid["error"] = failure["error"].clone();
                assert!(!validator.is_valid(&invalid));
                invalid = value.clone();
                invalid["data"] = Value::Null;
                assert!(!validator.is_valid(&invalid));
                invalid = value.clone();
                invalid["schema_version"] = json!(1);
                assert!(!validator.is_valid(&invalid));
                if [
                    "search_messages",
                    "get_recent_messages",
                    "get_messages_batch",
                ]
                .contains(&name)
                {
                    let m = &value["data"]["messages"][0];
                    assert_eq!(m["channel_id"], LARGE_ID.to_string());
                    assert_eq!(m["peer_namespace"], "channel");
                    assert_eq!(m["id"], 1);
                    assert_eq!(m["sender_id"], (LARGE_ID + 4).to_string());
                    assert_eq!(
                        m["forwarded_from"]["channel_id"],
                        (LARGE_ID + 2).to_string()
                    );
                }
                if name == "transcribe_voice_message" {
                    assert_eq!(value["data"]["channel_id"], LARGE_ID.to_string());
                    assert_eq!(value["data"]["message_id"], 1);
                }
                if name == "generate_message_link" {
                    let mut bad_id = value.clone();
                    bad_id["data"]["channel_id"] = json!("@not_numeric");
                    assert!(
                        !validator.is_valid(&bad_id),
                        "canonical channel IDs must have decimal-string schemas"
                    );
                }
                if name == "get_message_media" || name == "get_messages_media_batch" {
                    let items = value["data"]["items"].as_array().expect("items");
                    assert_eq!(items[0]["content_index"], 1);
                    assert_eq!(items[0]["channel_id"], LARGE_ID.to_string());
                    assert_eq!(reply["result"]["content"][1]["type"], "image");
                    assert!(
                        !value.to_string().contains(
                            reply["result"]["content"][1]["data"]
                                .as_str()
                                .expect("image bytes")
                        )
                    );
                    if items.len() > 1 {
                        assert!(items[1].get("content_index").is_none());
                        assert_eq!(items[1]["error"]["code"], "message_not_found");
                    }
                }
                if name == "open_message_in_telegram" || name == "transcribe_voice_message" {
                    assert_eq!(tool["annotations"]["readOnlyHint"], false);
                    assert_eq!(tool["annotations"]["idempotentHint"], false);
                } else {
                    assert_eq!(tool["annotations"]["readOnlyHint"], true);
                }
            }
            wire.close().await;
        }
    }
}

#[tokio::test]
async fn canonical_contract_local_validation_is_free_for_all_applicable_tools() {
    let invalid = vec![
        ("get_subscribed_channels", json!({"limit":0})),
        ("get_channel_info", json!({"channel_identifier":""})),
        (
            "generate_message_link",
            json!({"channel_id":"news","message_id":0}),
        ),
        (
            "open_message_in_telegram",
            json!({"channel_id":"news","message_id":0}),
        ),
        ("search_messages", json!({"query":""})),
        (
            "get_recent_messages",
            json!({"channel_id":"news","limit":0}),
        ),
        ("get_message_by_link", json!({"link":"invalid"})),
        ("get_last_responses", json!({"n":{}})),
        (
            "get_message_media",
            json!({"channel_id":"news","message_id":0}),
        ),
        (
            "transcribe_voice_message",
            json!({"channel_id":"news","message_id":0}),
        ),
        ("search_public_channels", json!({"query":""})),
        (
            "get_messages_batch",
            json!({"channel_id":"news","message_ids":[]}),
        ),
        ("resolve_channels", json!({"identifiers":[]})),
        (
            "get_channel_stats",
            json!({"channel_id":"news","days_back":0}),
        ),
        (
            "get_messages_media_batch",
            json!({"channel_id":"news","message_ids":[]}),
        ),
    ];
    for (index, (name, args)) in invalid.into_iter().enumerate() {
        let mut wire = Wire::start();
        let reply = call(&mut wire, index as i64 + 1, name, args, true).await;
        assert_eq!(reply["result"]["isError"], true, "{name}: {reply}");
        assert_eq!(envelope(&reply)["error"]["code"], "invalid_input", "{name}");
        wire.close().await;
    }
}

#[tokio::test]
async fn canonical_contract_wire_errors_preserve_types_and_mask_provider_details() {
    for (error, code, retry, after) in [
        (
            Error::RateLimit {
                retry_after_seconds: 7,
                detail: "PRIVATE PROVIDER VALUE".into(),
            },
            "rate_limited",
            true,
            Some(7),
        ),
        (
            Error::Timeout {
                operation: "fetch".into(),
                secs: 2,
            },
            "timeout",
            true,
            None,
        ),
        (
            Error::MessageNotFound {
                channel: "news".into(),
                message_id: 1,
            },
            "message_not_found",
            false,
            None,
        ),
        (
            Error::Auth("PRIVATE PROVIDER VALUE".into()),
            "unauthenticated",
            false,
            None,
        ),
        (
            Error::TelegramApi("PRIVATE PROVIDER VALUE".into()),
            "telegram_error",
            false,
            None,
        ),
        (
            Error::Unavailable(StateUnavailable::RateLimiter),
            "unavailable",
            false,
            None,
        ),
    ] {
        let mut client = MockTelegramClientTrait::new();
        client
            .expect_get_message_by_id()
            .return_once(move |_, _| Err(error));
        let mut wire = Wire::with_handler(McpServer::new(
            Arc::new(client),
            Arc::new(permissive_limiter()),
        ));
        let reply = call(
            &mut wire,
            1,
            "get_message_by_link",
            json!({"link":"https://t.me/news/1"}),
            true,
        )
        .await;
        let value = envelope(&reply);
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(value["error"]["code"], code);
        assert_eq!(value["error"]["retryable"], retry);
        assert!(!reply.to_string().contains("PRIVATE PROVIDER VALUE"));
        assert_eq!(
            value["error"]
                .get("retry_after_seconds")
                .and_then(Value::as_u64),
            after
        );
        wire.close().await;
    }
}

#[tokio::test]
async fn canonical_contract_rejects_conflicting_compact_headers() {
    for mode in [OutputFormat::JsonV1, OutputFormat::Legacy] {
        let mut client = MockTelegramClientTrait::new();
        client.expect_get_recent_messages().returning(|params| {
            let mut m = message();
            m.channel_name =
                types::ChannelName::new(params.channel_identifier.as_deref().unwrap_or("unknown"))
                    .expect("name");
            Ok(create_test_search_result(vec![m], "", 1))
        });
        let mut wire = Wire::with_handler(
            McpServer::new(Arc::new(client), Arc::new(permissive_limiter()))
                .with_output_format(mode),
        );
        let reply = call(
            &mut wire,
            1,
            "get_recent_messages",
            json!({"channel_ids":["first","second"],"format":"compact"}),
            true,
        )
        .await;
        assert_eq!(reply["result"]["isError"], true);
        if mode == OutputFormat::JsonV1 {
            assert_eq!(envelope(&reply)["error"]["code"], "ambiguous_channel");
        }
        wire.close().await;
    }
}

#[tokio::test]
async fn canonical_contract_bad_channel_is_rejected_before_rate_acquisition() {
    for name in [
        "generate_message_link",
        "get_message_media",
        "get_messages_batch",
        "get_messages_media_batch",
        "get_channel_stats",
    ] {
        let mut wire = Wire::start();
        let reply = call(
            &mut wire,
            1,
            name,
            json!({"channel_id":"0","message_id":1,"message_ids":[1]}),
            true,
        )
        .await;
        assert_eq!(reply["result"]["isError"], true, "{name}: {reply}");
        assert_eq!(envelope(&reply)["error"]["code"], "invalid_input");
        wire.close().await;
    }
}

#[tokio::test]
async fn canonical_contract_envelope_faults_and_argument_faults_are_distinct() {
    for modern_mode in [true, false] {
        let mut wire = start_mode(complete_server(OutputFormat::JsonV1), modern_mode).await;
        for (id, args) in [json!(true), json!([]), json!(1), json!("bad")]
            .into_iter()
            .enumerate()
        {
            let reply = call(
                &mut wire,
                id as i64 + 1,
                "get_messages_batch",
                args,
                modern_mode,
            )
            .await;
            assert!(reply.get("result").is_none(), "malformed envelope: {reply}");
            assert!(reply["error"]["code"].is_number());
        }
        let unknown = call(&mut wire, 10, "no_such_tool", json!({}), modern_mode).await;
        assert_eq!(unknown["error"]["code"], -32602);
        assert!(unknown.get("result").is_none());
        for (id, args) in [(11, Some(Value::Null)), (12, None), (13, Some(json!({})))] {
            let mut req = request(id, "tools/call", modern_mode);
            req["params"]["name"] = json!("get_messages_batch");
            if let Some(args) = args {
                req["params"]["arguments"] = args;
            }
            wire.send(req).await;
            let reply = wire.recv().await;
            assert_eq!(reply["result"]["isError"], true);
            assert_eq!(envelope(&reply)["error"]["code"], "invalid_input");
        }
        let coerced = call(
            &mut wire,
            14,
            "get_subscribed_channels",
            json!({"limit":"1","offset":"0","future_field":{"ignored":true}}),
            modern_mode,
        )
        .await;
        assert_eq!(envelope(&coerced)["data"]["returned"], 1);
        let no_required = call(
            &mut wire,
            15,
            "get_last_responses",
            Value::Null,
            modern_mode,
        )
        .await;
        assert_eq!(no_required["result"]["isError"], false);
        wire.close().await;
    }
}

#[tokio::test]
async fn canonical_contract_rate_refusals_are_typed_for_every_network_tool() {
    for (name, args) in cases()
        .into_iter()
        .filter(|(name, _)| !matches!(*name, "check_mcp_status" | "get_last_responses"))
    {
        let mut client = MockTelegramClientTrait::new();
        client
            .expect_cached_account_status()
            .returning(|| types::AccountStatus {
                authorized: true,
                premium: Some(true),
                ..Default::default()
            });
        let mut limiter = MockRateLimiterTrait::new();
        limiter.expect_acquire().times(1).return_once(|_| {
            Err(Error::RateLimit {
                retry_after_seconds: 9,
                detail: String::new(),
            })
        });
        let mut handler = McpServer::new(Arc::new(client), Arc::new(limiter));
        handler.desktop_launcher = Arc::new(FakeDesktop { fail: false });
        let mut wire = Wire::with_handler(handler);
        let reply = call(&mut wire, 1, name, args, true).await;
        assert_eq!(reply["result"]["isError"], true, "{name}: {reply}");
        let body = envelope(&reply);
        assert_eq!(body["error"]["code"], "rate_limited", "{name}");
        assert_eq!(body["error"]["retry_after_seconds"], 9);
        wire.close().await;
    }
}

#[tokio::test]
async fn canonical_contract_desktop_failure_is_error_but_legacy_keeps_old_shape() {
    for mode in [OutputFormat::JsonV1, OutputFormat::Legacy] {
        let mut handler = complete_server(mode);
        handler.desktop_launcher = Arc::new(FakeDesktop { fail: true });
        let mut wire = Wire::with_handler(handler);
        let reply = call(
            &mut wire,
            1,
            "open_message_in_telegram",
            json!({"channel_id":"news","message_id":1}),
            true,
        )
        .await;
        if mode == OutputFormat::JsonV1 {
            assert_eq!(reply["result"]["isError"], true);
            assert_eq!(envelope(&reply)["error"]["code"], "desktop_open_failed");
        } else {
            assert_ne!(reply["result"]["isError"], true);
            let value: Value = serde_json::from_str(
                reply["result"]["content"][0]["text"]
                    .as_str()
                    .expect("text"),
            )
            .expect("JSON");
            assert_eq!(value["success"], false);
            assert_eq!(value["app_opened"], false);
        }
        wire.close().await;
    }
}

#[tokio::test]
async fn canonical_contract_corrupt_replay_is_an_error() {
    let handler = complete_server(OutputFormat::JsonV1);
    handler
        .response_buffer()
        .try_push(crate::mcp::observability::BufferedResponse {
            request_id: "old".into(),
            tool_name: "get_message_by_link".into(),
            written_at: std::time::SystemTime::now(),
            size_bytes: 8,
            payload: "not JSON".into(),
        })
        .expect("store");
    let mut wire = Wire::with_handler(handler);
    let reply = call(&mut wire, 1, "get_last_responses", json!({}), true).await;
    assert_eq!(reply["result"]["isError"], true);
    assert_eq!(envelope(&reply)["error"]["code"], "internal_error");
    wire.close().await;
}

#[tokio::test]
async fn canonical_contract_invalid_fanout_reference_is_free() {
    for name in ["search_messages", "get_recent_messages"] {
        let mut wire = Wire::start();
        let reply = call(
            &mut wire,
            1,
            name,
            json!({"query":"q","channel_ids":["news","0"]}),
            true,
        )
        .await;
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(envelope(&reply)["error"]["code"], "invalid_input");
        wire.close().await;
    }
}

#[test]
fn canonical_contract_output_mode_config_and_route_metadata_agree() {
    let default: crate::config::ServerConfig = toml::from_str("").expect("defaults");
    assert_eq!(default.output_format, OutputFormat::JsonV1);
    let explicit: crate::config::ServerConfig =
        toml::from_str("output_format = 'legacy'").expect("legacy");
    assert_eq!(explicit.output_format, OutputFormat::Legacy);
    assert!(toml::from_str::<crate::config::ServerConfig>("output_format = 'unknown'").is_err());
    let mut handler = server();
    for mode in [
        OutputFormat::Legacy,
        OutputFormat::JsonV1,
        OutputFormat::Legacy,
    ] {
        handler = handler.with_output_format(mode);
        for listed in handler.tools_list_result().tools {
            let individual = rmcp::ServerHandler::get_tool(&handler, &listed.name).expect("tool");
            assert_eq!(
                individual.output_schema.is_some(),
                mode == OutputFormat::JsonV1
            );
            assert_eq!(individual.output_schema, listed.output_schema);
        }
    }
}

#[tokio::test]
async fn canonical_contract_all_missing_batches_keep_resolved_identity() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_get_messages_batch().returning(|_, _| {
        Ok(types::MessageBatch {
            channel_id: ChannelId::new(LARGE_ID).expect("id"),
            peer_namespace: PeerNamespace::Chat,
            messages: vec![],
            missing_ids: vec![1, 2],
        })
    });
    client
        .expect_download_messages_media()
        .returning(|_, _, _| {
            Ok(types::MediaBatch {
                channel_id: ChannelId::new(LARGE_ID).expect("id"),
                peer_namespace: PeerNamespace::Chat,
                outcomes: vec![
                    types::MediaFetchOutcome {
                        message_id: 1,
                        result: Err(types::MediaFetchError::NotFound),
                    },
                    types::MediaFetchOutcome {
                        message_id: 2,
                        result: Err(types::MediaFetchError::NoVisualMedia {
                            media_type: "poll".into(),
                        }),
                    },
                ],
            })
        });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().returning(|_| Ok(()));
    limiter
        .expect_refund()
        .with(mockall::predicate::eq(6))
        .times(1)
        .return_const(());
    let mut wire = Wire::with_handler(McpServer::new(Arc::new(client), Arc::new(limiter)));
    let reply = call(
        &mut wire,
        1,
        "get_messages_batch",
        json!({"channel_id":"news","message_ids":[1,2]}),
        true,
    )
    .await;
    let data = &envelope(&reply)["data"];
    assert_eq!(data["channel_id"], LARGE_ID.to_string());
    assert_eq!(data["peer_namespace"], "chat");
    assert_eq!(data["returned"], 0);
    assert_eq!(data["missing"].as_array().expect("missing").len(), 2);
    let reply = call(
        &mut wire,
        2,
        "get_messages_media_batch",
        json!({"channel_id":"news","message_ids":[1,2]}),
        true,
    )
    .await;
    let data = &envelope(&reply)["data"];
    assert_eq!(reply["result"]["isError"], false);
    assert_eq!(
        reply["result"]["content"]
            .as_array()
            .expect("content")
            .len(),
        1
    );
    assert_eq!(data["summary"]["returned"], 0);
    assert_eq!(data["summary"]["failed"], 2);
    for item in data["items"].as_array().expect("items") {
        assert_eq!(item["channel_id"], LARGE_ID.to_string());
        assert_eq!(item["peer_namespace"], "chat");
        assert!(item.get("content_index").is_none());
    }
    wire.close().await;
}

#[tokio::test]
async fn canonical_contract_partial_fanout_keeps_typed_errors_and_deadline_metadata() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_get_recent_messages().returning(|params| {
        if params.channel_identifier.as_deref() == Some("bad") {
            return Err(Error::Timeout {
                operation: "history".into(),
                secs: 5,
            });
        }
        let mut result = create_test_search_result(vec![message()], "", 1);
        result.query_metadata.partial = true;
        result.query_metadata.timed_out = true;
        Ok(result)
    });
    let mut wire = Wire::with_handler(McpServer::new(
        Arc::new(client),
        Arc::new(permissive_limiter()),
    ));
    let reply = call(
        &mut wire,
        1,
        "get_recent_messages",
        json!({"channel_ids":["news","bad"]}),
        true,
    )
    .await;
    let data = &envelope(&reply)["data"];
    assert_eq!(reply["result"]["isError"], false);
    assert_eq!(data["channel_errors"][0]["error"]["code"], "timeout");
    assert_eq!(data["query_metadata"]["partial"], true);
    assert_eq!(data["query_metadata"]["timed_out"], true);
    assert!(data.get("next_cursor").is_none());
    wire.close().await;
}

#[tokio::test]
async fn canonical_contract_full_output_distinguishes_same_number_namespaces() {
    for format in ["full", "compact"] {
        let mut client = MockTelegramClientTrait::new();
        client.expect_get_recent_messages().returning(|params| {
            let mut m = message();
            if params.channel_identifier.as_deref() == Some("group") {
                m.peer_namespace = Some(PeerNamespace::Chat);
            }
            Ok(create_test_search_result(vec![m], "", 1))
        });
        let mut wire = Wire::with_handler(McpServer::new(
            Arc::new(client),
            Arc::new(permissive_limiter()),
        ));
        let reply = call(
            &mut wire,
            1,
            "get_recent_messages",
            json!({"channel_ids":["group","news"],"format":format}),
            true,
        )
        .await;
        let value = envelope(&reply);
        if format == "compact" {
            assert_eq!(value["error"]["code"], "ambiguous_channel");
        } else {
            assert_eq!(reply["result"]["isError"], false);
            let data = &value["data"];
            assert_eq!(data["query_metadata"]["channels_in_results"], 2);
            let messages = data["messages"].as_array().expect("messages");
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[0]["channel_id"], messages[1]["channel_id"]);
            assert_ne!(messages[0]["peer_namespace"], messages[1]["peer_namespace"]);
        }
        wire.close().await;
    }
}

#[tokio::test]
async fn canonical_contract_status_retains_explicit_unavailability() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_cached_account_status()
        .returning(types::AccountStatus::default);
    let mut limiter = MockRateLimiterTrait::new();
    limiter
        .expect_try_available_tokens()
        .returning(|| Err(StateUnavailable::RateLimiter));
    limiter.expect_capacity().returning(|| 30.0);
    limiter.expect_refill_rate().returning(|| 1.0);
    let mut wire = Wire::with_handler(McpServer::new(Arc::new(client), Arc::new(limiter)));
    let reply = call(&mut wire, 1, "check_mcp_status", json!({}), true).await;
    let data = &envelope(&reply)["data"];
    assert_eq!(reply["result"]["isError"], false);
    assert!(data["rate_limiter"]["tokens"].is_null());
    assert!(data["rate_limiter"]["unavailable"].is_string());
    wire.close().await;
}

#[tokio::test]
async fn qualified_search_keeps_namespace_from_username_resolution() {
    for fanout in [false, true] {
        let mut client = MockTelegramClientTrait::new();
        client
            .expect_resolve_channel_identity()
            .times(1)
            .return_once(|_| {
                Ok(types::ChannelIdentity {
                    peer_namespace: PeerNamespace::Channel,
                    id: ChannelId::new(LARGE_ID).expect("id"),
                    username: Some("news".into()),
                })
            });
        client
            .expect_search_messages()
            .times(1)
            .return_once(|params| {
                if params.channel_namespace != Some(PeerNamespace::Channel) {
                    return Err(Error::AmbiguousChannel("same-number group/channel".into()));
                }
                Ok(create_test_search_result(vec![message()], "q", 1))
            });
        let mut wire = Wire::with_handler(McpServer::new(
            Arc::new(client),
            Arc::new(permissive_limiter()),
        ));
        let args = if fanout {
            json!({"query":"q","channel_ids":["news"]})
        } else {
            json!({"query":"q","channel_id":"news"})
        };
        let reply = call(&mut wire, 1, "search_messages", args, true).await;
        assert_eq!(reply["result"]["isError"], false);
        assert_eq!(
            envelope(&reply)["data"]["messages"][0]["peer_namespace"],
            "channel"
        );
        wire.close().await;
    }
}

#[test]
fn canonical_contract_boundary_refuses_null_success_data() {
    use crate::mcp::tools::types::contract::{ToolResponse, tool_result};
    assert!(
        tool_result(ToolResponse::success(Option::<u32>::None)).is_err(),
        "a generic serializer must not emit both data and error null"
    );
}
