//! Response types for MCP tools.

use crate::telegram::types::{Channel, MediaType};
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

    #[schemars(description = "Serialized payload size in bytes")]
    pub size_bytes: usize,

    #[schemars(description = "The JSON-RPC envelope exactly as written to stdout")]
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
}
