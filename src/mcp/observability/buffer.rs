//! Ring buffer of recent serialized responses for `get_last_responses`.
//!
//! Unit of `observability` (LM-5).

#![warn(missing_docs)]

use crate::error::StateUnavailable;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::SystemTime;

/// One response kept for recovery via `get_last_responses`.
#[derive(Debug, Clone)]
pub struct BufferedResponse {
    /// Display form of the request ID; internal correlation separately preserves ID type.
    pub request_id: String,
    /// Recorded tool/method name.
    pub tool_name: String,
    /// Wall-clock time at successful write observation.
    pub written_at: SystemTime,
    /// Byte count of the serialized JSON-RPC envelope (excludes the framing newline
    /// appended by the transport layer).
    pub size_bytes: usize,
    /// The serialized JSON-RPC envelope exactly as written to stdout (no framing newline).
    /// Payloads larger than `max_buffered_payload_bytes` are replaced with
    /// `OVERSIZED_PAYLOAD_STUB` before being stored; `size_bytes` still reflects
    /// the real wire size of the original response.
    pub payload: String,
}

/// Payload stored in place of response bodies larger than
/// `[observability] max_buffered_payload_bytes`. Valid JSON so
/// get_last_responses can embed it as-is.
pub const OVERSIZED_PAYLOAD_STUB: &str =
    r#"{"omitted":"payload exceeded max_buffered_payload_bytes"}"#;

/// A consistent replay snapshot and its total retained entry count.
#[derive(Debug)]
pub struct ResponseSnapshot {
    /// Requested newest-first subset from the same lock acquisition as the count.
    pub responses: Vec<BufferedResponse>,
    /// Total entries retained before limiting the returned subset.
    pub buffered: usize,
}

/// Ring buffer of the last N serialized responses (capacity 0 = disabled).
pub struct ResponseBuffer {
    capacity: usize,
    max_payload_bytes: usize,
    entries: Mutex<VecDeque<BufferedResponse>>,
}

impl ResponseBuffer {
    /// Create empty in-memory storage. Zero capacity disables replay; payloads
    /// above `max_payload_bytes` are retained as size-preserving omission stubs.
    pub fn new(capacity: usize, max_payload_bytes: usize) -> Self {
        Self {
            capacity,
            max_payload_bytes,
            entries: Mutex::new(VecDeque::new()),
        }
    }

    /// Best-effort compatibility write; unavailable replay refuses the entry.
    /// Use [`Self::try_push`] to observe failure.
    pub fn push(&self, entry: BufferedResponse) {
        let _ = self.try_push(entry);
    }

    /// Retain a response unless buffering is disabled.
    ///
    /// # Errors
    /// Returns [`StateUnavailable::ResponseBuffer`] if enabled replay is poisoned.
    pub fn try_push(&self, mut entry: BufferedResponse) -> Result<(), StateUnavailable> {
        if self.capacity == 0 {
            return Ok(());
        }
        if entry.payload.len() > self.max_payload_bytes {
            entry.payload = OVERSIZED_PAYLOAD_STUB.to_string();
        }
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| StateUnavailable::ResponseBuffer)?;
        if entries.len() == self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry);
        Ok(())
    }

    /// Read newest-first responses and total count under one lock.
    /// Disabled buffering is an ordinary successful empty snapshot.
    ///
    /// # Errors
    /// Returns [`StateUnavailable::ResponseBuffer`] if enabled replay is poisoned.
    pub fn try_snapshot(&self, n: Option<usize>) -> Result<ResponseSnapshot, StateUnavailable> {
        if self.capacity == 0 {
            return Ok(ResponseSnapshot {
                responses: Vec::new(),
                buffered: 0,
            });
        }
        let entries = self
            .entries
            .lock()
            .map_err(|_| StateUnavailable::ResponseBuffer)?;
        Ok(ResponseSnapshot {
            responses: entries
                .iter()
                .rev()
                .take(n.unwrap_or(entries.len()))
                .cloned()
                .collect(),
            buffered: entries.len(),
        })
    }

    /// Compatibility snapshot: suppresses untrusted responses on unavailable storage.
    /// Empty therefore means either absent or unavailable; use [`Self::try_snapshot`]
    /// for diagnostics/recovery. `None` returns all retained entries, newest first.
    pub fn last(&self, n: Option<usize>) -> Vec<BufferedResponse> {
        self.try_snapshot(n)
            .map(|snapshot| snapshot.responses)
            .unwrap_or_default()
    }

    /// Count retained entries.
    ///
    /// # Errors
    /// Returns [`StateUnavailable::ResponseBuffer`] if enabled replay is poisoned.
    pub fn try_len(&self) -> Result<usize, StateUnavailable> {
        if self.capacity == 0 {
            return Ok(0);
        }
        self.entries
            .lock()
            .map(|entries| entries.len())
            .map_err(|_| StateUnavailable::ResponseBuffer)
    }

    /// Compatibility count: zero also means unavailable, conservatively exposing no
    /// replayable entries. Use [`Self::try_len`] when availability matters.
    pub fn len(&self) -> usize {
        self.try_len().unwrap_or(0)
    }

    /// Compatibility predicate: unavailable replay is treated as having no usable entries.
    /// Use [`Self::try_len`] to distinguish unavailability from ordinary emptiness.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Name of the recovery tool — its own responses are never buffered, so calling
/// it cannot evict the data it exists to recover.
pub const GET_LAST_RESPONSES_TOOL: &str = "get_last_responses";

#[cfg(test)]
#[path = "tests/buffer_tests.rs"]
pub(crate) mod tests;
