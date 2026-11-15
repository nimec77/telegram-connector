use super::*;

fn budget_server(
    mode: OutputFormat,
    metadata_only: bool,
) -> McpServer<MockTelegramClientTrait, MockRateLimiterTrait> {
    let mut client = MockTelegramClientTrait::new();
    fn page(metadata_only: bool) -> types::SearchResult {
        let messages = if metadata_only {
            vec![]
        } else {
            (1..=6)
                .rev()
                .map(|id| {
                    let mut m = message();
                    m.id = types::MessageId::new(id).expect("id");
                    m.text = "\"\\🦀".repeat(400);
                    m.timestamp =
                        chrono::DateTime::from_timestamp(1_800_000_000, 0).expect("timestamp");
                    m
                })
                .collect()
        };
        let mut result = create_test_search_result(
            messages,
            &if metadata_only {
                "q".repeat(20_000)
            } else {
                "q".into()
            },
            1,
        );
        result.query_metadata.window_from =
            chrono::DateTime::from_timestamp(1_800_000_000, 0).expect("timestamp");
        result
    }
    client
        .expect_search_messages()
        .returning(move |_| Ok(page(metadata_only)));
    client
        .expect_get_recent_messages()
        .returning(move |_| Ok(page(metadata_only)));
    client.expect_get_messages_batch().returning(|_, _| {
        Ok(types::MessageBatch {
            channel_id: ChannelId::new(LARGE_ID).expect("id"),
            peer_namespace: PeerNamespace::Channel,
            messages: page(false).messages,
            missing_ids: vec![7],
        })
    });
    McpServer::new(Arc::new(client), Arc::new(permissive_limiter()))
        .with_output_format(mode)
        .with_response_byte_budget(14_000)
}
fn data(reply: &Value, mode: OutputFormat) -> Value {
    if mode == OutputFormat::JsonV1 {
        envelope(reply)["data"].clone()
    } else {
        serde_json::from_str(
            reply["result"]["content"][0]["text"]
                .as_str()
                .expect("text"),
        )
        .expect("legacy JSON")
    }
}

#[tokio::test]
async fn task8_message_budget_counts_the_entire_wire_response() {
    for mode in [OutputFormat::JsonV1, OutputFormat::Legacy] {
        for modern_mode in [true, false] {
            let mut wire = start_mode(budget_server(mode, false), modern_mode).await;
            for (id, name, args) in [
                (
                    1,
                    "search_messages",
                    json!({"query":"q","channel_id":LARGE_ID,"max_text_length":20_000}),
                ),
                (
                    2,
                    "get_recent_messages",
                    json!({"channel_id":LARGE_ID,"max_text_length":20_000}),
                ),
                (
                    3,
                    "get_messages_batch",
                    json!({"channel_id":LARGE_ID,"message_ids":[6,5,4,3,2,1,7],"max_text_length":20_000}),
                ),
                (
                    4,
                    "get_recent_messages",
                    json!({"channel_ids":[LARGE_ID.to_string()],"max_text_length":20_000,"format":"compact"}),
                ),
            ] {
                let reply = call(&mut wire, id, name, args, modern_mode).await;
                let bytes = serde_json::to_vec(&reply).expect("wire JSON").len();
                assert!(
                    bytes <= 14_000,
                    "{name} {mode:?} modern={modern_mode}: {bytes} bytes"
                );
                let body = data(&reply, mode);
                let messages = body["messages"].as_array().expect("messages");
                assert!(!messages.is_empty());
                assert!(messages.len() < 6);
                assert_eq!(body["returned"], messages.len());
                if name == "get_messages_batch" {
                    assert!(!body["omitted_ids"].as_array().expect("omitted").is_empty());
                    assert_eq!(body["missing"].as_array().expect("missing").len(), 1);
                } else if id == 4 {
                    assert!(body.get("next_cursor").is_none());
                } else {
                    assert_eq!(
                        body["next_cursor"]["before_id"],
                        messages.last().expect("last")["id"]
                    );
                }
            }
            wire.close().await;
        }
    }
}

