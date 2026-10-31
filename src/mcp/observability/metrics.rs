//! Session counters and in-flight request tracking.
//!
//! Unit of `observability` (LM-5).

#![warn(missing_docs)]

use crate::error::StateUnavailable;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime};

/// A request that has been received but not yet answered.
#[derive(Debug, Clone)]
pub struct InFlightRequest {
    /// Tool/method identity associated with the request.
    pub tool_name: String,
    /// Monotonic receipt instant used for latency observation.
    pub received_at: Instant,
}

/// Independent outcomes from recording a successful write. A failed timestamp update
/// does not prevent request correlation, or vice versa.
#[derive(Debug)]
pub struct WriteObservation {
    /// Matched identity, ordinary absence, or correlation unavailability.
    pub request: Result<Option<InFlightRequest>, StateUnavailable>,
    /// Independent timestamp update result; does not govern the request result.
    pub last_write: Result<(), StateUnavailable>,
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
    /// Start a local session with zero counts and no writes or pending identities.
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

    /// Record a request best-effort; the independent atomic count always advances.
    /// Use [`Self::try_record_request`] to observe correlation availability.
    pub fn record_request(&self, request_id: &str, tool_name: &str) {
        let _ = self.try_record_request(request_id, tool_name);
    }

    /// Record an inbound request and preserve availability information.
    ///
    /// # Errors
    /// Returns [`StateUnavailable::InFlight`] on poisoned correlation state.
    /// The independent request counter is still incremented.
    pub fn try_record_request(
        &self,
        request_id: &str,
        tool_name: &str,
    ) -> Result<(), StateUnavailable> {
        self.requests_received.fetch_add(1, Ordering::Relaxed);
        self.in_flight
            .lock()
            .map_err(|_| StateUnavailable::InFlight)?
            .insert(
                request_id.to_string(),
                InFlightRequest {
                    tool_name: tool_name.to_string(),
                    received_at: Instant::now(),
                },
            );
        Ok(())
    }

    /// Compatibility write accounting. Returns no match if correlation is unavailable;
    /// it loses the distinction between absent and unavailable request identity.
    /// Use [`Self::observe_response_written`] when that distinction matters.
    pub fn record_response_written(&self, request_id: &str) -> Option<InFlightRequest> {
        self.observe_response_written(request_id)
            .request
            .ok()
            .flatten()
    }

    /// Count a successful write and attempt each independent mutex update separately.
    /// No protected contents are read after poisoning.
    pub fn observe_response_written(&self, request_id: &str) -> WriteObservation {
        self.responses_written.fetch_add(1, Ordering::Relaxed);
        let last_write = self
            .last_write
            .lock()
            .map(|mut written| {
                *written = Some(Instant::now());
            })
            .map_err(|_| StateUnavailable::LastWrite);
        let request = self
            .in_flight
            .lock()
            .map(|mut pending| pending.remove(request_id))
            .map_err(|_| StateUnavailable::InFlight);
        WriteObservation {
            request,
            last_write,
        }
    }

    /// Release cancelled or unwritable work without counting a successful response.
    pub(crate) fn discard_request(&self, request_id: &str) {
        if let Ok(mut pending) = self.in_flight.lock() {
            pending.remove(request_id);
        }
    }

    /// Independent relaxed atomic count of requests recorded, even if correlation failed.
    pub fn requests_received(&self) -> u64 {
        self.requests_received.load(Ordering::Relaxed)
    }

    /// Independent relaxed atomic count of successful writes observed.
    pub fn responses_written(&self) -> u64 {
        self.responses_written.load(Ordering::Relaxed)
    }

    /// Compatibility snapshot: `None` also means unavailable. This omits an untrusted
    /// timestamp conservatively; diagnostics must use [`Self::try_last_write_age_secs`].
    pub fn last_write_age_secs(&self) -> Option<u64> {
        self.try_last_write_age_secs().ok().flatten()
    }

    /// Seconds since the last successful write; `Ok(None)` before the first write.
    ///
    /// # Errors
    /// Returns [`StateUnavailable::LastWrite`] on poisoned timestamp state.
    pub fn try_last_write_age_secs(&self) -> Result<Option<u64>, StateUnavailable> {
        self.last_write
            .lock()
            .map_err(|_| StateUnavailable::LastWrite)
            .map(|written| written.map(|instant| instant.elapsed().as_secs()))
    }

    /// Wall-clock session start in RFC 3339 UTC, independent of poisoned locks.
    pub fn session_started_at_rfc3339(&self) -> String {
        chrono::DateTime::<chrono::Utc>::from(self.session_started_at).to_rfc3339()
    }

    /// Monotonic elapsed seconds since creation, independent of poisoned locks.
    pub fn uptime_secs(&self) -> u64 {
        self.started_instant.elapsed().as_secs()
    }

    /// Compatibility snapshot: omits all identities when correlation is unavailable.
    /// This prevents using untrusted identities but loses availability information;
    /// diagnostics must use [`Self::try_abandoned_requests`].
    pub fn abandoned_requests(&self) -> Vec<(String, String)> {
        self.try_abandoned_requests().unwrap_or_default()
    }

    /// Ids and tool names received but never answered.
    ///
    /// # Errors
    /// Returns [`StateUnavailable::InFlight`] on poisoned correlation state.
    pub fn try_abandoned_requests(&self) -> Result<Vec<(String, String)>, StateUnavailable> {
        self.in_flight
            .lock()
            .map_err(|_| StateUnavailable::InFlight)
            .map(|pending| {
                pending
                    .iter()
                    .map(|(id, request)| (id.clone(), request.tool_name.clone()))
                    .collect()
            })
    }

    /// One-line session summary, emitted at stdin EOF and on signal shutdown.
    pub fn log_summary(&self, reason: &str) {
        tracing::info!(
            reason,
            uptime_secs = self.uptime_secs(),
            requests_received = self.requests_received(),
            responses_written = self.responses_written(),
            last_write_age_secs = ?self.try_last_write_age_secs(),
            abandoned_in_flight = ?self.try_abandoned_requests(),
            "Session summary"
        );
    }
}

#[cfg(test)]
#[path = "tests/metrics_tests.rs"]
pub(crate) mod tests;
