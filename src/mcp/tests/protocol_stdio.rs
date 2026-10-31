//! Raw NDJSON tests: the peer never performs SDK negotiation on our behalf.
use crate::mcp::observability::InstrumentedTransport;
use crate::mcp::server::McpServer;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use rmcp::ServiceExt;
use rmcp::transport::async_rw::AsyncRwTransport;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn server() -> McpServer<MockTelegramClientTrait, MockRateLimiterTrait> {
    McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    )
}

fn modern(id: Value, method: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":{"_meta":{
        "io.modelcontextprotocol/protocolVersion":"2026-07-28",
        "io.modelcontextprotocol/clientCapabilities":{}
    }}})
}

async fn sdk_round_trip(request: Value) -> Value {
    let (client, io) = tokio::io::duplex(65536);
    let (read, write) = tokio::io::split(io);
    let server = server();
    let transport = InstrumentedTransport::new(
        AsyncRwTransport::new_server(read, write),
        server.metrics(),
        server.response_buffer(),
        Duration::from_secs(1),
    );
    let task = tokio::spawn(async move {
        let service = server.serve(transport).await.expect("SDK starts");
        service.waiting().await.expect("SDK stops");
    });
    let (read, mut write) = tokio::io::split(client);
    write
        .write_all(format!("{request}\n").as_bytes())
        .await
        .expect("write request");
    let mut line = String::new();
    tokio::time::timeout(
        Duration::from_secs(2),
        BufReader::new(read).read_line(&mut line),
    )
    .await
    .expect("response deadline")
    .expect("read response");
    write.shutdown().await.expect("EOF");
    task.await.expect("server task");
    serde_json::from_str(&line).expect("JSON response")
}

#[tokio::test]
async fn sdk_modern_first_list_has_result_discriminator_and_cache_hints() {
    let reply = sdk_round_trip(modern(json!(1), "tools/list")).await;
    assert_eq!(reply["result"]["resultType"], "complete");
    assert_eq!(reply["result"]["ttlMs"], 3_600_000);
    assert_eq!(
        reply["result"]["tools"].as_array().expect("tools").len(),
        16
    );
}

#[tokio::test]
async fn sdk_initialize_never_negotiates_a_handshakeless_revision() {
    let reply = sdk_round_trip(json!({"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":"2026-07-28","capabilities":{},
        "clientInfo":{"name":"offline-test","version":"1"}}}))
    .await;
    assert_eq!(reply["result"]["protocolVersion"], "2025-11-25");
}

struct Wire {
    read: BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    write: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    task: tokio::task::JoinHandle<()>,
    metrics: Arc<crate::mcp::observability::SessionMetrics>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Wire {
    fn start() -> Self {
        Self::with_handler(server())
    }

    fn with_handler<S: rmcp::ServerHandler>(handler: S) -> Self {
        Self::configured(
            handler,
            crate::config::ServerConfig {
                shutdown_timeout_seconds: 1,
                ..Default::default()
            },
            65536,
        )
    }
    fn configured<S: rmcp::ServerHandler>(
        handler: S,
        config: crate::config::ServerConfig,
        capacity: usize,
    ) -> Self {
        let (client, io) = tokio::io::duplex(capacity);
        let (read, write) = tokio::io::split(io);
        let server = server();
        let metrics = server.metrics();
        let runtime = crate::mcp::runtime::Runtime::try_new(
            config,
            Arc::clone(&metrics),
            server.response_buffer(),
            Duration::from_secs(1),
        )
        .expect("runtime");
        let (shutdown, signal) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            runtime
                .serve(handler, (read, write), async {
                    let _ = signal.await;
                    Ok(())
                })
                .await
                .expect("runtime stops");
        });
        let (read, write) = tokio::io::split(client);
        Self {
            read: BufReader::new(read),
            write,
            task,
            metrics,
            shutdown: Some(shutdown),
        }
    }
    async fn send(&mut self, message: Value) {
        self.raw(format!("{message}\n").as_bytes()).await;
    }
    async fn raw(&mut self, bytes: &[u8]) {
        self.write.write_all(bytes).await.expect("write frame");
    }
    async fn recv(&mut self) -> Value {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(2), self.read.read_line(&mut line))
            .await
            .expect("response deadline")
            .expect("read frame");
        serde_json::from_str(&line).expect("JSON frame")
    }
    async fn close(self) {
        let Self {
            mut write, task, ..
        } = self;
        write.shutdown().await.expect("EOF");
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("shutdown bound")
            .expect("task");
    }
}

