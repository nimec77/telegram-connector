//! Live Telegram broadcast subscriptions with per-channel starting boundaries.

use crate::UtcTimestamp;
use crate::error::Error;
use crate::telegram::types::{ChannelId, MediaType, MessageId};
use grammers_client::tl;
use serde::{Serialize, Serializer};
use std::time::Duration;

mod queue;
mod reader;
mod subscription;

pub use queue::{QueuedEvent, StreamBudget};
pub use subscription::{AccountStreams, ChannelSubscription};

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod test_support;
#[cfg(test)]
mod tests;

/// Resource limits for subscriptions sharing one account reader.
#[derive(Debug, Clone, Copy)]
pub struct StreamLimits {
    /// Maximum distinct selected channels per subscription.
    pub max_channels: usize,
    /// Maximum pending and active subscriptions for one account.
    pub max_subscriptions: usize,
    /// Maximum queued and in-flight events per subscription.
    pub max_queue_items: usize,
    /// Maximum queued and in-flight encoded bytes per subscription.
    pub max_queue_bytes: usize,
    /// Maximum encoded bytes per event, including SSE framing.
    pub max_event_bytes: usize,
    /// Maximum total setup time, including admission and all channel heads.
    pub setup_timeout: Duration,
}

impl Default for StreamLimits {
    fn default() -> Self {
        Self {
            max_channels: 20,
            max_subscriptions: 4,
            max_queue_items: 128,
            max_queue_bytes: 1024 * 1024,
            max_event_bytes: 64 * 1024,
            setup_timeout: Duration::from_secs(30),
        }
    }
}

/// Failure before a live subscription is accepted.
#[derive(Debug, thiserror::Error)]
pub enum SubscribeError {
    /// No channels were requested or the distinct-channel limit was exceeded.
    #[error("Invalid subscription channel selection")]
    InvalidChannels,
    /// The account has reached its subscription count limit.
    #[error("Subscription capacity reached")]
    Capacity,
    /// At least one requested channel lacks current joined broadcast access.
    #[error("Channel unavailable")]
    Ineligible,
    /// Pending events exceeded a queue or event limit before setup finished.
    #[error("Subscription setup queue overflowed")]
    SetupOverflow,
    /// The account's update reader cannot accept this subscription.
    #[error("Channel update reader unavailable")]
    ReaderUnavailable,
    /// The caller cancelled, timed out, or encountered an account operation error.
    #[error(transparent)]
    Operation(#[from] Error),
}

/// The latest visible message captured for one joined channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ChannelHead {
    /// Scoped Telegram channel, serialized as a decimal string.
    #[serde(serialize_with = "serialize_channel_id")]
    pub channel_id: ChannelId,
    /// Latest visible message, or null for a confirmed empty channel.
    pub head_message_id: Option<MessageId>,
}

/// A new ordinary message from a subscribed broadcast channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChannelPost {
    /// Scoped Telegram channel, serialized as a decimal string.
    #[serde(serialize_with = "serialize_channel_id")]
    pub channel_id: ChannelId,
    /// Positive ID within the channel.
    pub message_id: MessageId,
    /// Telegram publication timestamp in UTC.
    pub published_at: UtcTimestamp,
    /// Text or caption. A captionless media post has empty text.
    pub text: String,
    /// Bounded metadata for attached media.
    pub media: Option<ChannelPostMedia>,
    /// Shared album ID, when present, serialized as a decimal string.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_album_id"
    )]
    pub album_id: Option<i64>,
}

/// Scalar metadata already present in an update. Fetch media bytes separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChannelPostMedia {
    /// Media classification shared with fetched messages.
    pub kind: MediaType,
    /// Original attachment size where available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size_bytes: Option<u64>,
    /// Playback duration when supplied for an audio or video attachment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u32>,
    /// Video width in pixels when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// Video height in pixels when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

/// The safe cause of a subscription's loss of continuity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptionReason {
    /// A subscriber exceeded its queue or encoded event allowance.
    SlowConsumer,
    /// The native reader stopped or observed an unresolved discontinuity.
    UpstreamDisconnected,
    /// Telegram reported that the account lost membership or broadcast access.
    MembershipLost,
    /// The account or service closed the reader.
    ServiceDraining,
}

/// An event in an accepted live subscription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelEvent {
    /// The first event, in the order of the distinct requested channels.
    Ready(Vec<ChannelHead>),
    /// An individual post above its channel's starting boundary.
    Post(ChannelPost),
    /// Best-effort terminal notification. EOF also ends continuity.
    Interrupted(InterruptionReason),
}

/// Account session seam for current membership and authoritative head reads.
#[doc(hidden)]
#[async_trait::async_trait]
pub trait ChannelHeadLookup: Send + Sync {
    /// Verify every requested channel and return one head per channel in input order.
    ///
    /// # Errors
    /// Returns channel-unavailable when the account lacks joined broadcast access,
    /// or an operation error when the lookup cannot establish the channel's state.
    async fn channel_heads(&self, channels: &[ChannelId]) -> Result<Vec<ChannelHead>, Error>;
}

fn serialize_channel_id<S: Serializer>(id: &ChannelId, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(id)
}

fn serialize_album_id<S: Serializer>(id: &Option<i64>, serializer: S) -> Result<S::Ok, S::Error> {
    match id {
        Some(id) => serializer.collect_str(id),
        None => serializer.serialize_none(),
    }
}

pub(crate) fn broadcast_membership(chat: &tl::enums::Chat) -> Option<(ChannelId, bool)> {
    let (id, joined) = match chat {
        tl::enums::Chat::Channel(channel) if !channel.min => (
            channel.id,
            channel.broadcast
                && !channel.left
                && !channel.megagroup
                && !channel.gigagroup
                && !channel.monoforum,
        ),
        tl::enums::Chat::ChannelForbidden(channel) => (channel.id, false),
        _ => return None,
    };
    Some((ChannelId::new(id).ok()?, joined))
}
