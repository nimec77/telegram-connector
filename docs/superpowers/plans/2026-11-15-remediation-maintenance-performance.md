# Maintenance and Performance Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the missing CI process matrix, remove obsolete test guidance, reduce redundant shaping work and make artifact verification lookup linear, then verify all repairs together.

**Architecture:** Keep exact complete-wire serialization as authority, with separate direct-adapter budget selection. Build one temporary artifact path index inside the existing mutex; leave filesystem validation and ownership checks in place.

**Tech Stack:** Rust, serde_json, existing std HashMap/borrowed paths, offline Python clients, pinned macOS security/lint tools.

**Spec:** `docs/superpowers/specs/2026-11-15-review-remediation-design.md`

## Global Constraints

- Preserve complete serialized JSON-RPC accounting as the authoritative wire budget, including external request ID, negotiated fields, duplicated text fallback, cursors, omitted IDs and the metadata-qualified one-message floor.
- Direct Rust/legacy adapters retain their documented budget behavior. Keep the additional serialization in InstrumentedTransport: it serves byte logging and recovery.
- Do not replace exact accounting with estimated character counts or assume size is monotonic when omission metadata grows.
- Preserve fail-closed tamper handling, logical expiry, eviction, response pins, deletion ownership, quotas, CPU-worker bounds and mutex atomicity.
- Do not move validation outside the lock or trust a cached filesystem observation solely to improve speed.
- Validate locally on macOS. Existing Linux CI remains; provisioning Linux and Windows validation are outside this work.
- No dependency additions, broad style churn, version bump, private configuration/session changes, pushes, merge or release publication.
- Update the changelog's Unreleased section during implementation. Delete the active spec and corresponding plans only when the repair change merges.

## Review Focus

- Long escaped string request IDs must use their external wire identity in both protocol modes (Task 2).
- Popping a batch message grows omitted_ids metadata, so a previously fitting metadata floor can become too large (Task 2).
- Compact headers may conflict even when messages otherwise serialize under budget (Task 2).
- Missing/tampered/foreign artifact paths must fail closed without deleting foreign ownership (Task 3).
- Logical expiry/pins and failed cleanup must retain their behavior at 1,000 stored entries (Task 3).

---

### Task 1: Add artifact CI coverage and correct environment-test guidance (R13, O05)

**Files:**
- Modify: `.github/workflows/ci.yml`, `docs/memory.md`
- Validate existing `scripts/test_artifact_clients.py` disposable-root behavior; modify only if execution shows a defect

**Interfaces:**
- Consumes the built `target/debug/examples/offline_mcp_fixture` and existing offline process/client job on ubuntu-latest/macos-latest.
- Produces an additional invocation of `scripts/test_artifact_clients.py target/debug/examples/offline_mcp_fixture` in that existing matrix step, with timeout/permissions/pinned actions intact.

- [ ] **Step 1: Inspect the artifact script's explicit temporary roots and run it locally against the built offline fixture.** Run `cargo build --offline --locked --example offline_mcp_fixture`, then `python3 -B scripts/test_artifact_clients.py target/debug/examples/offline_mcp_fixture`. Expected all pass with no account/global artifacts. This configuration/prose change needs execution/lint evidence, not a source-text assertion test.
- [ ] **Step 2: Add the invocation to the existing process/client matrix.** Preserve both runners and all workflow controls. Remove obsolete EnvGuard/ENV_LOCK instructions in current memory, replacing them with injected lookups and subprocess-only real environment/path tests. Inspect references to those removed helpers for stale guidance; historical reports remain historical.
- [ ] **Step 3: Run pinned actionlint and review the workflow diff.** Use `/private/tmp/review-remediation-actionlint/actionlint -shellcheck= -pyflakes=` if it remains available; this is verified 1.7.12. Expected no diagnostics. Inspect documentation links and current subprocess fixtures. Do not claim hosted execution.
- [ ] **Step 4: Commit `ci: exercise offline artifact clients on both runners`; obtain independent task review.** Record A13/A18 actual local/configuration evidence.

### Task 2: Remove duplicate fitting and metadata body clones (O02)

**Files:**
- Modify: `src/mcp/tools/shaping.rs`, `src/mcp/tools/wire_budget.rs`, `src/mcp/server/impl_search.rs`, `src/mcp/server/impl_message_batch.rs`, `src/mcp/server/impl_legacy.rs`, thin callers in `src/mcp/server.rs`
- Test: shaping/wire_budget inline tests, `src/mcp/tests/protocol_budgets.rs`, direct-adapter parity tests

**Interfaces:**
- Consumes final SDK/runtime operation context from runtime-sdk plan and final fan-out signatures from read-correctness plan.
- Produces internal `BudgetMode::{Direct, Wire}` parameter on shared search/history/batch data bodies. Wire callers skip preliminary response-JSON fitting; direct convenience methods retain the existing fitting behavior.
- Retains `wire_budget::{search,batch}` signatures and complete `runtime::response_size` framing. Implement metadata-only projections/helpers that copy metadata without cloning message bodies, or temporarily move message vectors out while creating a metadata snapshot and restore them before continuing.

