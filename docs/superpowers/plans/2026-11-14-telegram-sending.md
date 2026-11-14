# Telegram Sending Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add opt-in text, image, audio, voice and video sends to channels, groups, private chats and Saved Messages with bounded resources and truthful delivery outcomes.

**Architecture:** Keep the current authenticated MTProto client and its read resolver. A separate write resolver retains one qualified recipient, a private staging service supplies immutable upload snapshots, and a raw final-send adapter bypasses application retries. Thin canonical MCP routes validate and budget locally, acquire once, then call typed client operations under one absolute deadline.

**Tech Stack:** Rust edition 2024, nightly-2026-10-31, Tokio, grammers 0.11 at the existing pin, rmcp 3.8, serde/schemars v1, getrandom 0.4.3, existing mockall/offline fixtures.

**Spec:** [Approved sending design](../specs/2026-11-14-telegram-sending-design.md), approved 2026-11-14 including its defaults. Read both documents before execution.

## Global Constraints

- Keep all three grammers pins at `1e31e73cd9797790905bbea82cd75adcbe6f1242`; no new third-party dependency, unsafe code, external metadata program, or lockfile regeneration.
- Run local validation on macOS; Linux is owner-managed. No authentication, credentials, production filesystem fixtures or live Telegram sends during implementation.
- Default `[sending].enabled = false`; disabled catalogs have 16 tools, enabled canonical catalogs 18. Enabled sending with `output_format="legacy"` is invalid. Support MCP `2025-11-25` and `2026-07-28`.
- Two immediate tools, one recipient, one attachment maximum: `send_message`, `send_media`; plain text, no formatting parsing, splitting, truncation, alternate sender, scheduling, queue, paid send or draft clearing.
- Text: nonblank, at most 4,096 UTF-16 units. Caption: at most 1,024. Audio title/performer: at most 256 each. Preserve accepted content verbatim.
- Qualified positive decimal recipient IDs, explicit username namespace, no bare numeric destination, title/phone/invite resolution or Bot API `-100...` coercion. Keep the existing read resolver unchanged.
- Image JPEG/PNG, audio MP3/M4A/AAC, voice Ogg/Opus, video MP4 with caller-supplied metadata. Audio/voice duration is `1..=i32::MAX`; video duration finite and positive; width/height `1..=8192`. Probe bounded headers, not full codecs.
- Images: `10485760` bytes maximum; other media: `104857600`. Defaults: two concurrent media operations, `209715200` aggregate staging bytes, `524288` copy buffer. Caps may decrease, never increase; concurrency is one or two.
- One operation deadline starts after basic decode and before local admission/staging: text `30` seconds, media `120`. Existing resolve/history caps also apply within its remaining time. Check expiry before every new stage/RPC and at completion.
- Recovery reservation/decode → enable/mode/local validation and worst-case wire budget → bounded local staging → one nonblocking account acquisition → resolve/reply/topic reads → upload → final dispatch → acknowledgement/receipt. No Telegram RPC before acquisition.
- Default account costs: text `1`, media `3`; nonzero and at most bucket capacity. Never refund an acquired send charge. Preserve every existing read/download refund rule.
- Exactly one final application submission with one nonzero CSPRNG `i64` random ID. No connector resend and no final-send `AutoSleep`; any lower-level MTProto retransmission preserves the serialized request/random ID. Upload helper retries are confined to the same deadline before final dispatch.
- A matched positive message ID and actual positive Telegram date are required for `sent`. A final send that may have committed is `delivery_unknown`, `delivery_state="unknown"`, `retryable=false`; definitive refusals or pre-dispatch failures are `not_sent`.
- Canonical receipts contain no text, caption, filesystem path, access hash or bytes. All errors/logging discard raw provider payloads and source chains. Recovery remains bounded and process-local, not an idempotency ledger.
- Explicit topic routing for every forum send, including General; validate destination/reply/topic before upload. Never redirect migration, substitute a recipient or fall back to another thread.
- Source roots are explicit existing absolute directories; private staging is separate and initially empty. Enforce existing ownership/ancestry/macOS ACL rules, descriptor identity, single regular files and create-new `0700`/`0600` storage. Cleanup and started blocking work retain permits/quota until actually finished.
- Relevant behavior tests precede production changes. Run the full locked offline quality gate before implementation commits. Delete this plan and its spec on eventual merge; preserve history as the archive.

## Review Focus

1. A raw error or response serialization failure after final dispatch must not escape through the generic retryable timeout/internal-error renderer (Tasks 1, 4, 7).
2. A username rebound, colliding numeric peer ID, General-topic send or cross-topic reply must retain the exact intended peer/thread or fail before upload (Task 3).
3. A protected path that does not exist yet, a credential hardlink, an inherited granting ACL, or a source/staging overlap must fail local admission without reading secrets or spending tokens (Tasks 2, 5).
4. Cancellation during a started copy/upload and failed cleanup must not release capacity while work/files still occupy it (Tasks 5–7).
5. A tiny response budget or long escaped request ID must refuse before a mutation, and a lost/cancelled recovery entry must never be evidence of non-delivery (Tasks 4, 7).

---

## Execution setup and file ownership

This is one feature with shared outcome/deadline contracts, implemented in order; it does not require separate subsystem plans. Work on a feature branch, retaining unrelated changes. The current clean branch is `docs/telegram-sending-spec`; recheck state before execution and use the worktree skill if isolation is needed. Do not create another worktree just to write this plan.

The existing `ast-index stats` command is blocked by its cache lease permissions in this sandbox. At execution time retry stats, then update/rebuild before indexed navigation; if still blocked, report it and use targeted `rg`/reads. No graph is needed for this plan.

