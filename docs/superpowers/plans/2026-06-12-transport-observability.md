# Transport Observability & Response Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every request/response observable end-to-end (correlated by JSON-RPC request id, including the actual stdout write), expose session counters via `check_mcp_status`, and keep a ring buffer of recent responses recoverable via a new `get_last_responses` tool.

**Architecture:** A decorator `InstrumentedTransport` wraps rmcp's `AsyncRwTransport` and feeds shared `SessionMetrics` + `ResponseBuffer` (all in new module `src/mcp/observability.rs`). Each `#[tool]` method becomes a thin logging wrapper around a private `*_impl` method so started/completed logging is symmetric on all paths. Spec: `docs/superpowers/specs/2026-06-12-transport-observability-design.md`.

**Tech Stack:** Rust nightly (edition 2024), rmcp 1.7 (`Transport<RoleServer>` trait, `RequestId` extractor), tokio, tracing, chrono, mockall (existing test mocks).

**Verified upstream facts** (rmcp 1.7.0 source, `~/.cargo/registry/.../rmcp-1.7.0/`):
- `Transport<R>` trait (`src/transport.rs:125`): `send()` returns a `Send + 'static` future; `AsyncRwTransport::send` does serialize+write+flush inside that future. `AsyncRwTransport::new_server(read, write)` exists; the `server` feature enables `transport-async-rw`.
- A type implementing `Transport` gets `IntoTransport` for free (identity adapter), so `self.serve(transport)` accepts our decorator.
- `RequestId = NumberOrString` (`model.rs:295`), implements `Display`. `FromContextPart` is implemented for `RequestId` (`handler/server/common.rs:189`) → `#[tool]` methods can take an `id: RequestId` parameter (not part of the advertised input schema).
- `TxJsonRpcMessage<RoleServer>` / `RxJsonRpcMessage<RoleServer>` are `JsonRpcMessage` enums with exactly 4 variants: `Request`, `Response`, `Notification`, `Error`. `JsonRpcError.id` is `Option<RequestId>`. `ClientRequest::method() -> &str` exists; `ClientRequest::CallToolRequest(r)` → tool name at `r.params.name` (`Cow<'static, str>`). `ServerResult::empty(())` builds an empty result for tests.

**Conventions reminders:** no `unwrap()` in production code (use `unwrap_or_else(PoisonError::into_inner)` for mutex locks); `expect()` allowed in tests; run `cargo fmt --all` after every code change; line length 100.

---

### Task 1: `ObservabilityConfig` (`[observability]` TOML table)

**Files:**
- Modify: `src/config.rs` (new struct + `Config` field + default fns)
- Modify: `src/config/tests.rs` (new tests + fix `create_test_config`)
- Modify: `config.example.toml` (documented commented-out section)

- [ ] **Step 1: Write the failing tests**

Append to `src/config/tests.rs`:

```rust
#[test]
fn test_observability_defaults_when_table_absent() {
    let config: Config = toml::from_str("[telegram]\napi_id = 12345\n").unwrap();
    assert_eq!(config.observability.slow_write_threshold_ms, 500);
    assert_eq!(config.observability.response_buffer_size, 10);
}

#[test]
fn test_observability_table_parsed() {
    let toml_str = r#"
[telegram]
api_id = 12345

[observability]
slow_write_threshold_ms = 250
response_buffer_size = 0
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.observability.slow_write_threshold_ms, 250);
    assert_eq!(config.observability.response_buffer_size, 0);
}

#[test]
fn test_observability_partial_table_fills_defaults() {
    let toml_str = "[telegram]\napi_id = 1\n\n[observability]\nresponse_buffer_size = 3\n";
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.observability.slow_write_threshold_ms, 500);
    assert_eq!(config.observability.response_buffer_size, 3);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test config -- --test-threads=1`
Expected: compile error — `no field `observability` on type `Config``

- [ ] **Step 3: Implement `ObservabilityConfig`**

In `src/config.rs`, next to the other `default_*` functions:

```rust
fn default_slow_write_threshold_ms() -> u64 {
    500
}

fn default_response_buffer_size() -> usize {
    10
}

fn default_observability_config() -> ObservabilityConfig {
    ObservabilityConfig::default()
}
```

Add the struct (near `TimeoutConfig`):

```rust
/// Transport observability settings (`[observability]` table).
///
/// Added after the 2026-06-12 lost-response incident (`docs/connetion-issue.md`)
/// so stdout write behavior is tunable in the field without a rebuild.
#[derive(Debug, Clone, Deserialize)]
pub struct ObservabilityConfig {
    /// WARN when a stdout write+flush exceeds this many milliseconds.
    /// 0 makes every write WARN (field diagnostic mode).
    #[serde(default = "default_slow_write_threshold_ms")]
    pub slow_write_threshold_ms: u64,

    /// Ring buffer capacity for `get_last_responses` (0 disables buffering).
    #[serde(default = "default_response_buffer_size")]
    pub response_buffer_size: usize,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            slow_write_threshold_ms: default_slow_write_threshold_ms(),
            response_buffer_size: default_response_buffer_size(),
        }
    }
}
```

