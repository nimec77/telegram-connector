//! Response types for MCP tools.

use crate::telegram::types::Channel;
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
}