| Files | Responsibility / owner task |
| --- | --- |
| `src/telegram/types/sending.rs`, `src/telegram/sending.rs` | Validated send domain, shared deadline and failure truth (1). |
| `src/telegram/client/send_transport.rs`, `send_receipt.rs` | Public thin-handle final submission and pure typed acknowledgement/error mapping (1). |
| `src/config/{types,defaults,validation,loading}.rs`, `src/config.rs`, `src/sending.rs` | Opt-in configuration, protected path policy and shared runtime settings (2). |
| `src/mcp/tools/types/sending.rs`, `contract.rs`, type facades | Strict wire request/receipt contract and safe send rendering (2, 4). |
| `src/telegram/client/resolve_recipient.rs`, `send_targets.rs` | Write-specific resolution and bounded reply/forum validation (3). |
| `src/telegram/client/ops_send.rs`, `src/telegram/{client,trait_def}.rs` | Typed send orchestration beneath whole-client mocks (4, 6). |
| `src/mcp/server/impl_send.rs`, `src/mcp/tools/send_budget.rs`, `src/mcp/server.rs` | Local preparation, charging, visibility, routes and receipt budgets (4, 6). |
| `src/sending/{staging,privacy,probe}.rs` | Private upload snapshots, resource accounting and bounded probes (5). |
| `src/telegram/client/send_media.rs` | Streaming upload and exact media TL attributes (6). |
| `src/mcp/server/impl_recovery.rs`, `impl_status.rs`, runtime integration | Write-aware early failures, recovery, cached availability and shutdown (4, 7). |
| Sibling `#[path]` test files, `src/mcp/tests/`, existing examples | Offline behavior and real wire validation owned by the corresponding task. |
| README, contract/API/config docs, validation report | Usable instructions and evidence (8). |

New APIs below are implementation decisions within the approved design, not additional product choices. Keep new helpers crate-private except types required by public trait/builder signatures. No `mod.rs`; declare focused sibling modules and tests in their owning facade. During Tasks 1–5 incomplete media behavior stays disabled and returns a typed pre-dispatch refusal.

## Shared interface decisions

- `telegram::types::sending::QualifiedRecipient::try_new(namespace: PeerNamespace, id: i64) -> Result<Self, SendFailure>` validates positive IDs through the existing domain rules. Fields are private; expose `namespace()` and `id()`. Any serde implementation delegates to construction.
- `RecipientSpec::{SavedMessages, Username { username: Username, expected_namespace: UsernameNamespace }, Peer(QualifiedRecipient)}`; `UsernameNamespace::{User, Channel}`. No name-based or bare-ID alternate constructors.
- `SendOptions { recipient: RecipientSpec, reply_to_message_id: Option<MessageId>, topic_id: Option<MessageId>, silent: bool }`; `SendText { options: SendOptions, text: String, link_preview: bool }`.
- `SendMediaKind::{Image, Audio { duration_seconds: i32, title: Option<String>, performer: Option<String> }, Voice { duration_seconds: i32 }, Video { duration_seconds: f64, width: u32, height: u32 }}`; validated construction rejects impossible values. `ContentKind::{Text, Image, Audio, Voice, Video}`.
- `SentMessage { recipient: QualifiedRecipient, saved_messages: bool, message_id: MessageId, date: DateTime<Utc>, content_kind: ContentKind, reply_to_message_id: Option<MessageId>, topic_id: Option<MessageId>, message_link: Option<String>, media: Option<SubmittedMedia> }`; `SubmittedMedia { mime_type: String, size_bytes: u64 }` describes submitted bytes.
- `SendFailure` is a separate public typed failure with private fields and safe `Display`/`Debug`/error sources. `SendFailureKind::{SendingDisabled, RecipientNotFound, RecipientMismatch, SendForbidden, InvalidReplyTarget, UploadSourceInvalid, UnsupportedMedia, UploadFailed, SendCapacityReached, DeliveryUnknown, InvalidInput, MediaTooLarge, RateLimited, Unauthenticated, Timeout, ResponseTooLarge, Unavailable, InternalError}` maps to the corresponding §8 snake-case codes. `SendFailure::not_sent(kind: SendFailureKind, retry_after_seconds: Option<u64>) -> Self` and `SendFailure::unknown() -> Self` are the constructors; `not_sent` must canonicalize `DeliveryUnknown` to unknown. Expose `kind()`, `delivery_state() -> FailureDeliveryState`, `retryable() -> bool`, and `retry_after_seconds() -> Option<u64>`. `FailureDeliveryState::{NotSent, Unknown}` has no success variant. Unknown always fixes code/state/retryability to `delivery_unknown`/`unknown`/false; retryability otherwise follows the spec's typed policy, never arbitrary caller input.
- `telegram::sending::SendDeadline::new(timeout_secs: u64) -> Result<Self, SendFailure>`, `instant() -> tokio::time::Instant`, `expired() -> bool`, and `run(stage: SendStage, future: impl Future<Output = Result<T, SendFailure>>) -> Result<T, SendFailure>`. `SendStage::{LocalPreparation, Resolve, ValidateTargets, Upload, FinalSend, Receipt}` supplies fixed safe diagnostics and dispatch-aware expiry behavior. Zero/overflowing durations fail locally. A single shared instant survives every stage; final-send/receipt expiry is unknown.
- `SendingRuntimeConfig::try_from_config(config: &Config, config_path: &Path) -> Result<Self, SendFailure>` in `src/sending.rs` copies settings, costs, timeouts and protected paths after CLI overrides. `McpServer::with_sending(self, config: SendingRuntimeConfig) -> Result<Self, SendFailure>` is async and fallible; default servers remain disabled. It initializes only configured enabled media storage, validates final output mode, and shares state across clones.
- New public client methods: `async fn send_message(&self, request: &SendText, deadline: SendDeadline) -> Result<SentMessage, SendFailure>` and `async fn send_media(&self, request: &PreparedMediaSend, deadline: SendDeadline) -> Result<SentMessage, SendFailure>`. Default trait bodies return `sending_disabled` for compatibility with third-party read-only implementers; production overrides both. They never acquire/refund MCP tokens themselves.
- `PreparedMediaSend { options: SendOptions, caption: String, media: SendMediaKind, staged: Arc<StagedUpload> }`. `StagedUpload` is public only because it crosses the trait; its fields and constructor remain private to validated staging. It exposes safe size/MIME/name accessors and an async verified file reader, never an arbitrary path constructor.

