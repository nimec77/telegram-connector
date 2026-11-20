//! Encoded event reservations survive dequeue until the last transport lease drops.

use super::{ChannelEvent, ChannelHead, InterruptionReason, StreamLimits};
use serde::Serialize;
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A byte budget shared by all account stream managers in one service.
pub struct StreamBudget {
    capacity: usize,
    used: AtomicUsize,
    readers: AtomicUsize,
    rejected_setups: AtomicU64,
    slow_consumers: AtomicU64,
    upstream_disconnects: AtomicU64,
    membership_losses: AtomicU64,
    service_drains: AtomicU64,
}

/// Aggregate live stream resource use and cumulative outcomes for a shared budget.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StreamDiagnostics {
    /// Started readers that have yet to close.
    pub active_readers: usize,
    /// Encoded bytes reserved by pending, queued, and in-flight events.
    pub queued_bytes: usize,
    /// Subscription setup attempts rejected by the transport.
    pub rejected_setups: u64,
    /// Accepted subscriptions interrupted by queue limits.
    pub slow_consumers: u64,
    /// Accepted subscriptions interrupted by a reader failure.
    pub upstream_disconnects: u64,
    /// Accepted subscriptions interrupted by observed membership loss.
    pub membership_losses: u64,
    /// Accepted subscriptions interrupted by service drain.
    pub service_drains: u64,
}

impl StreamBudget {
    /// Set a limit on queued and in-flight encoded event bytes across accounts.
    pub fn new(max_bytes: usize) -> Self {
        Self {
            capacity: max_bytes,
            used: AtomicUsize::new(0),
            readers: AtomicUsize::new(0),
            rejected_setups: AtomicU64::new(0),
            slow_consumers: AtomicU64::new(0),
            upstream_disconnects: AtomicU64::new(0),
            membership_losses: AtomicU64::new(0),
            service_drains: AtomicU64::new(0),
        }
    }

