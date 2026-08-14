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

    // Then: All 16 tools are present, with SEP-2549 cache hints attached
    assert_eq!(result.tools.len(), 16);
    assert_eq!(result.ttl_ms, Some(3_600_000));
    assert_eq!(result.cache_scope, Some(rmcp::model::CacheScope::Private));

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

#[test]
fn tools_list_hints_gated_on_protocol_version() {
    // Given: Server instance with mocks
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When/Then: A client that negotiated 2026-07-28 (SEP-2549's home version)
    // gets the cache hints...
    let current = server.tools_list_result_for(Some(rmcp::model::ProtocolVersion::V_2026_07_28));
    assert_eq!(current.ttl_ms, Some(3_600_000));
    assert_eq!(current.cache_scope, Some(rmcp::model::CacheScope::Private));
    assert_eq!(current.tools.len(), 16);

    // ...but a client on an older negotiated version does not, mirroring the
    // #[tool_handler] macro's own default list_tools gating...
    let legacy = server.tools_list_result_for(Some(rmcp::model::ProtocolVersion::V_2025_11_25));
    assert_eq!(legacy.ttl_ms, None);
    assert!(legacy.cache_scope.is_none());
    assert_eq!(legacy.tools.len(), 16); // tool list itself is unaffected

    // ...and neither does a client that never negotiated a protocol version at
    // all (the legacy `initialize`-handshake fallback CLAUDE.md documents).
    let unversioned = server.tools_list_result_for(None);
    assert_eq!(unversioned.ttl_ms, None);
    assert!(unversioned.cache_scope.is_none());
}

#[test]
fn server_defaults_match_the_shipped_config_defaults() {
    // The bug this guards: changing a default in config/defaults.rs while
    // server.rs keeps a hand-copied number desyncs every construction path
    // that does not call the matching with_* builder.
    use crate::config::defaults::*;

    let server = McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    );

    assert_eq!(server.media_download_cost(), default_media_download_cost());
    assert_eq!(server.transcription_cost(), default_transcription_cost());
    assert_eq!(
        server.response_byte_budget() as u64,
        default_response_byte_budget()
    );
    assert_eq!(
        server.media_batch_max_total_bytes() as u64,
        default_media_batch_max_total_bytes()
    );
}