## Task 1: Prove public final-send feasibility and delivery truth

**Files:** Create `src/telegram/{sending.rs,types/sending.rs,client/send_transport.rs,client/send_receipt.rs}` and `src/telegram/client/tests/send_transport_tests.rs`; modify `src/telegram.rs`, `src/telegram/types.rs`, `src/telegram/client.rs`, `src/telegram/client/lifecycle.rs`, `src/telegram/client/tests/client_fixture.rs`; create `docs/validation/2026-11-14-telegram-sending.md`.

**Interfaces:** Produces the domain result/failure/deadline types above, `FinalSendAdapter::submit(&self, dc_id: i32, body: Vec<u8>) -> Result<Vec<u8>, grammers_mtsender::InvocationError>` (async), and `map_ack(updates: tl::enums::Updates, expected: &ExpectedSend) -> Result<SentMessage, SendFailure>`. `ExpectedSend` retains qualified recipient, one random ID, content kind, options, optional submitted-media facts and a locally bounded optional link. Keep all adapter/mapping helpers crate-private.

The one-shot orchestration seam is `async fn submit_final_with<F, Fut>(prepared: PreparedFinalSend, deadline: SendDeadline, submit: F) -> Result<SentMessage, SendFailure>` where `F: FnOnce(Vec<u8>) -> Fut` and `Fut: Future<Output = Result<Vec<u8>, InvocationError>>`; `PreparedFinalSend { body: Vec<u8>, expected: ExpectedSend }` is private. The production closure captures the resolved DC and thin handle. No prepared request can be submitted twice by this helper.

- [x] **Step 1: Write failing `sending_transport_` tests.** Construct pinned TL fixtures asserting: short-sent positive ID/date succeeds; zero/negative ID/date fails unknown; full/combined Updates require the matching random-ID mapping and new-message destination; wrong namespace, random ID, message ID, missing date, empty message and unrelated updates fail unknown. Conflicting mappings/messages fail unknown even if one candidate looks valid. Table-test allowlisted typed `InvocationError::Rpc` refusals versus unknown unrecognized errors; flood/slow-mode preserve retry delay without sleeping.

  Representative assertions in those fixture tests:
  ```rust
  assert_eq!(map_ack(short_sent, &expected).expect("matched ack").message_id.get(), 42);
  let failure = map_ack(wrong_random_id, &expected).expect_err("unmatched ack");
  assert_eq!(failure.delivery_state(), FailureDeliveryState::Unknown);
  assert!(!failure.retryable());
  ```
- [x] **Step 2: Run the red tests.** `cargo test --offline --locked sending_transport_`; expect compile/test failure for the absent interfaces or rejected expected behavior, not an unrelated build/environment error.
- [x] **Step 3: Implement the domain, deadline and raw adapter.** Clone `pool.handle.thin` before `Client::new(pool.handle)`, retain `SenderPoolHandle` in `TelegramClient`, and update the stalled-client fixture. Use the public session `home_dc_id()` before dispatch and TL serialization/deserialization traits. Final requests use `raw_invoke_in_dc` directly, never `Client::send_message`, `invoke_in_dc`, or `do_invoke_in_dc`. Use one nonzero CSPRNG ID; serialization/storage/CSPRNG failures occur before dispatch. Set dispatch immediately before invoking the one-shot submit closure; any ambiguous subsequent failure is unknown. Pure preparation supplies all bounded receipt fields before that boundary. Do not log raw Updates/provider errors.
- [x] **Step 4: Prove the seam offline.** Add `sending_transport_single_submit`, `sending_transport_expired_before_dispatch`, `sending_transport_timeout_after_dispatch`, and `sending_transport_public_api_compiles`. A `FnOnce` submit future records one body and can return typed bytes/errors or stall; assert one application submission, unchanged random ID and unknown after dispatch. Exercise the actual public raw method with an unpolled retained runner to verify compilation/cancellation without network I/O. Inspect the pinned uploader: Tokio `AsyncRead`, `512 KiB` parts, four workers above `10 MiB`. Record what source inspection and the fixture establish; do not claim live resend/playback validation.
- [x] **Step 5: Run green tests and gate, inspect, commit.** Run the focused command and the implementation gate below. Commit `feat: add bounded raw send adapter and delivery outcomes`. If public-API feasibility fails, stop dependent tasks and resolve the finding; never substitute the high-level resend path.

## Task 2: Establish opt-in settings and strict canonical requests

**Files:** Modify `src/config/{types,defaults,validation,loading}.rs`, `src/config.rs`, `src/lib.rs`, `src/mcp/tools/types.rs`, `src/mcp/tools/types/contract.rs`; create `src/sending.rs`, `src/mcp/tools/types/sending.rs`, `src/config/tests/sending_tests.rs`, `src/mcp/tools/types/tests/sending_tests.rs`; register sibling tests in their owning modules.

