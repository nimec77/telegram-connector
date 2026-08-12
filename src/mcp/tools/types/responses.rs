//! Response types for MCP tools.

use crate::telegram::types::{
    AlbumInfo, AudioInfo, Channel, ChannelId, ChannelName, ForwardInfo, LinkPreview, MediaType,
    Message, MessageId, MessageReaction, QueryMetadata, SearchResult, UserId, Username, VideoInfo,
};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Response for check_mcp_status tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StatusResponse {
    #[schemars(description = "Whether Telegram client is connected")]
    pub telegram_connected: bool,

    #[schemars(description = "Available rate limiter tokens")]
    pub rate_limiter_tokens: f64,

    #[schemars(description = "Server version")]
    pub server_version: String,

    #[schemars(description = "Inbound JSON-RPC requests received this session")]
    pub requests_received: u64,

    #[schemars(description = "Responses successfully written to stdout this session")]
    pub responses_written: u64,

    #[schemars(
        description = "Seconds since the last successful response write (null before the first)"
    )]
    pub last_response_write_age_secs: Option<u64>,

    #[schemars(description = "Session start time (RFC3339 UTC)")]
    pub session_started_at: String,

    #[schemars(description = "Session uptime in seconds")]
    pub session_uptime_secs: u64,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[schemars(
        description = "Whether the connected account has Telegram Premium (null if unknown)"
    )]
    pub premium: Option<bool>,
}

/// Response for transcribe_voice_message tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TranscribeVoiceMessageResponse {
    #[schemars(description = "The transcription text (may be partial)")]
    pub text: String,

    #[schemars(description = "True if the timeout elapsed before Telegram finished transcribing")]
    pub partial: bool,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[schemars(description = "Audio duration in seconds (from message metadata), if available")]
    pub duration_seconds: Option<u32>,

    #[schemars(
        description = "Media type of the transcribed message (\"voice\" or \"video_note\")"
    )]
    pub media_type: MediaType,
}

/// Response for channel-returning tools (subscriptions and discovery).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChannelsResponse {
    #[schemars(description = "Channels in this page")]
    pub channels: Vec<Channel>,

    #[schemars(description = "Number of channels in this response")]
    pub returned: usize,

    #[schemars(description = "Genuine total matches; null when the source cannot know it")]
    pub total: Option<usize>,

    #[schemars(description = "Whether more results exist; null when unknown (D10)")]
    pub has_more: Option<bool>,
}

/// Response for generate_message_link tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MessageLinkResponse {
    #[schemars(description = "Canonical numeric channel ID")]
    pub channel_id: String,

    #[schemars(description = "Message ID")]
    pub message_id: i64,

    #[schemars(
        description = "Best shareable HTTPS link: https://t.me/{username}/{message_id} for public channels, https://t.me/c/{channel_id}/{message_id} for private chats"
    )]
    pub https_link: String,

    #[schemars(
        description = "tg:// protocol link (tg://resolve for public channels, tg://privatepost for private chats)"
    )]
    pub tg_protocol_link: Option<String>,

    #[schemars(description = "Members-only https://t.me/c/… form (always present)")]
    pub internal_link: String,

    #[schemars(description = "Whether the channel has a public username")]
    pub is_public: bool,
}

/// Response for open_message_in_telegram tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OpenMessageResponse {
    #[schemars(description = "Whether the operation succeeded")]
    pub success: bool,

    #[schemars(description = "Human-readable message")]
    pub message: String,

    #[schemars(description = "The link that was opened")]
    pub link_used: String,

    #[schemars(description = "Whether the Telegram app was launched")]
    pub app_opened: bool,
}

/// One recovered response returned by get_last_responses
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BufferedResponseEntry {
    #[schemars(description = "JSON-RPC request id of the original call")]
    pub request_id: String,

    #[schemars(description = "Tool that produced the response")]
    pub tool_name: String,

    #[schemars(description = "When the response was written to stdout (RFC3339 UTC)")]
    pub written_at: String,

    #[schemars(
        description = "Wire size of the original response in bytes (payloads above max_buffered_payload_bytes are stored as a stub)"
    )]
    pub size_bytes: usize,

    #[schemars(
        description = "The JSON-RPC envelope as written to stdout; very large payloads are replaced with a stub object containing an \"omitted\" key"
    )]
    pub response: serde_json::Value,
}

/// Response for get_last_responses tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LastResponsesResponse {
    #[schemars(description = "Recovered responses, newest first")]
    pub responses: Vec<BufferedResponseEntry>,

    #[schemars(description = "Total responses currently buffered")]
    pub buffered: usize,
}