Add the field to `Config`:

```rust
    #[serde(default = "default_observability_config")]
    pub observability: ObservabilityConfig,
```

Fix `create_test_config` in `src/config/tests.rs` — add to the `Config` literal:

```rust
        observability: default_observability_config(),
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo fmt --all && cargo test config -- --test-threads=1`
Expected: PASS (all config tests)

- [ ] **Step 5: Document in `config.example.toml`**

Append (matching the commented-out `[telegram.timeouts]` style):

```toml
# Transport observability. All fields optional.
# slow_write_threshold_ms: WARN when a stdout write+flush exceeds this (0 = warn on every write).
# response_buffer_size: responses kept for the get_last_responses recovery tool (0 = disabled).
# [observability]
# slow_write_threshold_ms = 500
# response_buffer_size = 10
```

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/config/tests.rs config.example.toml
git commit -m "feat(config): add [observability] table with slow-write threshold and response buffer size"
```

---

### Task 2: `SessionMetrics`

**Files:**
- Create: `src/mcp/observability.rs` (struct + inline `#[cfg(test)]` tests)
- Modify: `src/mcp.rs` (declare module)

- [ ] **Step 1: Declare the module**

In `src/mcp.rs` add:

```rust
pub mod observability;
```

- [ ] **Step 2: Write the failing tests**

Create `src/mcp/observability.rs` with the module header and tests first:

```rust
//! Transport-layer observability: session metrics, response ring buffer, and an
//! instrumented stdio transport decorator.
//!
//! Built after the 2026-06-12 incident (`docs/connetion-issue.md`): a tool response
//! was produced but lost between connector stdout and the client, and the logs could
//! not prove delivery. These types log every actual stdout write (request id, payload
//! size, write+flush duration), warn on blocked writes, and emit a session summary
//! when the input stream ends.

#[cfg(test)]
mod tests {
    use super::*;

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
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test mcp::observability`
Expected: compile error — `SessionMetrics` not found

- [ ] **Step 4: Implement `SessionMetrics`**

Above the test module in `src/mcp/observability.rs`:

```rust
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
        *self.last_write.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo fmt --all && cargo test mcp::observability`
Expected: 6 tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/mcp.rs src/mcp/observability.rs
git commit -m "feat(mcp): add SessionMetrics with request/response counters and in-flight tracking"
```

---

### Task 3: `ResponseBuffer`

**Files:**
- Modify: `src/mcp/observability.rs`

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests`:

```rust
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
        let ids: Vec<String> = buffer.last(None).into_iter().map(|e| e.request_id).collect();
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test mcp::observability`
Expected: compile error — `BufferedResponse` / `ResponseBuffer` not found

- [ ] **Step 3: Implement `ResponseBuffer`**

Add `use std::collections::VecDeque;` to the imports, then below `SessionMetrics`:

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo fmt --all && cargo test mcp::observability`
Expected: 10 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/mcp/observability.rs
git commit -m "feat(mcp): add ResponseBuffer ring buffer for recent serialized responses"
```

---

### Task 4: `InstrumentedTransport`

**Files:**
- Modify: `src/mcp/observability.rs`

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` (note: these are `#[tokio::test]`):

```rust
    use rmcp::RoleServer;
    use rmcp::model::{JsonRpcMessage, JsonRpcResponse, JsonRpcVersion2_0, RequestId, ServerResult};
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

        fn receive(
            &mut self,
        ) -> impl Future<Output = Option<RxJsonRpcMessage<RoleServer>>> + Send {
            let next = self.incoming.pop_front();
            async move { next }
        }

        fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
            async { Ok(()) }
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
```