**Interfaces:** Consumes Task 1 domain types. Produces `SendingConfig` with the exact §9 fields/defaults, extended `TimeoutConfig`/`RateLimitConfig`, `SendingRuntimeConfig`, strict `SendMessageRequest`/`SendMediaRequest` and `SendReceipt`. DTO `try_into_domain(self)` returns validated domain input or `SendFailure`; media conversion retains a validated `UploadSource { root: String, relative_path: PathBuf }` for Task 5. `ToolError::from(SendFailure)` adds the optional failure delivery state while `From<Error>` always omits it. Export only intentional public types.

- [x] **Step 1: Write failing `sending_config_` and `sending_request_` tests.** Assert disabled/no-roots/no-staging defaults, `30`/`120` timeouts, `1`/`3` costs and exact byte/concurrency defaults. Reject legacy-enabled, zero/overflowing deadlines/costs, cost over bucket capacity, overlarge caps, concurrency outside `1..=2`, insufficient staging bytes for the largest configured kind, roots without staging and duplicate/invalid aliases. Text-only enabled needs no directory. Existing config fixtures still load with sending absent.
- [x] **Step 2: Add request assertions and run red.** Test every tagged destination/media variant; missing username namespace; numeric strings and safe scalar coercion; out-of-range/negative/Bot-API IDs; whitespace-only text; `"🦀".repeat(2048)` accepted versus `2049` rejected; caption `512` versus `513` emoji; metadata bounds including NaN/infinity at Rust construction; typo fields at every nested object. Run `cargo test --offline --locked sending_config_` and `cargo test --offline --locked sending_request_`; expect meaningful red.

  ```rust
  let config: Config = toml::from_str("[telegram]\napi_id = 1").expect("minimal config");
  assert!(!config.sending.enabled);
  assert_eq!(config.telegram.timeouts.send_secs, 30);
  assert_eq!(config.telegram.timeouts.upload_send_secs, 120);
  assert_eq!(config.sending.max_staging_bytes, 209_715_200);
  let request: SendMessageRequest = serde_json::from_value(serde_json::json!({
      "recipient": {"kind": "saved_messages"}, "text": "🦀".repeat(2049)
  })).expect("decodes before domain validation");
  assert!(request.try_into_domain().is_err());
  ```
- [x] **Step 3: Implement configuration and DTOs.** Apply `#[serde(deny_unknown_fields)]` to every new object/variant, and `#[schemars(inline)]` to request unions/enums; use existing scalar boundary helpers without weakening domain types. Reply/topic IDs must additionally fit positive `i32` through `MessageId::as_i32()`, not just the broader domain `i64`. Keep `recovery_key` wire-only: actual dispatch removes it before strict decode and advertised schemas still add it. Resolve configured aliases as unique ASCII `[A-Za-z0-9_-]{1,64}`; reject duplicate alias definitions through normal TOML duplicate-key validation. Preserve the exact §8 receipt field names, string peer IDs, UTC dates and optional submitted-media object. New `ToolError` state accepts only `not_sent|unknown`, never `sent`.
- [x] **Step 4: Capture effective protected paths.** Add `Config::selected_path(explicit: Option<&Path>) -> anyhow::Result<PathBuf>` in `loading.rs`; use it both in `load_from` and `main` later so staging policy receives the actual selected config path. `SendingRuntimeConfig::try_from_config` receives that path after CLI session overrides and records config, DB/sidecars and log locations. Canonicalize existing ancestors when protected leaf files are absent; final filesystem/ACL validation belongs to Task 5. Disabled config performs no storage creation or scan.
- [x] **Step 5: Run focused green, gate, inspect and commit.** Check every explicit struct literal affected by added timeout/cost fields, including fixtures/examples. Commit `feat: define opt-in sending configuration and schemas`.

## Task 3: Resolve recipients and validate exact reply/topic routing

**Files:** Create `src/telegram/client/{resolve_recipient.rs,send_targets.rs}`, `src/telegram/client/tests/{recipient_tests.rs,send_target_tests.rs}`; modify `src/telegram/client.rs`. Reference `resolve.rs`, `raw_fetch.rs`, `converters.rs`, `link.rs` without changing existing read behavior.

**Interfaces:** `resolve_recipient(&self, recipient: &RecipientSpec, deadline: SendDeadline) -> Result<ResolvedRecipient, SendFailure>` and `validate_send_targets(&self, recipient: &ResolvedRecipient, options: &SendOptions, deadline: SendDeadline) -> Result<ValidatedTargets, SendFailure>` (async). `ResolvedRecipient` privately retains the real peer reference, qualified identity, Saved Messages marker, forum/migration state and bounded optional link facts. `ValidatedTargets` carries a validated optional `tl::enums::InputReplyTo`, never a fallback destination.

