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

        let media_artifacts = if self.output_format == crate::config::OutputFormat::JsonV1 {
            if let Some(store) = &self.media_artifacts {
                use crate::mcp::tools::types::responses::{
                    MediaArtifactsStatus, MediaArtifactsUsage,
                };
                let store = std::sync::Arc::clone(store);
                let snapshot = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    crate::mcp::runtime::image_job(move || store.snapshot()),
                )
                .await;
                let (usage, unavailable) = match snapshot {
                    Ok(Ok(Ok(snapshot))) => (
                        Some(MediaArtifactsUsage {
                            entries: snapshot.entries,
                            bytes: snapshot.bytes,
                        }),
                        None,
                    ),
                    _ => (None, Some("Media artifact storage is unavailable".into())),
                };
                Some(MediaArtifactsStatus {
                    enabled: true,
                    file_export_enabled: self.media_artifacts_config.file_export_enabled,
                    ttl_seconds: self.media_artifacts_config.ttl_seconds,
                    max_entries: self.media_artifacts_config.max_entries,
                    max_total_bytes: self.media_artifacts_config.max_total_bytes,
                    max_resource_read_bytes: super::impl_resources::MAX_RESOURCE_BYTES,
                    resource_read_timeout_seconds: super::impl_resources::RESOURCE_READ_SECONDS,
                    usage,
                    unavailable,
                })
            } else {
                None
            }
        } else {
            None
        };
        let response = StatusResponse {
            media_artifacts,
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
                non_binary_max_bytes:
                    crate::mcp::tools::types::media_contract::MAX_MEDIA_OVERHEAD_BYTES,
                raw_buffer_slots: crate::telegram::media_pipeline::RAW_MEDIA_SLOTS,
                raw_memory_budget_bytes: crate::telegram::media_pipeline::RAW_MEDIA_MEMORY_BYTES,
                image_processing_slots: crate::mcp::runtime::IMAGE_PROCESSING_SLOTS,
                max_decoded_dimension: crate::mcp::tools::image::MAX_DECODED_DIMENSION,
                max_decoded_pixels: crate::mcp::tools::image::MAX_DECODED_PIXELS,
                max_decoded_allocation_bytes: crate::mcp::tools::image::MAX_DECODED_BYTES,
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

/// Replace image and resource binary payloads with size-annotated stubs (work-order
/// D6): the replay tool exists for when context is already damaged, so binary
/// payloads only replay on explicit request.
fn omit_binary_blocks(response: &mut serde_json::Value) {
    let Some(result) = response.get_mut("result") else {
        return;
    };
    if let Some(contents) = result
        .get_mut("contents")
        .and_then(|contents| contents.as_array_mut())
    {
        for resource in contents {
            omit_resource_blob(resource);
        }
    }
    if let Some(blocks) = result
        .get_mut("content")
        .and_then(|content| content.as_array_mut())
    {
        for block in blocks {
            match block.get("type").and_then(|kind| kind.as_str()) {
                Some("image") => {
                    let Some(data) = block.get("data").and_then(|data| data.as_str()) else {
                        continue;
                    };
                    let size_bytes = binary_size(data);
                    let mime_type = block
                        .get("mimeType")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    *block = serde_json::json!({"type":"image", "omitted":true, "mime_type":mime_type, "size_bytes":size_bytes});
                }
                Some("resource") => {
                    if let Some(resource) = block.get_mut("resource") {
                        omit_resource_blob(resource);
                    }
                }
                _ => {}
            }
        }
    }
}
fn binary_size(data: &str) -> usize {
    let padding = data.bytes().rev().take_while(|&byte| byte == b'=').count();
    (data.len() / 4 * 3).saturating_sub(padding)
}
fn omit_resource_blob(resource: &mut serde_json::Value) {
    let Some(blob) = resource.get("blob").and_then(|blob| blob.as_str()) else {
        return;
    };
    let size_bytes = binary_size(blob);
    let mime_type = resource
        .get("mimeType")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let uri = resource
        .get("uri")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    *resource = serde_json::json!({"uri":uri, "omitted":true, "mime_type":mime_type, "size_bytes":size_bytes});
}
