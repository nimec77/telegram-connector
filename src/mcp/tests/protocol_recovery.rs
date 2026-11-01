use super::*;

#[tokio::test]
async fn task8_recovery_selects_only_explicit_keys_despite_reused_rpc_ids() {
    let mut wire = Wire::with_handler(complete_server(OutputFormat::JsonV1));
    let original_a = call(
        &mut wire,
        7,
        "get_message_by_link",
        json!({"link":"https://t.me/news/1","recovery_key":"conversation-a"}),
        true,
    )
    .await;
    let original_b = call(
        &mut wire,
        7,
        "get_messages_batch",
        json!({"channel_id":"news","message_ids":[1,2],"recovery_key":"conversation-b"}),
        true,
    )
    .await;
    for (id, key, original) in [
        (8, "conversation-a", original_a),
        (9, "conversation-b", original_b),
    ] {
        let recovered = call(
            &mut wire,
            id,
            "get_last_responses",
            json!({"recovery_keys":[key]}),
            true,
        )
        .await;
        let body = envelope(&recovered);
        assert_eq!(
            body["data"]["buffered"], 2,
            "count includes all retained keyed entries"
        );
        let responses = body["data"]["responses"]
            .as_array()
            .expect("selected results");
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["recovery_key"], key);
        assert_eq!(responses[0]["response"], original);
    }
    let missing = call(
        &mut wire,
        10,
        "get_last_responses",
        json!({"recovery_keys":["unknown"]}),
        true,
    )
    .await;
    assert_eq!(
        envelope(&missing)["data"]["responses"][0]["error"]["code"],
        "response_unavailable"
    );
    wire.close().await;
}

#[tokio::test]
async fn task8_duplicate_and_invalid_recovery_keys_are_rejected_before_rpc() {
    let mut client = MockTelegramClientTrait::new();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    client
        .expect_get_subscribed_channels()
        .returning(move |_, _| {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(types::ChannelPage {
                channels: vec![],
                total: 0,
            })
        });
    let mut wire = Wire::with_handler(McpServer::new(
        Arc::new(client),
        Arc::new(permissive_limiter()),
    ));
    let first = call(
        &mut wire,
        1,
        "get_subscribed_channels",
        json!({"recovery_key":"same"}),
        true,
    )
    .await;
    assert_eq!(first["result"]["isError"], false);
    let duplicate = call(
        &mut wire,
        2,
        "get_subscribed_channels",
        json!({"recovery_key":"same"}),
        true,
    )
    .await;
    assert_eq!(
        envelope(&duplicate)["error"]["code"],
        "duplicate_recovery_key"
    );
    for (id, key) in [
        json!(""),
        json!("a".repeat(129)),
        json!("bad\nkey"),
        json!("é"),
        json!(1),
        json!(["key"]),
    ]
    .into_iter()
    .enumerate()
    {
        let reply = call(
            &mut wire,
            id as i64 + 3,
            "get_subscribed_channels",
            json!({"recovery_key":key}),
            true,
        )
        .await;
        assert_eq!(
            envelope(&reply)["error"]["code"],
            "invalid_input",
            "{reply}"
        );
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    wire.close().await;
}

#[tokio::test]
async fn task8_canonical_recovery_requires_selection_and_cannot_itself_be_reserved() {
    let mut wire = Wire::with_handler(complete_server(OutputFormat::JsonV1));
    for (id, args) in [
        (1, json!({})),
        (2, json!({"recovery_keys":[]})),
        (3, json!({"recovery_keys":["x"],"recovery_key":"recursive"})),
    ] {
        let reply = call(&mut wire, id, "get_last_responses", args, true).await;
        assert_eq!(envelope(&reply)["error"]["code"], "invalid_input");
    }
    wire.close().await;
}

#[tokio::test]
async fn task8_every_recoverable_wire_schema_advertises_the_shared_key() {
    let mut wire = Wire::start();
    wire.send(modern(json!(1), "tools/list")).await;
    let reply = wire.recv().await;
    for tool in reply["result"]["tools"].as_array().expect("tools") {
        if tool["name"] == "get_last_responses" {
            assert!(tool["inputSchema"]["properties"]["recovery_keys"].is_object());
        } else {
            assert!(
                tool["inputSchema"]["properties"]["recovery_key"].is_object(),
                "{}",
                tool["name"]
            );
        }
    }
    wire.close().await;
}
