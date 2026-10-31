//! `McpServer` inherent `*_impl` method: get_channel_stats (work-order A5).

use super::McpServer;
use crate::error::Error;
use crate::mcp::tools::GetChannelStatsRequest;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::stats::ChannelStats;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn get_channel_stats_data(
        &self,
        request: GetChannelStatsRequest,
    ) -> Result<ChannelStats, Error> {
        if request.channel_id.trim().is_empty() {
            return Err(Error::InvalidInput("channel_id is required".to_string()));
        }
        let days_back = request
            .days_back
            .unwrap_or(ChannelStats::DEFAULT_DAYS_BACK)
            .min(ChannelStats::MAX_DAYS_BACK);
        if days_back == 0 {
            return Err(Error::InvalidInput(
                "days_back must be greater than 0".to_string(),
            ));
        }

        crate::mcp::tools::validation::validate_channel_reference(&request.channel_id)?;
        self.rate_limiter.acquire(1).await?;

        let stats = self
            .telegram_client
            .get_channel_stats(&request.channel_id, days_back)
            .await?;

        tracing::info!(
            channel = %request.channel_id,
            days_back,
            posts = stats.post_count,
            complete = stats.sample.complete,
            "Channel stats results"
        );
        Ok(stats)
    }
}
