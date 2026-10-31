# Stdio runtime contract

The connector tests and advertises MCP 2026-07-28 and legacy 2025-11-25. Modern
clients can send tools/list or tools/call first, without discovery or initialize.
Every modern request supplies protocolVersion and clientCapabilities under the
`io.modelcontextprotocol/` metadata prefix. Client identity is optional and never
inherited from another modern request. Legacy clients explicitly initialize at
2025-11-25 and send notifications/initialized.

Modern complete tool/list results identify the server in result metadata. Tool
lists retain one-hour/private caching hints; discovery uses zero/private hints.
Optional extensions, elicitation and changing tool lists remain unadvertised.
Empty resource/prompt listings retain the SDK's harmless defaults; no product
resource or prompt handlers have been added.

## Limits and errors

The `[server]` settings apply to the production and injectable I/O paths:

| Setting | Default | Accepted range |
| --- | --- | --- |
| max_request_bytes | 1048576 | 1..=16777216 |
| max_in_flight_requests | 16 | 1..=1024 |
| shutdown_timeout_seconds | 5 | 0..=3600 |

Input size includes the frame's newline. Oversized frames are discarded through
that newline and return -32600; later valid frames can still be processed. This
limit does not constrain output images or recovery responses. Invalid JSON returns
-32700; malformed envelopes return -32600 with a readable valid ID when possible.
Unknown methods return -32601; unknown tools return -32602. Typed tool argument
failures retain the SDK's isError tool-result behavior.

Admission is nonwaiting. An overload or draining rejection returns application
code 1001 before tool dispatch or account token acquisition. Numeric and string
request IDs have distinct accounting identities. Up to 16 protocol-error writes
can wait behind stdout backpressure, so cancellations remain readable; exceeding
that error queue closes the connection with a transport error. Admission state and the protocol-error write queue have finite bounds.

## Cancellation, progress and shutdown

Cancellation drops the operation future and releases its accounting without
counting a delivered response. Progress requires the caller's token and reports
strictly increasing phase counts using static messages without message/media
content. Progress makes one nonblocking enqueue attempt into the SDK’s bounded peer queue;
saturated queues drop optional updates without spending the application deadline. Direct Rust tool-method calls have
no wire progress context. Queued results/progress are suppressed after cancellation,
and completion closes the notification gate before releasing the writer. A frame
whose bytes have already started writing is committed and finishes if cancellation
races; clients must handle that protocol-defined delivery race. Only transport
shutdown interrupts a partially written frame, after which the connection ends.

An owned 8 KiB reader pump detects physical EOF even while the SDK is awaiting a
bootstrap response write. EOF starts one absolute configured drain deadline;
complete frames already buffered remain admissible until consumed EOF. OS signals
close admission immediately without restarting an existing EOF deadline.
Accepted operations and output writes may finish within it. Idle shutdown is
immediate. Deadline expiry cancels remaining operation futures and pending writes;
SDK cleanup and the input-pump task are awaited. Signal registration and transport failures propagate after
cleanup. The fixed 100 ms shutdown sleep is gone.

Image processing runs off the async executor with two process-wide permits.
A permit stays inside each started blocking closure until it finishes, even if its
request is cancelled. Cancellation stops scheduling further work; it cannot forcibly
abort a started CPU job. The CLI explicitly owns Tokio and releases the runtime after
protocol cleanup without indefinitely waiting for a platform blocking stdin read.
Library embedders must likewise own their runtime/process termination policy.

Bootstrap discover/initialize handlers must remain local and nonblocking. The
production handlers perform no Telegram RPCs. Custom asynchronous bootstrap
extensions are not a supported runtime guarantee: retaining the SDK bootstrap
protects legacy negotiation and request isolation instead of duplicating them.
The input pump does cover actual product bootstrap output blocked at EOF.

## Offline verification and probe

`src/mcp/tests/protocol_stdio.rs` and sibling protocol test files drive raw NDJSON
through the same runtime, framing and instrumentation as production. They use mocks
or test-only protocol handlers, never Telegram credentials. The extra handlers used
to verify MRTR and subscriptions do not change advertised production capabilities.

The `offline_mcp_fixture` example exposes deterministic echo/wait tools and shares
production runtime/signal handling. It reads no Telegram configuration or session.

```sh
cargo build --locked --example offline_mcp_fixture
python3 scripts/test_stdio_process.py target/debug/examples/offline_mcp_fixture
python3 scripts/test_mcp_probe.py target/debug/examples/offline_mcp_fixture
```

The process check leaves stdin open during idle and pending SIGTERM shutdown, which
reproduces Tokio's macOS blocking-stdin lifecycle issue. Linux and Windows behavior
must also be validated on those platforms; this check uses POSIX signals.

`mcp_probe.py --protocol modern BINARY CALLS_JSON` sends discovery and per-request
metadata. Add `--skip-discovery` to exercise direct modern calls. `--protocol legacy`
uses the explicit 2025-11-25 handshake. Response reads and child cleanup are bounded;
stray stdout fails verification. A real connector binary still requires its usual
authenticated session, and real tool calls spend their normal account rate tokens.
