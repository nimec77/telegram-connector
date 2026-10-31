# Telegram Connector Development Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Modernize the local stdio connector to verified MCP 2026-07-28 behavior,
consistent JSON contracts, current dependencies, and corrected security/reliability defects.

**Architecture:** Keep the existing generic server, Telegram client seam, domain types,
and split operation modules. Add protocol-level test coverage and a typed output boundary;
fix RPC safety at the narrowest existing operation boundary.

**Tech Stack:** Rust edition 2024, nightly, Tokio, rmcp, grammers, serde/schemars.

**Spec:** [Audit and proposed design](../specs/2026-10-31-development-audit-design.md).

**Additional requested workstreams:** [Rust coding manual](../../rust-style.md),
[standards alignment plan](2026-10-31-rust-standards-alignment.md), and
[media delivery proposal](../specs/2026-10-31-media-delivery-design.md).

**Status:** All remaining tasks and linked alignment/media-M1 work are authorized.
Tasks 1–4 are complete. Tasks 5–10 and standards alignment are in progress; optional
media M2/M3 remain separate product decisions. Fresh dependency targets are verified
when each migration begins.

## Global constraints

- Local stdio; MCP `2026-07-28`; optional extensions assessed separately.
- JSON + JSON Schema 2020-12; canonical application envelope `schema_version: "1"`.
- Preserve tested `2025-11-25` compatibility; advertise other versions only with evidence.
- Keep nightly and all three grammers dependencies on the same reviewed Codeberg revision.
- No production change before a failing behavior regression; no real credentials for tests.
- Local validation before rate-limit acquisition; every RPC after its defined operation
  charge. Startup authentication is outside tool charging but must have timeout bounds.
- Preserve per-channel pagination, partial results, per-item errors, album grouping,
  search deadline, image budgets and secret redaction.
- Feature branches for execution; preserve unrelated work. Review each stage before merge.
- No lockfile deletion, broad rewrite, remote deployment or release publication by default.

## Review focus

1. Malformed expanded config must never print secrets, even through nested debug errors (Task 1).
2. An upstream that never resolves must not hang startup, status, channel listing or transcription (Tasks 2–3).
3. Interleaved modern and legacy requests must not share implicit protocol/conversation state (Tasks 5 and 8).
4. Mixed media failures and duplicated JSON fallback must preserve budgets and item association (Tasks 7–8).
5. Dependency/Telegram changes must preserve short usernames, peer kinds, missing messages,
   forward attribution and cursor semantics (Tasks 4 and 6).

## Sequence and release boundaries

`1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10` is the default integration order.
Tasks 1–4 form a useful security/correctness maintenance release. Tasks 5–6 modernize
dependencies and prove protocol behavior. Tasks 7–8 form the documented output-contract
migration. Tasks 9–10 establish ongoing maintenance and final acceptance. Independent work
may proceed in parallel only with separate file ownership; Cargo.lock has one owner.
The Rust alignment plan integrates as S1 after urgent patches, S2 alongside Tasks 2/5,
and S3–S6 after dependency/API stabilization. Media phase M1 is part of Task 8; optional
resource/file phases M2/M3 remain separate product decisions, not core acceptance blockers.

## Task 1: Remove credential exposure and patch advisory matches

**Files:** `src/config.rs`, `src/config/tests/load_tests.rs`,
`src/config/tests/env_tests.rs`, `Cargo.toml`, `Cargo.lock`.
**Consumes:** existing config parser/error path and locked dependency graph.
**Produces:** safe config diagnostics; patched ctutils/Tokio graph.

- [x] Add `config_parse_error_redacts_literal_and_expanded_secrets`: malformed syntax and
  wrong types containing fake API hash/phone values; neither display, debug nor chained
  formatting may contain the values. Safe location information must remain useful.
- [x] Run `cargo test --locked config_parse_error` and record the pre-fix failure.
- [x] Sanitize TOML diagnostics at `Config::load_from` before retaining the source error;
  do not rely solely on replacing the top-level anyhow message.
