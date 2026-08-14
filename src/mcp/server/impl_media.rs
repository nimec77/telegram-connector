//! `McpServer` inherent `*_impl` methods: Media download & transcription tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::*;
use crate::mcp::tools::image::{ProcessedImage, process_image_with_cap};
use crate::mcp::tools::media_budget::Base64Budget;
use crate::telegram::types::MediaDownload;

/// Longest-side pixel limit applied when a request omits `max_dimension`.
pub(super) const DEFAULT_MAX_DIMENSION: u32 = 1280;
pub(super) const MIN_DIMENSION: u32 = 64;
pub(super) const MAX_DIMENSION: u32 = 2048;
/// Hard cap on ids per batch media call. Smaller than `MAX_BATCH_IDS` (50)
/// because each id costs a download, not just a row in a response.
pub(super) const MAX_MEDIA_BATCH_IDS: usize = 10;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn get_message_media_impl(
        &self,
        request: GetMessageMediaRequest,
    ) -> Result<CallToolResult, String> {
        let message_id = parse_message_id(request.message_id)?;
        let max_dimension = request
            .max_dimension
            .unwrap_or(DEFAULT_MAX_DIMENSION)
            .clamp(MIN_DIMENSION, MAX_DIMENSION);

        // Media downloads are heavier than searches; charge the configured cost.
        self.rate_limiter
            .acquire(self.media_download_cost)
            .await
            .map_err(|e| e.to_string())?;

        let download = self
            .telegram_client
            .download_message_media(&request.channel_id, message_id.get() as i32, max_dimension)
            .await
            .map_err(|e| e.to_string())?;

        let processed = process_image(&download.bytes, max_dimension).map_err(|e| e.to_string())?;
        let metadata = media_metadata(
            request.channel_id.clone(),
            message_id.get(),
            download,
            &processed,
        );

        tracing::info!(
            channel = %request.channel_id,
            message_id = message_id.get(),
            media_type = ?metadata.media_type,
            is_thumbnail = metadata.is_thumbnail,
            returned_bytes = metadata.returned_size_bytes,
            "Message media results"
        );

        let metadata_json = json_response(&metadata)?;

        Ok(CallToolResult::success(vec![
            ContentBlock::image(processed.base64_jpeg, "image/jpeg"),
            ContentBlock::text(metadata_json),
        ]))
    }

    pub(super) async fn get_messages_media_batch_impl(
        &self,
        request: GetMessagesMediaBatchRequest,
    ) -> Result<CallToolResult, String> {
        if request.channel_id.trim().is_empty() {
            return Err("channel_id is required".to_string());
        }
        if request.message_ids.is_empty() {
            return Err("message_ids must contain at least one id".to_string());
        }

        // Dedupe silently, preserving first-seen order (same rule as
        // get_messages_batch).
        let mut seen = std::collections::HashSet::new();
        let unique: Vec<i64> = request
            .message_ids
            .iter()
            .copied()
            .filter(|id| seen.insert(*id))
            .collect();
        if unique.len() > MAX_MEDIA_BATCH_IDS {
            return Err(format!(
                "message_ids accepts at most {MAX_MEDIA_BATCH_IDS} ids per call, got {}",
                unique.len()
            ));
        }

        let mut wire_ids = Vec::with_capacity(unique.len());
        for id in &unique {
            let parsed = parse_message_id(*id)?;
            wire_ids.push(
                parsed.as_i32().ok_or_else(|| {
                    format!("message_id {} exceeds Telegram's message id range", id)
                })?,
            );
        }

        let max_dimension = request
            .max_dimension
            .unwrap_or(DEFAULT_MAX_DIMENSION)
            .clamp(MIN_DIMENSION, MAX_DIMENSION);

        // Acquire pessimistically for every requested id BEFORE any network
        // work: charging only for what succeeds would mean the limiter could
        // never refuse a batch, since the downloads would already have happened.
        // One atomic acquire keeps the D5 deficit message accurate.
        let charged = self.media_download_cost.saturating_mul(unique.len() as u32);
        self.rate_limiter
            .acquire(charged)
            .await
            .map_err(|e| e.to_string())?;

        let outcomes = match self
            .telegram_client
            .download_messages_media(&request.channel_id, &wire_ids, max_dimension)
            .await
        {
            Ok(outcomes) => outcomes,
            Err(e) => {
                // The call still performed a channel resolution and a fetch RPC
                // against Telegram before failing (channel resolution failure,
                // network error, or a Telegram-side FLOOD_WAIT) — that work
                // happened and should not be free, or a caller could hammer an
                // unresolvable channel at zero cost, which is exactly the flood
                // behaviour the limiter exists to prevent. Refund everything
                // except one token: the same cost get_messages_batch already
                // charges (acquire(1)) for that identical resolve+fetch shape
                // of work. saturating_sub guards a hypothetical charged of 0.
                self.rate_limiter.refund(charged.saturating_sub(1));
                return Err(e.to_string());
            }
        };

        let mut content = Vec::new();
        let mut failed = Vec::new();
        let mut total_base64_bytes = 0usize;
        let mut returned = 0usize;
        let mut budget = Base64Budget::new(self.media_batch_max_total_bytes);

        for outcome in outcomes {
            let id = i64::from(outcome.message_id);
            let download = match outcome.result {
                Ok(download) => download,
                Err(e) => {
                    failed.push(MediaBatchFailure {
                        id,
                        reason: failure_reason(&e),
                    });
                    continue;
                }
            };

            // Encoding runs in request order, so allocation is deterministic no
            // matter which download finished first.
            let Some(allowance) = budget.allowance() else {
                failed.push(MediaBatchFailure {
                    id,
                    reason: "payload_cap_reached".to_string(),
                });
                continue;
            };

            // process_image_with_cap already shrinks the target dimension
            // iteratively until the encoded payload fits — that is the
            // progressive downscaling, no second implementation needed.
            let processed = match process_image_with_cap(&download.bytes, max_dimension, allowance)
            {
                Ok(processed) => processed,
                Err(e) => {
                    // Budget deliberately untouched: nothing was emitted, so
                    // later ids keep their full allowance.
                    failed.push(MediaBatchFailure {
                        id,
                        reason: post_download_failure_reason(&e),
                    });
                    continue;
                }
            };
            // Serialize before mutating any batch-level state, so a failure
            // here (unreachable today — GetMessageMediaResponse has no map
            // keys or floats, the only things that make serde_json::to_string
            // fail — but not a compile-time guarantee) lands this id in
            // `failed` instead of returning early and leaking every other
            // id's charge along with it (`json_response(&metadata)?` used to
            // do exactly that).
            let metadata = media_metadata(request.channel_id.clone(), id, download, &processed);
            let metadata_json = match json_response(&metadata) {
                Ok(json) => json,
                Err(e) => {
                    // Neither a download failure nor a cap drop: serializing the
                    // metadata failed. Unreachable today (the response type has
                    // no map keys or floats, the only things that make
                    // serde_json::to_string fail) but not a compile-time
                    // guarantee, so it gets an honest token of its own.
                    // Budget deliberately untouched, same reasoning as above.
                    failed.push(MediaBatchFailure {
                        id,
                        reason: format!("internal_error: {e}"),
                    });
                    continue;
                }
            };
            budget.consume(processed.base64_jpeg.len());

            total_base64_bytes += processed.base64_jpeg.len();
            content.push(ContentBlock::image(processed.base64_jpeg, "image/jpeg"));
            content.push(ContentBlock::text(metadata_json));
            returned += 1;
        }

        // Ids that produced no image cost nothing — hand their tokens back.
        // The bucket clamps at capacity, so this can never inflate it.
        // unique.len() >= returned always holds (`returned` counts a subset
        // of the requested ids), so this subtraction cannot underflow.
        let refunded = self.media_download_cost * (unique.len() - returned) as u32;
        self.rate_limiter.refund(refunded);

        tracing::info!(
            channel = %request.channel_id,
            requested = unique.len(),
            returned,
            failed = failed.len(),
            total_base64_bytes,
            "Messages media batch results"
        );

        let summary = MediaBatchSummary {
            channel_id: request.channel_id,
            requested: unique.len(),
            returned,
            failed,
            total_base64_bytes,
            max_total_bytes: self.media_batch_max_total_bytes as u64,
        };
        content.push(ContentBlock::text(json_response(&summary)?));

        Ok(CallToolResult::success(content))
    }

    pub(super) async fn transcribe_voice_message_impl(
        &self,
        request: TranscribeVoiceMessageRequest,
    ) -> Result<String, String> {
        let message_id = parse_message_id(request.message_id)?;
        // Bounds come from [transcription] config (AD-6). Floor at 1 and cap at the
        // configured max via min/max rather than clamp(1, max) so a misconfigured
        // max of 0 can't panic clamp's min<=max invariant.
        let timeout_secs = request
            .timeout_seconds
            .unwrap_or(self.transcription_default_timeout_secs)
            .min(self.transcription_max_timeout_secs)
            .max(1);

        // Premium fast-fail: only a definitive `false` short-circuits (before
        // spending a rate-limit token or a transcribeAudio call). Unknown
        // (`None`) falls through to the RPC-error path.
        if self.telegram_client.is_premium().await == Some(false) {
            return Err(Error::PremiumRequired.to_string());
        }

        // Transcription is precious (Telegram weekly quota); charge more than a search.
        self.rate_limiter
            .acquire(self.transcription_cost)
            .await
            .map_err(|e| e.to_string())?;

        let outcome = self
            .telegram_client
            .transcribe_audio(&request.channel_id, message_id.get() as i32, timeout_secs)
            .await
            .map_err(|e| e.to_string())?;

        // Only voice and video_note are transcribable. Pass the MediaType through
        // unchanged so its serde derive (snake_case) is the single source of the
        // "voice" / "video_note" wire names — shared with search_messages.
        let media_type = match outcome.media_type {
            mt @ (crate::telegram::types::MediaType::Voice
            | crate::telegram::types::MediaType::VideoNote) => mt,
            other => {
                return Err(format!(
                    "unexpected media type for transcription: {:?}",
                    other
                ));
            }
        };

        let response = TranscribeVoiceMessageResponse {
            text: outcome.text,
            partial: outcome.partial,
            duration_seconds: outcome.duration_seconds,
            media_type,
        };

        tracing::info!(
            channel = %request.channel_id,
            message_id = message_id.get(),
            media_type = ?response.media_type,
            partial = response.partial,
            duration_seconds = ?response.duration_seconds,
            "Transcription results"
        );

        json_response(&response)
    }
}

