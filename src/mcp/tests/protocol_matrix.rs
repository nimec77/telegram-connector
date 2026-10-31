use super::*;

#[tokio::test]
async fn missing_and_malformed_metadata_never_inherit_previous_request() {
    let mut wire = Wire::start();
    for (index, meta) in [json!({}), json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28"}),
        json!({"io.modelcontextprotocol/clientCapabilities":{}}),
        json!({"io.modelcontextprotocol/protocolVersion":5,"io.modelcontextprotocol/clientCapabilities":{}}),
        json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":false})].into_iter().enumerate() {
        for established in [false, true] {
            if established { wire.send(modern(json!(90), "tools/list")).await; assert!(wire.recv().await.get("result").is_some()); }
            let mut request = modern(json!(index), "tools/list");
            request["params"]["_meta"] = meta.clone();
            wire.send(request).await;
            assert_eq!(wire.recv().await["error"]["code"], -32602);
        }
    }
    wire.close().await;
}

#[tokio::test]
async fn unsupported_versions_return_exact_supported_set_and_allow_retry() {
    let mut wire = Wire::start();
    for version in ["1900-01-01", "2099-01-01", "2025-06-18"] {
        let mut request = modern(json!(version), "tools/list");
        request["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!(version);
        wire.send(request).await;
        let reply = wire.recv().await;
        assert_eq!(reply["id"], version);
        assert_eq!(reply["error"]["code"], -32022);
        assert_eq!(reply["error"]["data"]["requested"], version);
        assert_eq!(
            reply["error"]["data"]["supported"],
            json!(["2025-11-25", "2026-07-28"])
        );
        wire.send(modern(json!(10), "tools/list")).await;
        assert!(wire.recv().await.get("result").is_some());
    }
    wire.close().await;
}

async fn initialize(wire: &mut Wire) {
    wire.send(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
        "protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"legacy","version":"1"}}})).await;
    assert_eq!(wire.recv().await["result"]["protocolVersion"], "2025-11-25");
    wire.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .await;
}

#[tokio::test]
async fn discovery_then_legacy_handshake_retains_legacy_fields_and_ping() {
    let mut wire = Wire::start();
    wire.send(modern(json!(1), "server/discover")).await;
    let discovery = wire.recv().await;
    assert_eq!(discovery["result"]["ttlMs"], 0);
    assert_eq!(discovery["result"]["cacheScope"], "private");
    assert_eq!(discovery["result"]["capabilities"], json!({"tools":{}}));
    initialize(&mut wire).await;
    wire.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
        .await;
    let list = wire.recv().await;
    assert!(list["result"].get("resultType").is_none());
    assert!(list["result"].get("ttlMs").is_none());
    assert!(list["result"].get("cacheScope").is_none());
    wire.send(json!({"jsonrpc":"2.0","id":3,"method":"ping"}))
        .await;
    assert_eq!(
        wire.recv().await,
        json!({"jsonrpc":"2.0","id":3,"result":{}})
    );
    wire.close().await;
}

#[tokio::test]
async fn interleaved_protocol_metadata_controls_only_its_own_result() {
    let mut wire = Wire::start();
    let mut legacy = modern(json!("legacy"), "tools/list");
    legacy["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("2025-11-25");
    wire.send(modern(json!("modern"), "tools/list")).await;
    wire.send(legacy).await;
    wire.send(modern(json!("again"), "tools/list")).await;
    for _ in 0..3 {
        let reply = wire.recv().await;
        if reply["id"] == "legacy" {
            assert!(reply["result"].get("resultType").is_none());
            assert!(reply["result"].get("ttlMs").is_none());
        } else {
            assert_eq!(reply["result"]["resultType"], "complete");
            assert_eq!(reply["result"]["ttlMs"], 3_600_000);
        }
    }
    wire.close().await;
}

#[tokio::test]
async fn unknown_tools_methods_and_unadvertised_features_are_protocol_errors() {
    let mut wire = Wire::start();
    let mut request = modern(json!(1), "tools/call");
    request["params"]["name"] = json!("unknown-tool");
    wire.send(request).await;
    assert_eq!(wire.recv().await["error"]["code"], -32602);
    for method in [
        "unknown/method",
        "ping",
        "resources/subscribe",
        "resources/unsubscribe",
        "logging/setLevel",
        "tasks/get",
        "subscriptions/listen",
    ] {
        let mut request = modern(json!(method), method);
        request["params"]["uri"] = json!("fake://resource");
        request["params"]["taskId"] = json!("fake");
        request["params"]["level"] = json!("debug");
        request["params"]["notifications"] = json!({"toolsListChanged":true});
        wire.send(request).await;
        assert_eq!(wire.recv().await["error"]["code"], -32601, "{method}");
    }
    wire.close().await;
}

#[tokio::test]
async fn invalid_envelopes_keep_valid_ids_and_notifications_have_no_response() {
    let mut wire = Wire::start();
    for (value, expected_id) in [
        (
            json!({"jsonrpc":"1.0","id":"readable","method":"tools/list"}),
            json!("readable"),
        ),
        (
            json!({"jsonrpc":"2.0","id":7,"method":"tools/list","params":[]}),
            json!(7),
        ),
        (
            json!({"jsonrpc":"2.0","id":8,"method":"tools/list","result":{},"error":{}}),
            json!(8),
        ),
        (
            json!({"jsonrpc":"2.0","id":1.5,"method":"tools/list"}),
            Value::Null,
        ),
        (
            json!({"jsonrpc":"2.0","id":null,"method":"tools/list"}),
            Value::Null,
        ),
        (json!([]), Value::Null),
    ] {
        wire.send(value).await;
        let reply = wire.recv().await;
        assert_eq!(reply["error"]["code"], -32600);
        assert_eq!(reply["id"], expected_id);
    }
    wire.send(modern(json!(1), "tools/list")).await;
    wire.recv().await;
    for notification in [
        json!({"jsonrpc":"2.0","method":"notifications/unknown","params":{}}),
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":999}}),
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":{}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    ] {
        wire.send(notification).await;
    }
    wire.send(modern(json!(100), "tools/list")).await;
    assert_eq!(wire.recv().await["id"], 100);
    wire.close().await;
}

#[tokio::test]
async fn input_cap_includes_newline_and_recovers_after_fragmented_oversize() {
    let config = crate::config::ServerConfig {
        max_request_bytes: 256,
        ..Default::default()
    };
    let mut wire = Wire::configured(server(), config, 4096);
    let request = modern(json!(1), "tools/list").to_string();
    let exact = format!("{}{}\n", request, " ".repeat(255 - request.len()));
    wire.raw(exact.as_bytes()).await;
    assert_eq!(wire.recv().await["id"], 1);
    wire.raw(&[b'x'; 257]).await;
    assert_eq!(wire.recv().await["error"]["code"], -32600);
    wire.raw(b"tail\n").await;
    wire.send(modern(json!(2), "tools/list")).await;
    assert_eq!(wire.recv().await["id"], 2);
    wire.raw(b"{\"partial\":").await;
    wire.close().await;
}

#[tokio::test(start_paused = true)]
async fn admission_is_bounded_and_control_cancellation_releases_capacity() {
    let handler = PendingHandler::new();
    let mut wire = Wire::configured(
        handler.clone(),
        crate::config::ServerConfig {
            max_in_flight_requests: 1,
            ..Default::default()
        },
        4096,
    );
    wire.send(pending_call(json!(1))).await;
    handler.started.notified().await;
    wire.send(pending_call(json!(2))).await;
    assert_eq!(wire.recv().await["error"]["code"], 1001);
    wire.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}))
        .await;
    handler.dropped.notified().await;
    wire.send(modern(json!(3), "tools/list")).await;
    assert_eq!(wire.recv().await["id"], 3);
    assert!(wire.metrics.abandoned_requests().is_empty());
    assert_eq!(wire.metrics.responses_written(), 1);
    wire.close().await;
}

