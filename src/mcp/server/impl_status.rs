//! `McpServer` inherent `*_impl` methods: Status & recovery tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::McpServer;
use crate::error::Error;
use crate::mcp::tools::{
    BufferedResponseEntry, GetLastResponsesRequest, LastResponsesResponse, MediaLimits,
    RateLimiterCosts, RateLimiterStatus, StatusResponse,
};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn check_mcp_status_data(&self) -> Result<StatusResponse, Error> {
        let account = self.telegram_client.cached_account_status().await;
        let tokens = self.rate_limiter.try_available_tokens();
        let token_unavailable = tokens.as_ref().err().map(ToString::to_string);
        let last_write = self.metrics.try_last_write_age_secs();
        let last_response_write_unavailable = last_write.as_ref().err().map(ToString::to_string);

        let response = StatusResponse {
            telegram_connected: account.authorized,
            rate_limiter: RateLimiterStatus {
                tokens: tokens.ok(),
                unavailable: token_unavailable,
                capacity: self.rate_limiter.capacity(),
                refill_per_sec: self.rate_limiter.refill_rate(),
                costs: RateLimiterCosts {
                    // Every non-media tool acquires a literal 1.
                    search: 1,
                    media_download: self.media_download_cost,
                    transcription: self.transcription_cost,
                },
            },
            media: MediaLimits {
                batch_max_ids: super::impl_media::MAX_MEDIA_BATCH_IDS,
                max_total_bytes: self.media_batch_max_total_bytes as u64,
                per_image_max_bytes: crate::mcp::tools::image::MAX_BASE64_LEN,
                default_max_dimension: super::impl_media::DEFAULT_MAX_DIMENSION,
                max_dimension_limit: super::impl_media::MAX_DIMENSION,
            },
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            requests_received: self.metrics.requests_received(),
            responses_written: self.metrics.responses_written(),
            last_response_write_age_secs: last_write.ok().flatten(),
            last_response_write_unavailable,
            session_started_at: self.metrics.session_started_at_rfc3339(),
            session_uptime_secs: self.metrics.uptime_secs(),
            premium: account.premium,
        };

        Ok(response)
    }

    pub(super) async fn get_last_responses_data(
        &self,
        request: GetLastResponsesRequest,
    ) -> Result<LastResponsesResponse, Error> {
        let include_binary = request.include_binary.unwrap_or(false);
        if let Some(keys) = &request.recovery_keys {
            let selected = self.response_buffer.select_keys(keys)?;
            let buffered = selected.retained;
            let keyed = selected
                .responses
                .into_iter()
                .map(|(key, outcome)| {
                    let entry = match outcome {
                        Ok(entry) => entry,
                        Err(error) => {
                            return Ok(
                                crate::mcp::tools::types::canonical::RecoveryEntry::Failure {
                                    recovery_key: key,
                                    error: error.into(),
                                },
                            );
                        }
                    };
                    let mut response = serde_json::from_str(&entry.payload)
                        .map_err(|_| Error::Internal("invalid replay JSON"))?;
                    if !include_binary {
                        omit_binary_blocks(&mut response);
                    }
                    Ok(
                        crate::mcp::tools::types::canonical::RecoveryEntry::Response {
                            recovery_key: Some(key),
                            request_id: entry.request_id,
                            tool_name: entry.tool_name,
                            written_at: chrono::DateTime::<chrono::Utc>::from(entry.written_at),
                            size_bytes: entry.size_bytes,
                            response,
                        },
                    )
                })
                .collect::<Result<Vec<_>, Error>>()?;
            return Ok(LastResponsesResponse {
                buffered,
                responses: Vec::new(),
                keyed: Some(keyed),
            });
        }
        let snapshot = self
            .response_buffer
            .try_snapshot(request.n.map(|n| n as usize))?;
        let entries = snapshot.responses;
        let responses: Vec<BufferedResponseEntry> = entries
            .into_iter()
            .map(|entry| {
                let mut response = serde_json::from_str(&entry.payload)
                    .map_err(|_| Error::Internal("invalid replay JSON"))?;
                if !include_binary {
                    omit_binary_blocks(&mut response);
                }
                Ok(BufferedResponseEntry {
                    request_id: entry.request_id,
                    tool_name: entry.tool_name,
                    written_at: chrono::DateTime::<chrono::Utc>::from(entry.written_at)
                        .to_rfc3339(),
                    size_bytes: entry.size_bytes,
                    response,
                })
            })
            .collect::<Result<_, Error>>()?;

        let response = LastResponsesResponse {
            keyed: None,
            buffered: snapshot.buffered,
            responses,
        };

        Ok(response)
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
        // Saturating: a malformed payload shorter than one base64 quantum
        // (e.g. "==") makes the quotient 0 while padding is 2.
        let size_bytes = (data.len() / 4 * 3).saturating_sub(padding);
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
