//! MCP tool request and response compatibility types with JSON schemas.
//!
//! These are wire-facing DTOs, not a replacement for strict domain constructors.
//! Existing nested request/response/deserializer paths and facade exports remain
//! supported compatibility paths. Flexible scalar coercion occurs here; failures
//! use serde/string boundary errors. Public field literals remain allowed, so
//! adding fields or changing Rust field types is distinct from wire compatibility.
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
    GetMessagesBatchRequest, GetMessagesMediaBatchRequest, GetRecentMessagesRequest,
    OpenMessageRequest, ResolveChannelsRequest, ResponseFormat, SearchPublicChannelsRequest,
    SearchRequest, TranscribeVoiceMessageRequest,
};
pub use responses::{
    BufferedResponseEntry, ChannelHeader, ChannelsResponse, GetMessageMediaResponse,
    LastResponsesResponse, MediaBatchFailure, MediaBatchSummary, MediaLimits, MessageLinkResponse,
    MessageResponse, MessagesBatchResponse, MissingMessageEntry, NextCursor, OpenMessageResponse,
    RateLimiterCosts, RateLimiterStatus, ResolveChannelsResponse, SearchResponse, StatusResponse,
    TranscribeVoiceMessageResponse,
};
