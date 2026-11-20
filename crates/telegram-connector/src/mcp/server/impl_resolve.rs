//! `McpServer` inherent `*_impl` method: resolve_channels (work-order A7).

use super::McpServer;
use crate::error::Error;
use crate::mcp::tools::{ResolveChannelsRequest, ResolveChannelsResponse};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn resolve_channels_data(
        &self,
        request: ResolveChannelsRequest,
    ) -> Result<ResolveChannelsResponse<Error>, Error> {
        let operations = self.account_operations();
        let context = operations.history_context(crate::mcp::runtime::operation_cancellation())?;
        let resolutions = operations
            .resolve_channels(&request.identifiers, &context)
            .await?;

        let resolved = resolutions.iter().filter(|r| r.channel.is_some()).count();
        tracing::info!(
            requested = resolutions.len(),
            resolved,
            "Resolve channels results"
        );

        let response = ResolveChannelsResponse {
            returned: resolutions.len(),
            resolved,
            resolutions,
        };
        Ok(response)
    }
}