Note: `send_error_propagates_and_skips_metrics` reaches into `transport.inner` — make the `inner` field `pub(crate)` (or add `#[cfg(test)]` accessor); `pub(crate)` is simplest.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test mcp::observability`
Expected: compile error — `InstrumentedTransport` / `is_slow_write` not found

- [ ] **Step 3: Implement `InstrumentedTransport`**

Extend the imports at the top of `src/mcp/observability.rs`:

```rust
use rmcp::RoleServer;
use rmcp::model::{ClientRequest, JsonRpcMessage};
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::Transport;
use std::sync::Arc;
use std::time::Duration;
```

(Merge with the existing `std::sync` / `std::time` imports; `Future` is in the
edition-2024 prelude.) Then add:

```rust
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
                    let total_ms = in_flight
                        .map(|request| request.received_at.elapsed().as_millis() as u64);
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

    fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleServer>>> + Send {
        async {
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
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.inner.close()
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo fmt --all && cargo test mcp::observability`
Expected: 16 tests PASS

- [ ] **Step 5: Run clippy early (new trait impl, easy to accumulate warnings)**

Run: `cargo clippy -- -D warnings`
Expected: clean

- [ ] **Step 6: Commit**

```bash
git add src/mcp/observability.rs
git commit -m "feat(mcp): add InstrumentedTransport decorator logging stdout writes with request ids"
```

---

### Task 5: Wire observability into `McpServer` and `run_stdio`

**Files:**
- Modify: `src/mcp/server.rs` (fields, constructor, builder, getters, `run_stdio`)

No new behavior tests here (IO wiring); existing tests must keep passing, clippy/fmt gate applies.

- [ ] **Step 1: Add fields and imports**

In `src/mcp/server.rs` add imports:

```rust
use crate::config::ObservabilityConfig;
use crate::mcp::observability::{InstrumentedTransport, ResponseBuffer, SessionMetrics};
use std::time::Duration;
```

Change the struct:

```rust
#[derive(Clone)]
pub struct McpServer<T: TelegramClientTrait, R: RateLimiterTrait> {
    telegram_client: Arc<T>,
    rate_limiter: Arc<R>,
    metrics: Arc<SessionMetrics>,
    response_buffer: Arc<ResponseBuffer>,
    slow_write_threshold: Duration,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}
```

- [ ] **Step 2: Update constructor, add builder + getters**

```rust
    pub fn new(telegram_client: Arc<T>, rate_limiter: Arc<R>) -> Self {
        let observability = ObservabilityConfig::default();
        Self {
            telegram_client,
            rate_limiter,
            metrics: Arc::new(SessionMetrics::new()),
            response_buffer: Arc::new(ResponseBuffer::new(observability.response_buffer_size)),
            slow_write_threshold: Duration::from_millis(observability.slow_write_threshold_ms),
            tool_router: Self::tool_router(),
        }
    }

    /// Apply `[observability]` settings (ring buffer capacity, slow-write threshold).
    pub fn with_observability(mut self, config: &ObservabilityConfig) -> Self {
        self.response_buffer = Arc::new(ResponseBuffer::new(config.response_buffer_size));
        self.slow_write_threshold = Duration::from_millis(config.slow_write_threshold_ms);
        self
    }

    /// Session metrics handle (shared with the transport; used for shutdown logging).
    pub fn metrics(&self) -> Arc<SessionMetrics> {
        Arc::clone(&self.metrics)
    }

    /// Response ring buffer handle (shared with the transport; used in tests).
    pub fn response_buffer(&self) -> Arc<ResponseBuffer> {
        Arc::clone(&self.response_buffer)
    }
```

- [ ] **Step 3: Instrument `run_stdio`**

Replace the body of `run_stdio`:

```rust
    pub async fn run_stdio(self) -> anyhow::Result<()> {
        use rmcp::transport::async_rw::AsyncRwTransport;
        use tokio::io::{stdin, stdout};

        let transport = InstrumentedTransport::new(
            AsyncRwTransport::new_server(stdin(), stdout()),
            Arc::clone(&self.metrics),
            Arc::clone(&self.response_buffer),
            self.slow_write_threshold,
        );

        let server = self.serve(transport).await?;
        server.waiting().await?;

        Ok(())
    }
```

- [ ] **Step 4: Verify build and existing tests**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test mcp`
Expected: PASS (no behavior change yet)

- [ ] **Step 5: Commit**

```bash
git add src/mcp/server.rs
git commit -m "feat(mcp): wire InstrumentedTransport, metrics and response buffer into McpServer"
```

---

### Task 6: Extend `StatusResponse` + split `check_mcp_status`

**Files:**
- Modify: `src/mcp/tools/types/responses.rs` (`StatusResponse` fields)
- Modify: `src/mcp/server.rs` (`check_mcp_status` wrapper/impl split, `log_tool_outcome` helper)
- Modify: `src/mcp/tests/status.rs` (new test + signature updates)

- [ ] **Step 1: Write the failing tests**

In `src/mcp/tests/status.rs`, add `use rmcp::model::RequestId;` to the imports, change both
existing calls from `server.check_mcp_status().await` to
`server.check_mcp_status(RequestId::Number(1)).await`, and append:

```rust
#[tokio::test]
async fn check_status_includes_session_counters() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_is_connected().return_once(|| true);
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_available_tokens().return_once(|| 10.0);

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let metrics = server.metrics();
    metrics.record_request("42", "search_messages");
    metrics.record_response_written("42");

    let result = server
        .check_mcp_status(RequestId::Number(1))
        .await
        .expect("status ok");
    let response: StatusResponse = serde_json::from_str(&result).expect("valid JSON");

    assert_eq!(response.requests_received, 1);
    assert_eq!(response.responses_written, 1);
    assert_eq!(response.last_response_write_age_secs, Some(0));
    assert!(chrono::DateTime::parse_from_rfc3339(&response.session_started_at).is_ok());
}

#[tokio::test]
async fn check_status_age_is_none_before_first_write() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_is_connected().return_once(|| true);
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_available_tokens().return_once(|| 10.0);

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .check_mcp_status(RequestId::Number(1))
        .await
        .expect("status ok");
    let response: StatusResponse = serde_json::from_str(&result).expect("valid JSON");

    assert_eq!(response.requests_received, 0);
    assert_eq!(response.responses_written, 0);
    assert_eq!(response.last_response_write_age_secs, None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test mcp::tests::status`
Expected: compile errors (unknown fields, wrong arity)

- [ ] **Step 3: Extend `StatusResponse`**

In `src/mcp/tools/types/responses.rs` append fields to `StatusResponse`:

```rust
    #[schemars(description = "Inbound JSON-RPC requests received this session")]
    pub requests_received: u64,

    #[schemars(description = "Responses successfully written to stdout this session")]
    pub responses_written: u64,

    #[schemars(description = "Seconds since the last successful response write (null before the first)")]
    pub last_response_write_age_secs: Option<u64>,

    #[schemars(description = "Session start time (RFC3339 UTC)")]
    pub session_started_at: String,

    #[schemars(description = "Session uptime in seconds")]
    pub session_uptime_secs: u64,
```

This breaks the `StatusResponse` literal in the inline test at the bottom of
`responses.rs` — extend it with plausible values
(`requests_received: 1, responses_written: 1, last_response_write_age_secs: Some(0), session_started_at: "2026-06-12T00:00:00+00:00".to_string(), session_uptime_secs: 60`).

- [ ] **Step 4: Split `check_mcp_status` and add the shared outcome logger**

In `src/mcp/server.rs`, add the import `use rmcp::model::RequestId;` and
`use std::time::Instant;` (merge with the `Duration` import). Add the free
function at module level (above the `#[cfg(test)]` line at the bottom):

```rust
/// Log the symmetric completion entry for a tool invocation.
fn log_tool_outcome(
    tool: &str,
    request_id: &str,
    started: Instant,
    result: &Result<String, String>,
) {
    let duration_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(_) => {
            tracing::info!(tool, request_id, duration_ms, "Tool invocation completed");
        }
        Err(error) => {
            tracing::warn!(
                tool,
                request_id,
                duration_ms,
                error = %error,
                "Tool invocation failed"
            );
        }
    }
}
```

Replace the `check_mcp_status` method in the `#[tool_router]` block:

```rust
    /// Tool 1: check_mcp_status - Health check and diagnostics
    #[tool(description = "Check MCP connection status, rate limiter state, and session counters")]
    pub async fn check_mcp_status(&self, id: RequestId) -> Result<String, String> {
        let request_id = id.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "check_mcp_status",
            request_id = %request_id,
            "Tool invocation started"
        );
        let result = self.check_mcp_status_impl().await;
        log_tool_outcome("check_mcp_status", &request_id, started, &result);
        result
    }
```

Contingency: rmcp's `#[tool]` macro resolves each non-`Parameters` argument through
`FromContextPart`, which is position-independent; if a build error nonetheless points
at the argument order, move `id: RequestId` before the `Parameters` argument (applies
to every tool in Tasks 6-8).

Add the impl method to the FIRST (non-router) `impl<T, R> McpServer<T, R>` block:

```rust
    async fn check_mcp_status_impl(&self) -> Result<String, String> {
        let connected = self.telegram_client.is_connected().await;
        let tokens = self.rate_limiter.available_tokens();

        let response = StatusResponse {
            telegram_connected: connected,
            rate_limiter_tokens: tokens,
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            requests_received: self.metrics.requests_received(),
            responses_written: self.metrics.responses_written(),
            last_response_write_age_secs: self.metrics.last_write_age_secs(),
            session_started_at: self.metrics.session_started_at_rfc3339(),
            session_uptime_secs: self.metrics.uptime_secs(),
        };

        serde_json::to_string(&response).map_err(|e| e.to_string())
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo fmt --all && cargo test mcp::tests::status && cargo test types`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/mcp/server.rs src/mcp/tools/types/responses.rs src/mcp/tests/status.rs
git commit -m "feat(mcp): expose session counters in check_mcp_status for zombie-session visibility"
```

---

### Task 7: Wrapper/impl split + `RequestId` for the remaining 7 tools

**Files:**
- Modify: `src/mcp/server.rs` (7 tools)
- Modify: `src/mcp/tests/{channels,links,search,history,message_by_link,server_core}.rs` (call sites)

The transformation is identical for each tool. For every tool below:
1. Rename the existing method to `<tool>_impl`, **move it to the first (non-router) impl block**, remove the `#[tool(...)]` attribute, make it non-`pub`, and delete the old `tracing::info!(... "Tool invocation started")` block from its body (the wrapper logs it now). The signature changes from `Parameters(request): Parameters<X>` to plain `request: X`.
2. Add the new wrapper with the original name in the `#[tool_router]` block.

- [ ] **Step 1: Update test call sites (the failing "tests")**

In each file under `src/mcp/tests/` add `use rmcp::model::RequestId;` and append
`RequestId::Number(1)` as the **last argument** of every tool-method call. Examples
of the pattern (apply to ALL call sites in all six files):

```rust
// before
let result = server.get_subscribed_channels(Parameters(request)).await;
// after
let result = server
    .get_subscribed_channels(Parameters(request), RequestId::Number(1))
    .await;
```

Run: `cargo test mcp` → expected: compile errors (wrong arity) — this is the red state.

- [ ] **Step 2: Tool 2 `get_subscribed_channels` wrapper**

```rust
    /// Tool 2: get_subscribed_channels - List user's Telegram channels with pagination
    #[tool(description = "List user's subscribed Telegram channels with pagination support")]
    pub async fn get_subscribed_channels(
        &self,
        Parameters(request): Parameters<GetChannelsRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let request_id = id.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "get_subscribed_channels",
            request_id = %request_id,
            limit = ?request.limit,
            offset = ?request.offset,
            "Tool invocation started"
        );
        let result = self.get_subscribed_channels_impl(request).await;
        log_tool_outcome("get_subscribed_channels", &request_id, started, &result);
        result
    }
```

`get_subscribed_channels_impl(&self, request: GetChannelsRequest)` keeps the body
(starting at `let limit = request.limit.unwrap_or(20);`), minus the old log call.

- [ ] **Step 3: Tool 3 `get_channel_info` wrapper**

```rust
    /// Tool 3: get_channel_info - Get detailed information about a Telegram channel
    #[tool(description = "Get detailed information about a Telegram channel by username or ID")]
    pub async fn get_channel_info(
        &self,
        Parameters(request): Parameters<GetChannelInfoRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let request_id = id.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "get_channel_info",
            request_id = %request_id,
            channel_identifier = %request.channel_identifier,
            "Tool invocation started"
        );
        let result = self.get_channel_info_impl(request).await;
        log_tool_outcome("get_channel_info", &request_id, started, &result);
        result
    }
```

- [ ] **Step 4: Tool 4 `generate_message_link` wrapper**

```rust
    /// Tool 4: generate_message_link - Generate deep links for a Telegram message
    #[tool(description = "Generate tg:// and https://t.me deep links for a Telegram message")]
    pub async fn generate_message_link(
        &self,
        Parameters(request): Parameters<GenerateLinkRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let request_id = id.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "generate_message_link",
            request_id = %request_id,
            channel_id = %request.channel_id,
            message_id = request.message_id,
            include_tg_protocol = ?request.include_tg_protocol,
            "Tool invocation started"
        );
        let result = self.generate_message_link_impl(request).await;
        log_tool_outcome("generate_message_link", &request_id, started, &result);
        result
    }
```

- [ ] **Step 5: Tool 5 `open_message_in_telegram` wrapper**

```rust
    /// Tool 5: open_message_in_telegram - Open message in Telegram Desktop (macOS)
    #[tool(description = "Open a specific message in Telegram Desktop application (macOS only)")]
    pub async fn open_message_in_telegram(
        &self,
        Parameters(request): Parameters<OpenMessageRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let request_id = id.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "open_message_in_telegram",
            request_id = %request_id,
            channel_id = %request.channel_id,
            message_id = request.message_id,
            use_tg_protocol = ?request.use_tg_protocol,
            "Tool invocation started"
        );
        let result = self.open_message_in_telegram_impl(request).await;
        log_tool_outcome("open_message_in_telegram", &request_id, started, &result);
        result
    }
```

- [ ] **Step 6: Tool 6 `search_messages` wrapper**

```rust
    /// Tool 6: search_messages - Search messages across Telegram channels
    #[tool(
        description = "Search messages across subscribed Telegram channels with optional filters"
    )]
    pub async fn search_messages(
        &self,
        Parameters(request): Parameters<SearchRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let request_id = id.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "search_messages",
            request_id = %request_id,
            query = %request.query,
            channel_id = ?request.channel_id,
            hours_back = ?request.hours_back,
            limit = ?request.limit,
            media_filter = ?request.media_filter,
            "Tool invocation started"
        );
        let result = self.search_messages_impl(request).await;
        log_tool_outcome("search_messages", &request_id, started, &result);
        result
    }
```

In `search_messages_impl`, rename the in-body completion message
`"Search completed"` → `"Search results"` (it is now a domain-detail entry; the
symmetric completion entry comes from the wrapper).

- [ ] **Step 7: Tool 7 `get_recent_messages` wrapper**

```rust
    /// Tool 7: get_recent_messages - Get recent messages from a channel by time window
    #[tool(
        description = "Get recent messages from a specific channel by time window (no search query needed)"
    )]
    pub async fn get_recent_messages(
        &self,
        Parameters(request): Parameters<GetRecentMessagesRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let request_id = id.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "get_recent_messages",
            request_id = %request_id,
            channel_id = %request.channel_id,
            hours_back = ?request.hours_back,
            limit = ?request.limit,
            media_filter = ?request.media_filter,
            "Tool invocation started"
        );
        let result = self.get_recent_messages_impl(request).await;
        log_tool_outcome("get_recent_messages", &request_id, started, &result);
        result
    }
```

In `get_recent_messages_impl`, rename `"Get recent messages completed"` →
`"Recent messages results"`.

- [ ] **Step 8: Tool 8 `get_message_by_link` wrapper**

```rust
    /// Tool 8: get_message_by_link - Get a specific message by its t.me link
    #[tool(
        description = "Get a specific Telegram message by its t.me link (e.g. https://t.me/swodki/575403)"
    )]
    pub async fn get_message_by_link(
        &self,
        Parameters(request): Parameters<GetMessageByLinkRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let request_id = id.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "get_message_by_link",
            request_id = %request_id,
            link = %request.link,
            "Tool invocation started"
        );
        let result = self.get_message_by_link_impl(request).await;
        log_tool_outcome("get_message_by_link", &request_id, started, &result);
        result
    }
```

In `get_message_by_link_impl`, rename `"Get message by link completed"` →
`"Message by link results"`.

- [ ] **Step 9: Run the full test suite**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: PASS

- [ ] **Step 10: Commit**

```bash
git add src/mcp/server.rs src/mcp/tests/
git commit -m "feat(mcp): symmetric request-id-correlated started/completed logging for all tools"
```

---

### Task 8: `get_last_responses` tool

**Files:**
- Modify: `src/mcp/tools/types/requests.rs` (`GetLastResponsesRequest`)
- Modify: `src/mcp/tools/types/responses.rs` (`BufferedResponseEntry`, `LastResponsesResponse`)
- Modify: `src/mcp/tools/types.rs` (re-exports)
- Modify: `src/mcp/server.rs` (tool 9)
- Create: `src/mcp/tests/last_responses.rs`
- Modify: `src/mcp/tests.rs` (declare test module)

- [ ] **Step 1: Write the failing tests**

Create `src/mcp/tests/last_responses.rs`:

```rust
//! Tests for get_last_responses tool

use crate::mcp::observability::BufferedResponse;
use crate::mcp::server::McpServer;
use crate::mcp::tools::{GetLastResponsesRequest, LastResponsesResponse};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::RequestId;
use std::sync::Arc;
use std::time::SystemTime;

fn make_server() -> McpServer<MockTelegramClientTrait, MockRateLimiterTrait> {
    McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    )
}

fn buffered(id: &str, payload: &str) -> BufferedResponse {
    BufferedResponse {
        request_id: id.to_string(),
        tool_name: "search_messages".to_string(),
        written_at: SystemTime::now(),
        size_bytes: payload.len(),
        payload: payload.to_string(),
    }
}

#[tokio::test]
async fn empty_buffer_returns_empty_list() {
    let server = make_server();
    let result = server
        .get_last_responses(
            Parameters(GetLastResponsesRequest { n: None }),
            RequestId::Number(1),
        )
        .await
        .expect("tool ok");
    let response: LastResponsesResponse = serde_json::from_str(&result).expect("valid JSON");
    assert!(response.responses.is_empty());
    assert_eq!(response.buffered, 0);
}

#[tokio::test]
async fn returns_buffered_responses_newest_first_with_parsed_payload() {
    let server = make_server();
    server
        .response_buffer()
        .push(buffered("7", r#"{"jsonrpc":"2.0","id":7,"result":{}}"#));
    server
        .response_buffer()
        .push(buffered("8", r#"{"jsonrpc":"2.0","id":8,"result":{}}"#));

    let result = server
        .get_last_responses(
            Parameters(GetLastResponsesRequest { n: None }),
            RequestId::Number(1),
        )
        .await
        .expect("tool ok");
    let response: LastResponsesResponse = serde_json::from_str(&result).expect("valid JSON");

    assert_eq!(response.buffered, 2);
    assert_eq!(response.responses.len(), 2);
    assert_eq!(response.responses[0].request_id, "8");
    // Payload is embedded as real JSON, not a double-encoded string.
    assert_eq!(response.responses[0].response["id"], 8);
    assert!(
        chrono::DateTime::parse_from_rfc3339(&response.responses[0].written_at).is_ok()
    );
}

#[tokio::test]
async fn n_caps_returned_entries_but_reports_total_buffered() {
    let server = make_server();
    for i in 1..=3 {
        server.response_buffer().push(buffered(
            &i.to_string(),
            r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
        ));
    }

    let result = server
        .get_last_responses(
            Parameters(GetLastResponsesRequest { n: Some(2) }),
            RequestId::Number(1),
        )
        .await
        .expect("tool ok");
    let response: LastResponsesResponse = serde_json::from_str(&result).expect("valid JSON");

    assert_eq!(response.responses.len(), 2);
    assert_eq!(response.responses[0].request_id, "3");
    assert_eq!(response.buffered, 3);
}
```

Declare in `src/mcp/tests.rs` (alphabetical position):

```rust
#[path = "tests/last_responses.rs"]
mod last_responses;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test mcp::tests::last_responses`
Expected: compile errors (missing types/method)

- [ ] **Step 3: Add the request type**

In `src/mcp/tools/types/requests.rs`:

```rust
/// Request for get_last_responses tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetLastResponsesRequest {
    #[schemars(description = "How many recent responses to return (default: all buffered)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub n: Option<u32>,
}
```

- [ ] **Step 4: Add the response types**

In `src/mcp/tools/types/responses.rs`:

```rust
/// One recovered response returned by get_last_responses
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BufferedResponseEntry {
    #[schemars(description = "JSON-RPC request id of the original call")]
    pub request_id: String,

    #[schemars(description = "Tool that produced the response")]
    pub tool_name: String,

    #[schemars(description = "When the response was written to stdout (RFC3339 UTC)")]
    pub written_at: String,

    #[schemars(description = "Serialized payload size in bytes")]
    pub size_bytes: usize,

    #[schemars(description = "The JSON-RPC envelope exactly as written to stdout")]
    pub response: serde_json::Value,
}

/// Response for get_last_responses tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LastResponsesResponse {
    #[schemars(description = "Recovered responses, newest first")]
    pub responses: Vec<BufferedResponseEntry>,

    #[schemars(description = "Total responses currently buffered")]
    pub buffered: usize,
}
```

- [ ] **Step 5: Extend re-exports**

In `src/mcp/tools/types.rs` add `GetLastResponsesRequest` to the `requests::` re-export
list and `BufferedResponseEntry, LastResponsesResponse` to the `responses::` list.

- [ ] **Step 6: Implement tool 9**

In `src/mcp/server.rs`: add `BufferedResponseEntry, GetLastResponsesRequest,
LastResponsesResponse` to the existing `use crate::mcp::tools::{...}` import list
(all three are re-exported through `tools::types`). In the `#[tool_router]` block:

```rust
    /// Tool 9: get_last_responses - Recover recently written responses
    #[tool(
        description = "Debug/recovery: return the last N tool responses written to stdout, so a response lost in transit can be re-fetched without re-querying Telegram or spending rate-limit budget"
    )]
    pub async fn get_last_responses(
        &self,
        Parameters(request): Parameters<GetLastResponsesRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let request_id = id.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "get_last_responses",
            request_id = %request_id,
            n = ?request.n,
            "Tool invocation started"
        );
        let result = self.get_last_responses_impl(request).await;
        log_tool_outcome("get_last_responses", &request_id, started, &result);
        result
    }
```

In the first (non-router) impl block:

```rust
    async fn get_last_responses_impl(
        &self,
        request: GetLastResponsesRequest,
    ) -> Result<String, String> {
        let entries = self.response_buffer.last(request.n.map(|n| n as usize));
        let responses: Vec<BufferedResponseEntry> = entries
            .into_iter()
            .map(|entry| BufferedResponseEntry {
                request_id: entry.request_id,
                tool_name: entry.tool_name,
                written_at: chrono::DateTime::<chrono::Utc>::from(entry.written_at)
                    .to_rfc3339(),
                size_bytes: entry.size_bytes,
                // Payload was valid JSON when written; Null only on corruption.
                response: serde_json::from_str(&entry.payload)
                    .unwrap_or(serde_json::Value::Null),
            })
            .collect();

        let response = LastResponsesResponse {
            buffered: self.response_buffer.len(),
            responses,
        };

        serde_json::to_string(&response).map_err(|e| e.to_string())
    }
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo fmt --all && cargo test mcp::tests::last_responses && cargo test`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add src/mcp/server.rs src/mcp/tools/types.rs src/mcp/tools/types/requests.rs \
        src/mcp/tools/types/responses.rs src/mcp/tests.rs src/mcp/tests/last_responses.rs
git commit -m "feat(mcp): add get_last_responses recovery tool backed by the response ring buffer"
```

---

### Task 9: `main.rs` wiring + `lib.rs` export

**Files:**
- Modify: `src/main.rs` (`run_mcp_server`)
- Modify: `src/lib.rs` (export `ObservabilityConfig`)

- [ ] **Step 1: Apply config and add the signal-shutdown summary**

In `src/main.rs` `run_mcp_server`, replace the server construction and select block:

```rust
    // Create MCP server
    let server = McpServer::new(Arc::new(telegram_client), Arc::new(rate_limiter))
        .with_observability(&config.observability);

    // Metrics handle survives the move of `server` into run_stdio
    let metrics = server.metrics();
```

and in the `tokio::select!` shutdown branch, after the "Initiating graceful shutdown" log:

```rust
        _ = shutdown_rx => {
            tracing::info!("Initiating graceful shutdown (timeout: {}s)...", shutdown_timeout);
            metrics.log_summary("shutdown signal");
        }
```

- [ ] **Step 2: Export the config type**

In `src/lib.rs`:

```rust
pub use config::{Config, ObservabilityConfig, ServerConfig};
```

- [ ] **Step 3: Verify the binary builds and runs the gate**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo build && cargo test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/main.rs src/lib.rs
git commit -m "feat: apply [observability] config and log session summary on signal shutdown"
```

---

### Task 10: Documentation + final gate

**Files:**
- Modify: `CLAUDE.md` (tool count 8 → 9, mention observability module)
- Modify: `CHANGELOG.md` (`[Unreleased]` entries)

- [ ] **Step 1: Update CLAUDE.md**

- Architecture diagram line: `src/mcp/server.rs (8 tools)` → `src/mcp/server.rs (9 tools)`;
  add `observability` to the MCP server layer line
  (`src/mcp/tools/ (helpers + types/...)` → also `src/mcp/observability.rs`).
- Key Patterns, rmcp tool macros: "All 8 tools" → "All 9 tools". Add one sentence:
  each `#[tool]` method is a logging wrapper (request-id-correlated started/completed)
  around a private `*_impl` method; the stdio transport is wrapped by
  `InstrumentedTransport` (`src/mcp/observability.rs`), which logs every stdout write
  and feeds `SessionMetrics`/`ResponseBuffer` (`[observability]` config table).

- [ ] **Step 2: Update CHANGELOG.md under `[Unreleased]`**

```markdown
### Added
- `[observability]` config table (`slow_write_threshold_ms`, default 500; `response_buffer_size`, default 10) and an instrumented stdio transport: every response write to stdout is logged with the JSON-RPC request id, tool name, payload size and write+flush duration; writes slower than the threshold emit a WARN (a stalling pipe means the peer stopped reading); stdin EOF and signal shutdown log a session summary (uptime, request/response counters, age of last write, abandoned in-flight requests). Built after the 2026-06-12 lost-response incident (`docs/connetion-issue.md`).
- `check_mcp_status` now reports `requests_received`, `responses_written`, `last_response_write_age_secs`, `session_started_at`, and `session_uptime_secs`, making a zombie bridge session visible from the client side.
- New `get_last_responses` debug/recovery tool (tool 9): returns the last N responses written to stdout from an in-memory ring buffer, so a response lost in transit can be re-fetched without re-querying Telegram or spending rate-limit budget.

### Changed
- All MCP tools now emit symmetric `Tool invocation started` / `Tool invocation completed` / `Tool invocation failed` log entries correlated by JSON-RPC `request_id` and carrying `duration_ms` (previously 5 of 8 tools logged only `started`, and no entry carried the request id).
```

- [ ] **Step 3: Run the full pre-merge gate**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all PASS

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md CHANGELOG.md
git commit -m "docs: document transport observability, session counters, and get_last_responses tool"
```

---

## Verification checklist (after all tasks)

- [ ] `cargo fmt --check && cargo clippy -- -D warnings && cargo test` — the pre-merge gate
- [ ] `cargo run --bin telegram-mcp` starts; a manual `tools/list` over stdio shows 9 tools
- [ ] Log file shows the chain for one call: `Request received` → `Tool invocation started` → `Tool invocation completed` → `Response written to stdout`, all with the same `request_id`
- [ ] Use superpowers:requesting-code-review before merging the branch
