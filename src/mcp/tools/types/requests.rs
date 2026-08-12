//! Request types for MCP tools.

use super::serde_helpers::{
    deserialize_optional_media_filter, deserialize_optional_response_format, flexible_i64,
    flexible_opt_bool, flexible_opt_i64, flexible_opt_string, flexible_opt_u32, flexible_string,
};
use crate::telegram::types::MediaFilter;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Response shape for message-returning tools (work-order A4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum ResponseFormat {
    /// Every message carries its own channel fields (v0.15 shape).
    #[default]
    Full,
    /// Channel fields hoisted into one response-level `channel` header and
    /// dropped from each message — saves kilobytes at limit=100 (A4).
    Compact,
}

/// Request for get_subscribed_channels tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetChannelsRequest {
    #[schemars(description = "Maximum number of channels to return (default: 50, max: 500)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub limit: Option<u32>,

    #[schemars(description = "Offset for pagination (default: 0)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub offset: Option<u32>,
}

/// Request for get_channel_info tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetChannelInfoRequest {
    #[schemars(description = "Channel username (@channel) or numeric ID")]
    #[serde(deserialize_with = "flexible_string")]
    pub channel_identifier: String,

    #[schemars(
        description = "Optional: fetch full channel info (description, member_count) with one extra Telegram RPC. Default false."
    )]
    #[serde(default, deserialize_with = "flexible_opt_bool")]
    pub include_full: Option<bool>,
}

/// Request for search_public_channels tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchPublicChannelsRequest {
    #[schemars(description = "Keyword or name to search Telegram's public directory for")]
    #[serde(deserialize_with = "flexible_string")]
    pub query: String,

    #[schemars(description = "Maximum results to return (default: 10, max: 50)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub limit: Option<u32>,
}

/// Request for generate_message_link tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GenerateLinkRequest {
    #[schemars(description = "Channel ID or username (e.g. @channelname or 1234567890)")]
    #[serde(deserialize_with = "flexible_string")]
    pub channel_id: String,

    #[schemars(description = "Message ID within the channel")]
    #[serde(deserialize_with = "flexible_i64")]
    pub message_id: i64,

    #[schemars(description = "Also return tg:// protocol link (default: true)")]
    #[serde(default, deserialize_with = "flexible_opt_bool")]
    pub include_tg_protocol: Option<bool>,
}

/// Request for open_message_in_telegram tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct OpenMessageRequest {
    #[schemars(description = "Channel ID or username (e.g. @channelname or 1234567890)")]
    #[serde(deserialize_with = "flexible_string")]
    pub channel_id: String,

    #[schemars(description = "Message ID within the channel")]
    #[serde(deserialize_with = "flexible_i64")]
    pub message_id: i64,

    #[schemars(description = "Use tg:// protocol (default: true). If false, uses https")]
    #[serde(default, deserialize_with = "flexible_opt_bool")]
    pub use_tg_protocol: Option<bool>,
}

/// Request for search_messages tool
#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct SearchRequest {
    #[schemars(
        description = "Search query. Required unless media_filter is set. Can be empty when filtering by media type only."
    )]
    #[serde(deserialize_with = "flexible_string")]
    pub query: String,

    #[schemars(description = "Optional: Filter by specific channel ID")]
    #[serde(default, deserialize_with = "flexible_opt_string")]
    pub channel_id: Option<String>,

    #[schemars(description = "How many hours back to search (default: 48, max: 72)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub hours_back: Option<u32>,

    #[schemars(description = "Maximum results to return (default: 20, max: 100)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub limit: Option<u32>,

    #[schemars(
        description = "Optional: Filter by media type. This is metadata-based filtering (filters by attachment type), NOT content recognition. No OCR, no speech-to-text. Example: 'photo' returns messages WITH photos attached."
    )]
    #[serde(default, deserialize_with = "deserialize_optional_media_filter")]
    pub media_filter: Option<MediaFilter>,

    #[schemars(
        description = "Optional: inclusive start of the time window as RFC 3339 UTC, e.g. \"2026-08-01T00:00:00Z\". Overrides hours_back. Reaching far back works best on low-traffic channels; on active channels prefer a narrower recent window, since deep windows are paged client-side and may time out."
    )]
    // Deliberately NOT `flexible_opt_string`: that helper folds a blank string to
    // `None`, which would silently drop the caller's window instead of reporting a
    // bad date. Cross-type number coercion was never advertised for dates.
    #[serde(default)]
    pub from_date: Option<String>,

    #[schemars(
        description = "Optional: inclusive end of the time window as RFC 3339 UTC. Messages newer than this are excluded. When set without from_date it must fall inside the hours_back window."
    )]
    // Blank-preserving for the same reason as `from_date` above.
    #[serde(default)]
    pub to_date: Option<String>,

    #[schemars(
        description = "Optional: collapse album (grouped media) siblings into one post-level result; when true, limit counts posts, and each collapsed post carries album.message_ids. Default: true."
    )]
    #[serde(default, deserialize_with = "flexible_opt_bool")]
    pub collapse_albums: Option<bool>,

    #[schemars(
        description = "Optional: exclusive upper message-id bound — return only messages with id < before_id. Use next_cursor.before_id from a previous response to fetch the next (older) page without offset drift. Requires channel_id."
    )]
    #[serde(default, deserialize_with = "flexible_opt_i64")]
    pub before_id: Option<i64>,

    #[schemars(
        description = "Optional: exclusive lower message-id bound — stop before messages with id <= after_id. Bounds a page at ids newer than a known message. Requires channel_id."
    )]
    #[serde(default, deserialize_with = "flexible_opt_i64")]
    pub after_id: Option<i64>,

    #[schemars(
        description = "Optional: maximum text length in characters per message (default: 2000). Longer texts are cut and flagged with text_truncated plus text_full_length; refetch the single message to get full text."
    )]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub max_text_length: Option<u32>,

    #[schemars(
        description = "Optional: response shape — 'full' (default) repeats channel fields on every message; 'compact' hoists them into one response-level channel header. compact requires a single-channel scope."
    )]
    #[serde(default, deserialize_with = "deserialize_optional_response_format")]
    pub format: Option<ResponseFormat>,
}

