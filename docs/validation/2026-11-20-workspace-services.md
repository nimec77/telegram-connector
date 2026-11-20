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
exchange, modern MCP, permissions and cancellation against a local API fixture.
These runs establish the boundaries with mocked Telegram. Live Telegram,
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
