# Sending and Authentication Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Report preparation failures accurately, stop qualified lookup promptly, inspect metadata-heavy media within strict bounds, and preserve passwords exactly.

**Architecture:** Keep preparation classification separate from final mutation uncertainty. Probe already-verified staged files with bounded reads/seeks, and test the actual terminal-to-authentication submission seam without live authentication.

**Tech Stack:** Rust, existing grammers/Tokio sending deadlines, std::io Read/Seek, existing image test codec, inert sending fixture.

**Spec:** `docs/superpowers/specs/2026-11-15-review-remediation-design.md`

## Global Constraints

- Preserve sending opt-in, source-root restrictions, staging leases, secret redaction, nonretryable unknown final delivery and recovery's non-idempotency semantics.
- Validate locally before acquisition; perform every RPC after acquisition. Preserve existing media refunds, including per-item accounting.
- Preserve total operation deadlines, cancellation, bounded input, admission, memory, image processing and shutdown. No new application retries.
- Keep all three direct grammers dependencies at the same existing Codeberg revision. Add no dependency.
- Do not add production codec decoding, FFmpeg, a transport framework, or a configuration framework. Existing test-only codecs may validate generated fixtures.
- Retain a 65,536-byte cumulative inspection-read budget, with a separate maximum of 4,096 JPEG markers per probe.
- Seeking past metadata does not consume that byte budget; all offsets remain bounded by the configured file cap and the existing total send deadline.
- Validate locally on macOS. No live Telegram sends/authentication are required; no private configuration/session changes.

## Review Focus

- Upload error handling currently shares the read mapper and must keep its own stage semantics (Task 1).
- A matching qualified dialog followed by an error/pending page must succeed without polling that page (Task 1).
- JPEG dimensions before a later malformed segment must not authorize an out-of-bounds snapshot read (Task 2).
- ID3 v2.4 footer accounting and malformed synchsafe sizes must stay within the verified file length (Task 2).
- Whitespace and Unicode returned by the terminal reader must survive both success and sanitized failure paths (Task 3).

---

### Task 1: Classify preparation reads and stop qualified dialog lookup (R08, O01)

**Files:**
- Modify: `src/telegram/client/resolve_recipient.rs`, `src/telegram/client/send_targets.rs`, `src/telegram/client/send_transport.rs`, `src/telegram/client/send_media.rs` for split upload policy
- Test: `src/telegram/client/tests/recipient_tests.rs`, `send_target_tests.rs`, `send_transport_tests.rs`, `send_media_tests.rs`

**Interfaces:**
- Consumes `InvocationError`, `SendFailure::{not_sent,...}`, `SendDeadline`, existing definitive missing/reply/topic validators and final classifier.
- Produces private `ReadContext::{Recipient, ReplyTarget, TopicTarget}` and `classify_read_error(error: InvocationError, context: ReadContext) -> SendFailure`. Call production reads through this mapping; upload errors keep a separate stage-specific mapper.
- Produces a qualified next-item traversal seam used by production: `find_qualified_with<T, State, Next, Fut>(wanted: QualifiedRecipient, state: State, next: Next) -> Result<Option<T>, SendFailure>` where `Next: FnMut(State) -> Fut` and `Fut: Future<Output=Result<Option<(State, QualifiedRecipient, T)>, SendFailure>>`. Threading the owned iterator state satisfies async_trait Send without lending-future feature gates or extra locks. It runs inside the existing total/stage deadline and stops at the first exact identity. Remove obsolete selection machinery if no longer needed.

- [x] **Step 1: Add failing preparation and traversal regressions.** Inject Dropped/I/O/driver/decoding and unknown 500 provider failures below production recipient/reply/topic read mapping. Assert unavailable/not_sent, canonical retryability unchanged and no secret in Display/Debug/source/log capture. Known definitive target refusals/empty successful results retain target-specific failures; auth/flood/timeout remain distinct. Identical uncertainty after dispatch remains delivery_unknown. A first-page exact qualified match succeeds with a would-panic/error/pending next page; mismatched namespace never selects; absent match traverses under paused bounded time. Preserve upload-specific errors.
- [x] **Step 2: Run focused recipient/target/classifier tests.** Expected RED: unknown preparation failures become recipient_not_found/invalid_reply_target; matching traversal still polls the failing next item. Compile errors are not behavior evidence.
- [x] **Step 3: Implement typed preparation classification and early return.** Inspect typed provider code/name only, discard raw text and sources. Unknown preparation failures are unavailable/not_sent; do not reuse final-dispatch ambiguity fallbacks. Preserve recognized definitive/auth/flood/timeout categories and upload stage mapping. Run ordinary metadata/from_peer validation after the exact qualified match. Leave unqualified numeric read resolution's ambiguity scan unchanged.
- [x] **Step 4: Run all sending client tests, full offline tests, formatting, strict all-target Clippy and warning-free docs.** Expected all pass with no changed automatic retry policy or extra page after a match.
- [x] **Step 5: Commit `fix: preserve sending preparation failure categories`; obtain independent task review.**

