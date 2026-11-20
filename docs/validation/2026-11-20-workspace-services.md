# Workspace services verification — 2026-11-20

This run used macOS on Apple Silicon and project toolchain
`nightly-2026-10-31` (Rust 1.101.0-nightly). The continuation started at
`e7e72fb`. The package version is 0.26.1. The tested hosted interfaces use a
loopback issuer, deterministic Telegram boundaries, private disposable SQLite
roots and independent HTTP and MCP clients.

## macOS gate

Workspace formatting and all-target Clippy with warnings denied passed. The
workspace test run passed 1,268 unit, integration and example tests, plus three
doctests. Fifteen test entrypoints and two doctests were marked ignored in the
top-level runs. Parent fixtures also ran the designated child entrypoints.
All four stdio process and client probes passed. The dependency policy passed
30 behavior fixtures and version tooling passed 17 fixtures. OpenAPI Generator
7.23.0 validated the v1 contract. Warning-free workspace rustdoc, the
workspace release build and all four isolated release builds passed. The three
built binaries returned help successfully, and a disposable locked root install
ran the installed `telegram-mcp` CLI.

## Acceptance evidence

| Specification slice | Evidence |
| --- | --- |
| Libraries | The registry inventory covers 413 names and 443 resolved external identities. Live Codeberg HEAD matches the locked grammers revision. Cargo audit, source checks and license declaration checks ran against the locked graph. Cargo-deny reports the Chrono advisory below. |
| Workspace and stdio | The four-member workspace builds with one lockfile. Public Rust facade and recreated legacy SQLite fixtures run with the workspace suite. The stdio clients exercise both negotiated protocol revisions and legacy and canonical output, process drain, repeatable recovery, actual media bytes and artifacts. |
| Account service and reads | Local issuer tests verify signatures, exclusive audience, origin client, rotation, state, PKCE, nonce and CSRF. Lifecycle fixtures cover distinct account generations, concurrent leases, expired and replaced challenges, the password step, exclusive root ownership across processes, crash after retirement and ambiguous sign-in recovery. Direct and delegated HTTP reads share one fixture account limiter and enforce the same current joined-channel boundary. |
| Native reader and SSE | Core tests drive native update batches through the production sequencer and validate duplicate suppression, membership and queue accounting. A real HTTP client reconnects with a fresh head, sees subsequent posts and explicitly retrieves the missed post through the inclusive history endpoint. |
| Capacity and interruption | Controlled time and a real stalled TCP peer exercise write progress and response lifetime. A stalled JSON body releases handler admission. The receiver tests cover queue overflow, setup failure and membership loss while retaining healthy subscriptions. The HTTP stream closes on grant revocation and service shutdown with a subscriber still connected. |
| Hosted operations | HTTP and MCP fixtures exercise per-account and per-client grants, scoped reads, preview handles, qualified destinations, upload validation and one-shot mutation semantics. The final gate refuses dispatch after revocation. A lost receipt returns `delivery = unknown` with `retryable = false`. |

The API test suite checks direct and delegated credentials through the same
hosted request path. The independently launched gateway process checks OAuth
exchange, modern MCP, permissions and cancellation against a simulated API.
A cross-service test also connects that gateway process to the real hosted
HTTP routes, seeded with one Telegram channel and a shared account limiter.
It completes discovery and exchange, returns a canonical MCP search result,
and lets a direct API request and a gateway request contend for the last
token. The second registered client has a separate grant, an empty tool list
and a denied direct MCP invocation. This fixture is enabled by the gateway's
development dependency. The isolated production gateway graph excludes the
API service, Telegram core, grammers and SQLite. With this fixture, the
workspace suite passes 1,269 tests and three doctests. Strict Clippy,
warning-free rustdoc and isolated API and gateway release builds pass.

These runs use mocked Telegram and a loopback identity provider. Live Telegram,
a production identity provider, and the reverse-proxy example require separate
integration evidence for an installation.

## Review and advisory

Independent review found a bare-ID namespace collision, loss of a possibly
authorized session after interrupted sign-in, unbounded JSON-body collection,
and a window between socket authorization loss and the next watcher poll.
The implementation now binds the exact currently joined peer, quarantines
in-flight verification on restart, times request-body collection and checks
expiry and cancellation at the socket. The reviewer rechecked all four paths
with no remaining Critical or Important finding in those reproductions.

