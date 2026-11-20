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
        let days_back = request
            .days_back
            .unwrap_or(ChannelStats::DEFAULT_DAYS_BACK)
            .min(ChannelStats::MAX_DAYS_BACK);
        let operations = self.account_operations();
        let context = operations.history_context(crate::mcp::runtime::operation_cancellation())?;
        let stats = operations
            .channel_stats_by_reference(&request.channel_id, days_back, &context)
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
