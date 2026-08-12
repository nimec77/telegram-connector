//! `McpServer` inherent `*_impl` method: resolve_channels (work-order A7).

use super::*;

/// Hard cap on identifiers per resolve call (roadmap: capped at 20).
pub(super) const MAX_RESOLVE_IDENTIFIERS: usize = 20;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn resolve_channels_impl(
        &self,
        request: ResolveChannelsRequest,
    ) -> Result<String, String> {
        if request.identifiers.is_empty() {
            return Err("identifiers must contain at least one entry".to_string());
        }
        if request.identifiers.len() > MAX_RESOLVE_IDENTIFIERS {
            return Err(format!(
                "identifiers accepts at most {MAX_RESOLVE_IDENTIFIERS} entries per call, got {}",
                request.identifiers.len()
            ));
        }
        if request.identifiers.iter().any(|i| i.trim().is_empty()) {
            return Err("identifiers must not contain blank entries".to_string());
        }

        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        let resolutions = self
            .telegram_client
            .resolve_channels(&request.identifiers)
            .await
            .map_err(|e| e.to_string())?;

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
        json_response(&response)
    }
}
