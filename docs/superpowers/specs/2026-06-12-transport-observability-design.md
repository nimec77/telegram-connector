# Design: Transport Observability & Response Recovery

## Summary

During the 2026-06-12 incident (`docs/connetion-issue.md`), a tool response was
produced by the connector in 546ms but never reached the client, and a
subsequent bridge session silently dropped every inbound request for ~24
minutes. The connector's logs could not distinguish "connector never answered"
from "answer was lost downstream" because the connector has **zero visibility
into its own stdout write path**: `run_stdio` hands `(stdin, stdout)` straight
to rmcp's `serve()`, and no log line anywhere carries the JSON-RPC request id.

This change adds transport-layer observability (correlated request/response
logging, write timing, blocked-write detection, shutdown context), exposes
session counters through `check_mcp_status` so a zombie bridge session is
visible from the client side, and keeps a small ring buffer of serialized
responses so a response lost in transit can be re-fetched without re-hitting
the Telegram API.

Out of scope (per the incident doc's non-goals): flood-wait-specific changes,
and anything in the Claude Desktop bridge / Anthropic backend.

## Approach

**Transport decorator (selected).** rmcp 1.7's `Transport<RoleServer>` trait is
public, and the stdio transport (`AsyncRwTransport`) performs serialize + write
+ flush inside the future returned by `send()`. A decorator transport therefore
sees structured JSON-RPC messages (request ids, tool names) on both directions
and can time the real pipe write.

### Approaches considered

- **A — Transport decorator (selected):** wraps `AsyncRwTransport`. Sees
  structured messages → exact request-id/tool-name correlation; timing the
  inner `send()` future measures the actual write+flush. Testable with a fake
  inner transport. Cost: one extra serialization per response to measure
  payload bytes.
- **B — Instrumented `AsyncWrite` around stdout:** exact bytes with no double
  serialization, but no request-id/tool-name correlation without re-parsing the
  byte stream, and timing fragments across `poll_write`/`poll_flush`. Rejected.
- **C — Custom stdio loop replacing rmcp's transport:** maximum control but
  reimplements rmcp internals; ongoing maintenance burden on top of an already
  fast-moving `grammers` git dependency. Rejected.

## Components

All three new units live in one new module, `src/mcp/observability.rs`
(file-as-module, declared in `src/mcp.rs`).

### 1. `SessionMetrics` (shared via `Arc`)

| Field | Type | Purpose |
|-------|------|---------|
| `session_started_at` | `SystemTime` | wall-clock start for status reporting (RFC3339) |
| `started_instant` | `Instant` | monotonic base for uptime |
| `requests_received` | `AtomicU64` | incremented on every inbound JSON-RPC request |
| `responses_written` | `AtomicU64` | incremented on every successful response write |
| `last_write` | `Mutex<Option<Instant>>` | timestamp of last successful write |
| `in_flight` | `Mutex<HashMap<String, InFlightRequest>>` | request id → `{tool_name, received_at}` |

- Request ids are keyed as strings (`RequestId::to_string()`); rmcp's
  `RequestId` is `NumberOrString`, string keys sidestep Hash/Eq concerns.
- `InFlightRequest.tool_name` is the tool name from `CallToolRequest` params,
  or the JSON-RPC method name for non-tool requests.
- `log_summary(reason: &str)` emits one INFO line with: session uptime (secs),
  requests received, responses written, seconds since last successful write,
  and the ids + tool names of abandoned in-flight requests. Called from two
  places: transport EOF (reason `"input stream terminated"`) and signal
  shutdown in `main.rs` (reason `"shutdown signal"`).

### 2. `ResponseBuffer` (shared via `Arc`)

`Mutex<VecDeque<BufferedResponse>>`, capacity = `response_buffer_size` from
config (0 disables buffering entirely).

`BufferedResponse`: `request_id: String`, `tool_name: String`, `written_at:
SystemTime`, `size_bytes: usize`, `payload: String` (the full serialized
JSON-RPC envelope exactly as written to stdout).

- Push evicts the oldest entry when full.
- Responses to `get_last_responses` itself are never stored (prevents the
  recovery tool from evicting the data it exists to recover).
- Worst case memory at default capacity 10 ≈ a few MB; acceptable.

### 3. `InstrumentedTransport<T: Transport<RoleServer>>`

Decorator holding `inner: T`, `Arc<SessionMetrics>`, `Arc<ResponseBuffer>`,
`slow_write_threshold: Duration`.

**`receive()`** — await inner:
- `Some(request)` → increment `requests_received`, insert into in-flight map,
  DEBUG-log `request received {request_id, method, tool}`.
- Notifications pass through uncounted (no id).
- `None` (stdin EOF) → `metrics.log_summary("input stream terminated")`, then
  return `None` (rmcp's own `input stream terminated` INFO follows).

**`send(item)`** — before constructing the returned future: extract the
request id (Response/Error variants), serialize `item` once with `serde_json`
for the exact payload size and ring-buffer copy. The future then times the
inner `send()` (the real serialize+write+flush) and on completion:
- success → increment `responses_written`, set `last_write`, remove from
  in-flight (yields tool name + total handling duration), push to ring buffer,
  INFO-log `response written {request_id, tool, bytes, write_ms,
  total_ms}`.
- `write_ms > slow_write_threshold_ms` → additional WARN `slow stdout write
  {request_id, write_ms, threshold_ms}` — the smoking-gun signature of a peer
  that stopped reading.
- failure → ERROR `response write failed {request_id, error}`; the inner error
  is propagated untouched.

**Robustness rule:** instrumentation never fails the message path.
Serialization-for-size or buffer errors are logged and swallowed; the inner
transport's result is always returned as-is. JSON-RPC batch variants are
handled best-effort (counted; ids extracted when present). `close()` is a
passthrough.

## Wiring

- `McpServer` gains `metrics: Arc<SessionMetrics>`, `response_buffer:
  Arc<ResponseBuffer>`, and the observability settings. `new()` keeps its
  current two-argument signature (defaults — existing tests unchanged);
  `main.rs` applies a builder-style `with_observability(cfg)`.
- `run_stdio` builds `AsyncRwTransport::new_server(stdin(), stdout())`, wraps
  it in `InstrumentedTransport`, and passes that to `self.serve(...)`.
- `main.rs` signal-shutdown path calls `metrics.log_summary("shutdown
  signal")` via a getter on `McpServer`.

## Tool-level symmetric logging

Every tool method gains an `id: RequestId` parameter — rmcp 1.7 implements
`FromContextPart` for `RequestId` (verified in
`rmcp-1.7.0/src/handler/server/common.rs:189`), so the `#[tool]` macro injects
it; the advertised tool schema is unchanged.

Each `#[tool]` method becomes a thin wrapper around a private `*_impl` method:

```rust
#[tool(description = "...")]
pub async fn search_messages(
    &self,
    id: RequestId,
    Parameters(request): Parameters<SearchRequest>,
) -> Result<String, String> {
    let request_id = id.to_string();
    let started = Instant::now();
    tracing::info!(tool = "search_messages", request_id = %request_id,
                   /* domain fields */, "Tool invocation started");
    let result = self.search_messages_impl(request).await;
    match &result {
        Ok(_) => tracing::info!(tool = "search_messages", request_id = %request_id,
            duration_ms = started.elapsed().as_millis() as u64,
            "Tool invocation completed"),
        Err(e) => tracing::warn!(tool = "search_messages", request_id = %request_id,
            duration_ms = started.elapsed().as_millis() as u64,
            error = %e, "Tool invocation failed"),
    }
    result
}
```

This guarantees started/completed symmetry on **all** paths, including early
`?` returns — which the current inline style cannot (5 of 8 tools currently
log only `started`: `check_mcp_status`, `get_subscribed_channels`,
`get_channel_info`, `generate_message_link`, `open_message_in_telegram`).

Log message strings are standardized to `"Tool invocation started"` /
`"Tool invocation completed"` / `"Tool invocation failed"`, always with `tool`
and `request_id` fields. Domain detail fields (query, channel_id, result
counts, `search_time_ms`, …) stay on the started entry as today. The existing
bespoke completion messages (`"Search completed"`, etc.) keep their rich result
fields but are renamed to domain-detail form (`"Search results"`, `"Recent
messages results"`, `"Message by link results"`) — they live inside the `*_impl`
bodies where the result data is available, while the single symmetric
`"Tool invocation completed"` entry always comes from the wrapper. A grep for
`Tool invocation completed` therefore returns exactly one line per successful
invocation.

The correlation chain for one request becomes:
`request received` → `Tool invocation started` → `Tool invocation
completed`/`failed` → `response written` — all sharing `request_id`.

## `check_mcp_status` extension

`StatusResponse` (in `src/mcp/tools/types/responses.rs`) gains:

| Field | Type | Meaning |
|-------|------|---------|
| `requests_received` | `u64` | inbound JSON-RPC requests this session |
| `responses_written` | `u64` | responses successfully written to stdout |
| `last_response_write_age_secs` | `Option<u64>` | `None` until first write |
| `session_started_at` | `String` | RFC3339 UTC |
| `session_uptime_secs` | `u64` | monotonic uptime |

A bridge session that routes nothing is instantly visible: `requests_received`
stays flat while the client knows it sent calls.

## New tool 9: `get_last_responses`

- Request: `GetLastResponsesRequest { n: Option<u32> }` with
  `flexible_opt_u32` coercion, matching the existing request-struct
  conventions. Default and upper bound: the whole buffer.
- Response: most-recent-first array of `{request_id, tool_name, written_at
  (RFC3339), size_bytes, response}` where `response` is the stored envelope
  embedded as real JSON, not a double-encoded string (the stored payload string
  is re-parsed into `serde_json::Value` at read time — at most N≤10 small
  documents, so the re-parse cost is negligible and avoids enabling
  serde_json's `raw_value` feature).
- Empty/disabled buffer → empty array (not an error).
- Recovery flow: client sees a timeout → calls `get_last_responses(1)` → gets
  the lost payload without re-hitting Telegram or spending rate budget.

## Configuration

New `[observability]` TOML table, mirroring the `TimeoutConfig` pattern
(`#[serde(default)]` on every field, defaults via `default_*` functions):

```toml
[observability]
slow_write_threshold_ms = 500   # WARN when a stdout write+flush exceeds this
response_buffer_size = 10       # ring buffer entries; 0 disables the buffer
```

`ObservabilityConfig { slow_write_threshold_ms: u64, response_buffer_size:
usize }` added to `Config` with `#[serde(default)]`. A threshold of 0 makes
every write WARN (useful as a field diagnostic mode); no validation needed.

## Testing (TDD throughout)

- **`SessionMetrics`**: counter increments; in-flight insert/remove lifecycle;
  abandoned-request listing; `last_response_write_age_secs` math; RFC3339
  formatting.
- **`ResponseBuffer`**: eviction order; capacity 0 = no-op; most-recent-first
  retrieval; `get_last_responses` self-exclusion.
- **`InstrumentedTransport`**: against a channel-backed fake inner
  `Transport<RoleServer>` — request counting + in-flight capture on receive;
  response counting, in-flight clearing, buffering on send; error propagation
  from inner send; EOF triggers summary. Slow-write classification tested as a
  pure threshold predicate (log-line content is not asserted).
- **Config**: `[observability]` parsing + defaults in `config/tests.rs`
  (serial, as required).
- **Tools**: `StatusResponse` new fields; `get_last_responses` (empty buffer,
  n-capping, payload integrity through RawValue); all existing tool tests
  updated to pass a `RequestId` (mechanical).

## Files touched

| File | Change |
|------|--------|
| `src/mcp/observability.rs` | new — `SessionMetrics`, `ResponseBuffer`, `InstrumentedTransport` |
| `src/mcp.rs` | declare `observability` module |
| `src/mcp/server.rs` | wrapper/`*_impl` split for 8 tools, `RequestId` params, tool 9, wiring in `run_stdio` |
| `src/mcp/tools/types/requests.rs` | `GetLastResponsesRequest` |
| `src/mcp/tools/types/responses.rs` | `StatusResponse` fields, `LastResponsesResponse` |
| `src/config.rs` (+ `config/tests.rs`) | `ObservabilityConfig`, `[observability]` table |
| `src/main.rs` | `with_observability(cfg)`, shutdown summary log |
| `src/mcp/tests/*.rs` | updated call sites + new tests |
| `src/lib.rs` | export new public types |