- [x] **Step 1: Write failing `sending_recipient_` tests.** Use pure selectors plus injected bounded lookup futures under the production resolver: user/chat/channel collision at the same ID; duplicate dialog entries; wrong username namespace; missing access reference; migrated/deleted peer; username changes after one resolution; Saved Messages resolves the connected account. Assert one retained recipient, one bounded dialog traversal where required, no invented access hash/import/join, and no wrong-namespace fallback. Run `cargo test --offline --locked sending_recipient_` and confirm red.
- [x] **Step 2: Implement recipient selection and bounded lookup.** Resolve username once with a write-specific redacted error boundary, not the existing logging resolver. Qualified IDs use a usable session reference or a single deadline-capped dialog walk including users. Read the pinned public session/peer APIs; do not manufacture peer references. Saved Messages uses the connected self identity. Keep `resolve.rs` read contracts and no-user rules intact. Map all lookup/storage/provider failures to safe typed send failures before dispatch.
- [x] **Step 3: Write failing `sending_targets_` tests.** Assert forum destinations require explicit topic, nonforums reject it, missing/deleted/closed topics fail, and a reply in another peer/topic fails. Cover General without requiring a normal root message at ID 1, General closed/hidden state according to pinned TL fields, replying to a topic root, replying within General, and stale permission/topic races. Verify no upload/final-submit future is started on failure. Run `cargo test --offline --locked sending_targets_` and confirm red.

  In the closed-topic and cross-topic fixtures, assert the production validator's result:
  ```rust
  let failure = client.validate_send_targets(&recipient, &options, deadline)
      .await.expect_err("invalid target");
  assert_eq!(failure.kind(), SendFailureKind::InvalidReplyTarget);
  assert_eq!(failure.delivery_state(), FailureDeliveryState::NotSent);
  ```
- [x] **Step 4: Implement target reads and TL routing.** Use pinned `messages.GetForumTopicsById`/generated equivalent and the returned forum-topic semantics; General is not established by ordinary message existence. Fetch replies with the namespace-correct raw get-messages method, reject `MessageEmpty`, and check actual reply/top-thread metadata against the selected topic. All reads consume the shared deadline plus resolve/history stage caps. Build `InputReplyToMessage` using the validated root/top-message fields; no topic-to-main-chat fallback. Races after validation remain authoritative Telegram failures.
- [x] **Step 5: Run focused green and existing resolver/history tests, gate, inspect and commit.** Commit `feat: resolve write recipients and validate reply targets`. Keep routing tests beneath `MockTelegramClientTrait`, where production selection decisions execute.

## Task 4: Deliver bounded canonical text sends

**Files:** Create `src/telegram/client/ops_send.rs`, `src/mcp/server/impl_send.rs`, `src/mcp/tools/send_budget.rs`, `src/mcp/tests/sending_text.rs`, `src/telegram/client/tests/send_text_tests.rs`; modify `src/telegram/{client,trait_def}.rs`, `src/mcp/{server,tools,tests}.rs`, `src/mcp/server/{impl_recovery,impl_status}.rs`, `src/mcp/tools/types/{sending,contract,responses,canonical}.rs`, `src/main.rs`, and affected existing schema/count/status fixtures.

**Interfaces:** Implements the `send_message` trait method and `McpServer::send_message_data(&self, request: SendMessageRequest) -> Result<SendReceipt, SendFailure>` (async), plus `with_sending`. `send_budget::preflight(bytes: usize) -> Result<(), SendFailure>` measures bounded worst-case send success/error via active runtime response context. `render_send(result: Result<SendReceipt, SendFailure>) -> CallToolResult` is canonical-only and never falls back to a generic post-dispatch failure.

- [x] **Step 1: Write failing `sending_text_` tests.** Real MCP calls with production local preparation and mock client/limiter assert: disabled/mode/decode/UTF-16/budget failures have zero charge and zero RPC; valid calls acquire exactly `send_message_cost` once before any lookup; success returns matching structured/text envelopes and real identity; pre-dispatch timeout is `not_sent`; after-dispatch timeout/parse failure is unknown and nonretryable. On source-independent faults, inspect `isError`, error code and delivery state, not only mock calls. Run `cargo test --offline --locked sending_text_`; confirm red.
- [x] **Step 2: Implement preflight, charging and the text operation.** Start the shared deadline after decode, before admission. Preflight a worst-case success and each bounded send error with `runtime::response_size`, including actual escaped request ID, negotiated identity, duplicate JSON text and maximal positive peer/message IDs. Generate optional links only from validated existing link rules and bound or omit them before dispatch; never echo arbitrary names in errors. If even a failure cannot fit, use existing small-overflow transport behavior before charging. Run resolution/target validation, prepare all local receipt facts and serialize the exact `messages.SendMessage` request, recheck expiry, then call Task 1's single-submit adapter. Default `no_webpage=true`; honor explicit preview and silent; leave clear-draft, entities, payment, schedule and alternate-sender fields unset.
- [x] **Step 3: Integrate routes and builders.** Add both thin write entries to the existing single `#[tool_router]` impl. `send_media` validates locally and refuses as unavailable until Task 6, never attempts a text fallback. Filter disabled tools from list and enforce disable at dispatch and operation boundaries; handle direct builder output-mode changes too. Add the required write annotations (`false,false,false,true` in read-only/destructive/idempotent/open-world order). Wire effective config and actual protected path from `main` after CLI overrides. Do not create media storage for text-only/disabled setups.
- [x] **Step 4: Preserve early-failure/recovery truth and status.** `impl_recovery` must render malformed/duplicate/capacity recovery failures for known write names with `delivery_state=not_sent` for this invocation; no mutation occurred. Preserve the existing recovery error code rather than widening `SendFailureKind` just for wire-only reservation faults. Add cached `sending` availability/limits/costs to status without filesystem paths, RPCs or charges; omit the new status field in legacy output to preserve its shape. Add only the text trait method in this task, with its compatibility default; the media method waits for Task 6 after staging exists. Explicit offline implementations arrive in Task 7. Update schema/count fixtures to preserve 16 default tools and assert 18 when enabled under both protocol versions, with self-contained strict inputs and all output schemas.
- [x] **Step 5: Verify budget and rendering failure behavior.** Add `sending_text_long_escaped_request_id`, `sending_text_budget_refuses_before_charge`, and a forced receipt-serialization failure after dispatch; it must return bounded unknown, never `not_sent` or generic retryable timeout. No post-send trimming may remove required receipt identity. Run focused text, schema, parity, protocol budget and existing status tests, then gate and commit `feat: add opt-in canonical Telegram text sending`.

  Assert the real framed result for the post-dispatch fixture:
  ```rust
  assert_eq!(response["result"]["isError"], true);
  let error = &response["result"]["structuredContent"]["error"];
  assert_eq!(error["code"], "delivery_unknown");
  assert_eq!(error["delivery_state"], "unknown");
  assert_eq!(error["retryable"], false);
  ```