#[tokio::test]
async fn modern_list_identifies_server_and_discovery_bounds_versions() {
    let mut wire = Wire::start();
    wire.send(modern(json!(1), "tools/list")).await;
    let reply = wire.recv().await;
    assert_eq!(
        reply["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "telegram-mcp"
    );
    wire.send(modern(json!(2), "server/discover")).await;
    let reply = wire.recv().await;
    assert_eq!(
        reply["result"]["supportedVersions"],
        json!(["2025-11-25", "2026-07-28"])
    );
    wire.close().await;
}

#[tokio::test]
async fn malformed_frames_reply_and_preserve_readable_id() {
    let mut wire = Wire::start();
    wire.raw(b"{broken\n").await;
    wire.send(modern(json!(99), "tools/list")).await;
    assert_eq!(wire.recv().await["error"]["code"], -32700);
    assert_eq!(wire.recv().await["id"], 99);
    wire.raw(b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":false}\n")
        .await;
    let error = wire.recv().await;
    assert_eq!(error["error"]["code"], -32600);
    assert_eq!(error["id"], 7);
    wire.close().await;
}

#[tokio::test]
async fn first_modern_ping_is_removed() {
    let mut wire = Wire::start();
    wire.send(modern(json!(1), "ping")).await;
    assert_eq!(wire.recv().await["error"]["code"], -32601);
    wire.send(modern(json!(2), "tools/list")).await;
    assert_eq!(wire.recv().await["id"], 2);
    wire.close().await;
}

#[derive(Clone)]
struct PendingHandler {
    started: Arc<tokio::sync::Notify>,
    dropped: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
impl PendingHandler {
    fn new() -> Self {
        Self {
            started: Arc::new(tokio::sync::Notify::new()),
            dropped: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        }
    }
}
struct DropNotice(Arc<tokio::sync::Notify>);
impl Drop for DropNotice {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
impl rmcp::ServerHandler for PendingHandler {
    async fn call_tool(
        &self,
        _: rmcp::model::CallToolRequestParams,
        _: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        let _guard = DropNotice(Arc::clone(&self.dropped));
        self.started.notify_one();
        self.release.notified().await;
        Ok(rmcp::model::CallToolResult::success(vec![]).into())
    }
}
fn pending_call(id: Value) -> Value {
    let mut req = modern(id, "tools/call");
    req["params"]["name"] = json!("pending");
    req
}

#[tokio::test(start_paused = true)]
async fn cancellation_drops_the_operation_future() {
    let handler = PendingHandler::new();
    let mut wire = Wire::with_handler(handler.clone());
    wire.send(pending_call(json!(1))).await;
    handler.started.notified().await;
    wire.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}))
        .await;
    tokio::time::timeout(Duration::from_millis(10), handler.dropped.notified())
        .await
        .expect("cancel must drop operation");
    wire.send(modern(json!(2), "tools/list")).await;
    assert_eq!(wire.recv().await["id"], 2);
    wire.close().await;
}

#[tokio::test(start_paused = true)]
async fn eof_drops_work_within_configured_one_second_deadline() {
    let handler = PendingHandler::new();
    let mut wire = Wire::with_handler(handler.clone());
    wire.send(pending_call(json!(1))).await;
    handler.started.notified().await;
    wire.write.shutdown().await.expect("EOF");
    tokio::time::timeout(Duration::from_millis(1100), handler.dropped.notified())
        .await
        .expect("configured drain deadline");
    wire.close().await;
}

#[path = "protocol_extensions.rs"]
mod extensions;
#[path = "protocol_failures.rs"]
mod failures;
#[path = "protocol_matrix.rs"]
mod matrix;
#[path = "protocol_progress.rs"]
mod progress;
