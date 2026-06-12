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
    pub size_bytes: usize,
    /// The serialized JSON-RPC envelope exactly as written to stdout.
    pub payload: String,
}

/// Ring buffer of the last N serialized responses (capacity 0 = disabled).
pub struct ResponseBuffer {
    capacity: usize,
    entries: Mutex<VecDeque<BufferedResponse>>,
}

impl ResponseBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: Mutex::new(VecDeque::new()),
        }
    }

    pub fn push(&self, entry: BufferedResponse) {
        if self.capacity == 0 {
            return;
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
mod tests {
    use super::*;
    use rmcp::RoleServer;
    use rmcp::model::{
        JsonRpcMessage, JsonRpcResponse, JsonRpcVersion2_0, RequestId, ServerResult,
    };
    use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
    use rmcp::transport::Transport;
    use std::sync::Arc;
    use std::time::Duration;

    /// Channel-free fake: receive() pops from a queue, send() records JSON.
    struct FakeTransport {
        incoming: VecDeque<RxJsonRpcMessage<RoleServer>>,
        sent: Arc<Mutex<Vec<String>>>,
        fail_sends: bool,
    }

    impl FakeTransport {
        fn new(incoming: Vec<RxJsonRpcMessage<RoleServer>>) -> Self {
            Self {
                incoming: incoming.into(),
                sent: Arc::new(Mutex::new(Vec::new())),
                fail_sends: false,
            }
        }
    }

    impl Transport<RoleServer> for FakeTransport {
        type Error = std::io::Error;

        fn send(
            &mut self,
            item: TxJsonRpcMessage<RoleServer>,
        ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
            let sent = Arc::clone(&self.sent);
            let fail = self.fail_sends;
            async move {
                if fail {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "peer gone",
                    ));
                }
                let json = serde_json::to_string(&item).map_err(std::io::Error::other)?;
                sent.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(json);
                Ok(())
            }
        }

        async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
            self.incoming.pop_front()
        }

        async fn close(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    fn call_tool_request(id: i64, tool: &str) -> RxJsonRpcMessage<RoleServer> {
        serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": tool, "arguments": {} }
        }))
        .expect("valid call-tool request")
    }

    fn tool_response(id: i64) -> TxJsonRpcMessage<RoleServer> {
        JsonRpcMessage::Response(JsonRpcResponse {
            jsonrpc: JsonRpcVersion2_0,
            id: RequestId::Number(id),
            result: ServerResult::empty(()),
        })
    }

    fn instrumented(
        incoming: Vec<RxJsonRpcMessage<RoleServer>>,
    ) -> (
        InstrumentedTransport<FakeTransport>,
        Arc<SessionMetrics>,
        Arc<ResponseBuffer>,
    ) {
        let metrics = Arc::new(SessionMetrics::new());
        let buffer = Arc::new(ResponseBuffer::new(10));
        let transport = InstrumentedTransport::new(
            FakeTransport::new(incoming),
            Arc::clone(&metrics),
            Arc::clone(&buffer),
            Duration::from_millis(500),
        );
        (transport, metrics, buffer)
    }

    #[tokio::test]
    async fn receive_counts_requests_and_tracks_in_flight() {
        let (mut transport, metrics, _) =
            instrumented(vec![call_tool_request(1, "search_messages")]);
        let message = transport.receive().await;
        assert!(message.is_some());
        assert_eq!(metrics.requests_received(), 1);
        assert_eq!(
            metrics.abandoned_requests(),
            vec![("1".to_string(), "search_messages".to_string())]
        );
    }

    #[tokio::test]
    async fn receive_eof_returns_none() {
        let (mut transport, metrics, _) = instrumented(vec![]);
        assert!(transport.receive().await.is_none());
        assert_eq!(metrics.requests_received(), 0);
    }

    #[tokio::test]
    async fn send_response_updates_metrics_and_buffer() {
        let (mut transport, metrics, buffer) =
            instrumented(vec![call_tool_request(1, "search_messages")]);
        transport.receive().await;
        transport.send(tool_response(1)).await.expect("send ok");

        assert_eq!(metrics.responses_written(), 1);
        assert!(metrics.abandoned_requests().is_empty());
        assert_eq!(metrics.last_write_age_secs(), Some(0));

        let buffered = buffer.last(None);
        assert_eq!(buffered.len(), 1);
        assert_eq!(buffered[0].request_id, "1");
        assert_eq!(buffered[0].tool_name, "search_messages");
        assert!(buffered[0].payload.contains("jsonrpc"));
        assert_eq!(buffered[0].size_bytes, buffered[0].payload.len());
    }

    #[tokio::test]
    async fn send_skips_buffering_for_get_last_responses() {
        let (mut transport, metrics, buffer) =
            instrumented(vec![call_tool_request(2, "get_last_responses")]);
        transport.receive().await;
        transport.send(tool_response(2)).await.expect("send ok");
        assert_eq!(metrics.responses_written(), 1);
        assert!(buffer.is_empty());
    }

    #[tokio::test]
    async fn send_error_propagates_and_skips_metrics() {
        let (mut transport, metrics, buffer) =
            instrumented(vec![call_tool_request(1, "search_messages")]);
        transport.receive().await;
        transport.inner.fail_sends = true;
        let result = transport.send(tool_response(1)).await;
        assert!(result.is_err());
        assert_eq!(metrics.responses_written(), 0);
        // Failed write leaves the request in-flight (it was never answered).
        assert_eq!(metrics.abandoned_requests().len(), 1);
        assert!(buffer.is_empty());
    }

    #[test]
    fn slow_write_predicate() {
        assert!(is_slow_write(
            Duration::from_millis(501),
            Duration::from_millis(500)
        ));
        assert!(!is_slow_write(
            Duration::from_millis(499),
            Duration::from_millis(500)
        ));
        // Threshold 0 = every (nonzero) write warns.
        assert!(is_slow_write(Duration::from_micros(1), Duration::ZERO));
    }

    #[test]
    fn new_metrics_start_at_zero() {
        let metrics = SessionMetrics::new();
        assert_eq!(metrics.requests_received(), 0);
        assert_eq!(metrics.responses_written(), 0);
        assert_eq!(metrics.last_write_age_secs(), None);
        assert!(metrics.abandoned_requests().is_empty());
    }

    #[test]
    fn record_request_increments_and_tracks_in_flight() {
        let metrics = SessionMetrics::new();
        metrics.record_request("1", "search_messages");
        metrics.record_request("2", "check_mcp_status");
        assert_eq!(metrics.requests_received(), 2);
        let mut abandoned = metrics.abandoned_requests();
        abandoned.sort();
        assert_eq!(
            abandoned,
            vec![
                ("1".to_string(), "search_messages".to_string()),
                ("2".to_string(), "check_mcp_status".to_string()),
            ]
        );
    }

    #[test]
    fn record_response_written_clears_in_flight_and_returns_info() {
        let metrics = SessionMetrics::new();
        metrics.record_request("1", "search_messages");
        let in_flight = metrics.record_response_written("1");
        assert_eq!(metrics.responses_written(), 1);
        assert_eq!(
            in_flight.expect("in-flight entry").tool_name,
            "search_messages"
        );
        assert!(metrics.abandoned_requests().is_empty());
        assert_eq!(metrics.last_write_age_secs(), Some(0));
    }

    #[test]
    fn record_response_written_unknown_id_still_counts() {
        let metrics = SessionMetrics::new();
        let in_flight = metrics.record_response_written("99");
        assert!(in_flight.is_none());
        assert_eq!(metrics.responses_written(), 1);
    }

    #[test]
    fn session_started_at_is_rfc3339() {
        let metrics = SessionMetrics::new();
        let stamp = metrics.session_started_at_rfc3339();
        assert!(chrono::DateTime::parse_from_rfc3339(&stamp).is_ok());
    }

    #[test]
    fn log_summary_does_not_panic() {
        let metrics = SessionMetrics::new();
        metrics.record_request("1", "search_messages");
        metrics.log_summary("test");
    }

    fn entry(id: &str) -> BufferedResponse {
        BufferedResponse {
            request_id: id.to_string(),
            tool_name: "search_messages".to_string(),
            written_at: SystemTime::now(),
            size_bytes: 2,
            payload: "{}".to_string(),
        }
    }

    #[test]
    fn buffer_returns_newest_first() {
        let buffer = ResponseBuffer::new(5);
        buffer.push(entry("1"));
        buffer.push(entry("2"));
        let last = buffer.last(None);
        assert_eq!(last.len(), 2);
        assert_eq!(last[0].request_id, "2");
        assert_eq!(last[1].request_id, "1");
    }

    #[test]
    fn buffer_evicts_oldest_at_capacity() {
        let buffer = ResponseBuffer::new(2);
        buffer.push(entry("1"));
        buffer.push(entry("2"));
        buffer.push(entry("3"));
        let ids: Vec<String> = buffer
            .last(None)
            .into_iter()
            .map(|e| e.request_id)
            .collect();
        assert_eq!(ids, vec!["3".to_string(), "2".to_string()]);
        assert_eq!(buffer.len(), 2);
    }

    #[test]
    fn buffer_capacity_zero_disables_buffering() {
        let buffer = ResponseBuffer::new(0);
        buffer.push(entry("1"));
        assert!(buffer.last(None).is_empty());
        assert!(buffer.is_empty());
    }

    #[test]
    fn buffer_last_caps_n_at_len() {
        let buffer = ResponseBuffer::new(5);
        buffer.push(entry("1"));
        buffer.push(entry("2"));
        assert_eq!(buffer.last(Some(1)).len(), 1);
        assert_eq!(buffer.last(Some(1))[0].request_id, "2");
        assert_eq!(buffer.last(Some(10)).len(), 2);
    }
}
