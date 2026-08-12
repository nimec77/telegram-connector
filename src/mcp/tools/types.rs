//! MCP tool request and response types with JSON schemas.
//!
//! ## Module Organization
//! - `requests` - Request types for each MCP tool
//! - `responses` - Response types for each MCP tool
//! - `serde_helpers` - Custom deserializers for handling edge cases

pub mod requests;
pub mod responses;
pub mod serde_helpers;

// Re-export all types for convenience
pub use requests::{
    GenerateLinkRequest, GetChannelInfoRequest, GetChannelStatsRequest, GetChannelsRequest,
    GetLastResponsesRequest, GetMessageByLinkRequest, GetMessageMediaRequest,
    GetMessagesBatchRequest, GetRecentMessagesRequest, OpenMessageRequest, ResolveChannelsRequest,
    ResponseFormat, SearchPublicChannelsRequest, SearchRequest, TranscribeVoiceMessageRequest,
};
pub use responses::{
    BufferedResponseEntry, ChannelHeader, ChannelsResponse, GetMessageMediaResponse,
    LastResponsesResponse, MessageLinkResponse, MessageResponse, MessagesBatchResponse,
    MissingMessageEntry, NextCursor, OpenMessageResponse, RateLimiterCosts, RateLimiterStatus,
    ResolveChannelsResponse, SearchResponse, StatusResponse, TranscribeVoiceMessageResponse,
};
pub use serde_helpers::{deserialize_optional_media_filter, deserialize_optional_response_format};
