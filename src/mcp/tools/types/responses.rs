//! Response types for MCP tools.

use crate::telegram::types::{
    Channel, ChannelId, ChannelName, ForwardInfo, LinkPreview, MediaType, Message, MessageId,
    QueryMetadata, SearchResult, UserId, Username,
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

/// Response for get_subscribed_channels tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChannelsResponse {
    #[schemars(description = "List of subscribed channels")]
    pub channels: Vec<Channel>,

    #[schemars(description = "Total number of channels (for pagination)")]
    pub total: usize,

    #[schemars(description = "Whether there are more channels available")]
    pub has_more: bool,
}

/// Response for generate_message_link tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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

    #[schemars(description = "Pixel width of the downloaded source variant")]
    pub original_width: Option<u32>,

    #[schemars(description = "Pixel height of the downloaded source variant")]
    pub original_height: Option<u32>,

    #[schemars(description = "Byte size of the downloaded source variant")]
    pub original_size_bytes: u64,

    #[schemars(description = "Pixel width of the returned image")]
    pub returned_width: u32,

    #[schemars(description = "Pixel height of the returned image")]
    pub returned_height: u32,

    #[schemars(description = "Encoded JPEG size in bytes (before base64 expansion)")]
    pub returned_size_bytes: usize,

    #[schemars(description = "Always image/jpeg")]
    pub mime_type: String,
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
    pub channel_username: Username,
    pub text: String,
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
}

impl From<Message> for MessageResponse {
    fn from(m: Message) -> Self {
        Self {
            id: m.id,
            channel_id: m.channel_id,
            channel_name: m.channel_name,
            channel_username: m.channel_username,
            text: m.text,
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
        }
    }
}

/// Wire representation of a search/history result set.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchResponse {
    pub messages: Vec<MessageResponse>,
    pub total_found: u64,
    pub search_time_ms: u64,
    pub query_metadata: QueryMetadata,
}

impl From<SearchResult> for SearchResponse {
    fn from(r: SearchResult) -> Self {
        Self {
            messages: r.messages.into_iter().map(MessageResponse::from).collect(),
            total_found: r.total_found,
            search_time_ms: r.search_time_ms,
            query_metadata: r.query_metadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_response_serializes() {
        let response = StatusResponse {
            telegram_connected: true,
            rate_limiter_tokens: 45.5,
            server_version: "0.1.0".to_string(),
            requests_received: 1,
            responses_written: 1,
            last_response_write_age_secs: Some(0),
            session_started_at: "2026-06-12T00:00:00+00:00".to_string(),
            session_uptime_secs: 60,
            premium: Some(true),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("telegram_connected"));
        assert!(json.contains("true"));
    }

    #[test]
    fn message_link_response_serializes() {
        let response = MessageLinkResponse {
            channel_id: "123".to_string(),
            message_id: 456,
            https_link: "https://t.me/c/123/456".to_string(),
            tg_protocol_link: Some("tg://privatepost?channel=123&post=456".to_string()),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("https_link"));
        assert!(json.contains("tg_protocol_link"));
    }

    #[test]
    fn open_message_response_serializes() {
        let response = OpenMessageResponse {
            success: true,
            message: "Message opened".to_string(),
            link_used: "tg://link".to_string(),
            app_opened: true,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("app_opened"));
    }

    #[test]
    fn get_message_media_response_serializes() {
        use crate::telegram::types::MediaType;

        let response = GetMessageMediaResponse {
            channel_id: "news".to_string(),
            message_id: 42,
            media_type: MediaType::Photo,
            is_thumbnail: false,
            caption: Some("benchmark table".to_string()),
            original_width: Some(2560),
            original_height: Some(1440),
            original_size_bytes: 400_000,
            returned_width: 1280,
            returned_height: 720,
            returned_size_bytes: 150_000,
            mime_type: "image/jpeg".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"media_type\":\"photo\""));
        assert!(json.contains("\"is_thumbnail\":false"));
        assert!(json.contains("benchmark table"));
    }

    #[test]
    fn message_response_maps_and_omits_absent_fields() {
        use crate::telegram::types::{
            ChannelId, ChannelName, MediaType, Message, MessageId, Username,
        };

        let msg = Message {
            id: MessageId::new(1).unwrap(),
            channel_id: ChannelId::new(100).unwrap(),
            channel_name: ChannelName::new("Test").unwrap(),
            channel_username: Username::new("testchan").unwrap(),
            text: "hi".to_string(),
            timestamp: chrono::Utc::now(),
            sender_id: None,
            sender_name: None,
            has_media: false,
            media_type: MediaType::None,
            forwarded_from: None,
            link_preview: None,
            views: Some(10),
            forwards: None,
            reply_to_message_id: None,
        };

        let dto = MessageResponse::from(msg);
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["views"], 10);
        assert!(json.get("forwards").is_none());
        assert!(json.get("forwarded_from").is_none());
        // sender_id mirrors the domain type: present as null, not skipped.
        assert!(json.get("sender_id").is_some());
        assert!(json["sender_id"].is_null());
    }

    #[test]
    fn search_response_maps_from_search_result() {
        use crate::telegram::types::{
            ChannelId, ChannelName, MediaType, Message, MessageId, QueryMetadata, SearchResult,
            Username,
        };

        let result = SearchResult {
            messages: vec![Message {
                id: MessageId::new(1).unwrap(),
                channel_id: ChannelId::new(100).unwrap(),
                channel_name: ChannelName::new("Test").unwrap(),
                channel_username: Username::new("testchan").unwrap(),
                text: "hi".to_string(),
                timestamp: chrono::Utc::now(),
                sender_id: None,
                sender_name: None,
                has_media: false,
                media_type: MediaType::None,
                forwarded_from: None,
                link_preview: None,
                views: None,
                forwards: None,
                reply_to_message_id: None,
            }],
            total_found: 1,
            search_time_ms: 5,
            query_metadata: QueryMetadata {
                query: "x".to_string(),
                hours_back: 48,
                channels_searched: 1,
            },
        };

        let dto = SearchResponse::from(result);
        assert_eq!(dto.messages.len(), 1);
        assert_eq!(dto.total_found, 1);
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["query_metadata"]["query"], "x");
    }
}