## Task 5: Stage immutable bounded upload snapshots

**Files:** Create `src/sending/{staging,privacy,probe,reader}.rs`, `src/sending/tests/{staging_tests,privacy_tests,probe_tests}.rs`; modify `src/sending.rs`, `src/lib.rs`, `src/mcp/server.rs`, `src/mcp/server/impl_send.rs`, `Cargo.toml`. Reference existing session/artifact privacy helpers without changing their policies.

**Interfaces:** `StagingStore::try_new(config: &SendingRuntimeConfig) -> Result<Arc<Self>, SendFailure>` (async); `stage(self: &Arc<Self>, source: &UploadSource, media: &SendMediaKind, deadline: SendDeadline) -> Result<Arc<StagedUpload>, SendFailure>` (async); `close(&self, deadline: tokio::time::Instant) -> Result<(), SendFailure>` (async). `StagedUpload::reader(self: &Arc<Self>) -> Result<StagedReader, SendFailure>` verifies descriptor/snapshot identity; safe accessors expose `size_bytes()`, `mime_type()`, `upload_name()`. `probe(file: &mut std::fs::File, kind: &SendMediaKind) -> Result<ProbedMedia, SendFailure>` inspects bounded headers only.

- [x] **Step 1: Write failing `sending_staging_`/`sending_privacy_` tests.** Disposable private fixtures cover absolute/parent/empty/path-component escape, intermediate/final symlink, hardlink, directory/FIFO/device refusal before blocking open, hostile basename, truncate/grow/same-size identity replacement, exact cap and cap+1, missing/overlapping roots, config/session/log containment (including missing leaf paths), macOS granting ACLs and unprotected/sticky immediate parents. Assert local failure before account acquisition, no source mutation, `0600` staged files, and unknown roots never touch arbitrary paths. Run the focused filters and confirm red.
- [x] **Step 2: Implement startup and descriptor validation.** Require source roots to exist and be absolute; verify canonical trusted ancestry and root/file ownership, reject granting directory ACLs, and reject submitted-path symlinks and multiply linked/nonregular files. Compare source descriptors with pre-open component identities and post-copy metadata (device/inode, size and modification facts). Protected-path comparison follows symlinks/canonical existing ancestors and excludes config, session/sidecars, log files/rotation directory and source/staging overlap. Use safe standard-library APIs under the existing trusted-ancestry/same-UID assumptions; do not introduce unsafe descriptor syscalls. Keep sources untouched.
- [x] **Step 3: Implement nonwaiting quotas and owned cleanup.** Reserve one media slot and the declared file size atomically before copying; reject insufficient quota immediately. Actual copying uses at most `524288` bytes and enforces actual cap, growth/truncation and snapshot identity. Quota/media permits move into any started blocking work, then remain with the staged snapshot until file deletion actually succeeds. Drop schedules bounded owned cleanup rather than blocking the async executor; failed deletion retains occupied quota and marks the entry unavailable. The store tracks active workers/cleanup and has no reusable filename collision. Do not sweep old directories; startup requires a fresh private empty staging root with create-new ownership lock.
- [x] **Step 4: Add bounded probe and resource tests.** `sending_probe_` covers JPEG/PNG headers/dimensions, MP3/ID3, M4A/AAC, Ogg/Opus and MP4 brands/declared kind; malformed/truncated/oversized metadata headers fail without file-sized allocation or scanning whole media. Use a `64 KiB` probe-read ceiling, controlled MIME/extension mappings and sanitized bounded ASCII upload names, never a path. This is a builder-selected probe ceiling, not full codec validation. Enable only Tokio `fs`/`io-util` features required by staging/streaming, with comments. Add two concurrent reservations and refused third, `209715200` byte quota, cancellation during a deliberately stalled copy, and failed-delete tests proving quota survives until actual cleanup. Probe cancellation/deadline checks use the same operation deadline; started work keeps its permits.

  After retaining the first two staged fixtures, pin the nonwaiting third admission:
  ```rust
  let failure = store.stage(&third_source, &kind, deadline)
      .await.expect_err("two media slots occupied");
  assert_eq!(failure.kind(), SendFailureKind::SendCapacityReached);
  assert_eq!(failure.delivery_state(), FailureDeliveryState::NotSent);
  assert!(failure.retryable());
  ```
- [x] **Step 5: Run focused green, inspect production features/lockfile, gate and commit.** Assert source bytes match the original and upload reads the staged snapshot after the source is changed. No PNG decoder production feature is needed for header-only upload inspection. Commit `feat: add private bounded upload staging`.

## Task 6: Stream image, audio, voice and video sends

**Files:** Create `src/telegram/client/send_media.rs`, `src/telegram/client/tests/send_media_tests.rs`, `src/mcp/tests/sending_media.rs`; modify `src/telegram/{client,trait_def}.rs`, `src/telegram/client/ops_send.rs`, `src/telegram/types/sending.rs`, `src/mcp/server/impl_send.rs`, and type/module declarations.

