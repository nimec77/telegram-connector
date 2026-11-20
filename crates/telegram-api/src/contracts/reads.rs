//! Wire types for bounded account reads.

use super::ErrorCode;
use serde::{Deserialize, Serialize};

/// Search scoped to explicit permitted channel IDs.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    /// Positive decimal channel IDs. At least one is required.
    pub channel_ids: Vec<String>,
    /// Text query. Empty is accepted with a media filter.
    pub query: String,
    /// Maximum number of returned rows, capped by the service.
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Relative search window when no lower date is supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hours_back: Option<u32>,
    /// Inclusive UTC lower date and time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_date: Option<String>,
    /// Inclusive UTC upper date and time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_date: Option<String>,
    /// Optional Telegram attachment or link filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_filter: Option<SearchMediaFilter>,
    /// Exclusive older-page cursor with exactly one channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_id: Option<u32>,
    /// Exclusive lower ID bound with exactly one channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_id: Option<u32>,
    /// Combine album members into one result. Defaults to false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub collapse_albums: bool,
}

impl Default for SearchRequest {
    fn default() -> Self {
        Self {
            channel_ids: Vec::new(),
            query: String::new(),
            limit: default_limit(),
            hours_back: None,
            from_date: None,
            to_date: None,
            media_filter: None,
            before_id: None,
            after_id: None,
            collapse_albums: false,
        }
    }
}

/// Search filter applied by Telegram.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMediaFilter {
    /// Photos.
    Photo,
    /// Videos.
    Video,
    /// Photos or videos.
    PhotoVideo,
    /// Attached files.
    Document,
    /// Music files.
    Audio,
    /// Voice messages.
    Voice,
    /// Round video messages.
    VideoNote,
    /// Animations.
    Gif,
    /// Messages containing links.
    Url,
    /// Pinned messages.
    Pinned,
}

/// Newest-first history bounds.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryQuery {
    /// Exclusive lower message ID. Missing or zero leaves it open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_id: Option<u32>,
    /// Inclusive upper message ID. Missing leaves it open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through_id: Option<u32>,
    /// Exclusive upper cursor for the next older page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_id: Option<u32>,
    /// Maximum number of messages, capped by the service.
    #[serde(default = "default_limit")]
    pub limit: u32,
}

const fn default_limit() -> u32 {
    20
}

/// Batch lookup bound to one channel.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BatchRequest {
    /// Positive decimal channel ID.
    pub channel_id: String,
    /// Positive identifiers, deduplicated in request order.
    pub message_ids: Vec<u32>,
}

/// Scope reserved for a single peer identity.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadPeerNamespace {
    /// Broadcast channel or supergroup.
    Channel,
}

/// Attachment type of a returned message.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadMediaKind {
    /// Text-only message.
    None,
    /// Photo.
    Photo,
    /// Video.
    Video,
    /// File.
    Document,
    /// Music.
    Audio,
    /// Voice message.
    Voice,
    /// Round video message.
    VideoNote,
    /// Animation.
    Animation,
    /// Sticker.
    Sticker,
    /// Shared contact.
    Contact,
    /// Location.
    Location,
    /// Named venue.
    Venue,
    /// Poll or quiz.
    Poll,
    /// Dice or similar game.
    Dice,
}

/// Album siblings represented by one search row.
#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
pub struct ReadAlbumInfo {
    /// Number of siblings represented in this result.
    pub media_count: u32,
    /// Attachment types in ascending message ID order.
    pub media_types: Vec<ReadMediaKind>,
    /// Represented sibling IDs in ascending order.
    pub message_ids: Vec<u32>,
}

/// Message metadata returned by hosted reads.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Debug, Deserialize, Serialize)]
pub struct ReadMessage {
    /// Positive message ID within the source channel.
    pub id: u32,
    /// Positive decimal source channel ID.
    pub channel_id: String,
    /// Source namespace, distinct from the bare numeric ID.
    pub peer_namespace: ReadPeerNamespace,
    /// Display name of the source.
    pub channel_name: String,
    /// Public username, when available.
    pub channel_username: Option<String>,
    /// Message text or attachment caption.
    pub text: String,
    /// UTC publication time.
    pub timestamp: String,
    /// Decimal Telegram user ID, when provided by Telegram.
    pub sender_id: Option<String>,
    /// Sender name from the response envelope.
    pub sender_name: Option<String>,
    /// Whether the message has an attachment.
    pub has_media: bool,
    /// Type of the attachment.
    pub media_type: ReadMediaKind,
    /// Telegram album ID shared by siblings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouped_id: Option<i64>,
    /// Siblings represented by this row when album collapse was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<ReadAlbumInfo>,
    /// The Telegram message link.
    pub link: String,
}

/// Outcome of a scoped search.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Debug, Deserialize, Serialize)]
pub struct SearchResponse {
    /// Newest matching rows in this response.
    pub messages: Vec<ReadMessage>,
    /// Whether qualifying rows remain beyond this page.
    pub has_more: bool,
    /// Exclusive older cursor, available for a single channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_before_id: Option<u32>,
    /// Whether search coverage is incomplete.
    pub partial: bool,
    /// Whether a search deadline interrupted traversal.
    pub timed_out: bool,
    /// Safe failure reason for each attempted channel that failed.
    pub failed_channels: Vec<SearchChannelError>,
}

/// Reportable error for one permitted channel in a successful fan-out.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Debug, Deserialize, Serialize)]
pub struct SearchChannelError {
    /// Decimal channel ID.
    pub channel_id: String,
    /// Safe failure code.
    pub code: ErrorCode,
}

/// Outcome of a history traversal.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct HistoryResponse {
    /// Individual messages in descending ID order.
    pub messages: Vec<ReadMessage>,
    /// Exclusive cursor from the last included row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_before_id: Option<u32>,
    /// Whether the traversal was interrupted after collecting rows.
    pub partial: bool,
    /// Whether interruption came from the traversal's deadline.
    pub timed_out: bool,
}

/// Outcome of a bounded message batch.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Debug, Deserialize, Serialize)]
pub struct BatchResponse {
    /// Decimal source channel ID.
    pub channel_id: String,
    /// Existing messages that fit the response budget.
    pub messages: Vec<ReadMessage>,
    /// Message IDs that Telegram could not find.
    pub missing_ids: Vec<u32>,
    /// Existing message IDs omitted to fit the response budget.
    pub omitted_ids: Vec<u32>,
}
