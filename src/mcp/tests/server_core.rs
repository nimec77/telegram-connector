//! Tests for MCP server core functionality (creation, ServerHandler)

use crate::mcp::server::McpServer;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use std::sync::Arc;

#[test]
fn server_new_creates_instance_with_valid_dependencies() {
    // Given: Mock client and rate limiter
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();

    let client_arc = Arc::new(mock_client);
    let limiter_arc = Arc::new(mock_limiter);

    // When: Create new server
    let server = McpServer::new(Arc::clone(&client_arc), Arc::clone(&limiter_arc));

    // Then: Server is created successfully
    // Verify Arc refcounts increased (2 refs each: original + server)
    assert_eq!(Arc::strong_count(&client_arc), 2);
    assert_eq!(Arc::strong_count(&limiter_arc), 2);

    // Cleanup
    drop(server);
    assert_eq!(Arc::strong_count(&client_arc), 1);
    assert_eq!(Arc::strong_count(&limiter_arc), 1);
}

#[test]
fn server_handler_provides_server_info() {
    // Given: Server instance with mocks
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Get server info via ServerHandler trait
    use rmcp::ServerHandler;
    let result = server.get_info();

    // Then: InitializeResult contains expected metadata
    assert_eq!(
        result.protocol_version,
        rmcp::model::ProtocolVersion::default()
    );
    assert_eq!(result.server_info.name, "telegram-mcp");
    assert_eq!(result.server_info.version, env!("CARGO_PKG_VERSION"));
    assert!(result.instructions.is_some());
    assert!(
        result
            .instructions
            .unwrap()
            .contains("Telegram MCP Connector")
    );
}

#[test]
fn tools_list_carries_cache_hints_and_stable_order() {
    // Given: Server instance with mocks
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Build the tools/list payload via the pure helper
    let result = server.tools_list_result();

    // Then: All 12 tools are present, with SEP-2549 cache hints attached
    assert_eq!(result.tools.len(), 12);
    assert_eq!(result.ttl_ms, Some(3_600_000));
    assert!(result.cache_scope.is_some());

    // And: Ordering is deterministic across calls
    let names: Vec<_> = result.tools.iter().map(|t| t.name.clone()).collect();
    let again: Vec<_> = server
        .tools_list_result()
        .tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(names, again);
}