/// Metadata text block accompanying the get_message_media image block
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetMessageMediaResponse {
    #[schemars(description = "Channel the message belongs to (as passed in the request)")]
    pub channel_id: String,

    #[schemars(description = "Message ID")]
    pub message_id: i64,

    #[schemars(description = "Media type of the source message (photo, video, ...)")]
    pub media_type: MediaType,

    #[schemars(description = "True when the image is a video's thumbnail, not the media itself")]
    pub is_thumbnail: bool,

    #[schemars(description = "Message caption, if any")]
    pub caption: Option<String>,

    #[schemars(
        description = "Pixel width of the downloaded source variant (the variant actually fetched, not necessarily the original)"
    )]
    pub source_variant_width: Option<u32>,

    #[schemars(
        description = "Pixel height of the downloaded source variant (the variant actually fetched, not necessarily the original)"
    )]
    pub source_variant_height: Option<u32>,

    #[schemars(description = "Byte size of the downloaded source variant")]
    pub source_variant_size_bytes: u64,

    #[schemars(description = "Pixel width of the largest variant Telegram offers")]
    pub largest_available_width: Option<u32>,

    #[schemars(description = "Pixel height of the largest variant Telegram offers")]
    pub largest_available_height: Option<u32>,

    #[schemars(description = "Pixel width of the returned image")]
    pub returned_width: u32,

    #[schemars(description = "Pixel height of the returned image")]
    pub returned_height: u32,

    #[schemars(description = "Encoded JPEG size in bytes (before base64 expansion)")]
    pub returned_size_bytes: usize,

    #[schemars(description = "Always image/jpeg")]
    pub mime_type: String,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[schemars(
        description = "Zero-cost video metadata (duration, dimensions, kind); video media only"
    )]
    pub video_info: Option<VideoInfo>,
}

/// Wire representation of a single message (mirrors the domain `Message`).
///
/// `sender_id` / `sender_name` mirror the domain type exactly (serialized as `null`
/// when absent, no `skip_serializing_if`) to preserve the existing wire format; the
/// enrichment fields are omitted when absent.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MessageResponse {
    pub id: MessageId,
    pub channel_id: ChannelId,
    pub channel_name: ChannelName,
    pub channel_username: Option<Username>,
    pub text: String,
    /// Present (true) only when text was cut at max_text_length (B4).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub text_truncated: Option<bool>,
    /// Full text length in characters, present only when truncated.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub text_full_length: Option<u64>,
    pub timestamp: DateTime<Utc>,
    pub sender_id: Option<UserId>,
    pub sender_name: Option<String>,
    pub has_media: bool,
    pub media_type: MediaType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub forwarded_from: Option<ForwardInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub link_preview: Option<LinkPreview>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub views: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub forwards: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reply_to_message_id: Option<MessageId>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub video_info: Option<VideoInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub audio_info: Option<AudioInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub grouped_id: Option<i64>,
    pub link: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reactions: Option<Vec<MessageReaction>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reactions_total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub album: Option<AlbumInfo>,
}

impl From<Message> for MessageResponse {
    fn from(m: Message) -> Self {
        Self {
            id: m.id,
            channel_id: m.channel_id,
            channel_name: m.channel_name,
            channel_username: m.channel_username,
            text: m.text,
            text_truncated: None,
            text_full_length: None,
            timestamp: m.timestamp,
            sender_id: m.sender_id,
            sender_name: m.sender_name,
            has_media: m.has_media,
            media_type: m.media_type,
            forwarded_from: m.forwarded_from,
            link_preview: m.link_preview,
            views: m.views,
            forwards: m.forwards,
            reply_to_message_id: m.reply_to_message_id,
            video_info: m.video_info,
            audio_info: m.audio_info,
            grouped_id: m.grouped_id,
            link: m.link,
            reactions: m.reactions,
            reactions_total: m.reactions_total,
            album: m.album,
        }
    }
}

/// Resume point for the next (older) page (work-order A8).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
pub struct NextCursor {
    /// Pass as `before_id` on the next call: pages strictly older than
    /// the last message included here.
    pub before_id: MessageId,
}

/// Wire representation of a search/history result set.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchResponse {
    pub messages: Vec<MessageResponse>,
    pub returned: u64,
    /// More qualifying messages exist beyond this page (limit- or
    /// budget-truncated) — page on via next_cursor (A8/B4).
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub next_cursor: Option<NextCursor>,
    pub search_time_ms: u64,
    pub query_metadata: QueryMetadata,
}

impl From<SearchResult> for SearchResponse {
    fn from(r: SearchResult) -> Self {
        Self {
            messages: r.messages.into_iter().map(MessageResponse::from).collect(),
            returned: r.returned,
            has_more: r.has_more,
            next_cursor: None,
            search_time_ms: r.search_time_ms,
            query_metadata: r.query_metadata,
        }
    }
}

#[cfg(test)]
#[path = "tests/responses_tests.rs"]
mod tests;
