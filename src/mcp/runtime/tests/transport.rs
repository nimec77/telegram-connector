use super::*;
use rmcp::model::{
    CallToolResult, ContentBlock, ProgressNotification, ProgressNotificationParam, ProgressToken,
    ServerResult,
};
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::test(start_paused = true)]
async fn completed_response_suppresses_progress_already_waiting_for_the_writer() {
    let (client, writer) = tokio::io::duplex(1);
    let state = State::new(
        crate::config::ServerConfig::default(),
        Arc::new(crate::mcp::observability::SessionMetrics::new()),
    );
    let id = RequestId::Number(1);
    let token = ProgressToken(RequestId::Number(1));
    let operation = state
        .admit(id.clone(), Some(token.clone()), false)
        .await
        .expect("admitted");
    let mut transport = StdioTransport::new(
        tokio::io::empty(),
        writer,
        Arc::clone(&state),
        Arc::new(ResponseBuffer::new(1, 1024)),
        Duration::from_secs(1),
    );
    let response = transport.send(JsonRpcMessage::response(
        ServerResult::CallToolResult(CallToolResult::success(vec![ContentBlock::text("done")])),
        operation.sdk_id.clone(),
    ));
    let result_task = tokio::spawn(response);
    tokio::task::yield_now().await;
    let progress = transport.send(JsonRpcMessage::notification(
        ServerNotification::ProgressNotification(ProgressNotification::new(
            ProgressNotificationParam::new(ProgressToken(operation.sdk_id.clone()), 1.0),
        )),
    ));
    let progress_task = tokio::spawn(progress);
    tokio::task::yield_now().await;
    let mut reader = BufReader::new(client);
    let mut line = String::new();
    reader.read_line(&mut line).await.expect("response");
    assert_eq!(serde_json::from_str::<Value>(&line).expect("JSON")["id"], 1);
    result_task
        .await
        .expect("join result")
        .expect("write result");
    line.clear();
    let late = tokio::time::timeout(Duration::from_millis(10), reader.read_line(&mut line)).await;
    assert!(
        late.is_err(),
        "queued progress must not appear after completion: {line}"
    );
    progress_task
        .await
        .expect("join progress")
        .expect("suppressed progress");
}

#[tokio::test(start_paused = true)]
async fn reordered_progress_never_decreases_on_the_wire() {
    let (client, writer) = tokio::io::duplex(4096);
    let state = State::new(
        crate::config::ServerConfig::default(),
        Arc::new(crate::mcp::observability::SessionMetrics::new()),
    );
    let token = ProgressToken(RequestId::Number(1));
    let operation = state
        .admit(RequestId::Number(1), Some(token.clone()), false)
        .await
        .expect("admitted");
    let mut transport = StdioTransport::new(
        tokio::io::empty(),
        writer,
        state,
        Arc::new(ResponseBuffer::new(1, 1024)),
        Duration::from_secs(1),
    );
    for progress in [2.0, 1.0] {
        transport
            .send(JsonRpcMessage::notification(
                ServerNotification::ProgressNotification(ProgressNotification::new(
                    ProgressNotificationParam::new(
                        ProgressToken(operation.sdk_id.clone()),
                        progress,
                    ),
                )),
            ))
            .await
            .expect("notification handled");
    }
    let mut reader = BufReader::new(client);
    let mut line = String::new();
    reader.read_line(&mut line).await.expect("first progress");
    assert_eq!(
        serde_json::from_str::<Value>(&line).expect("JSON")["params"]["progress"],
        2.0
    );
    line.clear();
    assert!(
        tokio::time::timeout(Duration::from_millis(10), reader.read_line(&mut line))
            .await
            .is_err(),
        "older queued progress escaped: {line}"
    );
}