    /// Current reserved bytes across account queues and in-flight events.
    pub fn used_bytes(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    /// Read the service's resource totals and interruption counts.
    pub fn diagnostics(&self) -> StreamDiagnostics {
        StreamDiagnostics {
            active_readers: self.readers.load(Ordering::Relaxed),
            queued_bytes: self.used_bytes(),
            rejected_setups: self.rejected_setups.load(Ordering::Relaxed),
            slow_consumers: self.slow_consumers.load(Ordering::Relaxed),
            upstream_disconnects: self.upstream_disconnects.load(Ordering::Relaxed),
            membership_losses: self.membership_losses.load(Ordering::Relaxed),
            service_drains: self.service_drains.load(Ordering::Relaxed),
        }
    }

    /// Count a failed subscription setup at the transport boundary.
    pub fn record_rejected_setup(&self) {
        self.rejected_setups.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn reader_started(&self) {
        self.readers.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn reader_stopped(&self) {
        self.readers.fetch_sub(1, Ordering::Relaxed);
    }

    pub(super) fn record_interruption(&self, reason: InterruptionReason) {
        let counter = match reason {
            InterruptionReason::SlowConsumer => &self.slow_consumers,
            InterruptionReason::UpstreamDisconnected => &self.upstream_disconnects,
            InterruptionReason::MembershipLost => &self.membership_losses,
            InterruptionReason::ServiceDraining => &self.service_drains,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn reserve(&self, bytes: usize) -> bool {
        self.used
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes)
                    .filter(|&next| next <= self.capacity)
            })
            .is_ok()
    }
}

impl Default for StreamBudget {
    fn default() -> Self {
        Self::new(64 * 1024 * 1024)
    }
}

#[derive(Default)]
pub(super) struct Usage {
    counts: Mutex<(usize, usize)>,
}

impl Usage {
    pub(super) fn has_reservations(&self) -> bool {
        self.counts.lock().map_or(true, |counts| counts.0 != 0)
    }
}

struct Reservation {
    account: Arc<Usage>,
    budget: Arc<StreamBudget>,
    bytes: usize,
}

impl Reservation {
    fn acquire(
        account: &Arc<Usage>,
        budget: &Arc<StreamBudget>,
        limits: StreamLimits,
        bytes: usize,
    ) -> Option<Self> {
        let mut counts = account.counts.lock().ok()?;
        let next_bytes = counts.1.checked_add(bytes)?;
        if counts.0 >= limits.max_queue_items
            || next_bytes > limits.max_queue_bytes
            || !budget.reserve(bytes)
        {
            return None;
        }
        *counts = (counts.0 + 1, next_bytes);
        Some(Self {
            account: Arc::clone(account),
            budget: Arc::clone(budget),
            bytes,
        })
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut counts) = self.account.counts.lock() {
            counts.0 -= 1;
            counts.1 -= self.bytes;
        }
        self.budget.used.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

/// An event whose byte and item reservations are kept until it is dropped.
///
/// Keep this value alive while asynchronously writing its output. The
/// out-of-band terminal event carries no reservation and is sent only once.
pub struct QueuedEvent {
    payload: Arc<EncodedEvent>,
    _reservation: Option<Reservation>,
}

impl QueuedEvent {
    pub(super) fn new(
        payload: &Arc<EncodedEvent>,
        usage: &Arc<Usage>,
        budget: &Arc<StreamBudget>,
        limits: StreamLimits,
    ) -> Option<Self> {
        Some(Self {
            _reservation: Some(Reservation::acquire(
                usage,
                budget,
                limits,
                payload.bytes(),
            )?),
            payload: Arc::clone(payload),
        })
    }

    pub(super) fn terminal(reason: InterruptionReason, max_bytes: usize) -> Option<Self> {
        Some(Self {
            payload: Arc::new(EncodedEvent::new(
                ChannelEvent::Interrupted(reason),
                max_bytes,
            )?),
            _reservation: None,
        })
    }

    /// The typed event carried by this lease.
    pub fn event(&self) -> &ChannelEvent {
        &self.payload.event
    }

    /// The single-line JSON body of the event.
    pub fn json(&self) -> &str {
        &self.payload.json
    }

    /// The SSE event field.
    pub fn event_name(&self) -> &'static str {
        self.payload.name()
    }
}

pub(super) struct EncodedEvent {
    event: ChannelEvent,
    json: String,
}

impl EncodedEvent {
    pub(super) fn new(event: ChannelEvent, max_bytes: usize) -> Option<Self> {
        let limit = max_bytes.checked_sub(framing_bytes(&event))?;
        let mut writer = CappedWriter {
            bytes: Vec::new(),
            limit,
        };
        let serialized = match &event {
            ChannelEvent::Ready(channels) => {
                serde_json::to_writer(&mut writer, &Ready { channels })
            }
            ChannelEvent::Post(post) => serde_json::to_writer(&mut writer, post),
            ChannelEvent::Interrupted(reason) => {
                serde_json::to_writer(&mut writer, &Interrupted { reason })
            }
        };
        serialized.ok()?;
        Some(Self {
            event,
            json: String::from_utf8(writer.bytes).ok()?,
        })
    }

    fn name(&self) -> &'static str {
        event_name(&self.event)
    }

    fn bytes(&self) -> usize {
        self.json.len() + framing_bytes(&self.event)
    }
}

fn event_name(event: &ChannelEvent) -> &'static str {
    match event {
        ChannelEvent::Ready(_) => "ready",
        ChannelEvent::Post(_) => "post",
        ChannelEvent::Interrupted(_) => "interrupted",
    }
}

fn framing_bytes(event: &ChannelEvent) -> usize {
    // event: NAME\ndata: JSON\n\n
    "event: \ndata: \n\n".len() + event_name(event).len()
}

#[derive(Serialize)]
struct Ready<'a> {
    channels: &'a [ChannelHead],
}

#[derive(Serialize)]
struct Interrupted<'a> {
    reason: &'a InterruptionReason,
}

struct CappedWriter {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for CappedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit - self.bytes.len() {
            return Err(io::Error::other("stream event exceeds encoded byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
