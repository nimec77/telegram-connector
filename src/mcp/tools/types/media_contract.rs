//! Typed media outcomes and mode-specific assembly. No binary appears in JSON data.
use super::canonical::ChannelId;
use super::contract::{ToolError, WireData};
use super::responses::{GetMessageMediaResponse, MediaBatchFailure, MediaBatchSummary};
use crate::error::Error;
use crate::mcp::artifacts::ArtifactPin;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::types::{self, PeerNamespace};
use rmcp::model::{CallToolResult, ContentBlock};
use schemars::JsonSchema;
use serde::Serialize;
use std::sync::Arc;

/// Independent of message response and base64 limits.
pub(crate) const MAX_MEDIA_OVERHEAD_BYTES: usize = 65_536;

pub(crate) struct MediaOutput {
    pub single: bool,
    // Batch-only late delivery refunds. Single-call charging remains unchanged.
    pub artifact_refund: Option<(Arc<dyn RateLimiterTrait>, u32)>,
    pub channel_reference: String,
    pub channel_id: types::ChannelId,
    pub peer_namespace: PeerNamespace,
    pub requested: usize,
    pub items: Vec<(i64, Result<MediaImage, Error>)>,
    pub max_total_bytes: u64,
}
pub(crate) struct MediaImage {
    pub base64: String,
    pub metadata: GetMessageMediaResponse,
    pub artifact: Option<MediaArtifact>,
    pub artifact_uri: Option<String>,
    pub artifact_pin: Option<Arc<ArtifactPin>>,
}

/// Metadata for a cached processed JPEG preview, never an original attachment.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MediaArtifact {
    pub kind: ArtifactKind,
    pub mime_type: String,
    pub size_bytes: usize,
    pub width: u32,
    pub height: u32,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_content_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    ProcessedJpeg,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MediaData {
    pub items: Vec<MediaItem>,
    pub summary: MediaSummary,
}

