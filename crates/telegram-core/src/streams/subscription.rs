//! Reader-scoped admission, the pending-to-live transition, and receiver ownership.

use super::queue::{EncodedEvent, QueuedEvent, StreamBudget, Usage};
use super::{
    ChannelEvent, ChannelHead, ChannelHeadLookup, ChannelPost, InterruptionReason, StreamLimits,
    SubscribeError, reader,
};
use crate::TelegramClient;
use crate::error::Error;
use crate::operations::{BoundedOperation, CancellationToken, OperationContext};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::types::{ChannelId, MessageId};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use tokio::task::{AbortHandle, JoinHandle};

/// One account's native update reader and its pending and active subscriptions.
pub struct AccountStreams {
    shared: Arc<Shared>,
    heads: Arc<dyn ChannelHeadLookup>,
    limiter: Arc<dyn RateLimiterTrait>,
    reader: Option<ReaderTask>,
}

struct ReaderTask {
    abort: AbortHandle,
    join: tokio::sync::Mutex<JoinHandle<()>>,
}

impl AccountStreams {
    /// Start the one native reader for an authenticated stream-enabled Telegram client.
    ///
    /// Use the same limiter as the account's other operations and share the byte
    /// budget across account readers. Reader failure ends this instance. A fresh
    /// reader requires a fresh client and sender after the previous sender closes.
    ///
    /// # Errors
    /// Returns an error for invalid limits, an inactive Tokio runtime, or a
    /// client whose native update receiver is absent or has already been taken.
    pub fn new(
        client: Arc<TelegramClient>,
        limiter: Arc<dyn RateLimiterTrait>,
        limits: StreamLimits,
        budget: Arc<StreamBudget>,
    ) -> Result<Self, SubscribeError> {
        if limits.max_channels == 0
            || limits.max_subscriptions == 0
            || limits.max_queue_items == 0
            || limits.max_queue_bytes == 0
            || limits.max_event_bytes == 0
            || limits.setup_timeout.is_zero()
        {
            return Err(Error::InvalidInput("Stream limits must be positive".into()).into());
        }
        let runtime =
            tokio::runtime::Handle::try_current().map_err(|_| SubscribeError::ReaderUnavailable)?;
        let updates = client.take_stream_updates()?;
        let reader_client = Arc::clone(&client);
        let maintenance_timeout = client.stream_maintenance_timeout();
        let mut streams = Self::with_head_lookup(client, limiter, limits, budget);
        let shared = Arc::clone(&streams.shared);
        let lifetime = ReaderLifetime(Arc::clone(&shared));
        let task = runtime.spawn(async move {
            let _lifetime = lifetime;
            reader::run(reader_client, updates, shared, maintenance_timeout).await;
        });
        streams.reader = Some(ReaderTask {
            abort: task.abort_handle(),
            join: tokio::sync::Mutex::new(task),
        });
        Ok(streams)
    }

    pub(super) fn with_head_lookup(
        heads: Arc<dyn ChannelHeadLookup>,
        limiter: Arc<dyn RateLimiterTrait>,
        limits: StreamLimits,
        budget: Arc<StreamBudget>,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                limits,
                budget,
                started: CancellationToken::new(),
                stopped: CancellationToken::new(),
                maintenance_requests: AtomicU64::new(0),
            }),
            heads,
            limiter,
            reader: None,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn shared(&self) -> Arc<Shared> {
        Arc::clone(&self.shared)
    }

    /// State and difference RPCs initiated by this reader outside the user's setup charges.
    pub fn maintenance_requests(&self) -> u64 {
        self.shared.maintenance_requests.load(Ordering::Relaxed)
    }

    /// Whether new listeners need a replacement account reader.
    pub fn is_closed(&self) -> bool {
        self.shared.stopped.is_cancelled()
    }

    /// Notify a supervisor when this reader can no longer accept subscriptions.
    pub fn reader_closed(&self) -> CancellationToken {
        self.shared.stopped.child_token()
    }

    /// Close existing subscriptions, stop native update work and join the reader.
    ///
    /// The Telegram sender is owned by the client separately.
    pub async fn close(&self) {
        self.abort();
        if let Some(reader) = &self.reader {
            let mut handle = reader.join.lock().await;
            if !handle.is_finished() {
                let _ = (&mut *handle).await;
            }
        }
    }

    /// End subscriptions and abort native update work without waiting for leases to drop.
    pub fn abort(&self) {
        self.shared.close(InterruptionReason::ServiceDraining);
        if let Some(reader) = &self.reader {
            reader.abort.abort();
        }
    }

    /// Confirm membership and capture each channel's live boundary under one deadline.
    ///
    /// The returned receiver has a ready event queued. The caller's setup
    /// context ends at acceptance. Dropping the receiver unregisters it.
    ///
    /// # Errors
    /// Returns errors for invalid selection, capacity, loss of current
    /// membership, setup overflow, reader failure, cancellation or timeout.
    pub async fn subscribe_channel_posts(
        &self,
        channels: &[ChannelId],
        context: &OperationContext,
    ) -> Result<ChannelSubscription, SubscribeError> {
        let timing = context.bound(self.shared.limits.setup_timeout, "subscribe_channel_posts");
        let channels = distinct_channels(channels, self.shared.limits.max_channels)?;
        let charge = u32::try_from(channels.len()).map_err(|_| SubscribeError::InvalidChannels)?;
        timing.check()?;

        let subscription = self.shared.register(&channels)?;
        let heads = tokio::select! {
            biased;
            _ = subscription.slot.ended.cancelled() => return Err(subscription.setup_error()),
            heads = timing.run(async {
                self.shared.started.cancelled().await;
                self.limiter.acquire(charge).await?;
                self.heads.channel_heads(&channels).await
            }) => heads.map_err(head_lookup_error)?,
        };
        self.shared
            .activate(&subscription, heads, &channels, &timing)?;
        Ok(subscription)
    }
}

