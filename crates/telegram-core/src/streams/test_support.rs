//! Offline injection at the account head and native observation boundaries.

use super::subscription::Shared;
use super::{
    AccountStreams, ChannelHeadLookup, ChannelPost, InterruptionReason, StreamBudget, StreamLimits,
};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::types::ChannelId;
use std::sync::Arc;

/// Fixture handle that drives the actual pending and active subscription queues.
pub struct StreamInput(Arc<Shared>);

impl StreamInput {
    /// Observe one ordered post.
    pub fn publish(&self, post: ChannelPost) {
        self.0.publish(post);
    }

    /// Observe lost membership affecting only subscriptions to this channel.
    pub fn membership_lost(&self, channel: ChannelId) {
        self.0
            .interrupt_channel(channel, InterruptionReason::MembershipLost);
    }

    /// End this reader generation.
    pub fn interrupt(&self, reason: InterruptionReason) {
        self.0.close(reason);
    }
}

/// Use a deterministic head source and controllable update input without Telegram I/O.
pub fn account_streams(
    heads: Arc<dyn ChannelHeadLookup>,
    limiter: Arc<dyn RateLimiterTrait>,
    limits: StreamLimits,
    budget: Arc<StreamBudget>,
) -> (AccountStreams, StreamInput) {
    let streams = AccountStreams::with_head_lookup(heads, limiter, limits, budget);
    let shared = streams.shared();
    shared.mark_started();
    (streams, StreamInput(shared))
}