The full external declarations and reverse edges are in the
[dependency inventory](2026-11-20-workspace-dependency-inventory.json).
The source gate passed. In the original run, the advisory gate rejected
Chrono 0.4.45, which stable rmcp 3.11.0 requires. Its RustSec advisory is
unmaintained and lists no patched version. [RUSTSEC-2026-0543](https://rustsec.org/advisories/RUSTSEC-2026-0543.html)

The follow-up pin to the Chrono-optional rmcp revision resolved the advisory
gate. Fresh cargo-audit and cargo-deny checks passed. The complete workspace
graph has 439 packages with Chrono absent. The repeat macOS gate passed 1,268
tests and three doctests, warning-denied Clippy and rustdoc, the workspace
release build, isolated stdio and gateway release builds, and all four stdio
client and process probes.
[Temporary pin and stable return condition](../dependency-status.md#temporary-rmcp-pin--2026-11-20)

## Hosted shutdown follow-up

The hosted listener and account tasks now share the first ten-second drain
deadline. A regression with a retained account lease timed out against the
previous commit. It passed after the service began aborting and joining
retained account readers and senders. A late-startup test keeps a second
reservation alive and verifies that the stopped registry discards the newly
constructed client.

The SSE loopback test holds a real registry lease and receives the
`service_draining` frame on a writable connection. Generation revocation and
service work cancellation have separate signals, so unlink, membership loss
and credential expiry can still cut off writes. The independent re-review
found no remaining Critical or Important issue in the shutdown paths.

The repeat macOS gate passed 1,273 tests and three doctests, strict Clippy,
warning-free rustdoc, the workspace release build and the isolated hosted API
release build. The stdio startup probe reached its response deadline during a
concurrent run. All four stdio probes passed on the isolated rerun.

## Plan closeout

| Task | Acceptance evidence |
| --- | --- |
| 1. Dependency baseline and refresh | The earlier baseline, the locked legacy-session reopen, the stateless loopback MCP transport test and the approved temporary SDK pin are recorded in the dependency report. |
| 2. Workspace extraction | The root compatibility integration test exercises supported imports and an external read-only trait implementer. Workspace policy fixtures verify exact first-party member paths. |
| 3. Charged search | Core search tests check deadline, attempted-channel charges and validation before admission. Stdio search still fits and renders both output modes. |
| 4. History and message batches | Core tests preserve exclusive cursors and ordered, deduplicated IDs. Stdio tests distinguish missing, returned and budget-omitted IDs. |
| 5. Hosted identity | A real loopback issuer covers exact audiences, signature and expiry checks, JWKS rotation and browser state, PKCE, nonce, cookies and CSRF. |
| 6. Accounts and grants | Disposable roots and re-executed child processes cover root ownership and durable retirement. Challenge tests cover code and password progression. Real owner grant edits are bound to the generation and revision returned by the API. |
| 7. First REST reads | Real HTTP route tests isolate same-number IDs across accounts. Inclusive recovery covers the highest Telegram message ID, old rows, album siblings, exact continuation, partial results and budget metadata. |
| 8. Gateway delegation | The separate gateway process completes OAuth against a loopback issuer and invokes real hosted API routes. It preserves the originating client, shares the direct API account budget and releases the held API lease on disconnect. |
| 9. Native updates | The pinned SDK's native adapter and core queue fixtures cover channel membership, heads, ordering, own-send updates, albums, deduplication and interrupted setup. |
| 10. SSE history handoff | A real HTTP client sees fresh heads on reconnect, catches up through the advertised head and receives later posts live. Setup timeout, overflow and reader loss return normal HTTP errors before stream headers. |
| 11. Capacity and interruption | Controlled and real stalled writers, a setup disconnect, membership loss, overflow, replacement with a retained limiter and shared shutdown all release their reservations. Aggregate diagnostics cover capacity and native interruptions. |
| 12. Additional reads, media and sends | The hosted metadata and JPEG route fixtures exercise real account charging, current access, pagination and typed results. Stdio and hosted routes share the charged core. Both hosted interfaces send to explicit peers outside read grants and preserve posting refusal and nonretryable unknown delivery. Upload fixtures cover quota, publication races and cleanup after a crash. |
| 13. Workspace integration | The final locked inventory, macOS gate, per-member builds, CLI and stdio probes, version and dependency policy, OpenAPI validation, TLS reverse-proxy smoke and independent branch review establish the local acceptance results. |

The plan's named HTTP integration targets correspond to private library
fixtures that share the production service state. Run the API cases with
`cargo test --offline --locked -p telegram-api --features service --lib`,
the child-process account tests with `--test account_lifecycle`, and the
gateway cases with `cargo test --offline --locked -p telegram-mcp-http --test mcp_http`.
The owner page's deterministic JavaScript fixture runs with Node's built-in test
runner. The hosted implementation landed in a combined feature commit with
subsequent scoped corrections.

The final review identified four concrete gaps. A disabled restart now purges
private upload files left by a terminated process. A grant edit must present the
generation and revision it loaded. The owner page ignores out-of-order client
snapshots. In-flight SSE output stays covered by membership observation after
queue overflow. Each correction has a focused regression.

A disposable NGINX 1.30.7 TLS smoke test used the operator guide's location
blocks with a local fixture upstream. It verified forwarded headers and source
address replacement, SSE readiness before the first post, disabled compression
and buffering, downstream disconnect propagation, streaming upload and the
100 MiB edge limit. The sustained synthetic TLS case did not establish a strict
downstream stall deadline. The API socket deadline has a separate real TCP
fixture. Production identity providers and live Telegram accounts remain
installation verification.


### Final macOS gate

The reviewed branch passed formatting, all-target Clippy with warnings denied,
1,294 Rust tests, three doctests, three browser regressions, and warning-free
rustdoc. Fifteen Rust test entrypoints and two doctests were ignored in the
top-level run. Parent tests exercised the isolated helper processes.

The workspace release build, four isolated member builds, all three binary
help commands, root examples and a locked installation into a disposable
root passed. All four stdio probes passed, and the production gateway and
stdio feature graphs remained isolated. The dependency policy passed 30
fixtures and the version checker passed 17. OpenAPI Generator 7.23.0
validated the final request and response schemas.

The locked HTTP/1 dispatcher regression reproduced early release of an
encoded post's reservations before a blocked socket write. Vectored output
now retains the original frame, membership guard and both admissions
through transport release. The re-review confirmed this fix and the other
three final-review corrections.
