//! `McpServer` inherent `*_impl` methods: Media download & transcription tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::McpServer;
use crate::error::Error;
use crate::mcp::artifacts::{ArtifactPin, ArtifactStore, StoreError};
use crate::mcp::tools::image::{ProcessedImage, process_image_with_cap};
use crate::mcp::tools::media_budget::Base64Budget;
use crate::mcp::tools::types::media_contract::{
    ArtifactKind, MediaArtifact, MediaImage, MediaOutput,
};
use crate::mcp::tools::types::requests::MediaDelivery;
use crate::mcp::tools::validation::{
    dedupe_and_validate_ids, parse_message_id, validate_channel_reference, wire_message_id,
};
use crate::mcp::tools::{
    GetMessageMediaRequest, GetMessageMediaResponse, GetMessagesMediaBatchRequest,
    TranscribeVoiceMessageRequest, TranscribeVoiceMessageResponse,
};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::transcription::TranscriptionDeadline;
use crate::telegram::types::{MediaDownload, MediaFetchError, MediaFetchOutcome};
use std::sync::Arc;

/// Longest-side pixel limit applied when a request omits `max_dimension`.
pub(super) const DEFAULT_MAX_DIMENSION: u32 = 1280;
pub(super) const MIN_DIMENSION: u32 = 64;
pub(super) const MAX_DIMENSION: u32 = 2048;
/// Hard cap on ids per batch media call. Smaller than `MAX_BATCH_IDS` (50)
/// because each id costs a download, not just a row in a response.
pub(super) const MAX_MEDIA_BATCH_IDS: usize = 10;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    fn artifact_delivery(
        &self,
        delivery: MediaDelivery,
    ) -> Option<(Arc<ArtifactStore>, MediaDelivery)> {
        if delivery == MediaDelivery::Inline {
            None
        } else {
            self.media_artifacts
                .as_ref()
                .map(|store| (Arc::clone(store), delivery))
        }
    }

    async fn rollback_artifacts(
        &self,
        output: &MediaOutput,
        deadline: Option<crate::telegram::media_pipeline::MediaDeadline>,
    ) {
        let Some(store) = &self.media_artifacts else {
            return;
        };
        let uris = output
            .items
            .iter()
            .filter_map(|(_, item)| item.as_ref().ok()?.artifact_uri.clone())
            .collect::<Vec<_>>();
        if uris.is_empty() {
            return;
        }
        let store = Arc::clone(store);
        let job = crate::mcp::runtime::image_job(move || {
            for uri in uris {
                store.remove(&uri)?;
            }
            Ok::<_, StoreError>(())
        });
        let result = match deadline {
            Some(deadline) => deadline
                .run(job)
                .await
                .map_err(|_| ())
                .and_then(|result| result.map_err(|_| ())),
            None => tokio::time::timeout(std::time::Duration::from_secs(5), job)
                .await
                .map_err(|_| ())
                .and_then(|result| result.map_err(|_| ())),
        };
        if !matches!(result, Ok(Ok(()))) {
            tracing::warn!(
                "Media artifact rollback incomplete; bounded entries remain until cleanup"
            );
        }
    }

    fn validate_media_delivery(&self, delivery: MediaDelivery) -> Result<(), Error> {
        if delivery != MediaDelivery::Inline
            && (self.output_format == crate::config::OutputFormat::Legacy
                || self.media_artifacts.is_none()
                || (delivery.file() && !self.media_artifacts_config.file_export_enabled))
        {
            return Err(Error::InvalidInput(
                "Requested media delivery is unavailable".into(),
            ));
        }
        Ok(())
    }

    pub(super) async fn get_message_media_data(
        &self,
        request: GetMessageMediaRequest,
    ) -> Result<MediaOutput, Error> {
        self.validate_media_delivery(request.delivery)?;
        let message_id = parse_message_id(request.message_id)?;
        let wire_id = wire_message_id(message_id)?;
        let max_dimension = request
            .max_dimension
            .unwrap_or(DEFAULT_MAX_DIMENSION)
            .clamp(MIN_DIMENSION, MAX_DIMENSION);

        crate::mcp::tools::validation::validate_channel_reference(&request.channel_id)?;
        crate::mcp::runtime::report_progress("Media request validated").await;

        // Media downloads are heavier than searches; charge the configured cost.
        self.rate_limiter.acquire(self.media_download_cost).await?;

        let mut download = self
            .telegram_client
            .download_message_media(&request.channel_id, wire_id, max_dimension)
            .await?;

        crate::mcp::runtime::report_progress("Media downloaded").await;
        let channel_id = download.channel_id;
        let peer_namespace = download.peer_namespace;
        let deadline = download.lease.as_ref().map(|lease| lease.deadline());
        let (processed, artifact, artifact_uri, artifact_pin) = process_download(
            &mut download,
            max_dimension,
            crate::mcp::tools::image::MAX_BASE64_LEN,
            deadline,
            self.artifact_delivery(request.delivery),
        )
        .await?;
        crate::mcp::runtime::report_progress("Image encoded").await;
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

        let output = MediaOutput {
            single: true,
            artifact_refund: None,
            channel_reference: request.channel_id,
            channel_id,
            peer_namespace,
            requested: 1,
            items: vec![(
                message_id.get(),
                Ok(MediaImage {
                    base64: processed.base64_jpeg,
                    metadata,
                    artifact,
                    artifact_uri,
                    artifact_pin,
                }),
            )],
            max_total_bytes: crate::mcp::tools::image::MAX_BASE64_LEN as u64,
        };
        if let Err(error) = output.check_overhead(self.output_format) {
            self.rollback_artifacts(&output, deadline).await;
            self.rate_limiter.refund(self.media_download_cost);
            return Err(error);
        }
        Ok(output)
    }

    pub(super) async fn get_messages_media_batch_data(
        &self,
        request: GetMessagesMediaBatchRequest,
    ) -> Result<MediaOutput, Error> {
        self.validate_media_delivery(request.delivery)?;
        if request.channel_id.trim().is_empty() {
            return Err(Error::InvalidInput("channel_id is required".to_string()));
        }
        if request.message_ids.is_empty() {
            return Err(Error::InvalidInput(
                "message_ids must contain at least one id".to_string(),
            ));
        }

        let (unique, wire_ids) =
            dedupe_and_validate_ids(&request.message_ids, MAX_MEDIA_BATCH_IDS)?;

        let max_dimension = request
            .max_dimension
            .unwrap_or(DEFAULT_MAX_DIMENSION)
            .clamp(MIN_DIMENSION, MAX_DIMENSION);

        crate::mcp::tools::validation::validate_channel_reference(&request.channel_id)?;
        crate::mcp::runtime::report_progress("Media request validated").await;

        // Acquire pessimistically for every requested id BEFORE any network
        // work: charging only for what succeeds would mean the limiter could
        // never refuse a batch, since the downloads would already have happened.
        // One atomic acquire keeps the D5 deficit message accurate.
        let charged = self.media_download_cost.saturating_mul(unique.len() as u32);
        self.rate_limiter.acquire(charged).await?;

        let outcomes = match self
            .telegram_client
            .download_messages_media_pipeline(&request.channel_id, &wire_ids, max_dimension)
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
                return Err(e);
            }
        };

        crate::mcp::runtime::report_progress("Media downloaded").await;
        let channel_id = outcomes.channel_id;
        let peer_namespace = outcomes.peer_namespace;
        let deadline = outcomes.deadline;
        let mut outcomes = outcomes.into_stream();
        let mut items = Vec::new();
        let mut failed = 0usize;
        let mut total_base64_bytes = 0usize;
        let mut returned = 0usize;
        let mut budget = Base64Budget::new(self.media_batch_max_total_bytes);

        use futures::StreamExt as _;
        for (&wire_id, requested_id) in wire_ids.iter().zip(&unique) {
            let next = match deadline {
                Some(deadline) => deadline.run(outcomes.next()).await,
                None => Ok(outcomes.next().await),
            };
            let outcome = match next {
                Ok(Some(outcome)) if outcome.message_id == wire_id => outcome,
                Ok(_) => MediaFetchOutcome {
                    message_id: wire_id,
                    result: Err(MediaFetchError::Failed(Error::Internal(
                        "media outcome identity unavailable",
                    ))),
                },
                Err(error) => MediaFetchOutcome {
                    message_id: wire_id,
                    result: Err(MediaFetchError::Failed(error)),
                },
            };
            let id = *requested_id;
            // Encoding runs in request order, so budget allocation is
            // deterministic no matter which download finished first.
            match process_media_outcome(
                &request.channel_id,
                max_dimension,
                &mut budget,
                outcome,
                deadline,
                self.artifact_delivery(request.delivery),
            )
            .await
            {
                Ok(image) => {
                    total_base64_bytes += image.base64.len();
                    items.push((id, Ok(image)));
                    returned += 1;
                }
                Err(error) => {
                    failed += 1;
                    items.push((id, Err(error)));
                }
            }
            crate::mcp::runtime::report_progress("Media item processed").await;
        }

        let artifact_refund = Arc::clone(&self.rate_limiter);
        let output = MediaOutput {
            single: false,
            artifact_refund: Some((artifact_refund, self.media_download_cost)),
            channel_reference: request.channel_id,
            channel_id,
            peer_namespace,
            requested: unique.len(),
            items,
            max_total_bytes: self.media_batch_max_total_bytes as u64,
        };
        if let Err(error) = output.check_overhead(self.output_format) {
            self.rollback_artifacts(&output, deadline).await;
            self.rate_limiter.refund(charged);
            return Err(error);
        }

        // Ids that produced no image cost nothing — hand their tokens back.
        // The bucket clamps at capacity, so this can never inflate it.
        // unique.len() >= returned always holds (`returned` counts a subset
        // of the requested ids), so this subtraction cannot underflow.
        let refunded = self
            .media_download_cost
            .saturating_mul(unique.len().saturating_sub(returned) as u32);
        self.rate_limiter.refund(refunded);

        tracing::info!(
            channel = %output.channel_reference,
            requested = unique.len(),
            returned,
            failed,
            total_base64_bytes,
            "Messages media batch results"
        );

        Ok(output)
    }

    pub(super) async fn transcribe_voice_message_data(
        &self,
        request: TranscribeVoiceMessageRequest,
    ) -> Result<TranscribeVoiceMessageResponse, Error> {
        validate_channel_reference(&request.channel_id)?;
        let message_id = parse_message_id(request.message_id)?;
        let wire_id = wire_message_id(message_id)?;
        // Bounds come from [transcription] config (AD-6). Floor at 1 and cap at the
        // configured max via min/max rather than clamp(1, max) so a misconfigured
        // max of 0 can't panic clamp's min<=max invariant.
        let timeout_secs = request
            .timeout_seconds
            .unwrap_or(self.transcription_default_timeout_secs)
            .min(self.transcription_max_timeout_secs)
            .max(1);

        crate::mcp::runtime::report_progress("Transcription request validated").await;

        // Cached non-Premium observations can reject for free; a cache miss
        // must not start account RPCs before acquiring the operation charge.
        let cached = self.telegram_client.cached_account_status().await;
        if cached.premium == Some(false) {
            return Err(Error::PremiumRequired);
        }

        // Transcription is precious (Telegram weekly quota); charge more than a search.
        self.rate_limiter.acquire(self.transcription_cost).await?;

        let deadline = TranscriptionDeadline::new(timeout_secs);
        if cached.premium.is_none() {
            let refreshed = deadline
                .run(self.telegram_client.refresh_account_status())
                .await?;
            if !refreshed.authorized {
                return Err(Error::Auth("Telegram account is not authenticated".into()));
            }
            if refreshed.premium == Some(false) {
                return Err(Error::PremiumRequired);
            }
        }

        crate::mcp::runtime::report_progress("Transcription prepared").await;
        let outcome = self
            .telegram_client
            .transcribe_audio(&request.channel_id, wire_id, deadline)
            .await?;

        crate::mcp::runtime::report_progress("Transcription received").await;

        // Only voice and video_note are transcribable. Pass the MediaType through
        // unchanged so its serde derive (snake_case) is the single source of the
        // "voice" / "video_note" wire names — shared with search_messages.
        let media_type = match outcome.media_type {
            mt @ (crate::telegram::types::MediaType::Voice
            | crate::telegram::types::MediaType::VideoNote) => mt,
            other => {
                return Err(Error::NotTranscribable {
                    media_type: format!("{other:?}"),
                });
            }
        };

        let response = TranscribeVoiceMessageResponse {
            target: Some((
                outcome.channel_id,
                outcome.peer_namespace,
                outcome.message_id,
            )),
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

        Ok(response)
    }
}

