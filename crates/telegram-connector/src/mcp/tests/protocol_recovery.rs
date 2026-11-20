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

#[tokio::test(start_paused = true)]
async fn task8_pending_duplicate_and_cancelled_recovery_share_the_real_request_lifecycle() {
    let (client, _unpolled_runner, _directory) = crate::telegram::client::stalled_client().await;
    let mut limiter = MockRateLimiterTrait::new();
    limiter
        .expect_acquire()
        .with(mockall::predicate::eq(3))
        .times(1)
        .returning(|_| Ok(()));
    let mut wire = Wire::with_handler(McpServer::new(Arc::new(client), Arc::new(limiter)));
    let mut original = modern(json!(1), "tools/call");
    original["params"]["name"] = json!("get_message_media");
    original["params"]["arguments"] =
        json!({"channel_id":"100","message_id":1,"recovery_key":"pending"});
    original["params"]["_meta"]["progressToken"] = json!("ready");
    wire.send(original).await;
    assert_eq!(wire.recv().await["method"], "notifications/progress");
    let duplicate = call(
        &mut wire,
        2,
        "get_message_media",
        json!({"channel_id":"100","message_id":1,"recovery_key":"pending"}),
        true,
    )
    .await;
    assert_eq!(
        envelope(&duplicate)["error"]["code"],
        "duplicate_recovery_key"
    );
    let pending = call(
        &mut wire,
        3,
        "get_last_responses",
        json!({"recovery_keys":["pending"]}),
        true,
    )
    .await;
    assert_eq!(
        envelope(&pending)["data"]["responses"][0]["error"]["code"],
        "response_pending"
    );
    wire.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}))
        .await;
    let mut unavailable = false;
    for id in 4..14 {
        let reply = call(
            &mut wire,
            id,
            "get_last_responses",
            json!({"recovery_keys":["pending"]}),
            true,
        )
        .await;
        let body = envelope(&reply);
        if body["data"]["responses"][0]["error"]["code"] == "response_unavailable" {
            unavailable = true;
            break;
        }
        assert_eq!(
            body["data"]["responses"][0]["error"]["code"],
            "response_pending"
        );
        tokio::task::yield_now().await;
    }
    assert!(unavailable, "cancelled reservation stayed pending");
    wire.close().await;
}

struct RecoveryAdmission {
    server: McpServer<MockTelegramClientTrait, MockRateLimiterTrait>,
    replacement_started: Arc<tokio::sync::Notify>,
}
impl rmcp::ServerHandler for RecoveryAdmission {
    fn get_info(&self) -> rmcp::model::InitializeResult {
        rmcp::ServerHandler::get_info(&self.server)
    }
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        if request.name == "get_messages_batch" {
            self.replacement_started.notify_one();
        }
        rmcp::ServerHandler::call_tool(&self.server, request, context).await
    }
}

#[tokio::test(start_paused = true)]
async fn cancelled_committed_response_keeps_distinct_recovery_keys_for_reused_ids() {
    use tokio::io::AsyncReadExt;
    for modern_mode in [false, true] {
        for id in [json!(7), json!("same-id-with-longer-wire-framing")] {
            let replacement_started = Arc::new(tokio::sync::Notify::new());
            let handler = RecoveryAdmission {
                server: complete_server(OutputFormat::JsonV1),
                replacement_started: Arc::clone(&replacement_started),
            };
            let mut wire = Wire::configured(handler, crate::config::ServerConfig::default(), 256);
            if !modern_mode {
                wire.send(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
                    "protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"identity-test","version":"1"}}})).await;
                assert_eq!(wire.recv().await["id"], 0);
                wire.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
                    .await;
            }
            let mut old = request(7, "tools/call", modern_mode);
            old["id"] = id.clone();
            old["params"]["name"] = json!("get_message_by_link");
            old["params"]["arguments"] =
                json!({"link":"https://t.me/news/1","recovery_key":"old-lifetime"});
            wire.send(old).await;
            let mut first = [0];
            wire.read
                .read_exact(&mut first)
                .await
                .expect("old frame committed");
            wire.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}})).await;
            let mut replacement = request(7, "tools/call", modern_mode);
            replacement["id"] = id.clone();
            replacement["params"]["name"] = json!("get_messages_batch");
            replacement["params"]["arguments"] =
                json!({"channel_id":"news","message_ids":[1,2],"recovery_key":"new-lifetime"});
            wire.send(replacement).await;
            tokio::time::timeout(Duration::from_secs(1), replacement_started.notified())
                .await
                .expect("replacement admitted before old frame drains");
            let mut old_line = String::from_utf8(first.to_vec()).expect("frame prefix");
            wire.read
                .read_line(&mut old_line)
                .await
                .expect("old frame completes");
            let old: Value = serde_json::from_str(&old_line).expect("old JSON");
            let replacement = wire.recv().await;
            assert_eq!(old["id"], id);
            assert_eq!(replacement["id"], id);
            assert_eq!(old["result"]["isError"], false);
            assert_eq!(replacement["result"]["isError"], false);
            assert_ne!(old["result"], replacement["result"]);
            for (key, expected) in [("old-lifetime", old), ("new-lifetime", replacement)] {
                let recovered = call(
                    &mut wire,
                    99,
                    "get_last_responses",
                    json!({"recovery_keys":[key]}),
                    modern_mode,
                )
                .await;
                assert_eq!(
                    envelope(&recovered)["data"]["responses"][0]["response"],
                    expected,
                    "each reservation retains its own original framing and payload"
                );
            }
            assert!(
                wire.metrics
                    .try_abandoned_requests()
                    .expect("healthy metrics")
                    .is_empty()
            );
            wire.close().await;
        }
    }
}
