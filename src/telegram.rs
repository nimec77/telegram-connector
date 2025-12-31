pub mod auth;
pub mod client;
pub mod converters;
pub mod trait_def;
pub mod types;

// Re-export main types for convenience
pub use client::TelegramClient;
pub use trait_def::TelegramClientTrait;
pub use types::{
    Channel, ChannelId, ChannelName, MediaFilter, MediaType, Message, MessageId, QueryMetadata,
    SearchParams, SearchResult, UserId, Username,
};

// Re-export mock for tests
#[cfg(test)]
pub use trait_def::MockTelegramClientTrait;

// Test module (conditionally compiled)
#[cfg(test)]
mod tests;