/// Internal lifetime wrapper; the DTO and its wire schema retain their existing shape.
#[derive(Serialize, JsonSchema)]
#[serde(transparent)]
#[schemars(transparent)]
pub(crate) struct ProtectedMediaData {
    data: MediaData,
    // Keep eviction protection alive through both JSON copies and content-block assembly.
    #[serde(skip)]
    #[schemars(skip)]
    _artifact_pins: Vec<Arc<ArtifactPin>>,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum MediaItem {
    Image {
        channel_id: ChannelId,
        peer_namespace: PeerNamespace,
        message_id: i64,
        content_index: usize,
        metadata: ImageMetadata,
        #[serde(skip_serializing_if = "Option::is_none")]
        artifact: Option<Box<MediaArtifact>>,
    },
    Failure {
        channel_id: ChannelId,
        peer_namespace: PeerNamespace,
        message_id: i64,
        error: ToolError,
    },
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct MediaSummary {
    pub requested: usize,
    pub returned: usize,
    pub failed: usize,
    pub total_base64_bytes: usize,
    pub max_total_bytes: u64,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct ImageMetadata {
    pub media_type: types::MediaType,
    pub is_thumbnail: bool,
    pub caption: Option<String>,
    pub source_variant_width: Option<u32>,
    pub source_variant_height: Option<u32>,
    pub source_variant_size_bytes: u64,
    pub largest_available_width: Option<u32>,
    pub largest_available_height: Option<u32>,
    pub returned_width: u32,
    pub returned_height: u32,
    pub returned_size_bytes: usize,
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_info: Option<types::VideoInfo>,
}
impl From<GetMessageMediaResponse> for ImageMetadata {
    fn from(m: GetMessageMediaResponse) -> Self {
        Self {
            media_type: m.media_type,
            is_thumbnail: m.is_thumbnail,
            caption: m.caption,
            source_variant_width: m.source_variant_width,
            source_variant_height: m.source_variant_height,
            source_variant_size_bytes: m.source_variant_size_bytes,
            largest_available_width: m.largest_available_width,
            largest_available_height: m.largest_available_height,
            returned_width: m.returned_width,
            returned_height: m.returned_height,
            returned_size_bytes: m.returned_size_bytes,
            mime_type: m.mime_type,
            video_info: m.video_info,
        }
    }
}
pub(crate) fn legacy_failure(error: &Error) -> String {
    match error {
        Error::MessageNotFound { .. } => "not_found".into(),
        Error::NoVisualMedia { .. } => "no_visual_media".into(),
        Error::PayloadCapExceeded { .. } => "payload_cap_reached".into(),
        Error::Internal(message) => format!("internal_error: {message}"),
        other => format!("download_failed: {other}"),
    }
}
impl WireData for MediaOutput {
    type Canonical = ProtectedMediaData;
    fn canonical(mut self) -> Result<(ProtectedMediaData, Vec<ContentBlock>), Error> {
        self.reject_undeliverable_artifacts();
        let mut artifact_pins = Vec::new();
        let mut images = Vec::new();
        let mut items = Vec::new();
        let mut bytes = 0;
        let mut returned = 0;
        for (message_id, outcome) in self.items {
            match outcome {
                Ok(mut image) => {
                    if let Some(pin) = image.artifact_pin.take() {
                        artifact_pins.push(pin);
                    }
                    returned += 1;
                    bytes += image.base64.len();
                    let content_index = images.len() + 1;
                    images.push(ContentBlock::image(image.base64, "image/jpeg"));
                    if let Some(artifact) = &mut image.artifact
                        && let Some(uri) = &artifact.resource_uri
                    {
                        artifact.resource_content_index = Some(images.len() + 1);
                        images.push(ContentBlock::resource_link(
                            rmcp::model::Resource::new(uri, "Processed JPEG preview")
                                .with_mime_type("image/jpeg")
                                .with_size(artifact.size_bytes as u64),
                        ));
                    }
                    items.push(MediaItem::Image {
                        channel_id: self.channel_id.into(),
                        peer_namespace: self.peer_namespace,
                        message_id,
                        content_index,
                        metadata: image.metadata.into(),
                        artifact: image.artifact.map(Box::new),
                    });
                }
                Err(error) => items.push(MediaItem::Failure {
                    channel_id: self.channel_id.into(),
                    peer_namespace: self.peer_namespace,
                    message_id,
                    error: error.into(),
                }),
            }
        }
        Ok((
            ProtectedMediaData {
                _artifact_pins: artifact_pins,
                data: MediaData {
                    items,
                    summary: MediaSummary {
                        requested: self.requested,
                        returned,
                        failed: self.requested - returned,
                        total_base64_bytes: bytes,
                        max_total_bytes: self.max_total_bytes,
                    },
                },
            },
            images,
        ))
    }
    fn legacy(self) -> Result<CallToolResult, Error> {
        let mut content = Vec::new();
        let mut failed = Vec::new();
        let mut bytes = 0;
        let mut returned = 0;
        for (id, outcome) in self.items {
            match outcome {
                Ok(image) => {
                    bytes += image.base64.len();
                    returned += 1;
                    content.push(ContentBlock::image(image.base64, "image/jpeg"));
                    content.push(ContentBlock::text(
                        serde_json::to_string(&image.metadata)
                            .map_err(|_| Error::Internal("media serialization failed"))?,
                    ));
                }
                Err(error) => failed.push(MediaBatchFailure {
                    id,
                    reason: legacy_failure(&error),
                }),
            }
        }
        if !self.single {
            content.push(ContentBlock::text(
                serde_json::to_string(&MediaBatchSummary {
                    channel_id: self.channel_reference,
                    requested: self.requested,
                    returned,
                    failed,
                    total_base64_bytes: bytes,
                    max_total_bytes: self.max_total_bytes,
                })
                .map_err(|_| Error::Internal("media serialization failed"))?,
            ));
        }
        Ok(CallToolResult::success(content))
    }
}

impl MediaOutput {
    /// Reconcile monotonic expiry and known store failure at the publication boundary.
    /// This checks only atomics/time: no filesystem or store mutex on the async executor.
    fn reject_undeliverable_artifacts(&mut self) {
        for (_, outcome) in &mut self.items {
            if outcome.as_ref().is_ok_and(|image| {
                image
                    .artifact_pin
                    .as_ref()
                    .is_some_and(|pin| !pin.is_deliverable())
            }) {
                *outcome = Err(Error::ArtifactUnavailable);
                if let Some((limiter, cost)) = &self.artifact_refund {
                    limiter.refund(*cost);
                }
            }
        }
    }
    /// Measure the full response with image data omitted, retaining its exact summaries.
    pub(crate) fn check_overhead(&self, format: crate::config::OutputFormat) -> Result<(), Error> {
        let bytes = self
            .items
            .iter()
            .filter_map(|(_, item)| item.as_ref().ok())
            .map(|image| image.base64.len())
            .sum::<usize>();
        let shadow = Self {
            single: self.single,
            artifact_refund: None,
            channel_reference: self.channel_reference.clone(),
            channel_id: self.channel_id,
            peer_namespace: self.peer_namespace,
            requested: self.requested,
            max_total_bytes: self.max_total_bytes,
            items: self
                .items
                .iter()
                .map(|(id, item)| {
                    (
                        *id,
                        match item {
                            Ok(image) => Ok(MediaImage {
                                base64: String::new(),
                                metadata: image.metadata.clone(),
                                artifact: image.artifact.clone(),
                                artifact_uri: image.artifact_uri.clone(),
                                // A measurement shadow never owns/releases delivery protection
                                // or reconciles/refunds the real response's items.
                                artifact_pin: None,
                            }),
                            Err(error) => Err(error.clone()),
                        },
                    )
                })
                .collect(),
        };
        let result = if format == crate::config::OutputFormat::JsonV1 {
            let (mut data, images) = shadow.canonical()?;
            data.data.summary.total_base64_bytes = bytes;
            let mut result =
                super::contract::tool_result(super::contract::ToolResponse::success(data))?;
            result.content.extend(images);
            result
        } else {
            let mut result = shadow.legacy()?;
            if !self.single {
                let summary = MediaBatchSummary {
                    channel_id: self.channel_reference.clone(),
                    requested: self.requested,
                    returned: self.items.iter().filter(|(_, item)| item.is_ok()).count(),
                    failed: self
                        .items
                        .iter()
                        .filter_map(|(id, item)| {
                            item.as_ref().err().map(|error| MediaBatchFailure {
                                id: *id,
                                reason: legacy_failure(error),
                            })
                        })
                        .collect(),
                    total_base64_bytes: bytes,
                    max_total_bytes: self.max_total_bytes,
                };
                if let Some(last) = result.content.last_mut() {
                    *last = ContentBlock::text(
                        serde_json::to_string(&summary)
                            .map_err(|_| Error::Internal("media serialization failed"))?,
                    );
                }
            }
            result
        };
        if crate::mcp::runtime::response_size(&result)? > MAX_MEDIA_OVERHEAD_BYTES {
            return Err(Error::ResponseTooLarge);
        }
        Ok(())
    }
}
