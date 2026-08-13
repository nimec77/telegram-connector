//! `McpServer` inherent `*_impl` methods: Media download & transcription tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::*;

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

        let metadata = GetMessageMediaResponse {
            channel_id: request.channel_id.clone(),
            message_id: message_id.get(),
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
        };

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

        let outcomes = self
            .telegram_client
            .download_messages_media(&request.channel_id, &wire_ids, max_dimension)
            .await
            .map_err(|e| e.to_string())?;

        let mut content = Vec::new();
        let mut failed = Vec::new();
        let mut total_base64_bytes = 0usize;

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

            let processed = match process_image(&download.bytes, max_dimension) {
                Ok(processed) => processed,
                Err(e) => {
                    failed.push(MediaBatchFailure {
                        id,
                        reason: format!("download_failed: {e}"),
                    });
                    continue;
                }
            };

            total_base64_bytes += processed.base64_jpeg.len();
            let metadata = GetMessageMediaResponse {
                channel_id: request.channel_id.clone(),
                message_id: id,
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
            };
            content.push(ContentBlock::image(processed.base64_jpeg, "image/jpeg"));
            content.push(ContentBlock::text(json_response(&metadata)?));
        }

        let returned = content.len() / 2;
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