/// Build the metadata block accompanying a returned image.
///
/// Shared by `get_message_media_data` and `get_messages_media_batch_data` so
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

/// Process one batch outcome end to end: download-failure mapping, budget
/// allowance, decode/shrink/encode on a blocking thread, metadata
/// serialization and optional artifact storage. Returns the preview for a success, or the
/// machine-readable failure reason. The budget is consumed only after every
/// fallible step has succeeded, so a failed id never charges the cap and
/// later ids keep their full allowance.
async fn process_media_outcome(
    channel_id: &str,
    max_dimension: u32,
    budget: &mut Base64Budget,
    outcome: MediaFetchOutcome,
    deadline: Option<crate::telegram::media_pipeline::MediaDeadline>,
    artifacts: Option<(Arc<ArtifactStore>, MediaDelivery)>,
) -> Result<MediaImage, Error> {
    let id = i64::from(outcome.message_id);
    let mut download = outcome.result.map_err(|e| match e {
        MediaFetchError::NotFound => Error::MessageNotFound {
            channel: channel_id.to_string(),
            message_id: i64::from(outcome.message_id),
        },
        MediaFetchError::NoVisualMedia { media_type } => Error::NoVisualMedia { media_type },
        MediaFetchError::Failed(error) => error,
    })?;

    let Some(allowance) = budget.allowance() else {
        return Err(Error::PayloadCapExceeded { limit: 0 });
    };

    // process_image_with_cap already shrinks the target dimension iteratively
    // until the encoded payload fits — that is the progressive downscaling.
    // Encode on a blocking thread: a Lanczos3 resize plus JPEG encode is
    // hundreds of milliseconds of pure CPU. A panicked/cancelled task reports
    // the id rather than failing the batch.
    let deadline = deadline.or_else(|| download.lease.as_ref().map(|lease| lease.deadline()));
    let (processed, artifact, artifact_uri, artifact_pin) =
        process_download(&mut download, max_dimension, allowance, deadline, artifacts).await?;

    // Serialize before consuming the budget, so a (today unreachable)
    // serialization failure lands this id in `failed` with the budget
    // untouched instead of leaking the allowance.
    let metadata = media_metadata(channel_id.to_string(), id, download, &processed);

    budget.consume(processed.base64_jpeg.len());
    Ok(MediaImage {
        base64: processed.base64_jpeg,
        metadata,
        artifact,
        artifact_uri,
        artifact_pin,
    })
}