- [ ] **Step 1: Add/extend behavioral size-parity regressions and collect baseline work measurements.** Actual serialized frames cover both output/protocol modes, numeric/long escaped string IDs, Unicode, compact/full, cursors, omitted IDs, one-message floor and metadata-only overflow. Retain direct-adapter output parity and compact-header ambiguity errors. Measure representative search/history sizes 1/20/100 and actual batch sizes 1/20/50 (the public cap) by serialization/allocation counters or reproducible profiling, recording command/input/counts; do not enforce timing thresholds. Any larger batch stress sample must be labeled helper-only. A regression should catch preliminary fitting changing final retained rows under exact wire accounting.
- [ ] **Step 2: Run new behavioral regressions and record RED if they expose a contract bug; characterize already-correct cases as compatibility evidence.** For a pure optimization, baseline measurement proves redundant fitting/body-clone work; do not invent a behavior failure. Keep all correctness tests green while refactoring.
- [ ] **Step 3: Select fitting policy explicitly and eliminate metadata message clones.** Shape/truncate/compact once, with direct-only preliminary fitting. Continue exact full-wire pop-until-fits on wire paths and recheck metadata after every pop. Retain the final rendered result when practical instead of rendering it again. Do not remove InstrumentedTransport's independent serialization or substitute estimated/monotonic sizing.
- [ ] **Step 4: Run protocol budgets, direct parity, all protocol tests and the full Rust gate.** Repeat the same representative measurement and report actual before/after work without overstating speed/RSS. Expected exact wire parity and reduced work.
- [ ] **Step 5: Commit `perf: avoid redundant wire response fitting`; obtain independent task review.**

### Task 3: Index artifact paths once per locked verification (O03)

**Files:**
- Modify: `src/mcp/artifacts.rs`
- Test: `src/mcp/artifacts/tests.rs`, `src/mcp/tests/media_artifacts.rs`

**Interfaces:**
- Retains `ArtifactStore::verify_contents(&self, state: &mut State) -> Result<(), StoreError>` and all artifact/public resource contracts.
- Produces temporary `HashMap<&Path, &OwnedFile>` from the stored entries plus lock-file identity, constructed once under the same lock for the same verification; no persistent secondary state.

- [ ] **Step 1: Characterize existing tamper/foreign/missing-file, expiry, eviction, pins and failed-cleanup behavior.** Add a meaningful regression only for an uncovered observable invariant. Measure verification at 1/100/1000 entries with a production-used lookup seam or benchmark, counting lookup/comparison work and reporting timing without a flaky timing pass threshold.
- [ ] **Step 2: Record the existing quadratic lookup measurement.** Expected per-entry linear entry search grows quadratically; filesystem validation remains separately linear and real.
- [ ] **Step 3: Build the temporary path index once inside locked verification.** Use a lookup for each directory entry; preserve all identity/ownership checks, unavailable-state handling and deletion policy. Do not cache filesystem observations, move validation outside the mutex, or broaden recognition of foreign paths.
- [ ] **Step 4: Run artifact unit/wire tests and full Rust gate, then repeat 1/100/1000 measurement.** Expected all behavior preserved and lookup work O(n); report measured timing with its limitations.
- [ ] **Step 5: Commit `perf: index artifact paths during locked verification`; obtain independent task review.**

### Task 4: Integrate evidence and verify the entire repair set

**Files:**
- Modify: `CHANGELOG.md` Unreleased, `docs/memory.md` durable current facts only
- Create: `docs/validation/2026-11-15-review-remediation.md`
- Preserve active spec/plans pending an authorized merge

**Interfaces:**
- Consumes all task reports/reviews from every 2026-11-15 remediation plan.
- Produces A01–A19 and R01–R13/O01–O05/D01 traceability, actual validation evidence and explicitly resolved independent whole-branch review.

- [ ] **Step 1: Reconcile every acceptance case.** For each item list production change or disproving counterexample, test/regression evidence, and any genuine validation gap. Resolve cross-plan interfaces and document the narrow subscription-context ruling. Update current SDK/config/global-scope/media-probe docs and concise Unreleased notes; leave historical reports unchanged.
- [ ] **Step 2: Run the exact final macOS matrix from the spec.** Formatting; strict all-target Clippy; full locked offline tests/doctests; warning-free docs; release and offline fixture builds; all four Python process/probe/media/artifact scripts; dependency-policy fixtures/declaration check; fresh cargo-audit 0.22.0; cargo-deny 0.20.2 sources/advisories; actionlint 1.7.12. Capture toolchain/SDK/features, counts, date, measurements and commands. Temporary tool paths are available under `/private/tmp/review-remediation-tools/bin` and `/private/tmp/review-remediation-actionlint` if not cleaned. Fetch only locked packages when needed before offline checks. Inspect complete lock and source diff plus `git diff --check`. Expected all available gates pass; any unavailable gate is a named gap, never a fabricated pass.
- [ ] **Step 3: Obtain a fresh most-capable whole-branch review.** Supply spec, plan paths, complete branch diff package, reports and review-focus/ruling ledger. Resolve findings using the skill fix loop with regressions and relevant revalidation. Do not rerun unchanged heavy gates without a new change/failure/concern.
- [ ] **Step 4: Finalize local commits and report.** Keep package version/pins/nightly unchanged except scoped patched rmcp. Report tests/security/review results and any limits; no Linux/Windows/live-account/hosted-CI claim from local validation. Leave branch local, preserving active docs until merge.
