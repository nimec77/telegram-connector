//! MCP tool request and response types with JSON schemas

use crate::telegram::types::{Channel, MediaFilter};
use schemars::JsonSchema;

use serde::{Deserialize, Deserializer, Serialize};

/// Deserialize Option<MediaFilter> treating empty strings as None.
/// This handles MCP clients that send `"media_filter": ""` instead of omitting the field.
fn deserialize_optional_media_filter<'de, D>(
    deserializer: D,
) -> Result<Option<MediaFilter>, D::Error>
where
    D: Deserializer<'de>,
{
    // First try to deserialize as an Option<String> to check for empty string
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrMediaFilter {
        String(String),
        MediaFilter(MediaFilter),
        Null,
    }

    match Option::<StringOrMediaFilter>::deserialize(deserializer)? {
        None => Ok(None),
        Some(StringOrMediaFilter::Null) => Ok(None),
        Some(StringOrMediaFilter::String(s)) if s.is_empty() => Ok(None),
        Some(StringOrMediaFilter::String(s)) => {
            // Try to parse non-empty string as MediaFilter
            serde_json::from_value(serde_json::Value::String(s))
                .map(Some)
                .map_err(serde::de::Error::custom)
        }
        Some(StringOrMediaFilter::MediaFilter(f)) => Ok(Some(f)),
    }
}

// ============================================================================
// Tool 1: check_mcp_status
// ============================================================================

/// Response for check_mcp_status tool
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StatusResponse {
    #[schemars(description = "Whether Telegram client is connected")]
    pub telegram_connected: bool,

    #[schemars(description = "Available rate limiter tokens")]
    pub rate_limiter_tokens: f64,

    #[schemars(description = "Server version")]
    pub server_version: String,
}

// ============================================================================
// Tool 2: get_subscribed_channels
// ============================================================================

/// Request for get_subscribed_channels tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetChannelsRequest {
    #[schemars(description = "Maximum number of channels to return (default: 50, max: 500)")]
    pub limit: Option<u32>,

    #[schemars(description = "Offset for pagination (default: 0)")]
    pub offset: Option<u32>,
}

/// Response for get_subscribed_channels tool
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ChannelsResponse {
    #[schemars(description = "List of subscribed channels")]
    pub channels: Vec<Channel>,

    #[schemars(description = "Total number of channels (for pagination)")]
    pub total: usize,

    #[schemars(description = "Whether there are more channels available")]
    pub has_more: bool,
}

// ============================================================================
// Tool 3: get_channel_info
// ============================================================================

/// Request for get_channel_info tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetChannelInfoRequest {
    #[schemars(description = "Channel username (@channel) or numeric ID")]
    pub channel_identifier: String,
}

// Response: Channel (from telegram/types.rs)

// ============================================================================
// Tool 4: generate_message_link
// ============================================================================

/// Request for generate_message_link tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GenerateLinkRequest {
    #[schemars(description = "Numeric channel ID")]
    pub channel_id: String,

    #[schemars(description = "Message ID within the channel")]
    pub message_id: i64,

    #[schemars(description = "Also return tg:// protocol link (default: true)")]
    pub include_tg_protocol: Option<bool>,
}

/// Response for generate_message_link tool
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MessageLinkResponse {
    #[schemars(description = "Channel ID")]
    pub channel_id: String,

    #[schemars(description = "Message ID")]
    pub message_id: i64,

    #[schemars(description = "HTTPS link: https://t.me/c/{channel_id}/{message_id}?single")]
    pub https_link: String,

    #[schemars(description = "tg:// protocol link for native macOS handling")]
    pub tg_protocol_link: Option<String>,
}

// ============================================================================
// Tool 5: open_message_in_telegram
// ============================================================================

/// Request for open_message_in_telegram tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct OpenMessageRequest {
    #[schemars(description = "Numeric channel ID")]
    pub channel_id: String,

    #[schemars(description = "Message ID within the channel")]
    pub message_id: i64,

    #[schemars(description = "Use tg:// protocol (default: true). If false, uses https")]
    pub use_tg_protocol: Option<bool>,
}

/// Response for open_message_in_telegram tool
#[derive(Debug, Clone, Serialize, JsonSchema)]
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

// ============================================================================
// Tool 6: search_messages
// ============================================================================

/// Request for search_messages tool
#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct SearchRequest {
    #[schemars(
        description = "Search query. Required unless media_filter is set. Can be empty when filtering by media type only."
    )]
    pub query: String,

    #[schemars(description = "Optional: Filter by specific channel ID")]
    pub channel_id: Option<String>,

    #[schemars(description = "How many hours back to search (default: 48, max: 168)")]
    pub hours_back: Option<u32>,

    #[schemars(description = "Maximum results to return (default: 20, max: 100)")]
    pub limit: Option<u32>,

    #[schemars(
        description = "Optional: Filter by media type. This is metadata-based filtering (filters by attachment type), NOT content recognition. No OCR, no speech-to-text. Example: 'photo' returns messages WITH photos attached."
    )]
    #[serde(default, deserialize_with = "deserialize_optional_media_filter")]
    pub media_filter: Option<MediaFilter>,
}

