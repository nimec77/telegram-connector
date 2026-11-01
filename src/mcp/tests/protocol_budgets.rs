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
                    m
                })
                .collect()
        };
        create_test_search_result(
            messages,
            &if metadata_only {
                "q".repeat(20_000)
            } else {
                "q".into()
            },
            1,
        )
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
    use futures::StreamExt as _;
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media_pipeline()
        .times(1)
        .return_once(|_, _, _| {
            let stream = futures::stream::iter(vec![types::MediaFetchOutcome {
                message_id: 1,
                result: Ok(photo()),
            }])
            .chain(futures::stream::pending())
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
    let reply = call(
        &mut wire,
        1,
        "get_messages_media_batch",
        json!({"channel_id":"news","message_ids":[1,2,3]}),
        true,
    )
    .await;
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