**Interfaces:** Implements `PreparedMediaSend` and the `send_media` trait method. `upload_staged(&self, staged: &StagedUpload, deadline: SendDeadline) -> Result<grammers_client::media::Uploaded, SendFailure>` (async); `prepare_media_request(uploaded: Uploaded, request: &PreparedMediaSend, recipient: &ResolvedRecipient, targets: &ValidatedTargets, random_id: i64) -> Result<tl::functions::messages::SendMedia, SendFailure>` is pure and crate-private. Reuses Tasks 1/3/4's final adapter and receipt-budget policy.

- [x] **Step 1: Write failing `sending_media_` tests.** Verify each of image/audio/voice/video produces one correct final TL mutation and one attachment/caption, with exact MIME and document attributes: photos use uploaded-photo, audio duration/title/performer, voice flag with no waveform, video dimensions/duration with `round_message=false`, `supports_streaming=false`. All flags prohibited by §4 remain unset; silent/reply/topic survive. Staging/type/metadata/capacity/budget failures spend zero tokens and start no RPC; failed reply/topic validation happens after one `3`-token acquisition but before upload. Run `cargo test --offline --locked sending_media_` and confirm red.

  Assert the resulting video receipt against the fixture's submitted bytes:
  ```rust
  let data = &response["result"]["structuredContent"]["data"];
  assert_eq!(data["delivery_state"], "sent");
  assert_eq!(data["content_kind"], "video");
  assert_eq!(data["media"]["mime_type"], "video/mp4");
  assert_eq!(data["media"]["size_bytes"], submitted_size);
  assert!(data.get("caption").is_none());
  ```
- [x] **Step 2: Implement the local/network boundary.** Complete enabled-media capability in `with_sending`; stage before account charging. Pass only `Arc<StagedUpload>` into the client. Resolve once, validate targets, open the verified staged reader, and stream using the actual pinned `Client::upload_stream`, exact snapshot size and sanitized name. No `read_to_end`, whole-video Vec or `upload_file` reopening a source path. Inspect/use public uploaded-file conversion APIs; no hidden upstream fields. Prepare the appropriate `InputMedia`, then issue one final raw SendMedia mutation with one new nonzero random ID.
- [x] **Step 3: Prove shared deadlines and failures.** Paused-time `sending_media_deadline_` tests cover staging → lookup → reply → upload worker retry wait → final dispatch without renewal. At exact expiry no continuation starts another stage. Upload-only timeout/failure is `not_sent`; after-dispatch timeout/loss/map failure is unknown. A definitive flood/slow-mode final refusal does not sleep or resend. Upload retries stay inside the same total budget; no final-send retry. Cancellation stops new stages and preserves permits for actual in-flight local work. Never refund the acquired send cost for any failing stage.
- [x] **Step 4: Verify streaming bounds without network.** Use a counting `AsyncRead` and injected upload-part sink to exercise production upload preparation and resource lifetimes, plus a compile/use test against actual pinned upload_stream with an unpolled runner. Assert per-read/part sizes never exceed `512 KiB`, four upstream active workers per large upload, no third media operation, and no file-sized buffer in connector code. Record source-inspected upstream limits separately from measured connector byte counters/RSS under two concurrent near-cap disposable streams; do not claim the injected sink measured actual Telegram buffering or a hard RSS ceiling. Any mismatch in required public APIs is a gate failure, not permission to replace streaming with whole-file reads.
- [x] **Step 5: Run focused green, media/read regressions, gate, inspect and commit.** Commit `feat: send staged Telegram image audio voice and video media`.

## Task 7: Verify real wire recovery, cancellation, shutdown and privacy

**Files:** Create `src/mcp/tests/{sending_protocol,sending_recovery,sending_shutdown,sending_privacy}.rs`; modify test declarations, `src/mcp/server/{impl_send,impl_recovery,impl_status}.rs`, `src/mcp/server.rs`, `examples/{offline_mcp_fixture,inert_server}.rs`, `examples/support/offline_client.rs`, and existing protocol fixtures as needed. Do not rewrite unrelated runtime behavior.

**Interfaces:** Reuses the completed routes, state and staging store. Add explicit deterministic send implementations to `OfflineClient` returning fixed safe receipts/failures; fixture flags enable sending only against these inert methods and require an explicit disposable staging/source root for media. Runtime teardown uses existing `DrainDeadline::get()` rather than allocating a fresh timeout.

- [x] **Step 1: Write failing `sending_protocol_` and `sending_recovery_` tests.** Exercise actual framed RPC over both negotiated protocols: 16/18 catalog, disabled direct name refusal, strict unknown field decode, annotations, matching structured/text success/error and schema validation. Recovery key reservation precedes all work; a duplicate pending/completed key adds no mutation, completed receipts recover, and cancelled/expired/evicted/restarted entries are inconclusive. Include duplicate numeric RPC IDs with distinct keys and oversized/escaped IDs. Existing reads retain legacy/canonical parity and old rate/refund behavior. Run the filters and confirm red before any production correction.

  In the duplicate-key fixture, after the first send finishes:
  ```rust
  let error = &duplicate["result"]["structuredContent"]["error"];
  assert_eq!(error["code"], "duplicate_recovery_key");
  assert_eq!(error["delivery_state"], "not_sent");
  assert_eq!(mutation_count.load(std::sync::atomic::Ordering::SeqCst), 1);
  ```