fn lifetime_request(id: Value, modern: bool, name: &str) -> Value {
    let mut request = serde_json::json!({"jsonrpc":"2.0", "id":id, "method":"tools/call",
        "params":{"name":name,"arguments":{},"_meta":{"progressToken":"reused-token"}}});
    if modern {
        request["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] =
            serde_json::json!("2026-07-28");
        request["params"]["_meta"]["io.modelcontextprotocol/clientCapabilities"] =
            serde_json::json!({});
    }
    request
}

fn lifetime_output(
    request: &rmcp::model::JsonRpcRequest<ClientRequest>,
    kind: &str,
) -> TxJsonRpcMessage<RoleServer> {
    match kind {
        "response" => JsonRpcMessage::response(
            ServerResult::CallToolResult(CallToolResult::success(vec![ContentBlock::text("old")])),
            request.id.clone(),
        ),
        "error" => JsonRpcMessage::error(
            ErrorData::invalid_params("old error", None),
            Some(request.id.clone()),
        ),
        "progress" => JsonRpcMessage::notification(ServerNotification::ProgressNotification(
            ProgressNotification::new(ProgressNotificationParam::new(
                request
                    .request
                    .get_meta()
                    .get_progress_token()
                    .expect("token"),
                1.0,
            )),
        )),
        "subscription" => {
            let mut notification =
                ServerNotification::ToolListChangedNotification(Default::default());
            notification
                .get_meta_mut()
                .set_subscription_id(request.id.clone());
            JsonRpcMessage::notification(notification)
        }
        _ => unreachable!("fixture kind"),
    }
}

async fn lifetime_case(
    id: Value,
    modern: bool,
    kind: &str,
    committed: bool,
    cancel_replacement: bool,
) {
    use tokio::io::AsyncReadExt;
    let input = format!(
        "{}\n{}\n{}\n",
        lifetime_request(id.clone(), modern, "old-tool"),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}),
        lifetime_request(id.clone(), modern, "replacement-tool")
    );
    let state = State::new(
        crate::config::ServerConfig::default(),
        Arc::new(crate::mcp::observability::SessionMetrics::new()),
    );
    let buffer = Arc::new(ResponseBuffer::new(10, 8192));
    let (mut client, writer) = tokio::io::duplex(1);
    let mut transport = StdioTransport::new(
        std::io::Cursor::new(input.into_bytes()),
        writer,
        Arc::clone(&state),
        Arc::clone(&buffer),
        Duration::from_secs(1),
    );
    // This fixture starts at the post-handshake adapter boundary for either protocol.
    transport.started = true;
    transport.modern = modern;
    let Some(JsonRpcMessage::Request(old)) = transport.receive().await else {
        panic!("old request")
    };
    let old_handle = Arc::downgrade(
        old.request
            .extensions()
            .get::<Arc<super::super::state::Operation>>()
            .expect("captured admission"),
    );
    let old_output = lifetime_output(&old, kind);
    let mut pending = Some(transport.send(old_output));
    let mut first = [0];
    let task = if committed {
        let task = tokio::spawn(pending.take().expect("queued write"));
        client
            .read_exact(&mut first)
            .await
            .expect("first committed byte");
        Some(task)
    } else {
        None
    };
    assert!(matches!(
        transport.receive().await,
        Some(JsonRpcMessage::Notification(_))
    ));
    let Some(JsonRpcMessage::Request(replacement)) = transport.receive().await else {
        panic!("replacement request")
    };
    if !committed {
        // The future was constructed before cancellation but first polled only
        // after replacement admission, just like SDK send-task scheduling.
        tokio::time::timeout(
            Duration::from_millis(10),
            pending.take().expect("queued output"),
        )
        .await
        .expect("first poll cannot rebind stale output")
        .expect("suppressed");
        // Also cover an item held in the SDK queue before send is called.
        tokio::time::timeout(
            Duration::from_millis(10),
            transport.send(lifetime_output(&old, kind)),
        )
        .await
        .expect("stale output must be suppressed before writing")
        .expect("discard stale output");
    }
    let mut reader = BufReader::new(client);
    if let Some(task) = task {
        let mut line = String::from_utf8(first.to_vec()).expect("ASCII prefix");
        reader.read_line(&mut line).await.expect("committed frame");
        let frame: Value = serde_json::from_str(&line).expect("complete JSON");
        match kind {
            "response" | "error" => assert_eq!(frame["id"], id),
            "progress" => assert_eq!(frame["params"]["progressToken"], "reused-token"),
            _ => assert_eq!(
                frame["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
                id
            ),
        }
        task.await.expect("writer task").expect("committed write");
    }
    let current = state
        .metrics
        .try_abandoned_requests()
        .expect("metrics available");
    assert_eq!(current.len(), 1, "old write consumed replacement metrics");
    assert_eq!(current[0].1, "replacement-tool");
    assert_eq!(
        serde_json::from_str::<Value>(&current[0].0).expect("diagnostic wire ID"),
        id
    );
    if cancel_replacement {
        let external_id = serde_json::from_value(id).expect("external ID");
        assert_eq!(
            state.cancel(&external_id).await,
            Some(replacement.id.clone())
        );
        tokio::time::timeout(
            Duration::from_millis(10),
            transport.send(lifetime_output(&replacement, "response")),
        )
        .await
        .expect("cancelled replacement stays suppressed")
        .expect("suppressed");
        assert!(state.operations.lock().await.is_empty());
        assert!(
            state
                .metrics
                .try_abandoned_requests()
                .expect("metrics")
                .is_empty()
        );
        drop(old);
        assert!(
            old_handle.upgrade().is_none(),
            "settled old work releases identity storage"
        );
        transport.close().await.expect("close");
        return;
    }
    let response = transport.send(JsonRpcMessage::response(
        ServerResult::CallToolResult(CallToolResult::success(vec![ContentBlock::text(
            "replacement",
        )])),
        replacement.id,
    ));
    let task = tokio::spawn(response);
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(1), reader.read_line(&mut line))
        .await
        .expect("replacement survives old completion")
        .expect("replacement line");
    let frame: Value = serde_json::from_str(&line).expect("JSON");
    assert_eq!(frame["id"], id);
    assert_eq!(
        frame["result"]["content"][0]["text"], "replacement",
        "stale output escaped"
    );
    task.await.expect("writer").expect("replacement write");
    assert!(state.operations.lock().await.is_empty());
    assert!(
        state
            .metrics
            .try_abandoned_requests()
            .expect("metrics")
            .is_empty()
    );
    let old_count = u64::from(committed && matches!(kind, "response" | "error"));
    assert_eq!(
        state.metrics.responses_written(),
        old_count + 1,
        "suppressed SDK items never reach write accounting"
    );
    let replay = buffer.last(Some(10));
    assert_eq!(
        replay.len(),
        1,
        "cancelled write must not create anonymous replay"
    );
    assert_eq!(replay[0].tool_name, "replacement-tool");
    assert_eq!(
        serde_json::from_str::<Value>(&replay[0].payload).expect("replay")["id"],
        id
    );
    drop(old);
    assert!(
        old_handle.upgrade().is_none(),
        "settled old work releases identity storage"
    );
    transport.close().await.expect("close");
}

