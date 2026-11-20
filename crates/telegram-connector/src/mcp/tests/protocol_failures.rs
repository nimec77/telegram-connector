use super::*;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};

struct FailedRead;
impl AsyncRead for FailedRead {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Err(std::io::Error::other("fixture read failure")))
    }
}
struct FailedWrite;
impl AsyncWrite for FailedWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Err(std::io::Error::other("fixture write failure")))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
fn runtime() -> (
    crate::mcp::runtime::Runtime,
    Arc<crate::mcp::observability::SessionMetrics>,
) {
    let server = server();
    let metrics = server.metrics();
    (
        crate::mcp::runtime::Runtime::try_new(
            crate::config::ServerConfig::default(),
            Arc::clone(&metrics),
            server.response_buffer(),
            Duration::from_secs(1),
        )
        .expect("runtime"),
        metrics,
    )
}
#[tokio::test]
async fn transport_read_errors_propagate_after_cleanup() {
    let (runtime, metrics) = runtime();
    let result = runtime
        .serve(
            server(),
            (FailedRead, tokio::io::sink()),
            std::future::pending(),
        )
        .await;
    assert!(result.is_err(), "read failure must not look like clean EOF");
    assert!(metrics.abandoned_requests().is_empty());
}
#[tokio::test]
async fn transport_write_errors_propagate_and_release_metrics() {
    let (runtime, metrics) = runtime();
    let request = format!("{}\n", modern(json!(1), "tools/list")).into_bytes();
    let result = runtime
        .serve(
            server(),
            (std::io::Cursor::new(request), FailedWrite),
            std::future::pending(),
        )
        .await;
    assert!(result.is_err(), "write failure must propagate");
    assert!(metrics.abandoned_requests().is_empty());
    assert_eq!(metrics.responses_written(), 0);
}
#[tokio::test]
async fn signal_registration_failure_propagates_without_waiting_for_input() {
    let (runtime, _) = runtime();
    let (_peer, io) = tokio::io::duplex(32);
    let (read, write) = tokio::io::split(io);
    let result = runtime
        .serve(server(), (read, write), async {
            Err(std::io::Error::other("signal installation"))
        })
        .await;
    assert!(
        result
            .expect_err("signal error")
            .to_string()
            .contains("signal installation")
    );
}

#[tokio::test]
async fn unsupported_bootstrap_request_does_not_select_modern_lifecycle() {
    let mut wire = Wire::start();
    let mut request = modern(json!(1), "tools/list");
    request["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("2099-01-01");
    wire.send(request).await;
    assert_eq!(wire.recv().await["error"]["code"], -32022);
    wire.send(json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).await;
    assert_eq!(wire.recv().await["result"]["protocolVersion"], "2025-11-25");
    wire.close().await;
}

#[tokio::test]
async fn bootstrap_ping_validates_version_and_metadata_before_dispatch() {
    let mut wire = Wire::start();
    let mut request = modern(json!(1), "ping");
    request["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("2099-01-01");
    wire.send(request).await;
    assert_eq!(wire.recv().await["error"]["code"], -32022);
    let mut request = modern(json!(2), "ping");
    request["params"]["_meta"]
        .as_object_mut()
        .expect("meta")
        .remove("io.modelcontextprotocol/clientCapabilities");
    wire.send(request).await;
    assert_eq!(wire.recv().await["error"]["code"], -32602);
    wire.close().await;
}

#[tokio::test(start_paused = true)]
async fn cancellation_remains_readable_while_an_overload_reply_is_blocked() {
    let handler = PendingHandler::new();
    let mut wire = Wire::configured(
        handler.clone(),
        crate::config::ServerConfig {
            max_in_flight_requests: 1,
            shutdown_timeout_seconds: 1,
            ..Default::default()
        },
        32,
    );
    wire.send(pending_call(json!(1))).await;
    handler.started.notified().await;
    wire.send(pending_call(json!(2))).await;
    tokio::time::timeout(Duration::from_millis(100), async {
        wire.send(
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}),
        )
        .await;
        handler.dropped.notified().await;
    })
    .await
    .expect("cancel must not wait for a rejected request's stdout write");
    wire.shutdown
        .take()
        .expect("sender")
        .send(())
        .expect("signal");
    tokio::time::timeout(Duration::from_millis(1100), wire.task)
        .await
        .expect("deadline")
        .expect("task");
    assert!(wire.metrics.abandoned_requests().is_empty());
}

