//! Build and inspect a real server without credentials, session files or RPCs.
//! Run: cargo run --offline --locked --example inert_server
#[path = "support/offline_client.rs"]
mod offline_client;
use rmcp::handler::server::common::RequestId;
use rmcp::model::NumberOrString;
use std::sync::Arc;
use telegram_connector::mcp::McpServer;
use telegram_connector::mcp::tools::StatusResponse;
use telegram_connector::{Config, RateLimiter, RateLimiterTrait};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // In-memory serde parsing supplies defaults but does not run loader validation.
    let config: Config = toml::from_str("[telegram]\napi_id = 123")?;
    config.telegram.timeouts.validate()?;
    config.telegram.validate_media_limits()?;
    config.search.validate()?;
    config.rate_limiting.validate()?;
    config.limits.validate()?;
    config.server.validate()?;
    let limiter = Arc::new(RateLimiter::new(&config.rate_limiting));
    let server = McpServer::new(
        Arc::new(offline_client::OfflineClient::new(false)),
        Arc::clone(&limiter),
    )
    .with_output_format(config.server.output_format)
    .with_observability(&config.observability)
    .with_media_download_cost(config.rate_limiting.media_download_cost)
    .with_transcription_cost(config.rate_limiting.transcription_cost)
    .with_transcription_timeouts(
        config.transcription.default_timeout_seconds,
        config.transcription.max_timeout_seconds,
    )
    .with_response_byte_budget(config.limits.response_byte_budget)
    .with_media_batch_max_total_bytes(config.limits.media_batch_max_total_bytes);
    let before = limiter.try_available_tokens()?;
    let json = server
        .check_mcp_status(RequestId(NumberOrString::Number(1)))
        .await
        .map_err(anyhow::Error::msg)?;
    let status: StatusResponse = serde_json::from_str(&json)?;
    assert!(!status.telegram_connected);
    assert_eq!(status.rate_limiter.tokens, Some(before));
    assert_eq!(
        status.rate_limiter.costs.media_download,
        config.rate_limiting.media_download_cost
    );
    assert_eq!(
        status.rate_limiter.costs.transcription,
        config.rate_limiting.transcription_cost
    );
    assert_eq!(
        status.media.max_total_bytes,
        config.limits.media_batch_max_total_bytes
    );
    assert_eq!(limiter.try_available_tokens()?, before); // status does not charge
    assert_eq!(server.metrics().requests_received(), 0); // no transport involved
    assert_eq!(server.response_buffer().try_len()?, 0);
    assert_eq!(
        limiter.capacity(),
        f64::from(config.rate_limiting.max_tokens)
    );
    Ok(())
}