#[tokio::test(start_paused = true)]
async fn cancelled_committed_frame_preserves_replacement_identity() {
    for id in [serde_json::json!(7), serde_json::json!("reused-id")] {
        for modern in [false, true] {
            for kind in ["response", "error", "progress", "subscription"] {
                for cancel_replacement in [false, true] {
                    lifetime_case(id.clone(), modern, kind, true, cancel_replacement).await;
                }
            }
        }
    }
}

#[tokio::test(start_paused = true)]
async fn cancelled_queued_output_cannot_bind_to_reused_id_or_token() {
    for id in [serde_json::json!(7), serde_json::json!("reused-id")] {
        for modern in [false, true] {
            for kind in ["response", "error", "progress", "subscription"] {
                for cancel_replacement in [false, true] {
                    lifetime_case(id.clone(), modern, kind, false, cancel_replacement).await;
                }
            }
        }
    }
}

#[tokio::test(start_paused = true)]
async fn shutdown_releases_admission_and_suppresses_owned_queued_work() {
    let input = format!(
        "{}\n",
        lifetime_request(serde_json::json!("shutdown"), true, "pending-tool")
    );
    let state = State::new(
        crate::config::ServerConfig::default(),
        Arc::new(crate::mcp::observability::SessionMetrics::new()),
    );
    let buffer = Arc::new(ResponseBuffer::new(1, 8192));
    let (mut reader, writer) = tokio::io::duplex(1);
    let mut transport = StdioTransport::new(
        std::io::Cursor::new(input.into_bytes()),
        writer,
        Arc::clone(&state),
        Arc::clone(&buffer),
        Duration::from_secs(1),
    );
    let Some(JsonRpcMessage::Request(request)) = transport.receive().await else {
        panic!("admitted request")
    };
    let handle = Arc::downgrade(
        request
            .request
            .extensions()
            .get::<Arc<super::super::state::Operation>>()
            .expect("owned operation"),
    );
    let queued = transport.send(lifetime_output(&request, "response"));
    drop(request);
    transport.close().await.expect("shutdown");
    queued.await.expect("suppressed queued write");
    assert!(state.operations.lock().await.is_empty());
    assert!(
        handle.upgrade().is_none(),
        "shutdown releases all operation handles"
    );
    assert!(
        state
            .metrics
            .try_abandoned_requests()
            .expect("metrics")
            .is_empty()
    );
    assert_eq!(state.metrics.responses_written(), 0);
    assert!(buffer.is_empty());
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.expect("closed output");
    assert!(bytes.is_empty());
}
