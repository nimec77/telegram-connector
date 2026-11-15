# Runtime Identity and SDK Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Isolate each request lifetime across cancellation, SDK queues, instrumentation and recovery, then update the vulnerable SDK.

**Architecture:** Keep the bounded raw adapter and SDK serializer. Give each admission a unique SDK-facing ID, retain the original ID in application context, and translate output at the adapter boundary. This prevents queued output from rebinding to a reused external ID; owned operation handles protect writes already started.

**Tech Stack:** Rust nightly-2026-10-31, Tokio, rmcp 3.8.0 followed by reviewed 3.9.0, existing offline fixtures.

**Spec:** `docs/superpowers/specs/2026-11-15-review-remediation-design.md`

## Global Constraints

- Keep `json_v1`, matching text fallback, explicit legacy reads, the 16/18 tool catalog, and both MCP 2026-07-28 and legacy 2025-11-25 behavior. No output schema version bump.
- Validate locally before acquisition; perform every RPC after acquisition. Do not refund failed fan-out attempts.
- Preserve total operation deadlines, search partial results, cancellation, bounded input, admission, memory, image processing and shutdown. No new application retries.
- Keep all three direct grammers dependencies at the same existing Codeberg revision.
- Only the scoped rmcp update and unavoidable coupled lockfile changes are authorized; do not regenerate the dependency graph or change the pinned nightly.
- Internal identifiers must not leak into client-visible IDs, progress tokens, metadata, replay payloads or byte-budget accounting.
- Keep correlation storage bounded by owned active/queued work and release it on completion, cancellation settlement and shutdown.
- Validate locally on macOS. No live Telegram sends/authentication, package-version changes, pushes, releases or private configuration changes.
- Before source changes, demonstrate each finding with a failing behavior test; record counterevidence for any disproved premise.

## Review Focus

- An old error queued before its first send poll must be suppressed after ID reuse, just as a success is (Task 1).
- A numeric external ID and its string spelling must remain distinct throughout cancellation and recovery (Task 1).
- Identical progress tokens reused across generations must never send stale progress or drop replacement progress (Task 1).
- Cancellation before handler dispatch must release admission and must not leave unbounded correlation tombstones (Task 1).
- A newer SDK must preserve omitted error IDs and legacy result stripping as well as successful tools (Task 2).

---

### Task 1: Carry immutable operation identity through the SDK

**Files:**
- Modify: `src/mcp/runtime.rs`, `src/mcp/runtime/state.rs`, `src/mcp/runtime/transport.rs`
- Modify: `src/mcp/observability/transport.rs`, `src/mcp/observability/metrics.rs` where needed for generation-specific accounting
- Test: `src/mcp/runtime/tests/transport.rs`, `src/mcp/tests/protocol_failures.rs`, `src/mcp/tests/protocol_progress.rs`, `src/mcp/tests/protocol_recovery.rs`

**Interfaces:**
- Consumes: current `Runtime::serve`, `InstrumentedTransport::new`, `SessionMetrics`, `ResponseBuffer` and keyed recovery reservations; their public compatibility APIs remain.
- Produces: `State::admit(id: RequestId, token: Option<ProgressToken>, bootstrap: bool) -> Result<Arc<Operation>, ErrorData>` and generation-checked `State::finish(operation: &Arc<Operation>)`. `Operation` retains `external_id: RequestId` and `sdk_id: RequestId` plus original progress token/cancellation state.
- Produces: generation-specific request correlation for instrumentation while preserving the original serialized request ID. `scope_response` and `response_size` continue measuring original IDs.

- [ ] **Step 1: Write deterministic regressions against the current implementation.** Extend the duplex-one-byte transport fixture to block an old response after one byte, cancel it, admit a replacement with the same ID, drain the committed frame, then prove the replacement still completes and can be cancelled. Add queued-before-first-poll success/error/progress cases and reused token cases. Assert the actual wire ID/token, replacement admission and recovery records, not just internal map shape. Parameterize numeric/string IDs and both protocol versions in the existing protocol harness. Include distinct recovery keys, old/new tool names in metrics/replay, and storage returning to baseline after completion/shutdown.

- [ ] **Step 2: Run the regressions and capture the expected failure.** Run `cargo test --offline --locked cancelled -- --nocapture` and the new exact test filters. Expected: at least the replacement is removed by the old write or stale queued output is emitted; compilation failures are not RED evidence.

