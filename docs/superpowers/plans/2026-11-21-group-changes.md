# Replayable group changes implementation plan

**Goal** Give authorized API clients a restart-safe group text change feed.

**Architecture** The native reader sequences incoming updates. Group capture filters and durably journals changes before persisting the native position. The hosted service supervises capture independently of HTTP requests and serves bounded JSON pages from SQLite.

**Tech stack** Rust, the pinned grammers update and session APIs, Tokio, Axum, and libsql.

**Design** [Replayable group changes](../specs/2026-11-21-group-changes-design.md).

## Contract constraints

Use typed group references, string IDs and sequences, whole-entry response and retention budgets, and one active group stream per account generation. Require bearer read scope, current membership, deployment eligibility and explicit group grants. Mark upstream coverage loss and keep sensitive data out of diagnostics.

## Implementation

- [x] Define wire types and write grant and HTTP boundary tests. Extend the existing revisioned grant and validate requests before admitting Telegram work.
- [x] Implement private journal storage and verify immutable replay, retention, cursor scope and prior stream recovery.
- [x] Add capture to the ordered native reader and verify checkpoint ordering, text filtering, membership and deletion attribution with raw update fixtures.
- [x] Supervise active group readers across account restarts and shutdown. Connect long polling to commits, grant changes and deadlines.
- [x] Document the API and operator configuration, review the integrated diff and run the macOS quality gate.


## Verification

The offline macOS workspace gate passed with formatting, strict Clippy, workspace tests and warning-free rustdoc. The owner page tests and OpenAPI reference checks passed. Hosted loopback fixtures exercised replay, polling, grant revocation and relinking. Native fixtures covered checkpoint ordering and Telegram difference recovery. Live Telegram and deployed proxy logs still require validation.
