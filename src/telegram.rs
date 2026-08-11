pub(crate) mod albums;
pub mod auth;
pub mod client;
pub mod converters;
pub mod timeout;
pub mod trait_def;
pub mod transcription;
pub mod types;

// Re-export main types for convenience
pub use client::TelegramClient;
pub use trait_def::TelegramClientTrait;
pub use types::{
    Channel, ChannelId, ChannelIdentity, ChannelName, ChannelPage, ChatType, HistoryParams,
    MediaFilter, MediaType, Message, MessageId, QueryMetadata, SearchParams, SearchResult,
    TranscriptionOutcome, TranscriptionState, UserId, Username,
};

// Re-export mock for tests
#[cfg(test)]
pub use trait_def::MockTelegramClientTrait;

// Test module (conditionally compiled)
#[cfg(test)]
mod tests;
