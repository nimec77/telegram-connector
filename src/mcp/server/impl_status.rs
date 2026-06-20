//! `McpServer` inherent `*_impl` methods: Status & recovery tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::*;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn check_mcp_status_impl(&self) -> Result<String, String> {
        let connected = self.telegram_client.is_connected().await;
        let tokens = self.rate_limiter.available_tokens();

        let response = StatusResponse {
            telegram_connected: connected,
            rate_limiter_tokens: tokens,
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            requests_received: self.metrics.requests_received(),
            responses_written: self.metrics.responses_written(),
            last_response_write_age_secs: self.metrics.last_write_age_secs(),
            session_started_at: self.metrics.session_started_at_rfc3339(),
            session_uptime_secs: self.metrics.uptime_secs(),
            premium: self.telegram_client.is_premium().await,
        };

        json_response(&response)
    }

    pub(super) async fn get_last_responses_impl(
        &self,
        request: GetLastResponsesRequest,
    ) -> Result<String, String> {
        let entries = self.response_buffer.last(request.n.map(|n| n as usize));
        let responses: Vec<BufferedResponseEntry> = entries
            .into_iter()
            .map(|entry| BufferedResponseEntry {
                request_id: entry.request_id,
                tool_name: entry.tool_name,
                written_at: chrono::DateTime::<chrono::Utc>::from(entry.written_at).to_rfc3339(),
                size_bytes: entry.size_bytes,
                // Payload was valid JSON when written; Null only on corruption.
                response: serde_json::from_str(&entry.payload).unwrap_or(serde_json::Value::Null),
            })
            .collect();

        let response = LastResponsesResponse {
            buffered: self.response_buffer.len(),
            responses,
        };

        json_response(&response)
    }
}
