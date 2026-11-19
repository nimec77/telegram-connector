//! Loopback compatibility fixture for the modern rmcp HTTP binding.

use rmcp::model::{
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig,
    Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, ServerHandler};
use serde_json::{Map, Value, json};
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Clone)]
struct Fixture;

impl ServerHandler for Fixture {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[ProtocolVersion::V_2026_07_28])
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let schema = Map::from_iter([("type".into(), "object".into())]);
        let mut result = ListToolsResult::default();
        result.tools.push(Tool::new(
            "fixture_status",
            "Read the offline fixture status",
            Arc::new(schema),
        ));
        Ok(result)
    }
}

#[tokio::test]
async fn modern_tools_list_over_http_needs_no_session() -> anyhow::Result<()> {
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_stateless_protocol_metadata_required(true)
        .with_json_response(true);
    let service = StreamableHttpService::new(
        || Ok(Fixture),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        axum::serve(listener, axum::Router::new().nest_service("/mcp", service)).await
    });
    let request = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/list",
        "params": {
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientInfo": {"name": "fixture", "version": "1"},
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }
    })
    .to_string();
    let response = tokio::time::timeout(Duration::from_secs(5), async {
        let mut socket = TcpStream::connect(address).await?;
        socket
            .write_all(
                format!(
                    "POST /mcp HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\
                     Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\n\
                     MCP-Protocol-Version: 2026-07-28\r\nMcp-Method: tools/list\r\n\
                     Content-Length: {}\r\n\r\n{request}",
                    request.len()
                )
                .as_bytes(),
            )
            .await?;
        let mut response = String::new();
        socket.read_to_string(&mut response).await?;
        Ok::<_, std::io::Error>(response)
    })
    .await;
    server.abort();
    let response = response??;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .expect("HTTP response contains a head and body");
    assert!(headers.starts_with("HTTP/1.1 200 "), "{response}");
    assert!(!headers.to_ascii_lowercase().contains("mcp-session-id:"));
    let payload: Value = serde_json::from_str(body)?;
    assert_eq!(payload["id"], 7);
    assert_eq!(payload["result"]["tools"].as_array().map(Vec::len), Some(1));
    assert_eq!(payload["result"]["tools"][0]["name"], "fixture_status");
    assert_eq!(
        payload["result"]["tools"][0]["inputSchema"]["type"],
        "object"
    );
    Ok(())
}
