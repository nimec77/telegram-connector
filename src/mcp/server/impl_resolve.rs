//! `McpServer` inherent `*_impl` method: resolve_channels (work-order A7).

use super::McpServer;
use crate::error::Error;
use crate::mcp::tools::{ResolveChannelsRequest, ResolveChannelsResponse};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;

/// Hard cap on identifiers per resolve call (roadmap: capped at 20).
pub(super) const MAX_RESOLVE_IDENTIFIERS: usize = 20;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn resolve_channels_data(
        &self,
        request: ResolveChannelsRequest,
    ) -> Result<ResolveChannelsResponse<Error>, Error> {
        if request.identifiers.is_empty() {
            return Err(Error::InvalidInput(
                "identifiers must contain at least one entry".to_string(),
            ));
        }
        if request.identifiers.len() > MAX_RESOLVE_IDENTIFIERS {
            return Err(Error::InvalidInput(format!(
                "identifiers accepts at most {MAX_RESOLVE_IDENTIFIERS} entries per call, got {}",
                request.identifiers.len()
            )));
        }
        if request.identifiers.iter().any(|i| i.trim().is_empty()) {
            return Err(Error::InvalidInput(
                "identifiers must not contain blank entries".to_string(),
            ));
        }

        self.rate_limiter.acquire(1).await?;

        let resolutions = self
            .telegram_client
            .resolve_channels(&request.identifiers)
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
