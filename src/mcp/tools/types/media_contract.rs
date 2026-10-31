//! Typed media outcomes and mode-specific assembly. No binary appears in JSON data.
use super::canonical::ChannelId;
use super::contract::{ToolError, WireData};
use super::responses::{GetMessageMediaResponse, MediaBatchFailure, MediaBatchSummary};
use crate::error::Error;
use crate::telegram::types::{self, PeerNamespace};
use rmcp::model::{CallToolResult, ContentBlock};
use schemars::JsonSchema;
use serde::Serialize;

pub(crate) struct MediaOutput {
    pub single: bool,
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
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MediaData {
    pub items: Vec<MediaItem>,
    pub summary: MediaSummary,
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
    type Canonical = MediaData;
    fn canonical(self) -> Result<(MediaData, Vec<ContentBlock>), Error> {
        let mut images = Vec::new();
        let mut items = Vec::new();
        let mut bytes = 0;
        for (message_id, outcome) in self.items {
            match outcome {
                Ok(image) => {
                    bytes += image.base64.len();
                    let content_index = images.len() + 1;
                    images.push(ContentBlock::image(image.base64, "image/jpeg"));
                    items.push(MediaItem::Image {
                        channel_id: self.channel_id.into(),
                        peer_namespace: self.peer_namespace,
                        message_id,
                        content_index,
                        metadata: image.metadata.into(),
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
        let returned = images.len();
        Ok((
            MediaData {
                items,
                summary: MediaSummary {
                    requested: self.requested,
                    returned,
                    failed: self.requested - returned,
                    total_base64_bytes: bytes,
                    max_total_bytes: self.max_total_bytes,
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