struct PendingDiscovery(Arc<tokio::sync::Notify>);
impl rmcp::ServerHandler for PendingDiscovery {
    async fn discover(
        &self,
        _: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::DiscoverResult, rmcp::ErrorData> {
        self.0.notify_one();
        std::future::pending().await
    }
}
#[tokio::test(start_paused = true)]
async fn shutdown_deadline_also_cancels_a_pending_bootstrap_handler() {
    let started = Arc::new(tokio::sync::Notify::new());
    let mut wire = Wire::with_handler(PendingDiscovery(Arc::clone(&started)));
    wire.send(modern(json!(1), "server/discover")).await;
    started.notified().await;
    wire.shutdown
        .take()
        .expect("sender")
        .send(())
        .expect("signal");
    tokio::time::timeout(Duration::from_millis(1100), wire.task)
        .await
        .expect("bootstrap deadline")
        .expect("task");
    assert!(wire.metrics.abandoned_requests().is_empty());
}

#[tokio::test(start_paused = true)]
async fn product_bootstrap_output_blocked_at_eof_obeys_configured_deadline() {
    for request in [
        modern(json!(1), "server/discover"),
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"offline","version":"1"}}}),
    ] {
        let mut wire = Wire::configured(
            server(),
            crate::config::ServerConfig {
                shutdown_timeout_seconds: 1,
                ..Default::default()
            },
            32,
        );
        wire.send(request).await;
        wire.read
            .read_exact(&mut [0; 1])
            .await
            .expect("bootstrap output started");
        assert_eq!(wire.metrics.requests_received(), 1);
        wire.write.shutdown().await.expect("EOF");
        let signal_sender = wire.shutdown;
        tokio::time::timeout(Duration::from_millis(1100), wire.task)
            .await
            .expect("blocked bootstrap EOF deadline")
            .expect("task");
        drop(signal_sender);
        assert!(wire.metrics.abandoned_requests().is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn product_eof_before_opener_or_partial_frame_exits_immediately() {
    for prefix in [b"".as_slice(), b"{\"jsonrpc\":"] {
        let mut wire = Wire::start();
        wire.raw(prefix).await;
        wire.write.shutdown().await.expect("EOF");
        let signal_sender = wire.shutdown;
        tokio::time::timeout(Duration::from_millis(10), wire.task)
            .await
            .expect("idle EOF exit")
            .expect("task");
        drop(signal_sender);
    }
}

#[tokio::test]
async fn observed_eof_drains_complete_buffered_requests_before_closing_admission() {
    let mut wire = Wire::start();
    let frames = format!(
        "{}\n{}\n",
        modern(json!(1), "tools/list"),
        modern(json!(2), "tools/list")
    );
    wire.raw(frames.as_bytes()).await;
    wire.write.shutdown().await.expect("EOF");
    let first = wire.recv().await;
    let second = wire.recv().await;
    let mut ids = vec![
        first["id"].as_i64().expect("id"),
        second["id"].as_i64().expect("id"),
    ];
    ids.sort();
    assert_eq!(ids, vec![1, 2]);
    assert!(first.get("result").is_some());
    assert!(second.get("result").is_some());
    let signal_sender = wire.shutdown;
    wire.task.await.expect("stopped");
    drop(signal_sender);
}

#[tokio::test(start_paused = true)]
async fn signal_during_actual_product_bootstrap_write_is_bounded() {
    for request in [
        modern(json!(1), "server/discover"),
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"offline","version":"1"}}}),
    ] {
        let mut wire = Wire::configured(
            server(),
            crate::config::ServerConfig {
                shutdown_timeout_seconds: 1,
                ..Default::default()
            },
            32,
        );
        wire.send(request).await;
        wire.read
            .read_exact(&mut [0; 1])
            .await
            .expect("bootstrap output started");
        wire.shutdown
            .take()
            .expect("sender")
            .send(())
            .expect("signal");
        tokio::time::timeout(Duration::from_millis(1100), wire.task)
            .await
            .expect("bootstrap signal deadline")
            .expect("task");
        assert!(wire.metrics.abandoned_requests().is_empty());
    }
}

#[tokio::test]
async fn invalid_no_id_envelopes_return_errors_before_notification_suppression() {
    let mut wire = Wire::start();
    for (index, malformed) in [
        json!({"jsonrpc":"1.0","method":"notifications/unknown"}),
        json!({"method":"notifications/unknown"}),
        json!({"jsonrpc":"2.0","method":"notifications/unknown","result":{}}),
        json!({"jsonrpc":"2.0","method":"notifications/unknown","params":[]}),
    ]
    .into_iter()
    .enumerate()
    {
        wire.send(malformed).await;
        wire.send(modern(json!(index), "tools/list")).await;
        let fault = wire.recv().await;
        assert_eq!(fault["error"]["code"], -32600);
        assert!(fault.get("id").is_none(), "dated MCP omits unreadable IDs");
        assert_eq!(wire.recv().await["id"], index);
    }
    wire.send(json!({"jsonrpc":"2.0","method":"notifications/unknown"}))
        .await;
    wire.send(modern(json!(99), "tools/list")).await;
    assert_eq!(
        wire.recv().await["id"],
        99,
        "valid notifications remain reply-free"
    );
    wire.close().await;
}

#[tokio::test(start_paused = true)]
async fn saturated_input_pump_cannot_hide_eof_behind_a_blocked_bootstrap_write() {
    for (method, budget) in [
        ("server/discover", 1),
        ("initialize", 1),
        ("server/discover", 0),
        ("initialize", 0),
    ] {
        let handler = server();
        let metrics = handler.metrics();
        let runtime = crate::mcp::runtime::Runtime::try_new(
            crate::config::ServerConfig {
                shutdown_timeout_seconds: budget,
                ..Default::default()
            },
            Arc::clone(&metrics),
            handler.response_buffer(),
            Duration::from_secs(1),
        )
        .expect("runtime");
        let (mut peer_input, input) = tokio::io::duplex(65536);
        let (output, mut peer_output) = tokio::io::duplex(32);
        let task = tokio::spawn(async move {
            runtime
                .serve(handler, (input, output), std::future::pending())
                .await
        });
        let opener = if method == "initialize" {
            json!({"jsonrpc":"2.0","id":1,"method":method,"params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"offline","version":"1"}}})
        } else {
            modern(json!(1), method)
        };
        peer_input
            .write_all(format!("{opener}\n{}", " ".repeat(32768)).as_bytes())
            .await
            .expect("buffered input");
        peer_input.shutdown().await.expect("physical stdin closed");
        peer_output
            .read_exact(&mut [0; 1])
            .await
            .expect("bootstrap write started");
        let stalled_at = tokio::time::Instant::now();
        tokio::time::timeout(Duration::from_millis(1100), task)
            .await
            .expect("bootstrap stall deadline")
            .expect("task")
            .expect("bounded shutdown");
        assert_eq!(stalled_at.elapsed(), Duration::from_secs(1));
        assert!(metrics.abandoned_requests().is_empty());
    }
}
