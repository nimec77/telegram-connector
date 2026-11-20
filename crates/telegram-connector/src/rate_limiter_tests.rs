use super::RateLimiter;
use crate::config::RateLimitConfig;
use std::sync::Arc;

fn poisoned_limiter() -> Arc<RateLimiter> {
    let limiter = Arc::new(RateLimiter::new(&RateLimitConfig {
        max_tokens: 5,
        refill_rate: 1.0,
        ..RateLimitConfig::default()
    }));
    limiter.poison_for_test();
    limiter
}

#[tokio::test]
async fn poisoned_limiter_blocks_tool_rpc_after_local_validation() {
    use crate::mcp::server::McpServer;
    use crate::mcp::tools::GetChannelInfoRequest;
    use crate::telegram::MockTelegramClientTrait;
    use rmcp::handler::server::common::RequestId;
    use rmcp::handler::server::wrapper::Parameters;
    use rmcp::model::NumberOrString;
    let limiter = poisoned_limiter();
    let mut client = MockTelegramClientTrait::new();
    client.expect_get_channel_info().never();
    client.expect_get_full_channel_info().never();
    let server = McpServer::new(Arc::new(client), limiter);
    for (identifier, expected) in [
        ("@channel", "rate limiter unavailable"),
        ("", "Invalid channel_id"),
    ] {
        let error = server
            .get_channel_info(
                Parameters(GetChannelInfoRequest {
                    channel_identifier: identifier.into(),
                    include_full: None,
                }),
                RequestId(NumberOrString::Number(1)),
            )
            .await
            .expect_err("must refuse");
        assert!(error.contains(expected), "{error}");
    }
}

#[tokio::test]
async fn poisoned_limiter_status_keeps_independent_budget_metadata() {
    use crate::mcp::server::McpServer;
    use crate::telegram::{AccountStatus, MockTelegramClientTrait};
    use rmcp::handler::server::common::RequestId;
    use rmcp::model::NumberOrString;
    let limiter = poisoned_limiter();
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_cached_account_status()
        .returning(AccountStatus::default);
    let server = McpServer::new(Arc::new(client), limiter);
    let response = server
        .check_mcp_status(RequestId(NumberOrString::Number(1)))
        .await
        .expect("usable status");
    let json: serde_json::Value = serde_json::from_str(&response).expect("JSON");
    assert!(json["rate_limiter"]["tokens"].is_null());
    assert!(json["rate_limiter"]["unavailable"].as_str().is_some());
    assert_eq!(json["rate_limiter"]["capacity"], 5.0);
    assert_eq!(json["rate_limiter"]["refill_per_sec"], 1.0);
    assert!(json.get("last_response_write_unavailable").is_none());
}
