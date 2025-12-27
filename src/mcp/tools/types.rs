//! MCP tool request and response types with JSON schemas

use crate::telegram::types::Channel;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchRequest {
    #[schemars(description = "Search query (required, minimum length: 1)")]
    pub query: String,

    #[schemars(description = "Optional: Filter by specific channel ID")]
    pub channel_id: Option<String>,

    #[schemars(description = "How many hours back to search (default: 48, max: 168)")]
    pub hours_back: Option<u32>,

    #[schemars(description = "Maximum results to return (default: 20, max: 100)")]
    pub limit: Option<u32>,
}

// Response: SearchResult (from telegram/types.rs) which contains Vec<Message>

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
    }
}
