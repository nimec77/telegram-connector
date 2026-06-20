//! Request types for MCP tools.

use super::serde_helpers::{
    deserialize_optional_media_filter, flexible_i64, flexible_opt_bool, flexible_opt_string,
    flexible_opt_u32, flexible_string,
};
use crate::telegram::types::MediaFilter;
use schemars::JsonSchema;
use serde::Deserialize;

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
}

/// Request for generate_message_link tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GenerateLinkRequest {
    #[schemars(description = "Numeric channel ID")]
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
    #[schemars(description = "Numeric channel ID")]
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

    #[schemars(description = "How many hours back to search (default: 48, max: 168)")]
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
mod tests {
    use super::*;

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
    fn get_message_by_link_request_deserializes() {
        let json = r#"{"link": "https://t.me/swodki/575403"}"#;
        let request: GetMessageByLinkRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.link, "https://t.me/swodki/575403");
    }

    #[test]
    fn get_recent_messages_request_empty_media_filter() {
        let json = r#"{"channel_id": "123", "media_filter": ""}"#;
        let request: GetRecentMessagesRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.channel_id, "123");
        assert_eq!(request.media_filter, None);
    }

    #[test]
    fn get_channels_request_accepts_string_numbers() {
        let json = r#"{"limit": "10", "offset": "5"}"#;
        let request: GetChannelsRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.limit, Some(10));
        assert_eq!(request.offset, Some(5));
    }

    #[test]
    fn get_channels_request_empty_string_limit_is_none() {
        let json = r#"{"limit": ""}"#;
        let request: GetChannelsRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.limit, None);
    }

    #[test]
    fn search_request_accepts_string_numbers() {
        let json = r#"{"query": "ai", "hours_back": "72", "limit": "50"}"#;
        let request: SearchRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.hours_back, Some(72));
        assert_eq!(request.limit, Some(50));
        assert_eq!(request.query, "ai");
    }

    #[test]
    fn search_request_channel_id_accepts_number() {
        let json = r#"{"query": "ai", "channel_id": 123456}"#;
        let request: SearchRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.channel_id, Some("123456".to_string()));
    }

    #[test]
    fn search_request_query_accepts_number() {
        let json = r#"{"query": 42}"#;
        let request: SearchRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.query, "42");
    }

    #[test]
    fn generate_link_request_message_id_accepts_string() {
        let json = r#"{"channel_id": "123", "message_id": "575403"}"#;
        let request: GenerateLinkRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.channel_id, "123");
        assert_eq!(request.message_id, 575403);
    }

    #[test]
    fn generate_link_request_channel_id_accepts_number() {
        let json = r#"{"channel_id": 456, "message_id": 1}"#;
        let request: GenerateLinkRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.channel_id, "456");
    }

    #[test]
    fn open_message_request_bool_accepts_string() {
        let json = r#"{"channel_id": "1", "message_id": "2", "use_tg_protocol": "false"}"#;
        let request: OpenMessageRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.use_tg_protocol, Some(false));
    }

    #[test]
    fn generate_link_request_bool_accepts_string_true() {
        let json = r#"{"channel_id": "1", "message_id": "2", "include_tg_protocol": "true"}"#;
        let request: GenerateLinkRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.include_tg_protocol, Some(true));
    }

    #[test]
    fn get_recent_messages_request_channel_id_accepts_number() {
        let json = r#"{"channel_id": 123456}"#;
        let request: GetRecentMessagesRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.channel_id, "123456");
    }

    #[test]
    fn get_message_by_link_request_link_accepts_number() {
        let json = r#"{"link": 575403}"#;
        let request: GetMessageByLinkRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.link, "575403");
    }

    #[test]
    fn get_channel_info_request_accepts_numeric_id() {
        let json = r#"{"channel_identifier": 1234567}"#;
        let request: GetChannelInfoRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.channel_identifier, "1234567");
    }

    #[test]
    fn get_message_media_request_deserializes_with_flexible_scalars() {
        // message_id arrives as a numeric string; channel_id as a number.
        let json = r#"{"channel_id": 123456, "message_id": "42", "max_dimension": "640"}"#;
        let request: GetMessageMediaRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.channel_id, "123456");
        assert_eq!(request.message_id, 42);
        assert_eq!(request.max_dimension, Some(640));
    }

    #[test]
    fn get_message_media_request_max_dimension_defaults_to_none() {
        let json = r#"{"channel_id": "news", "message_id": 42}"#;
        let request: GetMessageMediaRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.max_dimension, None);
    }

    #[test]
    fn transcribe_request_deserializes_with_flexible_scalars() {
        let json = r#"{"channel_id": 123456, "message_id": "42", "timeout_seconds": "60"}"#;
        let request: TranscribeVoiceMessageRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.channel_id, "123456");
        assert_eq!(request.message_id, 42);
        assert_eq!(request.timeout_seconds, Some(60));
    }

    #[test]
    fn transcribe_request_timeout_defaults_to_none() {
        let json = r#"{"channel_id": "news", "message_id": 42}"#;
        let request: TranscribeVoiceMessageRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.timeout_seconds, None);
    }
}