- [x] Recheck registry/advisories; target `ctutils 0.4.7` and `tokio 1.54.1` or newer
  stable versions. Use targeted Cargo updates, retaining Tokio dev `test-util`.
- [x] Run config regressions, `cargo tree --locked -i ctutils`, fresh `cargo audit`, and
  the full quality gate. The two known advisories must be resolved; document any new ones.
- [x] Review/commit as focused security patches. Do not wait for the full roadmap to ship them.

**Verification (2026-10-31):** The new config regression failed before the fix
for all ten literal/environment-expanded syntax, type, and secret-key fixtures. All 78
config tests then passed (5 ignored). `just check` passed formatting, all-target Clippy,
and 759 library tests (5 ignored; one doctest ignored). Targeted updates changed only
`ctutils 0.4.2 → 0.4.7` and `tokio 1.53.1 → 1.54.1`; both Tokio manifest floors now
require 1.54.1 and dev `test-util` is retained. `cargo tree --locked -i ctutils` confirms
the production crypto path uses 0.4.7. Fresh `cargo audit` reported zero vulnerabilities
and no warnings against RustSec commit `2295466c4e9df26647fd67d8fa1c705ac2fc7f81`
(2026-10-30). Independent review found no issues. Parser diagnostics deliberately retain
only numeric location because even parser messages and keys can contain secrets.

## Task 2: Bound startup/status/channel work and close unmetered paths

**Files:** `src/telegram/trait_def.rs`, `src/telegram/client.rs`,
`src/telegram/client/lifecycle.rs`, `src/telegram/client/channels.rs`,
`src/mcp/server/impl_status.rs`, `impl_channels.rs`, `impl_media.rs`, `src/main.rs`,
`src/telegram/client/tests/channels_tests.rs`, `src/mcp/tests/status.rs`,
`channels.rs`, `transcription.rs`, and shared fixtures affected by trait changes.
**Produces:** explicit fallible network status refresh and cached status access.

- [x] Add pending-future/paused-time tests below the existing whole-client mock seam:
  `startup_status_times_out`, `dialog_walk_has_total_deadline`, and
  `transport_failure_is_not_reported_as_unauthenticated`.
- [x] Add tool tests proving invalid requests spend zero tokens and exhausted tokens
  start zero channel/Premium RPCs, including an empty Premium cache.
- [x] Separate cached status reads from fallible refresh. Proposed internal interface:
  `refresh_account_status(&self) -> Result<AccountStatus, Error>`, with
  `AccountStatus { authorized: bool, premium: Option<bool> }`. Cache only successful
  observations with their timestamp; a failed refresh must not overwrite them as false.
  Retain prior bool wire fields in legacy mode; canonical mode may expose freshness/error.
- [x] Keep `check_mcp_status` zero-RPC and free; it reports cached observations. Refresh
  during bounded startup and already charged account-dependent operations. Make Premium
  warmup nonfatal; differentiate actual unauthenticated status from network outage.
- [x] Bound the entire dialog walk using the existing history timeout and account RPCs
  using the resolve timeout. Use one operation charge for channel list/basic info, with
  full-info enrichment included; do not introduce per-page charging unexpectedly.
- [x] Run focused status/channel/transcription tests and the quality gate. Review every
  path capable of issuing an RPC, including fallbacks, against the charge ordering.
- [x] Commit the bounded-status/metering change with its behavior documentation.

**Verification (2026-10-31):** Stalled production-client and tool regressions
reproduced the missing dialog deadline, RPCs before validation/acquisition, and the cold
Premium path bypassing a denied budget. Restoring the former authorization policy caused
startup timeout, transport-error and cache-preservation regressions to fail; the fixed
implementation was restored. Focused client (85) and MCP (167) tests passed, followed by
`just check`: formatting, all-target Clippy, and 774 library tests passed (5 ignored;
one doctest ignored). Independent review found no issues. Tests use temporary sessions
with an unpolled grammers runner; no Telegram network or credentials are required.
Version 0.22.7 and dependency pins are unchanged. Startup examples allow 90 seconds for
two default 30-second account RPC budgets; personal configuration is untouched.

