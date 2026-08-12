//! Tests for check_mcp_status tool

use crate::mcp::server::McpServer;
use crate::mcp::tools::StatusResponse;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use rmcp::handler::server::common::RequestId;
use rmcp::model::NumberOrString;
use std::sync::Arc;

#[tokio::test]
async fn check_status_returns_connection_info() {
    // Given: Server with mock client (connected) and rate limiter (tokens available)
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_is_connected().return_once(|| true);
    mock_client.expect_is_premium().return_once(|| Some(true));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_available_tokens().return_once(|| 45.5);
    mock_limiter.expect_capacity().return_once(|| 50.0);
    mock_limiter.expect_refill_rate().return_once(|| 2.0);

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Call check_mcp_status
    let result = server
        .check_mcp_status(RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns success with connection info
    assert!(result.is_ok());
    let response: StatusResponse = serde_json::from_str(&result.unwrap()).unwrap();
    assert!(response.telegram_connected);
    assert_eq!(response.rate_limiter_tokens, 45.5);
    assert_eq!(response.server_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(response.premium, Some(true));
}

#[tokio::test]
async fn check_status_reports_disconnected() {
    // Given: Server with disconnected client
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_is_connected().return_once(|| false);
    mock_client.expect_is_premium().return_once(|| Some(false));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_available_tokens().return_once(|| 0.0);
    mock_limiter.expect_capacity().return_once(|| 50.0);
    mock_limiter.expect_refill_rate().return_once(|| 2.0);

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Call check_mcp_status
    let result = server
        .check_mcp_status(RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns disconnected status
    assert!(result.is_ok());
    let response: StatusResponse = serde_json::from_str(&result.unwrap()).unwrap();
    assert!(!response.telegram_connected);
    assert_eq!(response.rate_limiter_tokens, 0.0);
}

#[tokio::test]
async fn check_status_includes_session_counters() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_is_connected().return_once(|| true);
    mock_client.expect_is_premium().return_once(|| Some(false));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_available_tokens().return_once(|| 10.0);
    mock_limiter.expect_capacity().return_once(|| 50.0);
    mock_limiter.expect_refill_rate().return_once(|| 2.0);

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let metrics = server.metrics();
    metrics.record_request("42", "search_messages");
    metrics.record_response_written("42");

    let result = server
        .check_mcp_status(RequestId(NumberOrString::Number(1)))
        .await
        .expect("status ok");
    let response: StatusResponse = serde_json::from_str(&result).expect("valid JSON");

    assert_eq!(response.requests_received, 1);
    assert_eq!(response.responses_written, 1);
    assert_eq!(response.last_response_write_age_secs, Some(0));
    assert!(chrono::DateTime::parse_from_rfc3339(&response.session_started_at).is_ok());
}

#[tokio::test]
async fn check_status_age_is_none_before_first_write() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_is_connected().return_once(|| true);
    mock_client.expect_is_premium().return_once(|| Some(false));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_available_tokens().return_once(|| 10.0);
    mock_limiter.expect_capacity().return_once(|| 50.0);
    mock_limiter.expect_refill_rate().return_once(|| 2.0);

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .check_mcp_status(RequestId(NumberOrString::Number(1)))
        .await
        .expect("status ok");
    let response: StatusResponse = serde_json::from_str(&result).expect("valid JSON");

    assert_eq!(response.requests_received, 0);
    assert_eq!(response.responses_written, 0);
    assert_eq!(response.last_response_write_age_secs, None);
}

#[tokio::test]
async fn check_status_reports_rate_limiter_budget() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_is_connected().return_once(|| true);
    mock_client.expect_is_premium().return_once(|| Some(true));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_available_tokens().return_once(|| 23.5);
    mock_limiter.expect_capacity().return_once(|| 50.0);
    mock_limiter.expect_refill_rate().return_once(|| 2.0);

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter))
        .with_media_download_cost(5)
        .with_transcription_cost(5);

    let result = server
        .check_mcp_status(RequestId(NumberOrString::Number(1)))
        .await;

    let response: StatusResponse = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(response.rate_limiter.tokens, 23.5);
    assert_eq!(response.rate_limiter.capacity, 50.0);
    assert_eq!(response.rate_limiter.refill_per_sec, 2.0);
    assert_eq!(response.rate_limiter.costs.search, 1);
    assert_eq!(response.rate_limiter.costs.media_download, 5);
    assert_eq!(response.rate_limiter.costs.transcription, 5);
    // Deprecated alias must mirror the nested value for one release.
    assert_eq!(response.rate_limiter_tokens, response.rate_limiter.tokens);
}
