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
    state
        .admit(id.clone(), Some(token.clone()))
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
        id,
    ));
    let result_task = tokio::spawn(response);
    tokio::task::yield_now().await;
    let progress = transport.send(JsonRpcMessage::notification(
        ServerNotification::ProgressNotification(ProgressNotification::new(
            ProgressNotificationParam::new(token, 1.0),
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
    state
        .admit(RequestId::Number(1), Some(token.clone()))
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
                    ProgressNotificationParam::new(token.clone(), progress),
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