#[tokio::test]
async fn task8_metadata_only_overflow_is_a_bounded_tool_error() {
    let mut wire = Wire::with_handler(budget_server(OutputFormat::JsonV1, true));
    let reply = call(&mut wire, 1, "search_messages", json!({"query":"q"}), true).await;
    assert_eq!(envelope(&reply)["error"]["code"], "response_too_large");
    assert_eq!(reply["result"]["isError"], true);
    assert!(serde_json::to_vec(&reply).expect("wire JSON").len() < 1_000);
    wire.close().await;
}

#[tokio::test]
async fn task8_one_message_floor_survives_when_only_the_message_is_oversized() {
    let mut wire = Wire::with_handler(
        budget_server(OutputFormat::JsonV1, false).with_response_byte_budget(2_000),
    );
    let reply = call(
        &mut wire,
        1,
        "get_recent_messages",
        json!({"channel_id":LARGE_ID,"max_text_length":20_000}),
        true,
    )
    .await;
    let body = envelope(&reply);
    assert_eq!(body["data"]["messages"].as_array().expect("floor").len(), 1);
    assert_eq!(body["data"]["has_more"], true);
    wire.close().await;
}

#[tokio::test]
async fn task8_media_overhead_overflow_returns_no_images_and_refunds() {
    for mode in [OutputFormat::JsonV1, OutputFormat::Legacy] {
        let mut client = MockTelegramClientTrait::new();
        client
            .expect_download_messages_media_pipeline()
            .return_once(|_, _, _| {
                let mut image = photo();
                image.caption = Some("\"".repeat(40_000));
                Ok(types::MediaBatch {
                    stream: None,
                    deadline: None,
                    channel_id: ChannelId::new(LARGE_ID).expect("id"),
                    peer_namespace: PeerNamespace::Channel,
                    outcomes: vec![types::MediaFetchOutcome {
                        message_id: 1,
                        result: Ok(image),
                    }],
                })
            });
        let mut limiter = MockRateLimiterTrait::new();
        limiter
            .expect_acquire()
            .with(mockall::predicate::eq(3))
            .times(1)
            .returning(|_| Ok(()));
        let refunded = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let observed = Arc::clone(&refunded);
        limiter.expect_refund().returning(move |amount| {
            observed.fetch_add(amount, std::sync::atomic::Ordering::SeqCst);
        });
        let mut wire = Wire::with_handler(
            McpServer::new(Arc::new(client), Arc::new(limiter)).with_output_format(mode),
        );
        let reply = call(
            &mut wire,
            1,
            "get_messages_media_batch",
            json!({"channel_id":"news","message_ids":[1]}),
            true,
        )
        .await;
        assert_eq!(reply["result"]["isError"], true, "{mode:?}");
        assert_eq!(refunded.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert_eq!(
            reply["result"]["content"]
                .as_array()
                .expect("content")
                .len(),
            1
        );
        assert!(serde_json::to_vec(&reply).expect("JSON").len() < 1_000);
        if mode == OutputFormat::JsonV1 {
            assert_eq!(envelope(&reply)["error"]["code"], "response_too_large");
        }
        wire.close().await;
    }
}

#[tokio::test]
async fn task8_mixed_media_indices_point_to_actual_image_blocks() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media_pipeline()
        .return_once(|_, _, _| {
            Ok(types::MediaBatch {
                stream: None,
                deadline: None,
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
                    types::MediaFetchOutcome {
                        message_id: 3,
                        result: Err(types::MediaFetchError::Failed(Error::DownloadFailed(
                            "fixture".into(),
                        ))),
                    },
                    types::MediaFetchOutcome {
                        message_id: 4,
                        result: Ok(photo()),
                    },
                ],
            })
        });
    let mut wire = Wire::with_handler(McpServer::new(
        Arc::new(client),
        Arc::new(permissive_limiter()),
    ));
    let reply = call(
        &mut wire,
        1,
        "get_messages_media_batch",
        json!({"channel_id":"news","message_ids":[1,2,3,4]}),
        true,
    )
    .await;
    let body = envelope(&reply);
    let items = body["data"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 4);
    assert_eq!(body["data"]["summary"]["returned"], 2);
    assert_eq!(body["data"]["summary"]["failed"], 2);
    let blocks = reply["result"]["content"].as_array().expect("content");
    assert_eq!(blocks.len(), 3);
    for (item, expected) in items.iter().zip([Some(1), None, None, Some(2)]) {
        if let Some(index) = expected {
            assert_eq!(item["content_index"], index);
            assert_eq!(blocks[index]["type"], "image");
        } else {
            assert!(item.get("content_index").is_none());
            assert!(item["error"].is_object());
        }
    }
    assert!(
        !body
            .to_string()
            .contains(blocks[1]["data"].as_str().expect("image bytes"))
    );
    wire.close().await;
}

