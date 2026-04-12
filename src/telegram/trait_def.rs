//! Trait definition for Telegram client operations
//!
//! This trait allows mocking the Telegram client in tests.

use crate::error::Error;
use crate::telegram::types::{Channel, HistoryParams, Message, SearchParams, SearchResult};

/// Trait for Telegram client operations (allows mocking in tests)
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait TelegramClientTrait: Send + Sync {
    /// Search for messages matching the given parameters
    async fn search_messages(&self, params: &SearchParams) -> Result<SearchResult, Error>;

    /// Get recent messages from a channel by time window (no search query needed)
    ///
    /// Uses `iter_messages()` to iterate message history, not search.
    /// Media filter is applied client-side.
    async fn get_recent_messages(&self, params: &HistoryParams) -> Result<SearchResult, Error>;

    /// Get information about a specific channel by username or ID
    async fn get_channel_info(&self, identifier: &str) -> Result<Channel, Error>;

    /// Get list of subscribed channels with pagination
    async fn get_subscribed_channels(&self, limit: u32, offset: u32)
    -> Result<Vec<Channel>, Error>;

    /// Check if client is connected and authorized
    async fn is_connected(&self) -> bool;

    /// Get a single message by its ID from a specific channel.
    ///
    /// The `channel_ref` can be a username (e.g. "swodki") or a numeric ID string (e.g. "1234567").
    /// Uses grammers' `get_messages_by_id` under the hood.
    async fn get_message_by_id(&self, channel_ref: &str, message_id: i32)
    -> Result<Message, Error>;
}