## Task 3: Enforce transcription deadlines and clean cancellation

**Files:** `src/telegram/transcription.rs`, `src/telegram/client/ops_transcribe.rs`,
`src/mcp/server/impl_media.rs`, `src/mcp/tests/transcription.rs`.
**Produces:** one deadline for the requested transcription operation.

- [x] Add paused-time regressions `timeout_shorter_than_poll_interval_does_not_poll`,
  `pending_poll_returns_latest_partial_at_deadline`, and
  `late_completion_does_not_override_deadline`.
- [x] Define `timeout_seconds` as the total operation budget starting after local
  validation/acquisition, including resolution, initial call, sleeps and polls. Pass an
  absolute Tokio `Instant` deadline internally; each RPC uses the smaller of remaining
  time and its configured timeout. Never restart the budget per poll.
- [x] If a state has been received, return its latest text with `partial:true` on expiry;
  before any state exists return a typed timeout. Cancelled requests start no further RPCs.
- [x] Run transcription tests with paused time, then the quality gate and commit.

**Verification (2026-10-31):** Four polling regressions reproduced oversleep,
late completion, a pending poll exceeding the budget, and setup resetting the clock.
Production-client resolution and the MCP cold account refresh also exceeded a 1-second
request before the fix. One absolute `TranscriptionDeadline` now covers all post-charge
stages while preserving configured RPC caps. Cancellation tests confirm the in-flight
future is dropped and no further poll starts. Independent review found one expiry-edge
continuation defect; `stage_resuming_at_expiry_cannot_start_its_next_rpc` reproduced it,
and a guard before every future poll fixed it. Final `just check` passed formatting,
all-target Clippy and 785 library tests (5 ignored; one doctest ignored). No live network
or credentials were used. Version 0.22.8 and dependency pins are unchanged.

## Task 4: Correct log retention and short usernames

**Files:** `src/logging.rs`, `src/telegram/types/names.rs`,
`src/telegram/converters/channel.rs`, related forward/message converters and their tests.
**Produces:** narrowly scoped cleanup and consistent received-username validation.

- [x] Add `cleanup_preserves_foreign_logs_and_backups` with old app rolling logs,
  `other-service.log`, `notes.log.backup`, recent files and directories. Assert only old
  owned rolling files are removed. Derive the exact supported filename pattern from the
  appender (`telegram-connector` prefix, `log` suffix, daily rotation), not a stale comment.
- [x] Add conversion regressions for real short-name fixtures such as `mash`, and 3-char
  upstream names, through channel and forwarded-message output. Keep blank/invalid forms
  rejected. Use observed-username validation rather than new-username registration rules.
- [x] Run logging/conversion/domain tests and quality gate. Review and commit these as
  independently revertible fixes; update the resolved memory open item when merged.

**Verification:** Commits `27d5399` and `098b1f5`. Retention and short-name
regressions failed before production edits; focused suites and `just check` passed
(791 library tests, 5 ignored, 1 doctest ignored). Independent review approved spec and
quality with no findings. The controller verified the unchanged appender builder and
pinned tracing-appender date/prefix/suffix assembly; central memory is corrected.

## Task 5: Establish stdio conformance tests and update rmcp

**Files:** create `src/mcp/tests/protocol_stdio.rs`; register in `src/mcp/tests.rs`;
modify `src/mcp/server.rs`, `src/mcp/tests/server_core.rs`,
`src/mcp/observability/transport.rs`, transport/metrics tests,
`scripts/mcp_probe.py`, `src/main.rs`, `Cargo.toml`, `Cargo.lock` as required.
**Produces:** offline wire harness and verified rmcp 3.8.0+ integration.

- [ ] Build a mock-backed in-memory duplex harness using the same rmcp async transport
  and instrumentation as production. Never require Telegram setup to test the protocol.
