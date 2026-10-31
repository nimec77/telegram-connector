//! Build and inspect a real server without credentials, session files or RPCs.
//! Run: cargo run --offline --locked --example inert_server
use rmcp::handler::server::common::RequestId;
use rmcp::model::NumberOrString;
use std::sync::Arc;
use telegram_connector::mcp::McpServer;
use telegram_connector::mcp::tools::StatusResponse;
use telegram_connector::telegram::transcription::TranscriptionDeadline;
use telegram_connector::telegram::types::{
    AccountStatus, Channel, ChannelIdentity, ChannelPage, ChannelResolution, ChannelStats,
    HistoryParams, MediaDownload, MediaFetchOutcome, Message, MessageBatch, SearchParams,
    SearchResult, TranscriptionOutcome,
};
use telegram_connector::{Config, Error, RateLimiter, RateLimiterTrait, TelegramClientTrait};

// An external implementation of the unsealed client seam. Any accidental RPC-like
// call fails instead of touching a network; status reads only the local observation.
struct InertClient;
fn unavailable<T>() -> Result<T, Error> {
    Err(Error::InvalidInput(
        "inert example has no Telegram connection".into(),
    ))
}

#[async_trait::async_trait]
impl TelegramClientTrait for InertClient {
    async fn search_messages(&self, _params: &SearchParams) -> Result<SearchResult, Error> {
        unavailable()
    }
    async fn get_recent_messages(&self, _params: &HistoryParams) -> Result<SearchResult, Error> {
        unavailable()
    }
    async fn get_channel_info(&self, _identifier: &str) -> Result<Channel, Error> {
        unavailable()
    }
    async fn get_full_channel_info(&self, _identifier: &str) -> Result<Channel, Error> {
        unavailable()
    }
    async fn get_subscribed_channels(
        &self,
        _limit: u32,
        _offset: u32,
    ) -> Result<ChannelPage, Error> {
        unavailable()
    }
    async fn search_public_channels(
        &self,
        _query: &str,
        _limit: u32,
    ) -> Result<Vec<Channel>, Error> {
        unavailable()
    }
    async fn cached_account_status(&self) -> AccountStatus {
        AccountStatus::default()
    }
    async fn refresh_account_status(&self) -> Result<AccountStatus, Error> {
        unavailable()
    }
    async fn get_message_by_id(
        &self,
        _channel_ref: &str,
        _message_id: i32,
    ) -> Result<Message, Error> {
        unavailable()
    }
    async fn get_messages_batch(
        &self,
        _channel_ref: &str,
        _message_ids: &[i32],
    ) -> Result<MessageBatch, Error> {
        unavailable()
    }
    async fn resolve_channel_identity(&self, _channel_ref: &str) -> Result<ChannelIdentity, Error> {
        unavailable()
    }
    async fn download_message_media(
        &self,
        _channel_ref: &str,
        _message_id: i32,
        _max_dimension: u32,
    ) -> Result<MediaDownload, Error> {
        unavailable()
    }
    async fn download_messages_media(
        &self,
        _channel_ref: &str,
        _message_ids: &[i32],
        _max_dimension: u32,
    ) -> Result<Vec<MediaFetchOutcome>, Error> {
        unavailable()
    }
    async fn transcribe_audio(
        &self,
        _channel_ref: &str,
        _message_id: i32,
        _deadline: TranscriptionDeadline,
    ) -> Result<TranscriptionOutcome, Error> {
        unavailable()
    }
    async fn resolve_channels(
        &self,
        _identifiers: &[String],
    ) -> Result<Vec<ChannelResolution>, Error> {
        unavailable()
    }
    async fn get_channel_stats(
        &self,
        _channel_ref: &str,
        _days_back: u32,
    ) -> Result<ChannelStats, Error> {
        unavailable()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // In-memory serde parsing supplies defaults but does not run loader validation.
    let config: Config = toml::from_str("[telegram]\napi_id = 123")?;
    config.telegram.timeouts.validate()?;
    config.search.validate()?;
    config.rate_limiting.validate()?;
    config.limits.validate()?;
    config.server.validate()?;
    let limiter = Arc::new(RateLimiter::new(&config.rate_limiting));
    let server = McpServer::new(Arc::new(InertClient), Arc::clone(&limiter))
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