impl Drop for AccountStreams {
    fn drop(&mut self) {
        self.abort();
    }
}

fn distinct_channels(
    channels: &[ChannelId],
    limit: usize,
) -> Result<Vec<ChannelId>, SubscribeError> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for &channel in channels {
        if seen.insert(channel) {
            if ordered.len() == limit {
                return Err(SubscribeError::InvalidChannels);
            }
            ordered.push(channel);
        }
    }
    if ordered.is_empty() {
        return Err(SubscribeError::InvalidChannels);
    }
    Ok(ordered)
}

fn head_lookup_error(error: Error) -> SubscribeError {
    match error {
        Error::ChannelNotFound(_) | Error::UnsupportedPeer => SubscribeError::Ineligible,
        error => SubscribeError::Operation(error),
    }
}

struct ReaderLifetime(Arc<Shared>);

impl Drop for ReaderLifetime {
    fn drop(&mut self) {
        self.0.close(InterruptionReason::UpstreamDisconnected);
    }
}

pub(super) struct Shared {
    state: Mutex<State>,
    limits: StreamLimits,
    budget: Arc<StreamBudget>,
    started: CancellationToken,
    stopped: CancellationToken,
    maintenance_requests: AtomicU64,
}

#[derive(Default)]
struct State {
    listeners: HashMap<u64, Listener>,
    last_id: u64,
}

struct Listener {
    positions: HashMap<ChannelId, Option<MessageId>>,
    pending: bool,
    events: VecDeque<QueuedEvent>,
    usage: Arc<Usage>,
    slot: Arc<Slot>,
}

#[derive(Default)]
struct Slot {
    available: Notify,
    ended: CancellationToken,
    membership_lost: CancellationToken,
    terminal: Mutex<Option<InterruptionReason>>,
}

impl Slot {
    fn end(&self, reason: InterruptionReason) {
        if let Ok(mut terminal) = self.terminal.lock() {
            *terminal = Some(reason);
        }
        if reason == InterruptionReason::MembershipLost {
            self.membership_lost.cancel();
        }
        self.ended.cancel();
        self.available.notify_one();
    }
}

