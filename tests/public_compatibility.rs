//! An external read-only embedder using the connector's existing public paths.

use rmcp::handler::server::common::RequestId;
use rmcp::model::NumberOrString;
use std::sync::Arc;
use telegram_connector::config::Config;
use telegram_connector::mcp::McpServer;
use telegram_connector::mcp::tools::StatusResponse;
use telegram_connector::telegram::trait_def::TelegramClientTrait;
use telegram_connector::telegram::transcription::TranscriptionDeadline;
use telegram_connector::telegram::types::{
    AccountStatus, Channel, ChannelIdentity, ChannelPage, ChannelResolution, ChannelStats,
    HistoryParams, MediaBatch, MediaDownload, Message, MessageBatch, SearchParams, SearchResult,
    TranscriptionOutcome,
};
use telegram_connector::{Error, RateLimiter, RateLimiterTrait};

struct ReadOnlyClient;

fn unavailable<T>() -> Result<T, Error> {
    Err(Error::Internal("offline compatibility fixture"))
}

#[async_trait::async_trait]
impl TelegramClientTrait for ReadOnlyClient {
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
    ) -> Result<MediaBatch, Error> {
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
    ) -> Result<Vec<ChannelResolution<Error>>, Error> {
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

#[tokio::test]
async fn existing_embedder_runs_legacy_status_without_implementing_writes() -> anyhow::Result<()> {
    let config: Config = toml::from_str("[telegram]\napi_id = 123")?;
    let limiter = Arc::new(RateLimiter::new(&config.rate_limiting));
    let tokens = RateLimiterTrait::try_available_tokens(limiter.as_ref())?;
    let server: telegram_connector::mcp::server::McpServer<_, _> =
        McpServer::new(Arc::new(ReadOnlyClient), Arc::clone(&limiter));

    let request_id = RequestId(NumberOrString::Number(1));
    let status = server
        .check_mcp_status(request_id)
        .await
        .map_err(anyhow::Error::msg)?;
    let status: StatusResponse = serde_json::from_str(&status)?;
    assert!(!status.telegram_connected);
    assert_eq!(status.rate_limiter.tokens, Some(tokens));
    assert_eq!(limiter.try_available_tokens()?, tokens);
    Ok(())
}

#[test]
fn core_account_primitives_keep_the_facade_type_identity() -> anyhow::Result<()> {
    let config: Config = toml::from_str("[telegram]\napi_id = 123")?;
    let limits: &telegram_core::config::RateLimitConfig = &config.rate_limiting;
    let limiter: telegram_core::rate_limiter::RateLimiter = RateLimiter::new(limits);
    fn core_error(error: Error) -> telegram_core::error::Error {
        error
    }
    assert!(matches!(
        core_error(Error::Internal("fixture")),
        Error::Internal("fixture")
    ));
    assert_eq!(
        RateLimiterTrait::capacity(&limiter),
        f64::from(limits.max_tokens)
    );
    Ok(())
}
