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
    GenerateLinkRequest, GetChannelInfoRequest, GetChannelsRequest, GetLastResponsesRequest,
    GetMessageByLinkRequest, GetMessageMediaRequest, GetRecentMessagesRequest, OpenMessageRequest,
    SearchPublicChannelsRequest, SearchRequest, TranscribeVoiceMessageRequest,
};
pub use responses::{
    BufferedResponseEntry, ChannelsResponse, GetMessageMediaResponse, LastResponsesResponse,
    MessageLinkResponse, MessageResponse, OpenMessageResponse, SearchResponse, StatusResponse,
    TranscribeVoiceMessageResponse,
};
pub use serde_helpers::deserialize_optional_media_filter;
