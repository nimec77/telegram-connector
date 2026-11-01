//! Telegram client facade and domain data used by the MCP server.
//!
//! Prefer [`TelegramClient`], [`TelegramClientTrait`] and [`types`] for new Rust
//! integrations. Nested client/trait/type paths, converters and timeout modules
//! remain for compatibility. Real client construction opens a session and starts
//! its sender task; domain construction alone is inert. Client methods do not
//! acquire the MCP account limiter; callers own charging and local validation.

pub(crate) mod albums;
pub mod auth;
pub mod client;
pub mod converters;
pub(crate) mod envelope;
pub mod media_pipeline;
pub(crate) mod session_storage;
pub mod timeout;
pub mod trait_def;
pub mod transcription;
pub mod types;

// Re-export main types for convenience
pub use client::TelegramClient;
pub use trait_def::TelegramClientTrait;
pub use types::{
    AccountStatus, Channel, ChannelId, ChannelIdentity, ChannelName, ChannelPage,
    ChannelResolution, ChannelStats, ChatType, HistoryParams, MediaFetchError, MediaFetchOutcome,
    MediaFilter, MediaType, Message, MessageBatch, MessageId, QueryMetadata, SearchParams,
    SearchResult, StatsSample, TranscriptionOutcome, TranscriptionState, UserId, Username,
};

// Re-export mock for tests
#[cfg(test)]
pub use trait_def::MockTelegramClientTrait;

// Test module (conditionally compiled)
#[cfg(test)]
mod tests;
