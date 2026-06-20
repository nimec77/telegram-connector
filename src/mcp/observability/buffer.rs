//! Ring buffer of recent serialized responses for `get_last_responses`.
//!
//! Unit of `observability` (LM-5).

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

/// One response kept for recovery via `get_last_responses`.
#[derive(Debug, Clone)]
pub struct BufferedResponse {
    pub request_id: String,
    pub tool_name: String,
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

/// Ring buffer of the last N serialized responses (capacity 0 = disabled).
pub struct ResponseBuffer {
    capacity: usize,
    max_payload_bytes: usize,
    entries: Mutex<VecDeque<BufferedResponse>>,
}

impl ResponseBuffer {
    pub fn new(capacity: usize, max_payload_bytes: usize) -> Self {
        Self {
            capacity,
            max_payload_bytes,
            entries: Mutex::new(VecDeque::new()),
        }
    }

    pub fn push(&self, mut entry: BufferedResponse) {
        if self.capacity == 0 {
            return;
        }
        if entry.payload.len() > self.max_payload_bytes {
            entry.payload = OVERSIZED_PAYLOAD_STUB.to_string();
        }
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.len() == self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    /// The most recent `n` entries, newest first. `None` returns everything.
    pub fn last(&self, n: Option<usize>) -> Vec<BufferedResponse> {
        let entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let n = n.unwrap_or(entries.len()).min(entries.len());
        entries.iter().rev().take(n).cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Name of the recovery tool — its own responses are never buffered, so calling
/// it cannot evict the data it exists to recover.
pub const GET_LAST_RESPONSES_TOOL: &str = "get_last_responses";

#[cfg(test)]
#[path = "tests/buffer_tests.rs"]
mod tests;