/// Build the metadata block accompanying a returned image.
///
/// Shared by `get_message_media_impl` and `get_messages_media_batch_impl` so
/// a batch-of-one produces byte-identical metadata to the single-image tool
/// (`batch_of_one_matches_the_single_tool_metadata`).
fn media_metadata(
    channel_id: String,
    message_id: i64,
    download: MediaDownload,
    processed: &ProcessedImage,
) -> GetMessageMediaResponse {
    GetMessageMediaResponse {
        channel_id,
        message_id,
        media_type: download.media_type,
        is_thumbnail: download.is_thumbnail,
        caption: download.caption,
        source_variant_width: download.width,
        source_variant_height: download.height,
        source_variant_size_bytes: download.source_size_bytes,
        largest_available_width: download.largest_width,
        largest_available_height: download.largest_height,
        returned_width: processed.width,
        returned_height: processed.height,
        returned_size_bytes: processed.encoded_size_bytes,
        mime_type: "image/jpeg".to_string(),
        video_info: download.video_info,
    }
}

/// Map a per-id download failure to a stable, machine-readable reason.
///
/// Callers branch on these tokens, so they are deliberately not the `Display`
/// text of the underlying error — that text is free to change. The match is
/// total, so a new `MediaFetchError` variant is a compile error here rather
/// than a silent fall-through to `download_failed`.
fn failure_reason(error: &MediaFetchError) -> String {
    match error {
        MediaFetchError::NotFound => "not_found".to_string(),
        MediaFetchError::NoVisualMedia { .. } => "no_visual_media".to_string(),
        MediaFetchError::Failed(inner) => format!("download_failed: {inner}"),
    }
}

/// Map a failure that happened *after* a successful download to a stable,
/// machine-readable reason token.
///
/// Unlike `failure_reason`, this matches a catch-all: `Error` is the crate-wide
/// enum with sixteen variants, only one of which is meaningful to a caller
/// here. Enumerating the rest would be noise, and `download_failed` with the
/// error's text attached is the honest default for all of them.
fn post_download_failure_reason(error: &Error) -> String {
    match error {
        Error::PayloadCapExceeded { .. } => "payload_cap_reached".to_string(),
        other => format!("download_failed: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_exhaustion_maps_to_the_payload_cap_token() {
        let reason = post_download_failure_reason(&Error::PayloadCapExceeded { limit: 32_768 });
        assert_eq!(
            reason, "payload_cap_reached",
            "an image that downloaded fine but could not be shrunk is a cap drop, \
             not a download failure"
        );
    }

    #[test]
    fn a_real_failure_still_maps_to_the_download_failed_token() {
        let reason = post_download_failure_reason(&Error::DownloadFailed("boom".to_string()));
        assert_eq!(reason, "download_failed: media download failed: boom");
    }
}
