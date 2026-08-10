//! Trait definition for Telegram client operations
//!
//! This trait allows mocking the Telegram client in tests.

use crate::error::Error;
use crate::telegram::types::{
    Channel, HistoryParams, MediaDownload, Message, SearchParams, SearchResult,
    TranscriptionOutcome,
};

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

    /// Like [`Self::get_channel_info`], but additionally fetches
    /// `channels.GetFullChannel` to fill `description` and `member_count`.
    /// Falls back to basic info for non-channel peers (small groups,
    /// communities), whose full-info RPC differs.
    async fn get_full_channel_info(&self, identifier: &str) -> Result<Channel, Error>;

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

    /// Download the visual media of a message: the photo itself, or the
    /// server-side thumbnail for video-like media (video, animation, video note).
    ///
    /// `max_dimension` is a size-selection hint: the smallest server-side size
    /// whose longest side is at least `max_dimension` is downloaded (the largest
    /// available if none qualifies). Exact downscaling happens in the MCP layer.
    async fn download_message_media(
        &self,
        channel_ref: &str,
        message_id: i32,
        max_dimension: u32,
    ) -> Result<MediaDownload, Error>;

    /// Transcribe a voice / video-note message's audio via `messages.transcribeAudio`.
    ///
    /// Resolves the peer once, validates the media type (rejecting non-voice /
    /// non-video_note with `Error::NotTranscribable`), invokes `TranscribeAudio`,
    /// then polls by re-invoking until the transcription completes or
    /// `timeout_secs` elapses (returning a partial result on timeout).
    async fn transcribe_audio(
        &self,
        channel_ref: &str,
        message_id: i32,
        timeout_secs: u32,
    ) -> Result<TranscriptionOutcome, Error>;

    /// Cached Telegram Premium flag for the connected account. Returns the cached
    /// value; if unknown, performs one `get_me()` and caches it. Returns `None`
    /// only when Premium status could not be determined.
    async fn is_premium(&self) -> Option<bool>;
}