#[tokio::test]
async fn task8_media_payload_uses_its_own_budget_above_forty_thousand_bytes() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media_pipeline()
        .return_once(|_, _, _| {
            let pixels = image::RgbImage::from_fn(256, 256, |x, y| {
                let value = (x.wrapping_mul(747_796_405) ^ y.wrapping_mul(2_891_336_453))
                    .wrapping_mul(277_803_737);
                image::Rgb([value as u8, (value >> 8) as u8, (value >> 16) as u8])
            });
            let mut bytes = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
                .encode_image(&pixels)
                .expect("fixture JPEG");
            let mut image = photo();
            image.bytes = bytes;
            image.width = Some(256);
            image.height = Some(256);
            Ok(types::MediaBatch {
                stream: None,
                deadline: None,
                channel_id: ChannelId::new(LARGE_ID).expect("id"),
                peer_namespace: PeerNamespace::Channel,
                outcomes: vec![types::MediaFetchOutcome {
                    message_id: 1,
                    result: Ok(image),
                }],
            })
        });
    let mut wire = Wire::with_handler(
        McpServer::new(Arc::new(client), Arc::new(permissive_limiter()))
            .with_response_byte_budget(1_000),
    );
    let reply = call(
        &mut wire,
        1,
        "get_messages_media_batch",
        json!({"channel_id":"news","message_ids":[1]}),
        true,
    )
    .await;
    assert_eq!(reply["result"]["isError"], false);
    assert!(serde_json::to_vec(&reply).expect("wire JSON").len() > 40_000);
    assert_eq!(envelope(&reply)["data"]["summary"]["returned"], 1);
    wire.close().await;
}