impl Shared {
    pub(super) fn count_maintenance_request(&self) {
        self.maintenance_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn mark_started(&self) {
        self.started.cancel();
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, SubscribeError> {
        self.state
            .lock()
            .map_err(|_| SubscribeError::ReaderUnavailable)
    }

    fn register(
        self: &Arc<Self>,
        channels: &[ChannelId],
    ) -> Result<ChannelSubscription, SubscribeError> {
        let mut state = self.lock()?;
        if self.stopped.is_cancelled() {
            return Err(SubscribeError::ReaderUnavailable);
        }
        if state.listeners.len() >= self.limits.max_subscriptions {
            return Err(SubscribeError::Capacity);
        }

        let id = state
            .last_id
            .checked_add(1)
            .ok_or(SubscribeError::Capacity)?;
        let slot = Arc::new(Slot::default());
        state.last_id = id;
        state.listeners.insert(
            id,
            Listener {
                positions: channels.iter().map(|&channel| (channel, None)).collect(),
                pending: true,
                events: VecDeque::new(),
                usage: Arc::new(Usage::default()),
                slot: Arc::clone(&slot),
            },
        );
        Ok(ChannelSubscription {
            id,
            shared: Arc::clone(self),
            slot,
        })
    }

    fn activate(
        &self,
        subscription: &ChannelSubscription,
        heads: Vec<ChannelHead>,
        channels: &[ChannelId],
        timing: &BoundedOperation<'_>,
    ) -> Result<(), SubscribeError> {
        if heads.len() != channels.len()
            || !heads
                .iter()
                .zip(channels)
                .all(|(head, id)| head.channel_id == *id)
        {
            return Err(SubscribeError::ReaderUnavailable);
        }
        let positions: HashMap<_, _> = heads
            .iter()
            .map(|head| (head.channel_id, head.head_message_id))
            .collect();
        let ready = EncodedEvent::new(ChannelEvent::Ready(heads), self.limits.max_event_bytes)
            .map(Arc::new)
            .ok_or(SubscribeError::SetupOverflow)?;

        let mut state = self.lock()?;
        timing.check()?;
        let listener = state
            .listeners
            .get_mut(&subscription.id)
            .ok_or_else(|| subscription.setup_error())?;
        listener.events.retain(|event| {
            matches!(
                event.event(),
                ChannelEvent::Post(post) if positions.get(&post.channel_id)
                    .is_some_and(|head| above(post.message_id, *head))
            )
        });
        for (id, head) in positions {
            if let Some(head) = head {
                let position = listener
                    .positions
                    .get_mut(&id)
                    .ok_or(SubscribeError::ReaderUnavailable)?;
                if above(head, *position) {
                    *position = Some(head);
                }
            }
        }
        let ready = self
            .reserve(listener, &ready)
            .ok_or(SubscribeError::SetupOverflow)?;
        listener.events.push_front(ready);
        listener.pending = false;
        listener.slot.available.notify_one();
        Ok(())
    }

    pub(super) fn interested(&self, channel: ChannelId) -> bool {
        self.lock().is_ok_and(|state| {
            state
                .listeners
                .values()
                .any(|listener| listener.positions.contains_key(&channel))
        })
    }

    pub(super) fn publish(&self, post: ChannelPost) {
        let channel = post.channel_id;
        let id = post.message_id;
        let payload =
            EncodedEvent::new(ChannelEvent::Post(post), self.limits.max_event_bytes).map(Arc::new);
        let Ok(mut state) = self.lock() else {
            return;
        };
        state.listeners.retain(|_, listener| {
            let Some(position) = listener.positions.get_mut(&channel) else {
                return true;
            };
            if !above(id, *position) {
                return true;
            }
            *position = Some(id);
            let Some(event) = payload
                .as_ref()
                .and_then(|payload| self.reserve(listener, payload))
            else {
                listener.events.clear();
                listener.slot.end(InterruptionReason::SlowConsumer);
                return false;
            };
            listener.events.push_back(event);
            if !listener.pending {
                listener.slot.available.notify_one();
            }
            true
        });
    }

    fn reserve(&self, listener: &Listener, event: &Arc<EncodedEvent>) -> Option<QueuedEvent> {
        QueuedEvent::new(event, &listener.usage, &self.budget, self.limits)
    }

    pub(super) fn interrupt_channel(&self, channel: ChannelId, reason: InterruptionReason) {
        if let Ok(mut state) = self.lock() {
            state.listeners.retain(|_, listener| {
                if !listener.positions.contains_key(&channel) {
                    return true;
                }
                listener.events.clear();
                listener.slot.end(reason);
                false
            });
        }
    }

    pub(super) fn close(&self, reason: InterruptionReason) {
        if let Ok(mut state) = self.lock() {
            self.stopped.cancel();
            for (_, mut listener) in state.listeners.drain() {
                listener.events.clear();
                listener.slot.end(reason);
            }
        } else {
            self.stopped.cancel();
        }
    }
}

fn above(id: MessageId, position: Option<MessageId>) -> bool {
    position.is_none_or(|position| id.get() > position.get())
}

/// Registration and queue for one accepted subscription.
///
/// Dropping it releases queued events and frees its account subscription slot.
/// An event already leased to the caller remains reserved until dropped.
pub struct ChannelSubscription {
    id: u64,
    shared: Arc<Shared>,
    slot: Arc<Slot>,
}

impl ChannelSubscription {
    fn setup_error(&self) -> SubscribeError {
        let reason = self.slot.terminal.lock().ok().and_then(|reason| *reason);
        match reason {
            Some(InterruptionReason::MembershipLost) => SubscribeError::Ineligible,
            Some(InterruptionReason::SlowConsumer) => SubscribeError::SetupOverflow,
            _ => SubscribeError::ReaderUnavailable,
        }
    }

    /// Notify a transport that queued or in-flight output has lost Telegram authorization.
    pub fn membership_lost(&self) -> CancellationToken {
        self.slot.membership_lost.child_token()
    }

    /// Receive the next typed event and release its queue reservation.
    pub async fn next(&mut self) -> Option<ChannelEvent> {
        self.next_queued()
            .await
            .map(|queued| queued.event().clone())
    }

    /// Receive the next encoded event with its in-flight queue reservation.
    pub async fn next_queued(&mut self) -> Option<QueuedEvent> {
        loop {
            {
                let mut state = self.shared.lock().ok()?;
                match state.listeners.get_mut(&self.id) {
                    Some(listener) => {
                        if let Some(event) = listener.events.pop_front() {
                            return Some(event);
                        }
                    }
                    None => {
                        let reason = self.slot.terminal.lock().ok()?.take()?;
                        return QueuedEvent::terminal(reason, self.shared.limits.max_event_bytes);
                    }
                }
            }
            self.slot.available.notified().await;
        }
    }
}

impl Drop for ChannelSubscription {
    fn drop(&mut self) {
        if let Ok(mut state) = self.shared.lock() {
            state.listeners.remove(&self.id);
        }
    }
}