- [ ] Cover discovery, a modern first `tools/list` without discovery/initialize, unknown
  version, missing metadata, interleaved metadata, resultType/identity, cache fields,
  legacy initialization, malformed requests, unknown tools, notifications without replies,
  input size limits, EOF, cancellation and transport failures. Assert wire responses.
- [ ] Characterize current behavior; for a known missing requirement, record a failing
  regression before changing production code. Preserve existing behavior where compliant.
- [ ] Upgrade rmcp to current stable (snapshot 3.8.0); review 3.2/3.5/3.6 lifecycle and
  transport changes. Prefer SDK handling; override only proven project gaps. Explicitly
  bound advertised versions to tested revisions rather than accepting all future defaults.
- [ ] Verify MRTR/subscription dispatch and unadvertised-capability behavior. Keep dynamic
  lists, user elicitation and optional extensions unadvertised without product handlers.
- [ ] Add token-bound, monotonic progress for long transcription/media work, with no
  progress after completion/cancellation and no payload text in notifications. Verify
  cancellation cleanup of in-flight metrics; do not promise forced abortion of started
  `spawn_blocking` work, instead bound concurrency and stop scheduling more.
- [ ] Apply `shutdown_timeout_seconds` as the actual graceful drain deadline, then cancel
  remaining operations and close transport. Test immediate completion, pending RPCs and
  deadline expiry; remove the fixed 100 ms sleep as a substitute for lifecycle management.
- [ ] Extend the probe to select modern and legacy modes explicitly. Run protocol tests,
  large media/recovery tests and full quality gate. Commit the SDK migration separately.

## Task 6: Refresh remaining dependencies, including Telegram

**Files:** `Cargo.toml`, `Cargo.lock`, `src/telegram/client*.rs`,
`src/telegram/client/*.rs`, converters/tests touched by upstream types.
**Produces:** current direct versions and reviewed compatible transitives.

- [ ] Requery authoritative versions; record exact resolved targets and compare against
  the audit table. Patch compatible serialization/config dependencies first. Include
  declarations already satisfied by newer lockfile versions in the review.
- [ ] Update base64 0.22→0.23, mockall 0.14→0.15, proptest 1.11→1.12 as separate
  reviewable changes. Preserve byte-identical JPEG passthrough and paused-time tests.
- [ ] Move all three grammers pins together to one reviewed SHA (snapshot
  `1e31e73cd9797790905bbea82cd75adcbe6f1242`). Review all changed TL enum matches and error
  variants; retain raw pagers while `Message.peers` is private.
- [ ] Add missing behavior fixtures before adaptations: missing/deleted messages,
  communities/megagroups, forward-envelope attribution, same-number peer kinds,
  date windows, album boundaries and per-channel cursors. Test production conversion and
  paging decisions, not mocks alone; investigate peer collisions before changing behavior.
- [ ] Refresh remaining compatible transitives without deleting the lockfile. Inventory
  duplicate majors, yanked versions, licenses and sources. For every blocked latest major,
  record its upstream constraint and removal path; do not force incompatible versions.
- [ ] Run quality gate, release build, fresh audit and platform checks per migration
  group. No live migration is claimed from offline tests; use a disposable mocked session
  fixture for session format checks. Commit each dependency group independently.

## Task 7: Add canonical typed JSON and output schemas

**Files:** create `src/mcp/tools/types/contract.rs` and contract tests;
modify `src/mcp/tools/types.rs`, `types/responses.rs`, `tools/helpers.rs`,
`src/mcp/server.rs`, `src/mcp/server/impl_*.rs`, `src/config.rs`, defaults/tests,
`src/mcp/tests/schema_integrity.rs` and tool tests.
**Consumes:** upgraded rmcp and existing per-tool domain response types.
**Produces:** `ToolResponse<T>`, `ToolError`, canonical/legacy output modes.

- [ ] Define serialized contract types exactly as the design: tagged by
  `schema_version:"1"`, mutually exclusive non-null `data` or `error`; error fields
  `code`, `message`, `retryable`, optional `retry_after_seconds`.