#[tokio::test(start_paused = true)]
async fn task8_pipeline_deadline_keeps_completed_images_and_refunds_remaining_items() {
    let clock = crate::test_helpers::ManualClock::start().await;
    let processing_complete = Arc::new(tokio::sync::Notify::new());
    let observed = Arc::clone(&processing_complete);
    use futures::StreamExt as _;
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media_pipeline()
        .times(1)
        .return_once(move |_, _, _| {
            let stream = futures::stream::iter(vec![types::MediaFetchOutcome {
                message_id: 1,
                result: Ok(photo()),
            }])
            .chain(futures::stream::poll_fn(move |_| {
                // The next outcome is requested only after the first image has
                // passed processing and been collected by the production loop.
                observed.notify_one();
                std::task::Poll::Pending
            }))
            .boxed();
            Ok(types::MediaBatch {
                channel_id: ChannelId::new(LARGE_ID).expect("id"),
                peer_namespace: PeerNamespace::Channel,
                outcomes: Vec::new(),
                stream: Some(stream),
                deadline: Some(
                    crate::telegram::media_pipeline::MediaDeadline::try_new(1).expect("deadline"),
                ),
            })
        });
    let mut limiter = MockRateLimiterTrait::new();
    limiter
        .expect_acquire()
        .with(mockall::predicate::eq(9))
        .times(1)
        .returning(|_| Ok(()));
    limiter
        .expect_refund()
        .with(mockall::predicate::eq(6))
        .times(1)
        .return_const(());
    let mut wire = Wire::with_handler(McpServer::new(Arc::new(client), Arc::new(limiter)));
    let mut request = request(1, "tools/call", true);
    request["params"]["name"] = json!("get_messages_media_batch");
    request["params"]["arguments"] = json!({"channel_id":"news","message_ids":[1,2,3]});
    let started = tokio::time::Instant::now();
    wire.send(request).await;
    processing_complete.notified().await;
    assert_eq!(
        started.elapsed(),
        Duration::ZERO,
        "real image work must not consume virtual time"
    );
    tokio::time::advance(Duration::from_secs(1)).await;
    let reply = wire.recv().await;
    assert_eq!(started.elapsed(), Duration::from_secs(1));
    let body = envelope(&reply);
    assert_eq!(reply["result"]["isError"], false);
    assert_eq!(body["data"]["summary"]["returned"], 1);
    assert_eq!(body["data"]["summary"]["failed"], 2);
    for (item, id) in body["data"]["items"]
        .as_array()
        .expect("items")
        .iter()
        .skip(1)
        .zip([2, 3])
    {
        assert_eq!(item["message_id"], id);
        assert_eq!(item["error"]["code"], "timeout");
        assert!(item.get("content_index").is_none());
    }
    wire.close().await;
    clock.finish().await;
}

#[tokio::test]
async fn task8_sdk_embedding_counts_the_actual_rpc_id_without_the_custom_runtime() {
    let handler = budget_server(OutputFormat::JsonV1, false).with_response_byte_budget(25_000);
    let mut request = modern(json!("x".repeat(10_000)), "tools/call");
    request["params"]["name"] = json!("get_recent_messages");
    request["params"]["arguments"] = json!({"channel_id":LARGE_ID,"max_text_length":20_000});
    let reply = sdk_handler_round_trip(handler, request).await;
    let bytes = serde_json::to_vec(&reply).expect("wire JSON").len();
    assert!(
        bytes <= 25_000,
        "SDK-only embedding returned {bytes} bytes under a 25000-byte budget"
    );
    assert!(
        !envelope(&reply)["data"]["messages"]
            .as_array()
            .expect("messages")
            .is_empty()
    );
}

#[tokio::test]
async fn o02_wire_fitting_keeps_rows_when_typed_channel_errors_shrink() {
    for modern_mode in [true, false] {
        let mut client = MockTelegramClientTrait::new();
        client.expect_search_messages().returning(|params| {
            if params.channel_id.expect("scoped").get() == 222 {
                Err(crate::error::Error::TelegramApi(
                    "upstream detail".repeat(10_000),
                ))
            } else {
                Ok(create_test_search_result(
                    (1..=6)
                        .rev()
                        .map(|id| crate::test_helpers::create_test_message(id, "я\"\\🦀", 111))
                        .collect(),
                    "q",
                    1,
                ))
            }
        });
        client.expect_get_recent_messages().returning(|params| {
            if params.channel_id.expect("scoped").get() == 222 {
                Err(crate::error::Error::TelegramApi(
                    "upstream detail".repeat(10_000),
                ))
            } else {
                Ok(create_test_search_result(
                    (1..=6)
                        .rev()
                        .map(|id| crate::test_helpers::create_test_message(id, "я\"\\🦀", 111))
                        .collect(),
                    "",
                    1,
                ))
            }
        });
        let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()))
            .with_output_format(OutputFormat::JsonV1)
            .with_response_byte_budget(14_000);
        // The direct compatibility methods intentionally keep the historical raw fit.
        let direct_args = json!({"query":"q","channel_ids":["111","222"],"limit":100});
        let direct = server
            .search_messages_impl(
                serde_json::from_value(direct_args.clone()).expect("search request"),
            )
            .await
            .expect("direct search");
        assert_eq!(
            serde_json::from_str::<Value>(&direct).expect("legacy DTO")["returned"],
            1
        );
        let direct = server
            .get_recent_messages_impl(serde_json::from_value(direct_args).expect("history request"))
            .await
            .expect("direct history");
        assert_eq!(
            serde_json::from_str::<Value>(&direct).expect("legacy DTO")["returned"],
            1
        );
        let mut wire = start_mode(server, modern_mode).await;
        for (id, name) in [(1, "search_messages"), (2, "get_recent_messages")] {
            let reply = call(
                &mut wire,
                id,
                name,
                json!({"query":"q","channel_ids":["111","222"],"limit":100}),
                modern_mode,
            )
            .await;
            let body = envelope(&reply);
            assert_eq!(body["data"]["returned"], 6, "{name}: {reply}");
            assert_eq!(
                body["data"]["channel_errors"]
                    .as_array()
                    .expect("partial failures")
                    .len(),
                1
            );
            assert!(serde_json::to_vec(&reply).expect("actual frame").len() <= 14_000);
        }
        wire.close().await;
    }
}

