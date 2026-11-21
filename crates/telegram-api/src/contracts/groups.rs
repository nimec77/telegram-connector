//! Wire types for replayable changes in a Telegram group.

use serde::{Deserialize, Serialize};

/// The Telegram peer namespace and a positive numeric ID.
#[cfg(feature = "service")]
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum GroupRef {
    Chat(i64),
    Supergroup(i64),
}

#[cfg(feature = "service")]
impl std::str::FromStr for GroupRef {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (kind, id) = value.split_once(':').ok_or(())?;
        let numeric: i64 = id.parse().map_err(|_| ())?;
        if numeric <= 0 || numeric.to_string() != id {
            return Err(());
        }
        match kind {
            "chat" if numeric <= 999_999_999_999 => Ok(Self::Chat(numeric)),
            "supergroup" if numeric <= 997_852_516_352 => Ok(Self::Supergroup(numeric)),
            _ => Err(()),
        }
    }
}

#[cfg(feature = "service")]
impl std::fmt::Display for GroupRef {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Chat(id) => write!(output, "chat:{id}"),
            Self::Supergroup(id) => write!(output, "supergroup:{id}"),
        }
    }
}

/// The source of an added message.
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorKind {
    /// A non-bot Telegram user.
    User,
    /// A bot user.
    Bot,
    /// An anonymous or chat identity.
    Chat,
    /// The update could not establish the author type.
    Unknown,
}

/// Author information captured with the message.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct Author {
    /// Identity category.
    pub kind: AuthorKind,
    /// Decimal user ID, when known.
    pub user_id: Option<String>,
    /// Name supplied by the update, when known.
    pub display_name: Option<String>,
    /// Telegram username, when known.
    pub username: Option<String>,
}

/// The relationship of a reply to the monitored group.
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyKind {
    /// The Telegram update identifies a parent in the same group.
    Local,
    /// The message has no reply target.
    None,
    /// The target is external or the update could not resolve it.
    Unavailable,
}

/// A discontinuity in the connector's Telegram observations.
#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapReason {
    /// Telegram coverage was interrupted.
    UpstreamDisconnect,
    /// An update could not be safely attributed.
    AmbiguousUpdate,
    /// Journal writes were interrupted.
    JournalFailure,
}

/// An observed text change or a marker of uncertain coverage.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Change {
    /// A new ordinary text message.
    Added {
        /// Telegram message ID in decimal.
        message_id: String,
        /// Publication time in UTC.
        posted_at: String,
        /// Original author.
        author: Author,
        /// Original text.
        text: String,
        /// Reply target classification.
        reply_kind: ReplyKind,
        /// Decimal parent ID for a local reply.
        reply_to_message_id: Option<String>,
    },
    /// A complete text replacement observed in Telegram.
    Edited {
        /// Telegram message ID in decimal.
        message_id: String,
        /// Telegram edit time in UTC.
        edited_at: String,
        /// Replacement text.
        text: String,
    },
    /// An attributed deletion.
    Deleted {
        /// Telegram message ID in decimal.
        message_id: String,
        /// Deletion time, or observation time if absent.
        deleted_at: String,
    },
    /// A marker that some changes may be missing.
    Gap {
        /// Cause of uncertain coverage.
        reason: GapReason,
    },
}

/// One committed and independently resumable change.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct Entry {
    /// Save this cursor after processing the entry.
    pub cursor: String,
    /// Position within this stream in decimal.
    pub seq: String,
    /// Time the connector recorded the change, in UTC.
    pub observed_at: String,
    /// The recorded change.
    pub change: Change,
}

/// A bounded, consecutive page of retained changes.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct Page {
    /// UUID for the group stream.
    pub stream_id: String,
    /// Boundary immediately preceding this page.
    pub base_cursor: String,
    /// Decimal position of the boundary.
    pub base_seq: String,
    /// Committed entries in ascending sequence order.
    pub entries: Vec<Entry>,
    /// More committed entries follow.
    pub has_more: bool,
}

/// The current earliest available boundary after a cursor expires.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct CursorExpired {
    /// UUID of the active group stream.
    pub stream_id: String,
    /// Boundary immediately preceding the oldest retained entry.
    pub base_cursor: String,
    /// Decimal position of the boundary.
    pub base_seq: String,
}
