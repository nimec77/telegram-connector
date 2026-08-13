//! Trait definition for Telegram client operations
//!
//! This trait allows mocking the Telegram client in tests.

use crate::error::Error;
use crate::telegram::types::{
    Channel, ChannelIdentity, ChannelPage, ChannelResolution, ChannelStats, HistoryParams,
    MediaDownload, MediaFetchOutcome, Message, MessageBatch, SearchParams, SearchResult,
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

    /// Get one page of subscribed channels plus the full subscription count.
    ///
    /// The dialog walk always runs to completion regardless of `limit`/`offset`,
    /// so `ChannelPage::total` is the genuine subscription count, not the page
    /// size (work-order B6a).
    async fn get_subscribed_channels(&self, limit: u32, offset: u32) -> Result<ChannelPage, Error>;

    /// Search Telegram's public directory for channels/groups by keyword.
    ///
    /// `contacts.search` returns matches from the public directory *and* from the
    /// caller's own dialogs in one result set, so each result's `is_subscribed`
    /// reflects whether it appeared among the caller's own matches. That makes
    /// `is_subscribed: true` reliable but `false` best-effort: the dialog-side
    /// matches are server-capped and prefix-matched, so an actually-subscribed
    /// channel can still come back as `false`.
    async fn search_public_channels(&self, query: &str, limit: u32) -> Result<Vec<Channel>, Error>;

    /// Check if client is connected and authorized
    async fn is_connected(&self) -> bool;

    /// Get a single message by its ID from a specific channel.
    ///
    /// The `channel_ref` can be a username (e.g. "swodki") or a numeric ID string (e.g. "1234567").
    /// Uses `raw_pager::fetch_messages_by_id` under the hood — the same RPC as
    /// grammers' `get_messages_by_id`, but with the response envelope kept so
    /// forward attribution resolves.
    async fn get_message_by_id(&self, channel_ref: &str, message_id: i32)
    -> Result<Message, Error>;

    /// Fetch up to 50 specific messages from one channel in a single RPC via
    /// `raw_pager::fetch_messages_by_id` (an id vector, envelope preserved).
    /// Deleted or never-existed ids are reported in `MessageBatch::missing_ids`
    /// instead of failing the batch (work-order A1); the caller pre-validates
    /// count and sign.
    async fn get_messages_batch(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
    ) -> Result<MessageBatch, Error>;

    /// Resolve a channel reference (username or numeric-ID string) to its
    /// canonical numeric ID and public username, if any (`None` = no public
    /// username). One peer resolution, no full-info RPC — used by link
    /// generation so public channels get `t.me/<username>` links.
    async fn resolve_channel_identity(&self, channel_ref: &str) -> Result<ChannelIdentity, Error>;

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

    /// Download the visual media of several messages from ONE channel.
    ///
    /// Resolves the peer once and issues a single `get_messages_by_id` for all
    /// ids, then downloads with bounded concurrency — so N images cost one
    /// dialog walk and one fetch RPC rather than N of each.
    ///
    /// `Err` means the whole call failed (empty reference, channel not found,
    /// fetch RPC error). Per-id failures — deleted message, no visual media,
    /// oversize — are reported in the returned `MediaFetchOutcome`s, one per
    /// requested id, in request order.
    async fn download_messages_media(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
        max_dimension: u32,
    ) -> Result<Vec<MediaFetchOutcome>, Error>;

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

    /// Batch-resolve identifiers (numeric id, @username, or exact chat title)
    /// to channel entities in one dialog walk plus at most one
    /// `resolve_username` RPC per unmatched username-shaped identifier.
    /// Per-identifier failures are entries, not errors (work-order A7).
    async fn resolve_channels(
        &self,
        identifiers: &[String],
    ) -> Result<Vec<ChannelResolution>, Error>;

    /// One bounded history sweep computing album-collapsed posting stats:
    /// up to `days_back` days (caller-clamped), at most
    /// `ChannelStats::MAX_MESSAGES_SCANNED` raw records (work-order A5).
    /// `sample.complete` is false when the cap cut the sweep short.
    async fn get_channel_stats(
        &self,
        channel_ref: &str,
        days_back: u32,
    ) -> Result<ChannelStats, Error>;
}
