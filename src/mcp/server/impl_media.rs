//! `McpServer` inherent `*_impl` methods: Media download & transcription tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::*;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn get_message_media_impl(
        &self,
        request: GetMessageMediaRequest,
    ) -> Result<CallToolResult, String> {
        const DEFAULT_MAX_DIMENSION: u32 = 1280;
        const MIN_DIMENSION: u32 = 64;
        const MAX_DIMENSION: u32 = 2048;

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
            original_width: download.width,
            original_height: download.height,
            original_size_bytes: download.source_size_bytes,
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