- [ ] Add failing wire/serialization tests across all 16 tools: success, validation,
  rate-limit, timeout, missing message and partial batch cases. Validate outputs against
  advertised JSON Schema 2020-12 with a dev-only validator justified by these assertions.
- [ ] Include argument type/deserialization errors that occur before the tool method:
  convert actionable argument failures into canonical `isError:true` results at the
  parsing/router boundary. Keep malformed CallToolRequest envelopes and unknown tools
  as JSON-RPC errors; verify both paths on the wire, without string-matching error text.
- [ ] Add `[server] output_format = "json_v1" | "legacy"`, default `json_v1` for the
  documented migration release. JSON mode emits the typed value in structuredContent
  and matching text fallback; legacy keeps old text shapes and no mismatched schema.
- [ ] Convert operation helpers to typed results. Preserve typed library errors until
  the tool boundary; map stable error codes there. Map protocol errors separately.
  Share `fn tool_result<T: Serialize + JsonSchema>(ToolResponse<T>) -> Result<CallToolResult, Error>`
  in the output boundary; register explicit schemas where rmcp cannot derive them.
- [ ] Normalize canonical channel/user IDs to decimal strings, message IDs to integers,
  dates to UTC RFC3339. Keep legacy IDs unchanged. Preserve partial-result metadata and
  self-contained schemas, including nested errors and media metadata.
- [ ] Test annotations: reads are accurately marked; desktop opening is a side effect;
  transcription may spend quota. Do not mark every tool read-only/idempotent indiscriminately.
- [ ] Run schema/wire/tool tests, quality gate and commit with migration documentation.

## Task 8: Preserve budgets and isolate recovery across the new contract

**Files:** `src/mcp/tools/shaping.rs`, `media_budget.rs`, `image.rs`,
`src/mcp/server/impl_media.rs`, `impl_status.rs`,
`src/mcp/observability/{buffer,metrics,transport}.rs`, request/response types,
media/search/recovery tests.
**Produces:** correctly budgeted canonical responses and explicit recovery selection.

- [ ] Add fixtures with mixed successful/missing/failed media. Each JSON item's
  `content_index` identifies its actual MCP image block; failed items have none. Binary
  appears once, and the JSON summary accounts for every requested ID.
- [ ] Apply the 40,000-byte default message response budget only to search/history
  (including fan-out) and message batches, now counting the complete envelope and both
  JSON representations. Keep oldest-first trimming and the one-message floor; metadata
  alone exceeding the budget returns bounded `response_too_large`. Test that explicitly.
- [ ] Keep media base64 limits independent; cap total serialized non-binary media
  overhead at 65,536 bytes. If it exceeds the cap, return a small tool error, no images,
  and apply no-image refunds. Test media batches larger than 40,000 bytes still succeed
  within their own limits; record overhead separately in status/docs.
- [ ] Add optional `recovery_key` to recoverable request schemas and `recovery_keys`
  selection to get_last_responses. Validate 1–128 printable ASCII characters, reserve
  before dispatch, and reject duplicate pending/retained keys before RPC. Expire completed
  entries after 10 minutes, capacity eviction, or restart; return `response_pending` or
  `response_unavailable` appropriately. Keep key state bounded by configured buffer
  capacity and reject reservations when all slots are occupied by pending calls.
- [ ] Test lost original results, reused JSON-RPC IDs, duplicate keys, expiry/eviction,
  restart, and two interleaved callers selecting only their keys. This prevents accidental
  mixing, not authenticated isolation. Calls without keys have no targeted guarantee;
  unfiltered last-N stays legacy diagnostic only. Never buffer recovery responses.
- [ ] Verify default recovery still omits image bytes and does not recursively buffer
  recovery results. Add tests for new structured metadata and oversized replay entries.
- [ ] Test input/concurrency bounds, cancellation accounting, decoded-image pixel/memory
  limits, and session/database/WAL permissions with disposable fixtures. Fix demonstrated
  weaknesses in focused changes; record any investigation that does not reproduce a defect.
