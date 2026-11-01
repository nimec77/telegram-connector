//! Trait definition for Telegram client operations
//!
//! Compatibility module for the substitutable client seam; prefer
//! [`crate::TelegramClientTrait`] or [`crate::telegram::TelegramClientTrait`].

use crate::error::Error;
use crate::telegram::transcription::TranscriptionDeadline;
use crate::telegram::types::{
    AccountStatus, Channel, ChannelIdentity, ChannelPage, ChannelResolution, ChannelStats,
    HistoryParams, MediaDownload, Message, MessageBatch, SearchParams, SearchResult,
    TranscriptionOutcome,
};

/// Injectable Telegram operations; deliberately unsealed for external clients.
///
/// The contracts below describe the production [`crate::TelegramClient`]. Custom
/// implementations own their I/O/deadline policy; the server does not install
/// these network timeouts around arbitrary trait implementations. All operations
/// may perform RPCs except [`Self::cached_account_status`]. Methods never acquire
/// the MCP account limiter: tool callers validate locally, acquire, then call.
/// Direct Rust consumers must supply that policy themselves.
///
/// Resolution/dialog walks use `TimeoutConfig::resolve_secs`; subscribed-channel
/// enumeration and history/message fetches use `history_secs`, directory/message
/// search uses `search_secs`, and media streaming uses `download_secs`. These are
/// stage budgets, not one combined call deadline. Transcription alone accepts an explicit shared absolute budget.
/// Common failures are [`Error::InvalidInput`] (bad/missing references),
/// [`Error::TelegramApi`] (upstream failure), and [`Error::Timeout`] (dropped stage).
/// There are no automatic retries at this seam.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait TelegramClientTrait: Send + Sync {
    /// Search for messages matching the given parameters
    ///
    /// Accumulates under the cooperative search deadline: expiry returns a
    /// successful partial result with `query_metadata.timed_out/partial`. An
    /// outer search timeout or RPC failure still returns an error and discards
    /// that call's accumulation. Cursors require a single numeric channel and
    /// cover `after_id < id < before_id`; message IDs are channel-scoped.
    /// `has_more` means a further matching message was proven, not deadline expiry.
    ///
    /// # Errors
    /// Rejects empty query without a media filter, zero limit, invalid wire
    /// cursors and global cursors; also returns upstream/search timeout failures.
    async fn search_messages(&self, params: &SearchParams) -> Result<SearchResult, Error>;

    /// Get recent messages from a channel by time window (no search query needed)
    ///
    /// Uses `iter_messages()` to iterate message history, not search.
    /// Media filter is applied client-side.
    ///
    /// Username resolution is best effort: a miss or failure falls back to a
    /// dialog walk when a numeric channel ID is available. Without a usable
    /// fallback, the call returns an invalid-reference error. Resolution stages
    /// are separately bounded before the history walk. History has
    /// no cooperative search deadline: history timeout/RPC failure is an error,
    /// not a successful partial. Cursor bounds are exclusive and channel-scoped.
    ///
    /// # Errors
    /// Rejects zero limit, unusable reference or out-of-range cursors, and
    /// propagates final dialog/history timeouts and upstream failures.
    async fn get_recent_messages(&self, params: &HistoryParams) -> Result<SearchResult, Error>;

    /// Get information about a specific channel by username or ID
    ///
    /// # Errors
    /// Returns invalid-reference/not-found, resolve/dialog timeout or upstream
    /// errors. No join, subscription or membership mutation is performed.
    async fn get_channel_info(&self, identifier: &str) -> Result<Channel, Error>;

    /// Like [`Self::get_channel_info`], but additionally fetches
    /// `channels.GetFullChannel` to fill `description` and `member_count`.
    /// Falls back to basic info for non-channel peers (small groups,
    /// communities), whose full-info RPC differs.
    ///
    /// # Errors
    /// Basic lookup failures propagate. Optional full-info and newest-message
    /// enrichment failures/timeouts retain basic info instead of failing the call.
    /// Full-info uses a resolve budget, newest-message peek a history budget.
    async fn get_full_channel_info(&self, identifier: &str) -> Result<Channel, Error>;

    /// Get one page of subscribed channels plus the full subscription count.
    ///
    /// The dialog walk always runs to completion regardless of `limit`/`offset`,
    /// so `ChannelPage::total` is the genuine subscription count, not the page
    /// size (work-order B6a).
    ///
    /// The history budget bounds the full dialog walk, not each page. This
    /// reads current dialogs without subscribing to or joining any chat.
    ///
    /// # Errors
    /// Dialog timeout/API failure discards the page rather than returning an
    /// incomplete `total`. Last-message dates come from the same dialog envelope,
    /// without extra per-channel RPCs.
    async fn get_subscribed_channels(&self, limit: u32, offset: u32) -> Result<ChannelPage, Error>;

    /// Search Telegram's public directory for channels/groups by keyword.
    ///
    /// `contacts.search` returns matches from the public directory *and* from the
    /// caller's own dialogs in one result set, so each result's `is_subscribed`
    /// reflects whether it appeared among the caller's own matches. That makes
    /// `is_subscribed: true` reliable but `false` best-effort: the dialog-side
    /// matches are server-capped and prefix-matched, so an actually-subscribed
    /// channel can still come back as `false`.
    ///
    /// # Errors
    /// An empty query is invalid. Directory search uses the search budget and
    /// returns timeout/API failures; it does not subscribe to returned chats.
    async fn search_public_channels(&self, query: &str, limit: u32) -> Result<Vec<Channel>, Error>;

    /// Read local account observations without RPCs or rate-limit charges.
    async fn cached_account_status(&self) -> AccountStatus;

    /// Refresh authorization and best-effort Premium status with bounded RPCs.
    /// Authorization transport failures are errors and preserve cached observations.
    /// Call only at startup or after the calling tool acquires its operation charge.
    ///
    /// Authorization and Premium RPCs each use a resolve budget. Premium
    /// lookup failure preserves the last successful Premium observation and
    /// still returns account status; authorization failure is an error.
    ///
    /// # Errors
    /// Authorization RPC/timeouts propagate without inventing a new observation.
    async fn refresh_account_status(&self) -> Result<AccountStatus, Error>;

    /// Get a single message by its ID from a specific channel.
    ///
    /// The `channel_ref` can be a username (e.g. "swodki") or a numeric ID string (e.g. "1234567").
    /// Issues the same RPC as grammers' `get_messages_by_id`, but keeps the
    /// response envelope so forward attribution resolves.
    ///
    /// # Errors
    /// Invalid/missing reference or deleted/absent/unrepresentable message is an
    /// error, never a fabricated empty message. Resolution and history fetches
    /// have separate budgets; API and timeout failures propagate.
    async fn get_message_by_id(&self, channel_ref: &str, message_id: i32)
    -> Result<Message, Error>;

    /// Fetch up to 50 specific messages from one channel in a single RPC
    /// (an id vector, response envelope preserved).
    /// Deleted or never-existed ids are reported in `MessageBatch::missing_ids`
    /// instead of failing the batch (work-order A1); the caller pre-validates
    /// count and sign.
    ///
    /// # Errors
    /// Reference/resolution or whole fetch RPC/history timeout failures return
    /// `Err`. Missing/unrepresentable individual slots stay in `missing_ids`.
    /// Callers own nonempty, size and positive wire-ID validation.
    async fn get_messages_batch(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
    ) -> Result<MessageBatch, Error>;

    /// Resolve a channel reference (username or numeric-ID string) to its
    /// canonical numeric ID and public username, if any (`None` = no public
    /// username). One peer resolution, no full-info RPC — used by link
    /// generation so public channels get `t.me/<username>` links.
    ///
    /// # Errors
    /// Invalid/missing reference, resolve timeout or upstream failure. The
    /// returned optional username has no display sentinel substitution.
    async fn resolve_channel_identity(&self, channel_ref: &str) -> Result<ChannelIdentity, Error>;

    /// Download the visual media of a message: the photo itself, or the
    /// server-side thumbnail for video-like media (video, animation, video note).
    ///
    /// `max_dimension` is a size-selection hint: the smallest server-side size
    /// whose longest side is at least `max_dimension` is downloaded (the largest
    /// available if none qualifies). Exact downscaling happens in the MCP layer.
    ///
    /// # Errors
    /// Resolution/history/download timeout and API failures propagate. Missing
    /// messages and unsupported media fail; [`Error::MediaTooLarge`] rejects
    /// both reported and actually streamed bytes above the configured cap.
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
    ///
    /// Collected compatibility API: the caller owns retained raw batch memory.
    /// Production uses one download deadline; use `download_messages_media_pipeline`
    /// for incremental processing with global raw-buffer admission. The caller validates IDs.
    ///
    /// # Errors
    /// Whole resolution/fetch failure returns `Err`. Individual download,
    /// timeout, missing-media and oversize outcomes remain per-ID entries.
    async fn download_messages_media(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
        max_dimension: u32,
    ) -> Result<crate::telegram::types::MediaBatch, Error>;

    /// Stream a batch with production-wide raw-buffer backpressure and one media deadline.
    /// The default adapter retains compatibility with custom collected implementations;
    /// those implementations retain responsibility for their network and memory policy.
    ///
    /// # Errors
    /// Shared setup failures fail the call; subsequent item failures remain in the stream.
    async fn download_messages_media_pipeline(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
        max_dimension: u32,
    ) -> Result<crate::telegram::types::MediaBatch, Error> {
        self.download_messages_media(channel_ref, message_ids, max_dimension)
            .await
    }

    /// Transcribe a voice / video-note message's audio via `messages.transcribeAudio`.
    ///
    /// Resolves the peer once, validates the media type (rejecting non-voice /
    /// non-video_note with `Error::NotTranscribable`), invokes `TranscribeAudio`,
    /// then polls until completion or the shared absolute deadline. Resolution,
    /// message validation and every RPC consume the same budget. Expiry returns
    /// the latest partial once a state exists, otherwise a typed timeout.
    ///
    /// The RPC can consume Telegram's transcription quota and requires Premium.
    /// A transient poll failure after the initial state also returns the latest
    /// partial text; `partial` does not prove that Telegram failed permanently.
    ///
    /// # Errors
    /// Returns validation/resolution failures, Premium/quota/voice-length errors,
    /// initial RPC failures, or a typed timeout before any state is available.
    /// A zero deadline expires before starting network work.
    async fn transcribe_audio(
        &self,
        channel_ref: &str,
        message_id: i32,
        deadline: TranscriptionDeadline,
    ) -> Result<TranscriptionOutcome, Error>;

    /// Batch-resolve identifiers (numeric id, @username, or exact chat title)
    /// to channel entities in one dialog walk plus at most one
    /// `resolve_username` RPC per unmatched username-shaped identifier.
    /// Per-identifier failures are entries, not errors (work-order A7).
    ///
    /// The full dialog walk has one resolve budget; each unmatched username
    /// lookup gets its own resolve budget. Output preserves identifier order.
    ///
    /// # Errors
    /// Failure/timeout of the shared dialog walk fails the call. Missing or
    /// ambiguous identifiers and individual username RPC failures stay per-entry.
    async fn resolve_channels(
        &self,
        identifiers: &[String],
    ) -> Result<Vec<ChannelResolution<Error>>, Error>;

    /// One bounded history sweep computing album-collapsed posting stats:
    /// up to `days_back` days (caller-clamped), at most
    /// `ChannelStats::MAX_MESSAGES_SCANNED` raw records (work-order A5).
    /// `sample.complete` is false when the cap cut the sweep short.
    ///
    /// # Errors
    /// Empty reference/zero days are invalid; resolution and bounded history
    /// sweep failures propagate. The scan cap is a successful incomplete sample,
    /// while a history timeout is an error. Callers clamp the day range.
    async fn get_channel_stats(
        &self,
        channel_ref: &str,
        days_back: u32,
    ) -> Result<ChannelStats, Error>;
}