async fn o02_call_with_id(
    wire: &mut Wire,
    id: Value,
    name: &str,
    args: Value,
    modern_mode: bool,
) -> Value {
    let mut request = if modern_mode {
        modern(id.clone(), "tools/call")
    } else {
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{}})
    };
    request["params"]["name"] = json!(name);
    request["params"]["arguments"] = args;
    wire.send(request).await;
    let reply = wire.recv().await;
    assert_eq!(reply["id"], id, "original external ID");
    reply
}

#[tokio::test]
async fn o02_exact_frame_threshold_preserves_external_ids_and_greedy_rows() {
    for mode in [OutputFormat::JsonV1, OutputFormat::Legacy] {
        for modern_mode in [true, false] {
            for compact in [false, true] {
                for name in [
                    "search_messages",
                    "get_recent_messages",
                    "get_messages_batch",
                ] {
                    let id = json!("escaped\"\\я🦀".repeat(100));
                    let args = json!({"query":"q","channel_id":LARGE_ID,"message_ids":[6,5,4,3,2,1,7],
                        "max_text_length":20_000,"format":if compact {"compact"} else {"full"}});
                    let mut wire = start_mode(
                        budget_server(mode, false).with_response_byte_budget(1_000_000),
                        modern_mode,
                    )
                    .await;
                    let reply =
                        o02_call_with_id(&mut wire, id.clone(), name, args.clone(), modern_mode)
                            .await;
                    let exact = serde_json::to_vec(&reply).expect("complete frame").len();
                    assert_eq!(data(&reply, mode)["returned"], 6);
                    wire.close().await;
                    let mut thresholds = vec![(exact, 6), (exact - 1, 5)];
                    // Independently assemble exact complete frames for shorter prefixes.
                    // This checks that the row bound never jumps past a fitting prefix.
                    for retained in [4usize, 2] {
                        let mut expected = reply.clone();
                        let mut body = data(&reply, mode);
                        body["messages"]
                            .as_array_mut()
                            .expect("messages")
                            .truncate(retained);
                        body["returned"] = json!(retained);
                        if name == "get_messages_batch" {
                            body["omitted_ids"] =
                                json!((1..=(6 - retained)).rev().collect::<Vec<_>>());
                        } else {
                            body["has_more"] = json!(true);
                            body["next_cursor"] = json!({"before_id":7-retained});
                        }
                        if mode == OutputFormat::JsonV1 {
                            expected["result"]["structuredContent"]["data"] = body;
                            expected["result"]["content"][0]["text"] = json!(
                                serde_json::to_string(&expected["result"]["structuredContent"])
                                    .expect("matching fallback")
                            );
                        } else {
                            expected["result"]["content"][0]["text"] =
                                json!(serde_json::to_string(&body).expect("legacy fallback"));
                        }
                        thresholds.push((
                            serde_json::to_vec(&expected)
                                .expect("exact prefix frame")
                                .len(),
                            retained,
                        ));
                    }
                    for (budget, retained) in thresholds {
                        let mut wire = start_mode(
                            budget_server(mode, false).with_response_byte_budget(budget as u64),
                            modern_mode,
                        )
                        .await;
                        let reply = o02_call_with_id(
                            &mut wire,
                            id.clone(),
                            name,
                            args.clone(),
                            modern_mode,
                        )
                        .await;
                        let bytes = serde_json::to_vec(&reply).expect("complete frame").len();
                        let body = data(&reply, mode);
                        assert_eq!(
                            body["returned"], retained,
                            "{name} {mode:?} compact={compact} modern={modern_mode} budget={budget} bytes={bytes}"
                        );
                        assert!(bytes <= budget);
                        if retained == 6 {
                            assert_eq!(bytes, exact);
                        }
                        if retained == 5 {
                            assert_eq!(
                                body["messages"]
                                    .as_array()
                                    .expect("rows")
                                    .iter()
                                    .map(|m| m["id"].as_i64().unwrap())
                                    .collect::<Vec<_>>(),
                                vec![6, 5, 4, 3, 2]
                            );
                            if name == "get_messages_batch" {
                                assert_eq!(body["omitted_ids"], json!([1]));
                                assert_eq!(body["missing"].as_array().expect("missing").len(), 1);
                            } else {
                                assert_eq!(body["next_cursor"]["before_id"], 2);
                                assert_eq!(body["has_more"], true);
                            }
                        }
                        wire.close().await;
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn o02_direct_adapters_keep_raw_fitting_in_both_output_modes_and_wire_contexts() {
    use rmcp::handler::server::common::RequestId;
    use rmcp::handler::server::wrapper::Parameters;
    for mode in [OutputFormat::JsonV1, OutputFormat::Legacy] {
        for modern_mode in [true, false] {
            let server = budget_server(mode, false);
            for name in [
                "search_messages",
                "get_recent_messages",
                "get_messages_batch",
            ] {
                let args = json!({"query":"q","channel_id":LARGE_ID,"message_ids":[6,5,4,3,2,1,7],"max_text_length":20_000});
                let invoke = || async {
                    let id = RequestId(rmcp::model::NumberOrString::Number(9));
                    match name {
                        "search_messages" => {
                            server
                                .search_messages(
                                    Parameters(serde_json::from_value(args.clone()).unwrap()),
                                    id,
                                )
                                .await
                        }
                        "get_recent_messages" => {
                            server
                                .get_recent_messages(
                                    Parameters(serde_json::from_value(args.clone()).unwrap()),
                                    id,
                                )
                                .await
                        }
                        _ => {
                            server
                                .get_messages_batch(
                                    Parameters(serde_json::from_value(args.clone()).unwrap()),
                                    id,
                                )
                                .await
                        }
                    }
                    .expect("direct adapter")
                };
                let direct = invoke().await;
                let contextual = crate::mcp::runtime::scope_response(
                    rmcp::model::RequestId::String("long\"\\я🦀".repeat(1_000).into()),
                    Some(if modern_mode {
                        rmcp::model::ProtocolVersion::V_2026_07_28
                    } else {
                        rmcp::model::ProtocolVersion::V_2025_11_25
                    }),
                    invoke(),
                )
                .await;
                assert_eq!(direct, contextual, "direct budget must ignore wire context");
                let body: Value = serde_json::from_str(&direct).expect("legacy DTO");
                assert!(direct.len() <= 14_000);
                assert_eq!(body["returned"], 3, "raw DTO retains three rows for {name}");
                assert_eq!(
                    body["messages"]
                        .as_array()
                        .expect("rows")
                        .iter()
                        .map(|m| m["id"].as_i64().unwrap())
                        .collect::<Vec<_>>(),
                    vec![6, 5, 4]
                );
            }
        }
    }
}

#[tokio::test]
async fn o02_floor_requires_every_actual_metadata_byte_in_both_protocols() {
    for mode in [OutputFormat::JsonV1, OutputFormat::Legacy] {
        for modern_mode in [true, false] {
            for name in [
                "search_messages",
                "get_recent_messages",
                "get_messages_batch",
            ] {
                let id = json!("floor\"\\я🦀".repeat(100));
                let args = json!({"query":"q","channel_id":LARGE_ID,"message_ids":[6,5,4,3,2,1,7],"max_text_length":20_000,"format":"compact"});
                let mut wire = start_mode(
                    budget_server(mode, false).with_response_byte_budget(5_000),
                    modern_mode,
                )
                .await;
                let reply =
                    o02_call_with_id(&mut wire, id.clone(), name, args.clone(), modern_mode).await;
                assert_eq!(data(&reply, mode)["returned"], 1);
                wire.close().await;
                // Derive a metadata-only frame from the actual accepted response, retaining
                // its ID, identity, compact headers, cursor, errors and omitted IDs.
                let mut metadata = reply.clone();
                if mode == OutputFormat::JsonV1 {
                    let body = &mut metadata["result"]["structuredContent"];
                    body["data"]["messages"] = json!([]);
                    body["data"]["returned"] = json!(0);
                    if name != "get_messages_batch" {
                        body["data"]["query_metadata"]["channels_in_results"] = json!(0);
                    }
                    let text = serde_json::to_string(body).expect("matching fallback");
                    metadata["result"]["content"][0]["text"] = json!(text);
                } else {
                    let mut body = data(&reply, mode);
                    body["messages"] = json!([]);
                    body["returned"] = json!(0);
                    metadata["result"]["content"][0]["text"] =
                        json!(serde_json::to_string(&body).expect("legacy metadata"));
                }
                let exact = serde_json::to_vec(&metadata)
                    .expect("full metadata frame")
                    .len();
                for (budget, overflow) in [(exact, false), (exact - 1, true)] {
                    let mut wire = start_mode(
                        budget_server(mode, false).with_response_byte_budget(budget as u64),
                        modern_mode,
                    )
                    .await;
                    let reply =
                        o02_call_with_id(&mut wire, id.clone(), name, args.clone(), modern_mode)
                            .await;
                    assert_eq!(
                        reply["result"]["isError"], overflow,
                        "{name} {mode:?} modern={modern_mode} budget={budget}: {reply}"
                    );
                    if overflow {
                        if mode == OutputFormat::JsonV1 {
                            assert_eq!(envelope(&reply)["error"]["code"], "response_too_large");
                        }
                    } else {
                        assert_eq!(data(&reply, mode)["returned"], 1);
                        assert!(serde_json::to_vec(&reply).expect("floor frame").len() > budget);
                    }
                    wire.close().await;
                }
            }
        }
    }
}

#[tokio::test]
async fn o02_wire_fitting_never_hides_conflicting_compact_headers() {
    for mode in [OutputFormat::JsonV1, OutputFormat::Legacy] {
        for fanout in [false, true] {
            let mut client = MockTelegramClientTrait::new();
            client.expect_get_recent_messages().returning(|_| {
                let first = crate::test_helpers::create_test_message(
                    2,
                    "large".repeat(2_000).as_str(),
                    100,
                );
                let mut second = crate::test_helpers::create_test_message(1, "conflict", 100);
                second.peer_namespace = Some(PeerNamespace::Chat);
                Ok(create_test_search_result(vec![first, second], "", 2))
            });
            let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()))
                .with_output_format(mode)
                .with_response_byte_budget(2_000);
            let mut wire = start_mode(server, true).await;
            let mut args = json!({"format":"compact"});
            if fanout {
                args["channel_ids"] = json!(["100"]);
            } else {
                args["channel_id"] = json!("100");
            }
            let reply = call(&mut wire, 1, "get_recent_messages", args, true).await;
            assert_eq!(reply["result"]["isError"], true);
            if mode == OutputFormat::JsonV1 {
                assert_eq!(envelope(&reply)["error"]["code"], "ambiguous_channel");
            }
            wire.close().await;
        }
    }
}
