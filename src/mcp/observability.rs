//! Transport-layer observability: session metrics, response ring buffer, and an
//! instrumented stdio transport decorator.
//!
//! Built after the 2026-06-12 incident (`docs/connetion-issue.md`): a tool response
//! was produced but lost between connector stdout and the client, and the logs could
//! not prove delivery. These types log every actual stdout write (request id, payload
//! size, write+flush duration), warn on blocked writes, and emit a session summary
//! when the input stream ends.

use rmcp::RoleServer;
use rmcp::model::{ClientRequest, JsonRpcMessage};
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::Transport;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

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

/// `true` when a stdout write took longer than the configured threshold.
pub(crate) fn is_slow_write(elapsed: Duration, threshold: Duration) -> bool {
    elapsed > threshold
}

/// Decorator around the stdio transport that logs every inbound request and every
/// actual stdout write, feeding [`SessionMetrics`] and [`ResponseBuffer`].
///
/// Instrumentation never fails the message path: serialization or buffer problems
/// are logged and swallowed; the inner transport's result is returned untouched.
pub struct InstrumentedTransport<T> {
    pub(crate) inner: T,
    metrics: Arc<SessionMetrics>,
    buffer: Arc<ResponseBuffer>,
    slow_write_threshold: Duration,
}

impl<T> InstrumentedTransport<T> {
    pub fn new(
        inner: T,
        metrics: Arc<SessionMetrics>,
        buffer: Arc<ResponseBuffer>,
        slow_write_threshold: Duration,
    ) -> Self {
        Self {
            inner,
            metrics,
            buffer,
            slow_write_threshold,
        }
    }
}

impl<T: Transport<RoleServer>> Transport<RoleServer> for InstrumentedTransport<T> {
    type Error = T::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        // Pre-compute identity and payload here: the returned future must be
        // 'static, so it cannot borrow from self or the message.
        let response_id = match &item {
            JsonRpcMessage::Response(response) => Some(response.id.to_string()),
            JsonRpcMessage::Error(error) => error.id.as_ref().map(ToString::to_string),
            _ => None,
        };
        // One extra serialization buys exact payload size + the recovery copy.
        let payload = response_id
            .as_ref()
            .and_then(|_| serde_json::to_string(&item).ok());
        let metrics = Arc::clone(&self.metrics);
        let buffer = Arc::clone(&self.buffer);
        let threshold = self.slow_write_threshold;
        let inner_send = self.inner.send(item);

        async move {
            let write_started = Instant::now();
            let result = inner_send.await;
            let Some(request_id) = response_id else {
                return result; // notifications/requests pass through unaccounted
            };
            match &result {
                Ok(()) => {
                    let write_elapsed = write_started.elapsed();
                    let write_ms = write_elapsed.as_millis() as u64;
                    let in_flight = metrics.record_response_written(&request_id);
                    let tool_name = in_flight
                        .as_ref()
                        .map(|request| request.tool_name.clone())
                        .unwrap_or_default();
                    let total_ms =
                        in_flight.map(|request| request.received_at.elapsed().as_millis() as u64);
                    // Counts the serialized envelope bytes; excludes the framing '\n'
                    // that rmcp's AsyncRwTransport appends on the wire.
                    let size_bytes = payload.as_ref().map_or(0, String::len);
                    tracing::info!(
                        request_id = %request_id,
                        tool = %tool_name,
                        bytes = size_bytes,
                        write_ms,
                        total_ms = ?total_ms,
                        "Response written to stdout"
                    );
                    if is_slow_write(write_elapsed, threshold) {
                        tracing::warn!(
                            request_id = %request_id,
                            write_ms,
                            threshold_ms = threshold.as_millis() as u64,
                            "Slow stdout write - peer may have stopped reading"
                        );
                    }
                    if let Some(payload) = payload
                        && tool_name != GET_LAST_RESPONSES_TOOL
                    {
                        buffer.push(BufferedResponse {
                            request_id,
                            tool_name,
                            written_at: SystemTime::now(),
                            size_bytes,
                            payload,
                        });
                    }
                }
                Err(error) => {
                    tracing::error!(
                        request_id = %request_id,
                        error = %error,
                        "Response write failed"
                    );
                }
            }
            result
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        let message = self.inner.receive().await;
        match &message {
            Some(JsonRpcMessage::Request(request)) => {
                let request_id = request.id.to_string();
                let method = request.request.method();
                let tool = match &request.request {
                    ClientRequest::CallToolRequest(call) => call.params.name.as_ref(),
                    _ => method,
                };
                self.metrics.record_request(&request_id, tool);
                tracing::debug!(request_id = %request_id, method, tool, "Request received");
            }
            Some(_) => {}
            None => self.metrics.log_summary("input stream terminated"),
        }
        message
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.inner.close().await
    }
}

#[cfg(test)]
#[path = "observability/tests.rs"]
mod tests;