/// Move both raw bytes and their admission lease into the non-cancellable worker.
async fn process_download(
    download: &mut MediaDownload,
    max_dimension: u32,
    allowance: usize,
    deadline: Option<crate::telegram::media_pipeline::MediaDeadline>,
    artifacts: Option<(Arc<ArtifactStore>, MediaDelivery)>,
) -> Result<
    (
        ProcessedImage,
        Option<MediaArtifact>,
        Option<String>,
        Option<Arc<ArtifactPin>>,
    ),
    Error,
> {
    let bytes = std::mem::take(&mut download.bytes);
    let lease = download.lease.take();
    let job = crate::mcp::runtime::image_job(move || {
        let _lease = lease;
        let processed = process_image_with_cap(&bytes, max_dimension, allowance)?;
        let (artifact, uri, pin) = match artifacts {
            None => (None, None, None),
            Some((store, delivery)) => {
                use base64::Engine as _;
                let jpeg = base64::engine::general_purpose::STANDARD
                    .decode(&processed.base64_jpeg)
                    .map_err(|_| Error::Internal("processed preview encoding unavailable"))?;
                let (stored, pin) = store
                    .insert_protected(&jpeg, processed.width, processed.height)
                    .map_err(|error| match error {
                        StoreError::TooLarge => Error::ArtifactTooLarge,
                        _ => Error::ArtifactUnavailable,
                    })?;
                let file_path = if delivery.file() {
                    match stored.path.to_str() {
                        Some(path) => Some(path.to_owned()),
                        None => {
                            store
                                .remove(&stored.uri)
                                .map_err(|_| Error::ArtifactUnavailable)?;
                            return Err(Error::ArtifactUnavailable);
                        }
                    }
                } else {
                    None
                };
                (
                    Some(MediaArtifact {
                        kind: ArtifactKind::ProcessedJpeg,
                        mime_type: "image/jpeg".into(),
                        size_bytes: stored.size_bytes,
                        width: stored.width,
                        height: stored.height,
                        expires_at: stored.expires_at,
                        resource_uri: delivery.resource().then(|| stored.uri.clone()),
                        resource_content_index: None,
                        file_path,
                    }),
                    Some(stored.uri),
                    Some(pin),
                )
            }
        };
        Ok((processed, artifact, uri, pin))
    });
    let result = match deadline {
        Some(deadline) => deadline.run(job).await?,
        None => job.await,
    };
    result.map_err(|_| Error::Internal("image encode task failed"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_exhaustion_maps_to_the_payload_cap_token() {
        let reason =
            crate::mcp::tools::types::media_contract::legacy_failure(&Error::PayloadCapExceeded {
                limit: 32_768,
            });
        assert_eq!(
            reason, "payload_cap_reached",
            "an image that downloaded fine but could not be shrunk is a cap drop, \
             not a download failure"
        );
    }

    #[test]
    fn a_real_failure_still_maps_to_the_download_failed_token() {
        let reason = crate::mcp::tools::types::media_contract::legacy_failure(
            &Error::DownloadFailed("boom".to_string()),
        );
        assert_eq!(reason, "download_failed: media download failed: boom");
    }
}
