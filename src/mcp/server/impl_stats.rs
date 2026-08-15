//! `McpServer` inherent `*_impl` method: get_channel_stats (work-order A5).

use super::McpServer;
use crate::mcp::tools::{GetChannelStatsRequest, json_response};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::stats::ChannelStats;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn get_channel_stats_impl(
        &self,
        request: GetChannelStatsRequest,
    ) -> Result<String, String> {
        if request.channel_id.trim().is_empty() {
            return Err("channel_id is required".to_string());
        }
        let days_back = request
            .days_back
            .unwrap_or(ChannelStats::DEFAULT_DAYS_BACK)
            .min(ChannelStats::MAX_DAYS_BACK);
        if days_back == 0 {
            return Err("days_back must be greater than 0".to_string());
        }

        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        let stats = self
            .telegram_client
            .get_channel_stats(&request.channel_id, days_back)
            .await
            .map_err(|e| e.to_string())?;

        tracing::info!(
            channel = %request.channel_id,
            days_back,
            posts = stats.post_count,
            complete = stats.sample.complete,
            "Channel stats results"
        );
        json_response(&stats)
    }
}
