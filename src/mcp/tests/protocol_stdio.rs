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