/// Request for get_recent_messages tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetRecentMessagesRequest {
    #[schemars(description = "Channel ID or username (required)")]
    #[serde(deserialize_with = "flexible_string")]
    pub channel_id: String,

    #[schemars(description = "Hours of history to retrieve (default: 48, max: 168)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub hours_back: Option<u32>,

    #[schemars(description = "Maximum messages to return (default: 20, max: 100)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub limit: Option<u32>,

    #[schemars(
        description = "Optional: Filter by media type. Applied client-side. Example: 'photo' returns only messages with photos."
    )]
    #[serde(default, deserialize_with = "deserialize_optional_media_filter")]
    pub media_filter: Option<MediaFilter>,

    #[schemars(
        description = "Optional: inclusive start of the time window as RFC 3339 UTC, e.g. \"2026-08-01T00:00:00Z\". Overrides hours_back. Reaching far back works best on low-traffic channels; on active channels prefer a narrower recent window, since deep windows are paged client-side and may time out."
    )]
    // Deliberately NOT `flexible_opt_string`: that helper folds a blank string to
    // `None`, which would silently drop the caller's window instead of reporting a
    // bad date. Cross-type number coercion was never advertised for dates.
    #[serde(default)]
    pub from_date: Option<String>,

    #[schemars(
        description = "Optional: inclusive end of the time window as RFC 3339 UTC. Messages newer than this are excluded. When set without from_date it must fall inside the hours_back window."
    )]
    // Blank-preserving for the same reason as `from_date` above.
    #[serde(default)]
    pub to_date: Option<String>,

    #[schemars(
        description = "Optional: collapse album (grouped media) siblings into one post-level result; when true, limit counts posts, and each collapsed post carries album.message_ids. Default: true."
    )]
    #[serde(default, deserialize_with = "flexible_opt_bool")]
    pub collapse_albums: Option<bool>,

    #[schemars(
        description = "Optional: exclusive upper message-id bound — return only messages with id < before_id. Use next_cursor.before_id from a previous response to fetch the next (older) page without offset drift."
    )]
    #[serde(default, deserialize_with = "flexible_opt_i64")]
    pub before_id: Option<i64>,

    #[schemars(
        description = "Optional: exclusive lower message-id bound — stop before messages with id <= after_id. Bounds a page at ids newer than a known message."
    )]
    #[serde(default, deserialize_with = "flexible_opt_i64")]
    pub after_id: Option<i64>,

    #[schemars(
        description = "Optional: maximum text length in characters per message (default: 2000). Longer texts are cut and flagged with text_truncated plus text_full_length; refetch the single message to get full text."
    )]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub max_text_length: Option<u32>,

    #[schemars(
        description = "Optional: response shape — 'full' (default) repeats channel fields on every message; 'compact' hoists them into one response-level channel header. compact requires a single-channel scope."
    )]
    #[serde(default, deserialize_with = "deserialize_optional_response_format")]
    pub format: Option<ResponseFormat>,
}

/// Request for get_message_by_link tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetMessageByLinkRequest {
    #[schemars(
        description = "Telegram message link. Supported formats: https://t.me/username/12345, https://t.me/c/channel_id/12345, t.me/username/12345"
    )]
    #[serde(deserialize_with = "flexible_string")]
    pub link: String,
}

/// Request for get_message_media tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetMessageMediaRequest {
    #[schemars(description = "Channel ID or username (required)")]
    #[serde(deserialize_with = "flexible_string")]
    pub channel_id: String,

    #[schemars(description = "Message ID within the channel")]
    #[serde(deserialize_with = "flexible_i64")]
    pub message_id: i64,

    #[schemars(
        description = "Longest image side in pixels after downscaling (default: 1280, clamped to 64-2048)"
    )]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub max_dimension: Option<u32>,
}

/// Request for transcribe_voice_message tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TranscribeVoiceMessageRequest {
    #[schemars(description = "Channel ID or username (required)")]
    #[serde(deserialize_with = "flexible_string")]
    pub channel_id: String,

    #[schemars(description = "Message ID within the channel")]
    #[serde(deserialize_with = "flexible_i64")]
    pub message_id: i64,

    #[schemars(
        description = "Seconds to wait for transcription to complete (default: 30, max: 120)"
    )]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub timeout_seconds: Option<u32>,
}

/// Request for get_last_responses tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetLastResponsesRequest {
    #[schemars(description = "How many recent responses to return (default: all buffered)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub n: Option<u32>,
}

#[cfg(test)]
#[path = "tests/requests_tests.rs"]
mod tests;
