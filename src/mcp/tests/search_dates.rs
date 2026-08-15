//! search_messages: from_date/to_date validation and windowing.

use crate::mcp::server::McpServer;
use crate::mcp::tools::SearchRequest;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::test_helpers::{create_test_search_result, permissive_limiter};
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

#[tokio::test]
async fn search_passes_date_range_to_client() {
    // Given: Mock client that verifies from_date/to_date are parsed and passed through
    let mut mock_client = MockTelegramClientTrait::new();

    mock_client
        .expect_search_messages()
        .withf(|p| {
            p.from_date == Some("2026-08-01T00:00:00Z".parse().unwrap())
                && p.to_date == Some("2026-08-05T00:00:00Z".parse().unwrap())
        })
        .returning(move |_| Ok(create_test_search_result(vec![], "q", 0)));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Search with an explicit date range
    let request = SearchRequest {
        query: "q".to_string(),
        from_date: Some("2026-08-01T00:00:00Z".to_string()),
        to_date: Some("2026-08-05T00:00:00Z".to_string()),
        ..Default::default()
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Success, and the client received the parsed date range
    assert!(result.is_ok());
}

#[tokio::test]
async fn search_rejects_invalid_from_date() {
    // Given: Server (no client call is expected - parsing fails before dispatch)
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "q".to_string(),
        from_date: Some("not-a-date".to_string()),
        ..Default::default()
    };

    // When: Search with a malformed from_date
    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns a parse error
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid from_date"));
}

#[tokio::test]
async fn search_rejects_inverted_range() {
    // Given: Server (no client call is expected - validation fails before dispatch)
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "q".to_string(),
        from_date: Some("2026-08-05T00:00:00Z".to_string()),
        to_date: Some("2026-08-01T00:00:00Z".to_string()),
        ..Default::default()
    };

    // When: Search with from_date after to_date
    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns an inverted-range error
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .contains("from_date must be earlier than to_date")
    );
}

#[tokio::test]
async fn search_accepts_equal_from_and_to_date() {
    // Both bounds are documented as inclusive, so from_date == to_date is a
    // single-instant window, not an inverted range: it must reach the client.
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_search_messages()
        .withf(|p| {
            let instant: chrono::DateTime<chrono::Utc> = "2026-08-01T00:00:00Z".parse().unwrap();
            p.from_date == Some(instant) && p.to_date == Some(instant)
        })
        .returning(move |_| Ok(create_test_search_result(vec![], "q", 0)));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "q".to_string(),
        from_date: Some("2026-08-01T00:00:00Z".to_string()),
        to_date: Some("2026-08-01T00:00:00Z".to_string()),
        ..Default::default()
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(
        result.is_ok(),
        "equal inclusive bounds must be accepted, got {:?}",
        result.err()
    );
}

#[tokio::test]
async fn search_rejects_to_date_older_than_hours_back_window() {
    // to_date alone, older than `now - hours_back`, describes a structurally
    // empty window: the search would burn its whole timeout budget for nothing.
    // No client call and no rate-limiter token may be spent.
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let long_ago = chrono::Utc::now() - chrono::Duration::days(30);

    let request = SearchRequest {
        query: "q".to_string(),
        to_date: Some(long_ago.to_rfc3339()),
        ..Default::default()
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("from_date"),
        "error must tell the caller to supply from_date, got: {err}"
    );
}

#[tokio::test]
async fn search_accepts_to_date_inside_hours_back_window() {
    // A to_date that still overlaps the hours_back window is a real window.
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_search_messages()
        .returning(move |_| Ok(create_test_search_result(vec![], "q", 0)));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let recent = chrono::Utc::now() - chrono::Duration::hours(1);

    let request = SearchRequest {
        query: "q".to_string(),
        to_date: Some(recent.to_rfc3339()),
        ..Default::default()
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_ok(), "got {:?}", result.err());
}

#[tokio::test]
async fn search_rejects_blank_from_date() {
    // A present-but-blank date is a caller mistake; silently degrading it to
    // "no filter" would quietly return a different window than asked for.
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "q".to_string(),
        from_date: Some("   ".to_string()),
        ..Default::default()
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid from_date"));
}

#[tokio::test]
async fn search_accepts_padded_from_date() {
    // A valid date with surrounding whitespace parses rather than erroring.
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_search_messages()
        .withf(|p| p.from_date == Some("2026-08-01T00:00:00Z".parse().unwrap()))
        .returning(move |_| Ok(create_test_search_result(vec![], "q", 0)));

    let mock_limiter = permissive_limiter();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "q".to_string(),
        from_date: Some(" 2026-08-01T00:00:00Z ".to_string()),
        ..Default::default()
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_ok(), "got {:?}", result.err());
}
