//! Session counters and in-flight request tracking.
//!
//! Unit of `observability` (LM-5).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Instant, SystemTime};

/// A request that has been received but not yet answered.
#[derive(Debug, Clone)]
pub struct InFlightRequest {
    pub tool_name: String,
    pub received_at: Instant,
}

/// Counters and in-flight request tracking for one stdio session.
///
/// Shared (`Arc`) between the instrumented transport (writer), `check_mcp_status`
/// (reader), and the shutdown paths (summary logging).
pub struct SessionMetrics {
    session_started_at: SystemTime,
    started_instant: Instant,
    requests_received: AtomicU64,
    responses_written: AtomicU64,
    last_write: Mutex<Option<Instant>>,
    in_flight: Mutex<HashMap<String, InFlightRequest>>,
}

impl Default for SessionMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionMetrics {
    pub fn new() -> Self {
        Self {
            session_started_at: SystemTime::now(),
            started_instant: Instant::now(),
            requests_received: AtomicU64::new(0),
            responses_written: AtomicU64::new(0),
            last_write: Mutex::new(None),
            in_flight: Mutex::new(HashMap::new()),
        }
    }

    /// Record an inbound JSON-RPC request.
    pub fn record_request(&self, request_id: &str, tool_name: &str) {
        self.requests_received.fetch_add(1, Ordering::Relaxed);
        self.in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                request_id.to_string(),
                InFlightRequest {
                    tool_name: tool_name.to_string(),
                    received_at: Instant::now(),
                },
            );
    }

    /// Record a successfully written response. Returns the matching in-flight
    /// request, if any, so the caller can log tool name and total duration.
    pub fn record_response_written(&self, request_id: &str) -> Option<InFlightRequest> {
        self.responses_written.fetch_add(1, Ordering::Relaxed);
        *self
            .last_write
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
        self.in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(request_id)
    }

    pub fn requests_received(&self) -> u64 {
        self.requests_received.load(Ordering::Relaxed)
    }

    pub fn responses_written(&self) -> u64 {
        self.responses_written.load(Ordering::Relaxed)
    }

    /// Seconds since the last successful response write; `None` before the first.
    pub fn last_write_age_secs(&self) -> Option<u64> {
        self.last_write
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .map(|written| written.elapsed().as_secs())
    }

    pub fn session_started_at_rfc3339(&self) -> String {
        chrono::DateTime::<chrono::Utc>::from(self.session_started_at).to_rfc3339()
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started_instant.elapsed().as_secs()
    }

    /// Ids and tool names of requests received but never answered.
    pub fn abandoned_requests(&self) -> Vec<(String, String)> {
        self.in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(id, request)| (id.clone(), request.tool_name.clone()))
            .collect()
    }

    /// One-line session summary, emitted at stdin EOF and on signal shutdown.
    pub fn log_summary(&self, reason: &str) {
        tracing::info!(
            reason,
            uptime_secs = self.uptime_secs(),
            requests_received = self.requests_received(),
            responses_written = self.responses_written(),
            last_write_age_secs = ?self.last_write_age_secs(),
            abandoned_in_flight = ?self.abandoned_requests(),
            "Session summary"
        );
    }
}

#[cfg(test)]
#[path = "tests/metrics_tests.rs"]
mod tests;