- [ ] Execute media proposal M1: calculate base64 length before encoding; replace
  whole-batch raw-download retention with a bounded producer/consumer pipeline. Preserve
  image content blocks and single resolve/fetch per batch. Test global backpressure,
  deterministic order, concurrent calls and memory retention; measure RSS separately.
- [ ] Run budget/recovery/transport/media tests and quality gate; commit with explicit
  compatibility notes for changed byte accounting and recovery inputs.

## Task 9: Establish continuous security and dependency maintenance

**Files:** `.github/workflows/ci.yml`; create `.github/workflows/security.yml` and a
dependency-update configuration; `rust-toolchain.toml`, `justfile`, README/docs.
**Produces:** repeatable maintenance rather than a one-time update.

- [ ] Pin reviewed action SHAs (checkout snapshot v7.0.1); set minimal read permissions
  and disable credential persistence in checks. Verify updated runner/runtime requirements.
- [ ] Pin one tested current dated nightly in local and CI configuration; retain a
  separate scheduled floating-nightly check. Do not switch project toolchain policy.
- [ ] Add fresh advisory scans on PRs and a schedule, with vulnerability and unsoundness
  handling explicit. Any exception names advisory, rationale, owner and expiry.
- [ ] Add reviewed dependency update automation, with rmcp, grammers, and incompatible
  updates separated. Grammers revisions require review; do not automate tracking master.
- [ ] Introduce source/license/yank checks only with an explicit policy covering the
  existing Codeberg source; no blanket unknown-license approvals. Keep the normal gate fast.
- [ ] Validate workflow/config syntax and run offline quality checks on Linux/macOS.
  Document optional Windows support as unverified unless it is tested. Commit automation.

## Task 10: Final acceptance and independent review

**Files:** README, CHANGELOG, configuration examples, `docs/memory.md`, active plan/spec.
**Produces:** evidence-based release readiness, not automatic publication.

- [ ] Recheck all versions/advisories at release time; record installed/resolved values
  and any incompatible transitive blockers. Every direct crate has an explicit disposition.
- [ ] Run `cargo fmt --all --check`, `cargo clippy --locked --all-targets -- -D warnings`,
  `cargo test --locked`, `cargo build --locked --release`, fresh `cargo audit`, and the
  modern/legacy conformance suite. Use `--offline` for cached compile/test work only;
  advisory freshness still needs an updated database.
- [ ] Exercise the wire harness with at least two client implementations. If a live
  Telegram smoke check is later requested, use an already authorized test account and
  bounded reads; report it separately from offline results.
- [ ] Measure changed RPC/conversion coverage and representative latency/memory. Keep
  source-to-test traceability for A1–A9/D1–D2; do not substitute a global coverage number.
- [ ] Obtain independent code/security review, inspect the final diff, and update docs
  with protocol versions, output migration, configured bounds and known limitations.
- [ ] Review adoption against the Rust manual and S1–S6 plan, including justified exceptions;
  keep existing-format and stricter-policy checks distinct in the recorded results.
- [ ] Release only under subsequent authorization. Delete active implementation plan/spec
  when their change merges, per repository policy; Git history retains the audit.

## Effort and uncertainty

This is several reviewable work packages, not a dependency-bump afternoon. For an engineer
familiar with the repository, rough planning ranges are: security/correctness 3–6 days,
protocol/dependencies 3–6 days, output migration/hardening 4–8 days, automation/review 2–4
days. Total roughly **12–24 engineering days**, excluding optional extensions and external
review delays. These are estimates, not measured durations; grammers churn and client
compatibility are the largest uncertainties. Re-estimate after Tasks 1–2 and 5–6.
The subsequent Rust-standard request adds roughly **3–6** incremental days. Media M1
adds a **2–4 day work package**, partly already covered by Task 8; do not simply add all
ranges because safety, API and media-budget work overlaps. Optional artifact/file modes
need separate estimates after confirming client capabilities.
