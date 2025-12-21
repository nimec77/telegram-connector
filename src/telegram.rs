pub mod auth;
pub mod client;
pub mod types;

pub use client::TelegramClient;
pub use types::{
    Channel, ChannelId, ChannelName, MediaType, Message, MessageId, QueryMetadata, SearchParams,
    SearchResult, UserId, Username,
};
