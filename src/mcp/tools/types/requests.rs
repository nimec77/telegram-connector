//! Request types for MCP tools.

use super::serde_helpers::deserialize_optional_media_filter;
use crate::telegram::types::MediaFilter;
use schemars::JsonSchema;
use serde::Deserialize;

/// Request for get_subscribed_channels tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetChannelsRequest {
    #[schemars(description = "Maximum number of channels to return (default: 50, max: 500)")]
    pub limit: Option<u32>,

    #[schemars(description = "Offset for pagination (default: 0)")]
    pub offset: Option<u32>,
}

/// Request for get_channel_info tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetChannelInfoRequest {
    #[schemars(description = "Channel username (@channel) or numeric ID")]
    pub channel_identifier: String,
}

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

/// Request for get_message_by_link tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetMessageByLinkRequest {
    #[schemars(
        description = "Telegram message link. Supported formats: https://t.me/username/12345, https://t.me/c/channel_id/12345, t.me/username/12345"
    )]
    pub link: String,
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
}