#[tokio::test(start_paused = true)]
async fn numeric_and_string_ids_have_independent_cancellation_and_accounting() {
    let handler = PendingHandler::new();
    let mut wire = Wire::with_handler(handler.clone());
    wire.send(pending_call(json!(1))).await;
    handler.started.notified().await;
    wire.send(pending_call(json!("1"))).await;
    handler.started.notified().await;
    assert_eq!(wire.metrics.abandoned_requests().len(), 2);
    wire.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}))
        .await;
    handler.dropped.notified().await;
    handler.release.notify_one();
    assert_eq!(wire.recv().await["id"], "1");
    assert!(wire.metrics.abandoned_requests().is_empty());
    wire.close().await;
}

#[tokio::test(start_paused = true)]
async fn signal_drains_completed_work_and_idle_shutdown_is_immediate() {
    let handler = PendingHandler::new();
    let mut wire = Wire::with_handler(handler.clone());
    wire.send(pending_call(json!(1))).await;
    handler.started.notified().await;
    wire.shutdown
        .take()
        .expect("sender")
        .send(())
        .expect("signal");
    handler.release.notify_one();
    assert_eq!(wire.recv().await["id"], 1);
    tokio::time::timeout(Duration::from_millis(50), wire.task)
        .await
        .expect("immediate drain")
        .expect("task");
    let mut wire = Wire::start();
    let started = tokio::time::Instant::now();
    wire.shutdown
        .take()
        .expect("sender")
        .send(())
        .expect("signal");
    wire.task.await.expect("idle stops");
    assert_eq!(started.elapsed(), Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn blocked_stdout_obeys_the_same_shutdown_deadline() {
    let mut wire = Wire::configured(
        server(),
        crate::config::ServerConfig {
            shutdown_timeout_seconds: 1,
            ..Default::default()
        },
        256,
    );
    wire.send(modern(json!(1), "tools/list")).await;
    // The tool list is larger than the duplex capacity; never read stdout.
    tokio::task::yield_now().await;
    wire.shutdown
        .take()
        .expect("sender")
        .send(())
        .expect("signal");
    tokio::time::timeout(Duration::from_millis(1100), wire.task)
        .await
        .expect("writer drain deadline")
        .expect("task");
    assert!(wire.metrics.abandoned_requests().is_empty());
}

#[tokio::test(start_paused = true)]
async fn zero_shutdown_budget_cancels_pending_work_without_advancing_time() {
    let handler = PendingHandler::new();
    let mut wire = Wire::configured(
        handler.clone(),
        crate::config::ServerConfig {
            shutdown_timeout_seconds: 0,
            ..Default::default()
        },
        4096,
    );
    wire.send(pending_call(json!(1))).await;
    handler.started.notified().await;
    let started = tokio::time::Instant::now();
    wire.shutdown
        .take()
        .expect("sender")
        .send(())
        .expect("signal");
    wire.task.await.expect("stopped");
    assert_eq!(started.elapsed(), Duration::ZERO);
    assert!(wire.metrics.abandoned_requests().is_empty());
}

#[tokio::test]
async fn partial_input_survives_an_interleaved_response_write() {
    let handler = PendingHandler::new();
    let mut wire = Wire::with_handler(handler.clone());
    wire.send(pending_call(json!(1))).await;
    handler.started.notified().await;
    let next = format!("{}\n", modern(json!(2), "tools/list"));
    let split = next.len() / 2;
    wire.raw(&next.as_bytes()[..split]).await;
    handler.release.notify_one();
    assert_eq!(wire.recv().await["id"], 1);
    wire.raw(&next.as_bytes()[split..]).await;
    assert_eq!(wire.recv().await["id"], 2);
    wire.close().await;
}

#[tokio::test(start_paused = true)]
async fn zero_grace_initialization_stays_usable_and_observed_signal_is_immediate() {
    let mut wire = Wire::configured(
        server(),
        crate::config::ServerConfig {
            shutdown_timeout_seconds: 0,
            ..Default::default()
        },
        65536,
    );
    initialize(&mut wire).await;
    wire.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
        .await;
    assert_eq!(
        wire.recv().await["result"]["tools"]
            .as_array()
            .expect("legacy tools")
            .len(),
        16
    );
    let signalled_at = tokio::time::Instant::now();
    wire.shutdown
        .take()
        .expect("sender")
        .send(())
        .expect("signal");
    wire.task.await.expect("stopped");
    assert_eq!(signalled_at.elapsed(), Duration::ZERO);
}