// Response: SearchResult (from telegram/types.rs) which contains Vec<Message>

// ============================================================================
// Tool 7: get_recent_messages
// ============================================================================

/// Request for get_recent_messages tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetRecentMessagesRequest {
    #[schemars(description = "Channel ID or username (required)")]
    pub channel_id: String,

    #[schemars(description = "Hours of history to retrieve (default: 48, max: 168)")]
    pub hours_back: Option<u32>,

    #[schemars(description = "Maximum messages to return (default: 20, max: 100)")]
    pub limit: Option<u32>,

    #[schemars(
        description = "Optional: Filter by media type. Applied client-side. Example: 'photo' returns only messages with photos."
    )]
    #[serde(default, deserialize_with = "deserialize_optional_media_filter")]
    pub media_filter: Option<MediaFilter>,
}

// Response: SearchResult (from telegram/types.rs) - same as search_messages

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_response_serializes() {
        let response = StatusResponse {
            telegram_connected: true,
            rate_limiter_tokens: 45.5,
            server_version: "0.1.0".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("telegram_connected"));
        assert!(json.contains("true"));
    }

    #[test]
    fn get_channels_request_deserializes() {
        let json = r#"{"limit": 10, "offset": 5}"#;
        let request: GetChannelsRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.limit, Some(10));
        assert_eq!(request.offset, Some(5));
    }

    #[test]
    fn get_channels_request_defaults() {
        let json = r#"{}"#;
        let request: GetChannelsRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.limit, None);
        assert_eq!(request.offset, None);
    }

    #[test]
    fn search_request_validates_required_query() {
        let json = r#"{"query": "test"}"#;
        let request: SearchRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.query, "test");
        assert!(request.channel_id.is_none());
        assert!(request.media_filter.is_none());
    }

    #[test]
    fn search_request_with_media_filter_deserializes() {
        let json = r#"{"query": "AI news", "media_filter": "photo"}"#;
        let request: SearchRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.query, "AI news");
        assert_eq!(request.media_filter, Some(MediaFilter::Photo));
    }

    #[test]
    fn search_request_media_filter_snake_case() {
        let json = r#"{"query": "", "media_filter": "photo_video"}"#;
        let request: SearchRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.query, "");
        assert_eq!(request.media_filter, Some(MediaFilter::PhotoVideo));
    }

    #[test]
    fn search_request_all_media_filters_deserialize() {
        let filters = vec![
            ("photo", MediaFilter::Photo),
            ("video", MediaFilter::Video),
            ("photo_video", MediaFilter::PhotoVideo),
            ("document", MediaFilter::Document),
            ("audio", MediaFilter::Audio),
            ("voice", MediaFilter::Voice),
            ("video_note", MediaFilter::VideoNote),
            ("gif", MediaFilter::Gif),
            ("url", MediaFilter::Url),
            ("pinned", MediaFilter::Pinned),
        ];

        for (json_value, expected) in filters {
            let json = format!(r#"{{"query": "test", "media_filter": "{}"}}"#, json_value);
            let request: SearchRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(
                request.media_filter,
                Some(expected),
                "Failed for filter: {}",
                json_value
            );
        }
    }

    #[test]
    fn search_request_empty_string_media_filter_treated_as_none() {
        // MCP clients may send "" instead of null or omitting the field
        let json = r#"{"query": "test", "media_filter": ""}"#;
        let request: SearchRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.query, "test");
        assert_eq!(request.media_filter, None);
    }

    #[test]
    fn search_request_null_media_filter_treated_as_none() {
        let json = r#"{"query": "test", "media_filter": null}"#;
        let request: SearchRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.query, "test");
        assert_eq!(request.media_filter, None);
    }

    // =========================================================================
    // GetRecentMessagesRequest Tests
    // =========================================================================

    #[test]
    fn get_recent_messages_request_deserializes() {
        let json = r#"{"channel_id": "123456"}"#;
        let request: GetRecentMessagesRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.channel_id, "123456");
        assert!(request.hours_back.is_none());
        assert!(request.limit.is_none());
        assert!(request.media_filter.is_none());
    }

    #[test]
    fn get_recent_messages_request_with_all_params() {
        let json = r#"{
            "channel_id": "tech_news",
            "hours_back": 72,
            "limit": 50,
            "media_filter": "photo"
        }"#;
        let request: GetRecentMessagesRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.channel_id, "tech_news");
        assert_eq!(request.hours_back, Some(72));
        assert_eq!(request.limit, Some(50));
        assert_eq!(request.media_filter, Some(MediaFilter::Photo));
    }

    #[test]
    fn get_recent_messages_request_empty_media_filter() {
        let json = r#"{"channel_id": "123", "media_filter": ""}"#;
        let request: GetRecentMessagesRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.channel_id, "123");
        assert_eq!(request.media_filter, None);
    }
}
