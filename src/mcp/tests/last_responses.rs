//! Tests for get_last_responses tool

use crate::ObservabilityConfig;
use crate::mcp::observability::BufferedResponse;
use crate::mcp::server::McpServer;
use crate::mcp::tools::{GetLastResponsesRequest, LastResponsesResponse};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;
use std::time::SystemTime;

fn make_server() -> McpServer<MockTelegramClientTrait, MockRateLimiterTrait> {
    McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    )
}

fn buffered(id: &str, payload: &str) -> BufferedResponse {
    BufferedResponse {
        request_id: id.to_string(),
        tool_name: "search_messages".to_string(),
        written_at: SystemTime::now(),
        size_bytes: payload.len(),
        payload: payload.to_string(),
    }
}

#[tokio::test]
async fn empty_buffer_returns_empty_list() {
    let server = make_server();
    let result = server
        .get_last_responses(
            Parameters(GetLastResponsesRequest { n: None }),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("tool ok");
    let response: LastResponsesResponse = serde_json::from_str(&result).expect("valid JSON");
    assert!(response.responses.is_empty());
    assert_eq!(response.buffered, 0);
}

#[tokio::test]
async fn returns_buffered_responses_newest_first_with_parsed_payload() {
    let server = make_server();
    server
        .response_buffer()
        .push(buffered("7", r#"{"jsonrpc":"2.0","id":7,"result":{}}"#));
    server
        .response_buffer()
        .push(buffered("8", r#"{"jsonrpc":"2.0","id":8,"result":{}}"#));

    let result = server
        .get_last_responses(
            Parameters(GetLastResponsesRequest { n: None }),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("tool ok");
    let response: LastResponsesResponse = serde_json::from_str(&result).expect("valid JSON");

    assert_eq!(response.buffered, 2);
    assert_eq!(response.responses.len(), 2);
    assert_eq!(response.responses[0].request_id, "8");
    // Payload is embedded as real JSON, not a double-encoded string.
    assert_eq!(response.responses[0].response["id"], 8);
    assert!(chrono::DateTime::parse_from_rfc3339(&response.responses[0].written_at).is_ok());
}

#[tokio::test]
async fn n_caps_returned_entries_but_reports_total_buffered() {
    let server = make_server();
    for i in 1..=3 {
        server.response_buffer().push(buffered(
            &i.to_string(),
            r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
        ));
    }

    let result = server
        .get_last_responses(
            Parameters(GetLastResponsesRequest { n: Some(2) }),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("tool ok");
    let response: LastResponsesResponse = serde_json::from_str(&result).expect("valid JSON");

    assert_eq!(response.responses.len(), 2);
    assert_eq!(response.responses[0].request_id, "3");
    assert_eq!(response.buffered, 3);
}

#[tokio::test]
async fn oversized_payload_is_returned_as_stub_with_real_size() {
    // Server whose buffer stubs payloads over 100 bytes.
    let server = make_server().with_observability(&ObservabilityConfig {
        max_buffered_payload_bytes: 100,
        response_buffer_size: 10,
        ..ObservabilityConfig::default()
    });

    // Build a valid-JSON payload that is clearly over 100 bytes.
    let big_value = "x".repeat(180);
    let payload = format!(r#"{{"data":"{big_value}"}}"#); // ~190 bytes of valid JSON
    let real_size = payload.len();
    assert!(
        real_size > 100,
        "test payload must exceed the stub threshold (was {real_size})"
    );

    server.response_buffer().push(BufferedResponse {
        request_id: "42".to_string(),
        tool_name: "search_messages".to_string(),
        written_at: SystemTime::now(),
        size_bytes: real_size, // real wire size, unchanged by stubbing
        payload,
    });

    let result = server
        .get_last_responses(
            Parameters(GetLastResponsesRequest { n: None }),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("tool ok");
    let response: LastResponsesResponse = serde_json::from_str(&result).expect("valid JSON");

    assert_eq!(response.buffered, 1);
    let entry = &response.responses[0];
    // size_bytes must reflect the real wire size, not the stub size.
    assert_eq!(entry.size_bytes, real_size);
    // The stored payload must be the stub, identified by the "omitted" key.
    assert!(
        entry.response.get("omitted").is_some(),
        "expected stub with \"omitted\" key, got: {}",
        entry.response
    );
}