- [x] **Step 2: Write failing cancellation/shutdown tests.** Cancel before acquisition, during target read, during upload, after final dispatch and during receipt assembly. Assert no further stages; do not assert transport cancellation retracts an enqueued mutation. Recovery cancellation is unavailable/inconclusive. Run shutdown with held copy/cleanup permits and saturated output: finish within the original drain deadline, stop launching work, keep quota for unfinished cleanup, and never label post-dispatch loss `not_sent`. Zero grace stays immediate.
- [x] **Step 3: Implement only the integration corrections the failures expose.** Share staging close/cleanup with the existing artifact/runtime drain deadline; observe cleanup even on error teardown where time remains. Never grant a second grace period or block the process on a started filesystem job. Keep potentially committed sends on the unknown path through every rendering/budget/recovery branch. Preserve optional progress's existing nonblocking behavior and protocol-specific field gating.
- [x] **Step 4: Add privacy and operation-boundary tests.** Capture tracing locally (subprocess for global initialization); inject hostile filenames, bodies, captions, usernames, access hashes and raw provider errors. Assert their distinctive values never occur in serialized receipts/error chains/logs, while safe stage/category/byte/duration records remain useful. Call production send operations directly with expired/invalid typed values to ensure the MCP route is not the only guard. Status reads must cause zero Telegram RPC and zero filesystem-path disclosure.
- [x] **Step 5: Run all focused wire/resource tests, gate, inspect and commit.** Commit `test: verify sending recovery cancellation and wire contracts`. Inspect actual advertised schemas, not only standalone schema generators; retain explicit tool counts and per-tool assertions.

## Task 8: Document and independently review the completed feature

**Files:** Modify `README.md`, `config.example.toml`, `docs/{tool-contract,rust-api,memory,codex}.md`, `.codex/config.toml.example` only if its settings need changing, and `docs/validation/2026-11-14-telegram-sending.md`. Add `docs/sending.md` as the permanent operation guide; do not duplicate the entire design in memory.

**Interfaces:** Documentation describes the implemented APIs and observable behavior from Tasks 1–7. Implementation review uses `superpowers:requesting-code-review`; final verification uses `superpowers:verification-before-completion`.

- [x] **Step 1: Write the permanent guide and update public examples.** Document opt-in configuration and root alias meaning on the connector host, immediate account sending, qualified recipients, forum/General routing, caller metadata, image compression, supported MIME kinds/caps, costs, `30`/`120` total deadlines and client timeout headroom. Provide inert JSON examples for all five content kinds and Saved Messages; no account destination is selected for execution. State ordinary channel posting identity, no arbitrary files/URLs/base64, no codec/transcoding promise and no automatic safe resend after uncertainty.
- [x] **Step 2: Document delivery/recovery and operational ownership.** Explain `sent|not_sent|unknown`, recovery's ten-minute process-local retention and duplicate rejection, the danger of repeating uncertain calls, cleanup/crash fresh-root requirements and absence of a durable outbox. Keep the current 180-second client example if it covers defaults; tell operators increasing timeouts to adjust their client. Record measured offline streaming/resource results and distinguish injection/source inspection from unverified account/playback/MTProto behavior. Add only durable implemented facts to memory, replacing stale global tool-count statements with the opt-in catalog distinction.
- [x] **Step 3: Run final macOS validation and inspect the whole branch.** Execute the gate below, `cargo run --offline --locked --example inert_server`, the offline fixture/probe checks from the existing tool-contract workflow and `python3 scripts/test_mcp_probe.py target/debug/examples/offline_mcp_fixture`. Confirm every fixture path is disposable and its client inert. Review the exact base-to-HEAD diff and working tree; preserve all grammers pins and inspect any Cargo.lock/feature diff.
- [ ] **Step 4: Obtain independent whole-branch code review.** Assign review of delivery truth, namespace/topic identity, resource/cleanup lifetime, privacy and real wire behavior to a fresh reviewer through the installed review skill. Supply the approved spec, this plan, actual diff and verification evidence. Fix material findings with new failing behavior tests and rerun affected checks plus the final gate. No merge occurs without independent review; release, push/PR and live acceptance follow separately authorized scope.
- [ ] **Step 5: Commit docs and report the result.** Commit `docs: document Telegram sending contracts and offline validation`; report passed commands, review findings resolved, remaining live/platform limitations and the branch. Retain spec/plan until the feature actually merges. Do not claim live delivery, playback or Linux validation.

## Implementation gate

After a task's focused checks pass, run these before its implementation commit (do not repeat without a change/failure requiring it):

```bash
cargo fmt --all --check
cargo clippy --offline --locked --all-targets -- -D warnings
cargo test --offline --locked
RUSTDOCFLAGS="-D warnings" cargo doc --offline --locked --no-deps
git diff --check
```

Use `cargo fmt --all` before the format check after Rust edits. `cargo test` includes doctests. Documentation-only planning does not require compiling the unchanged application.

## Plan self-review and execution handoff

Coverage: spec §§1–4 → Tasks 1–4/6; §5 → Tasks 2–4/6; §6 → Tasks 2/5–7; §7 → Tasks 1/3–7; §8 → Tasks 1/2/4/7; §9 → Tasks 2/4/7/8; §10 → ownership/interfaces above; §11 → tests throughout and Tasks 7/8; §12 → the ordered feasibility gate and delivery tasks. Every Review Focus item has named test ownership above.

The plan contains eight ordered deliverables; later tasks consume the shared types and exact signatures listed here. Task 1 is a prerequisite gate before product integration. The builder may choose private helper structure and test fixture mechanics, but must not weaken outcome truth, storage boundaries or approved limits.

Recommended execution: native implementation in this session with one fresh whole-branch review, because these tasks share state/deadline/recipient contracts and are primarily sequential. If the user chooses task-by-task subagents, use the prescribed implementation/review skill and give each agent explicit file ownership, interfaces and acceptance commands. This plan authorizes no live send and does not record a chosen execution method.
