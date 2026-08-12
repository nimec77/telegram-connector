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
            rate_limiter: RateLimiterStatus {
                tokens,
                capacity: self.rate_limiter.capacity(),
                refill_per_sec: self.rate_limiter.refill_rate(),
                costs: RateLimiterCosts {
                    // Every non-media tool acquires a literal 1.
                    search: 1,
                    media_download: self.media_download_cost,
                    transcription: self.transcription_cost,
                },
            },
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
        let include_binary = request.include_binary.unwrap_or(false);
        let entries = self.response_buffer.last(request.n.map(|n| n as usize));
        let responses: Vec<BufferedResponseEntry> = entries
            .into_iter()
            .map(|entry| {
                // Payload was valid JSON when written; Null only on corruption.
                let mut response =
                    serde_json::from_str(&entry.payload).unwrap_or(serde_json::Value::Null);
                if !include_binary {
                    omit_binary_blocks(&mut response);
                }
                BufferedResponseEntry {
                    request_id: entry.request_id,
                    tool_name: entry.tool_name,
                    written_at: chrono::DateTime::<chrono::Utc>::from(entry.written_at)
                        .to_rfc3339(),
                    size_bytes: entry.size_bytes,
                    response,
                }
            })
            .collect();

        let response = LastResponsesResponse {
            buffered: self.response_buffer.len(),
            responses,
        };

        json_response(&response)
    }
}

/// Replace base64 image content blocks with size-annotated stubs (work-order
/// D6): the replay tool exists for when context is already damaged, so binary
/// payloads only replay on explicit request.
fn omit_binary_blocks(response: &mut serde_json::Value) {
    let Some(blocks) = response
        .get_mut("result")
        .and_then(|r| r.get_mut("content"))
        .and_then(|c| c.as_array_mut())
    else {
        return;
    };
    for block in blocks {
        if block.get("type").and_then(|t| t.as_str()) != Some("image") {
            continue;
        }
        let Some(data) = block.get("data").and_then(|d| d.as_str()) else {
            continue;
        };
        let padding = data.bytes().rev().take_while(|&b| b == b'=').count();
        let size_bytes = data.len() / 4 * 3 - padding;
        let mime_type = block
            .get("mimeType")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        *block = serde_json::json!({
            "type": "image",
            "omitted": true,
            "mime_type": mime_type,
            "size_bytes": size_bytes,
        });
    }
}