### Task 2: Inspect JPEG and MP3 metadata incrementally (R09)

**Files:**
- Modify: `src/sending/probe.rs`, `src/sending/staging.rs`
- Create focused helper module under `src/sending/probe/` only if required for bounded-reader responsibility; no mod.rs
- Test: `src/sending/tests/probe_tests.rs`, `reader_tests.rs`, `fixtures.rs`, `src/mcp/tests/sending_media.rs`, `scripts/test_mcp_probe.py` or existing inert process harness
- Create: small valid MP3 fixture with documented generation/provenance under `src/sending/tests/fixtures/`
- Modify: `docs/sending.md`

**Interfaces:**
- Produces `probe(file: &mut File, kind: &SendMediaKind, snapshot_len: u64, deadline: SendDeadline) -> Result<ProbedMedia, SendFailure>` called only with verified staged identity length and the original send deadline.
- Private bounded reader tracks actual bytes read, absolute offsets and JPEG marker count. Existing accepted kinds/MIME/ProbedMedia shape remain unchanged.

- [x] **Step 1: Add failing metadata-heavy fixtures and boundary tests.** Insert valid APP/COM segments into an existing real JPEG so SOF follows >65,536 metadata bytes, and independently decode it with the existing image test codec. Generate/document a small valid MP3 with installed `lame` from a synthetic stdlib WAV, verify it using installed `mpg123`/FFmpeg solely during fixture creation, then insert a large valid ID3 tag for the regression. No runtime tool/codec dependency. Assert accepted MIME through the inert sending fixture. Cover truncated/oversized segments, missing/zero dimensions, invalid marker structure, 4,096 versus 4,097 markers, read-budget exhaustion, snapshot/file caps, valid/invalid ID3 version flags/footer/synchsafe sizes, and original cursor restoration on success/failure.
- [x] **Step 2: Run new probe/fixture tests.** Expected RED: metadata-heavy valid JPEG/MP3 rejected by fixed-prefix inspection; original cursor position lost. Record fixture validity evidence separately from probe acceptance.
- [x] **Step 3: Implement bounded incremental inspection.** Read JPEG headers and checked-seek metadata payloads inside snapshot length; count actual reads cumulatively and markers separately. Validate supported ID3 version-specific flags/length/footer, checked-skip declared tag and inspect MPEG header beyond it. Preserve PNG/AAC/MP4/Opus recognition. Check the shared deadline during bounded work. Restore original stream_position regardless of result, sanitizing failures. Keep staging/opening/identity/quota policies unchanged and CPU work on the existing bounded blocking path. Update the fixed-prefix guide description.
- [x] **Step 4: Run probe, reader, staging and MCP sending tests plus inert process acceptance.** Then run full Rust gate. Expected accepted large metadata and bounded failures all pass; actual bytes validate independently; no live account use.
- [x] **Step 5: Commit `fix: bound media probes while skipping large metadata`; obtain independent task review.**

### Task 3: Preserve terminal password bytes (R10)

**Files:**
- Modify: `src/telegram/auth.rs`
- Test: inline tests or `src/telegram/tests/auth_tests.rs` via sibling #[path]

**Interfaces:**
- Consumes the terminal Password reader and `TelegramClient::check_password`, which already forwards exact `as_bytes()`.
- Produces a private reader/submission orchestration seam used by the real interactive branch. The supplied `String` passes owned or borrowed without trim/normalization to the check closure; retain existing public authentication API.

- [x] **Step 1: Add a failing captured-submission test.** Exercise the production interactive-password orchestration with injected reader/check closures. Assert byte-for-byte equality for `" password "`, tabs, Unicode whitespace and newline-as-data supplied below the terminal seam. Capture diagnostics on input/submission failure and assert no credential is present. Keep login-code handling unchanged.
- [x] **Step 2: Run the exact new auth test.** Expected RED: the real current branch trims the password before submission.
- [x] **Step 3: Remove trimming at the terminal-to-client boundary.** Let the input library handle terminal submission; do not normalize any password character or add credential storage/logging. The seam must drive the actual interactive branch rather than a standalone unused identity helper.
- [x] **Step 4: Run auth/client-auth tests and full Rust gate.** Expected all pass with safe errors and exact submission bytes.
- [x] **Step 5: Commit `fix: preserve authentication password whitespace`; obtain independent task review.**

## Completion

Record A08–A10/A14 evidence in task reports. Continue with configuration and maintenance/performance plans. The final integration task owns full acceptance, Unreleased notes and whole-branch review; no merge/push is authorized by these plans.