- [ ] **Step 3: Implement operation identity at admission and the adapter boundary.** Assign monotonically unique SDK-facing IDs for requests, forward cancellation using the current operation's SDK ID, and look up queued response/error/subscription output only by that SDK identity. Attach the admitted `Arc<Operation>` to request extensions before dispatch so `RuntimeService` captures the correct operation without a late external-ID lookup. Restore `RequestContext.id` for application code. Progress/subscription correlation must retain the generation through SDK queues (private SDK tokens/IDs are acceptable); restore original wire tokens/metadata before serialization. A cancelled queued item with no active generation is discarded; an already-started frame retains its owned handle until write completion. Finish compares the original handle/generation before removing admission.

  Avoid retaining a tombstone per cancelled ID: the active SDK-ID lookup is removed with admission; queued stale SDK IDs cannot match replacement admissions. If any secondary registry is needed, tie it to owned work and prove cleanup. Keep writer future construction after correlation/translation so suppressed output never calls instrumentation. Use a generation-specific metrics key separately from the external wire ID, including error paths, while retaining the existing public diagnostic wrappers. Preserve keyed recovery reservations rather than rebinding by external ID.

  SDK constraint discovered during implementation: its private SubscriptionContext constructor derives ack/sink correlation from RequestContext.id. For subscriptions/listen only, retain the SDK ID in that SDK lifecycle field and keep the original ID in the public nonserialized rmcp::service::OriginatingRequestId context extension and OPERATION.response_id; translate every wire subscription field. Tools and other application contexts receive the original context.id. Record and review this narrow internal-context compatibility exception explicitly; do not leak it into client-visible metadata or replay.

- [ ] **Step 4: Verify all lifetime paths.** Run `cargo test --offline --locked mcp::runtime`, `cargo test --offline --locked protocol_`, then `cargo test --offline --locked`. Expected: all pass; cancelled output remains absent, committed frames finish, original framing/size accounting is exact, replacement metrics and recovery are intact. Run formatting, strict all-target Clippy and warning-free docs before committing.

- [ ] **Step 5: Inspect and commit.** Record RED/GREEN commands and counts in the task report. Inspect `git diff --check` and scoped diff; commit `fix: isolate MCP request lifetimes across cancellation`. Obtain independent task review before Task 2.

### Task 2: Update the reviewed SDK without graph churn

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`
- Modify current-version references: `AGENTS.md`, `docs/memory.md`, `docs/stdio-runtime.md`, `docs/maintenance.md` if applicable; discover other current references with `rg`, preserving historical validation reports.
- Test: existing protocol/schema/recovery/progress/budget suites, `scripts/test_*clients.py`, dependency-policy fixtures

**Interfaces:**
- Consumes: Task 1's immutable operation context and complete protocol regressions.
- Produces: rmcp and coupled macros locked to reviewed compatible 3.9.0 (or later only if fresh advisory evidence requires it), existing server/stdio feature graph unchanged.

- [ ] **Step 1: Capture security RED evidence and review current primary evidence.** Run fresh `cargo audit --deny unsound --deny yanked`; confirm RUSTSEC-2026-0462. Consult the current RustSec advisory and the selected release's source/changelog, and record the date. The advisory currently lists `>=3.9.0` as patched; verify again before selection. Capture `cargo tree --offline --locked -e features -p rmcp`.
- [ ] **Step 2: Apply the targeted update.** Raise the manifest floor to `3.9`; run `cargo update -p rmcp --precise 3.9.0` and targeted coupled macro update only if required. Fetch newly locked packages before offline validation. Inspect the entire lock diff: no grammers pin, nightly, unrelated registry version, feature or package-version changes. Adapt SDK compatibility code only if a failing existing regression demonstrates the need.
- [ ] **Step 3: Verify security and wire compatibility.** Run the full Rust gate, release build, offline fixture build and four Python client/process scripts from the spec. Run policy fixtures/declaration policy, fresh audit, and `cargo deny --locked check sources advisories` using versions in `docs/maintenance.md`. Expected: all available gates pass with no new ignores; explicitly record any genuinely unavailable tool/network gate. Compare the feature graph and current-version docs, preserving historical reports.
- [ ] **Step 4: Commit and review.** Commit `fix: update rmcp to the reviewed security release`; report exact versions, lock changes, features, audit date and compatibility evidence. Obtain independent task review before later wire optimization.

## Subsequent Plans and Completion

Continue with the read-correctness, sending/input, configuration, and maintenance/performance plans dated 2026-11-15. They share this spec and consume the final SDK. Final acceptance requires all A01–A19 dispositions, the spec's complete local gate, updated Unreleased notes, and independent whole-branch review. Leave active spec/plans until an authorized merge; no merge/push is included in this request.
