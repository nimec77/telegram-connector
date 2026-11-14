# Project Memory — Telegram MCP Connector

Distilled project knowledge: current state, durable decisions, and hard-won lessons.
Keep this file compact — add only durable facts, dedupe on write, delete what stops being
true. (The full historical journal lives in git history, pre-2026-08-15.)

## Current state

- **Opt-in sending:** canonical-only `send_message`/`send_media` extend the default 16-tool
  catalog to 18 when enabled. Qualified recipients include users/groups/channels/Saved
  Messages; local media uses configured roots and a private bounded snapshot store.
  See [sending](sending.md) and its [offline validation](validation/2026-11-14-telegram-sending.md).
  Unknown final dispatch is nonretryable; recovery is not a durable idempotency ledger.
  Native logging suppresses payload-bearing SDK/MTProto worker diagnostics.


- **Optional media artifacts:** M2/M3 are integrated into local `master` as 0.25.0,
  with default inline previews, optional private cached resources/file export and bounded
  cleanup. DeepSeek Harness 0.2.1-rc.5 installed components passed actual connector-fixture
  byte/resource/image checks without model/UI/account execution. Use the permanent
  [media guide](media-artifacts.md) and [verification report](validation/2026-11-01-media-artifacts.md).
  Final independent review and scoped fix review passed on 2026-11-02; the 0.25.0 macOS
  gate passed 995 library tests + 3 doctests. Completed guide/media plans and specs are
  archived in Git history at the integrated release revision 84e170c. This local merge
  does not publish a release or update the remote.

- **All ten original roadmap tasks are implemented, accepted and integrated into
  master.** Their 0.24.1 acceptance/fix release was pushed; its changelog includes
  media-download regression tests, the animation classification fix and private
  session-parent hardening.
  Completed plans/specs are archived in Git history at accepted revision adaaac8.
  All ten numbered tasks have explicit acceptance evidence. Final post-review macOS gate passed
  942 library tests + 3 doctests, strict all-target Clippy, warning-free docs, production
  release, independent Rust/Python clients and native process checks. Fresh pinned
  cargo-deny/source/advisory/license gates pass. The final combined review's shared-sticky-
  parent sidecar finding was regression-fixed. See [final acceptance](validation/2026-11-01-final-acceptance.md),
  [Task 9 evidence](validation/2026-11-01-task9-workflows.md), and
  [Rust/S1–S6 assessment](validation/2026-11-01-rust-adoption-acceptance.md).
  There are 16 default tools (18 with canonical sending enabled), json_v1 plus explicit legacy reads; nightly stays 2026-10-31, rmcp 3.8,
  and direct grammers pins stay 1e31e73cd9797790905bbea82cd75adcbe6f1242 (0.11).
- **Pre-artifact media-test coverage baseline (2026-11-01):** final acceptance reports 82.58%
  library lines (9,301/11,263, unfiltered including inline unit code), with the original
  ops_media.rs at 51.62% (143/277), up from the integrated 0.24.0 baseline of 81.71% overall
  and 15.85% (39/246) operations coverage. Cached photo/video fixtures use the actual
  downloader without polling a sender. Thin live resolution/fetch, upstream Message
  extraction and some defensive branches remain unexercised. See the
  [scoped report](validation/2026-11-01-media-download-coverage.md). Task 9/10 closure is now recorded separately in final acceptance.
- **Audit 2026-08-15 is fully shipped and its spec deleted** (delete-on-merge; the file lives
  in git history). Four staged work packages — correctness fixes + dead code; module splits +
  test extraction; duplication/KISS refactors; ops-layer coverage via `MessageWalk` — plus the
  11-item hygiene backlog.
- **Code review 2026-08-22 is addressed and its findings doc deleted** (delete-on-merge).
  Deliberately *not* changed: fan-out failed-channel refunds (see Rate limiter below),
  the per-response re-serialization in `InstrumentedTransport` (it feeds the `bytes`
  log field, not only the recovery buffer), and `ResponseBuffer::is_empty` (clippy's
  `len_without_is_empty`).

## Open items

- **Operational limits:** chosen live model/desktop UI and Telegram/account behavior
  remain unverified; offline Rust/Python payloads and installed DeepSeek adapter
  rendering/attachment checks passed.
  Linux remains owner-managed and Windows ACLs unverified. Release publication requires
  separate authorization.
- **Raw pagers exist only because grammers pins `Message.peers: PeerMap` `pub(crate)`**
  (verified at pinned rev = upstream HEAD). If a future grammers rev exposes public peer
  resolution, collapse `raw_pager`/`raw_fetch` back to the high-level iterators.
- Channel-scoped `messages.Search` also carries `min_date`/`max_date` but they're
  deliberately not sent (path measured ~1 s). If it ever shows up slow, push the bounds onto
  `RawChannelSearchPager` first — not a redesign.

## Key decisions

- **Media artifacts:** opt-in per-process private empty root, create-new lock, 0700/0600,
  strict opaque URI lookup and no arbitrary-path tool arguments. Default TTL 600s,
  100 entries/256 MiB; oldest quota eviction may precede TTL. Current-response pins
  protect handles through assembly; insufficient capacity or expired/unavailable delivery
  becomes a typed per-item failure with batch refunds. Reads never re-download.
  Logical expiry is immediate; physical deletion can lag 60s plus worker admission.
  Shutdown shares the original drain deadline; crashes/failed cleanup require inspected
  manual cleanup or a fresh root. Client attachment copies follow client retention.
  Direct getrandom 0.4.3 reuses the locked CSPRNG identity, with no graph expansion.

- **Task 8 output/recovery:** message budgets measure complete serialized JSON-RPC,
  including identity, real request IDs and duplicated JSON fallback. Metadata-only overflow
  fails small; one oversized message remains only when all real metadata fits. Media has
  a separate 65,536-byte non-binary cap, preserving independent base64 limits and refunds.
  Wire-only recovery keys reserve before dispatch, expire ten minutes after completion,
  reject duplicates and survive reused request IDs. Canonical recovery selects explicit
  keys; last-N is legacy diagnostic. Keys are not authentication. Diagnostic/keyed stores
  are each bounded by configured capacity; total retention can double the previous ring.
- **Media memory/deadline:** production MCP uses lazy ordered binary outcomes, four global
  raw slots sharing 80 MiB, and two CPU workers. Leases follow bytes into started blocking
  work. One configured download deadline includes setup, capacity waits and processing.
  Source decoding: 8192 side, 8,388,608 pixels, 64 MiB decoder allowance; calculate base64
  size before encoding. Collected/cloned Rust batches remain caller-owned memory. Raw and
  decoder caps are not a hard RSS ceiling; a debug two-call/20-image near-cap fixture
  measured 40 MiB completed raw peak and 488,390,656-byte process RSS.
- **Session storage:** new directories 0700, owned regular DB/WAL/SHM/journal files 0600;
  linked/nonregular entries are refused. Verify current/root-owned canonical ancestry,
  rejecting unprotected writable ancestors while allowing root-owned sticky ancestors above a non-group/other-writable session parent.
  The immediate session parent must always exclude group/other writes, including sticky
  roots, to protect absent sidecar names. Reject granting directory ACLs and clear file
  ACLs on macOS; preserve existing directory
  modes/ACLs. Trusted OS utilities are captured and deadline-bounded. Same-UID hostile
  mutation is outside the boundary. Tests use disposable fixtures only.

- **Validation ownership:** remaining agent work uses macOS checks. The user handles
  Linux setup/validation if needed. Do not install Linux tooling, provision VMs/containers
  or arrange Linux runs unless explicitly requested later; Linux results do not block
  completion. Existing CI configuration remains. This ownership policy survives roadmap archival.

- **Codex development:** `AGENTS.md` is the canonical repository guide.
  Shared Codex skills live in `.agents/skills/`; obsolete Claude files were removed.
  `$bump-version` updates package/lockfile versions and rolls Unreleased notes into
  a dated changelog section, leaving a fresh Unreleased section.
  Keep personal permissions/config outside version control. `just check` matches
  CI (locked dependencies, all-target clippy). Nightly is selected explicitly in
  `rust-toolchain.toml`, not implied by the edition. See `docs/codex.md` for setup.

- **grammers pinned by `rev` on Codeberg, all three crates bumped together.** GitHub is a
  stale mirror (upstream left Feb 2026); tracking `branch = "master"` would re-create "fresh
  resolve breaks". Never delete `Cargo.lock` casually; prefer targeted `cargo update -p` —
  the lockfile once masked a dead dependency graph (yanked `core2`/`glass_pumpkin` chain)
  for months.
- **Stdio protocol/runtime:** rmcp 3.8 supports exactly tested 2026-07-28 and legacy
  2025-11-25. A bounded raw-frame adapter preserves readable IDs, while per-request
  context controls identity/caching/version fields. Default input cap 1MiB and 16
  nonwaiting in-flight admissions are independent of account charging/output budgets.
  Cooperative cancellation drops operations and suppresses queued late output; progress
  is optional, token-bound, monotonic and nonblocking. Shutdown drains against one
  configured deadline, including output; bootstrap writes separately use max(1s, grace)
  to prevent saturated input hiding EOF. Observed zero-grace shutdown remains immediate.
  Product bootstrap handlers stay local/nonblocking. A process-owned runtime avoids
  macOS open-stdin exit hangs. Reuse runtime::image_job's two process-wide CPU permits,
  retained inside started blocking closures. Uncorrelated errors omit id as allowed by
  both dated MCP schemas; do not replace the SDK serializer merely to insert generic
  JSON-RPC null IDs. Required CI/local nightly is pinned to 2026-10-31 with a separate
  scheduled floating-nightly check.
- **Canonical output:** application `json_v1` is independent of protocol revision.
  Concrete DTOs preserve channel/user IDs as strings, integer message IDs, UTC dates,
  typed safe errors and source namespaces; legacy adapters preserve old healthy shapes.
  Real wire tests validate all 16 advertised schemas across both modes/protocols. Never
  convert by recursively rewriting JSON keys or recover typed errors from display text.
  Domain Deserialize delegates to constructors. Numeric resolution scans one bounded
  traversal, excludes users, deduplicates typed peers and rejects namespace collisions;
  username-qualified search retains its namespace through the existing search walker.
  Compact header conflicts fail closed; full output keeps explicit source identity.
- **All tools in `server.rs` because `#[tool_router]` (attribute macro) scans the impl body
  before `macro_rules!` expansion** — a declaratively generated `#[tool]` method never
  registers. Boilerplate is factored via the `ToolInvocation` guard object instead.
  `#[tool_handler]` skips codegen per-method (`has_method()`), so manual
  `get_info`/`list_tools` coexist with macro-generated `call_tool` — but a manual override
  silently drops the macro's gating (e.g. SEP-2549 `ttlMs`/`cacheScope` cache hints must be
  gated on negotiated protocol ≥ 2026-07-28 yourself).
- **Config env-expansion runs on raw TOML before parsing**; a quoted value that is *only*
  `"${VAR}"` whose expansion is pure digits gets unquoted so `api_id = "${VAR}"` parses as
  integer, while `+phone`/hashes stay strings. `api_id` is always required (SenderPool needs
  it); `api_hash`/`phone_number` only for `--setup`.
- **Config parse diagnostics discard TOML errors**, including source excerpts, parser
  messages and keys, because all may contain credentials after environment expansion.
  Retain only numeric line/column through the loader's anyhow boundary; wrapping the
  original error with a safe top-level message still leaks through debug/error chains.
- **Rate limiter:** token bucket behind `Arc<Mutex>`, on-demand refill, non-blocking
  `acquire` (fail fast with `retry_after = ceil(deficit/refill_rate)` — MCP tools must not
  block the protocol). Batch tools acquire `cost × ids` up front and refund ids that
  produced no image; `channel_ids` fan-out does one atomic `acquire(N)` for deduped channels
  (never N racing acquires) and **never refunds failed channels** — each still spent a
  resolve/fetch RPC, and a refund would make an unresolvable channel free to hammer (the
  media-batch refund differs: its ids share one fetch RPC, so a per-id charge is
  pessimistic). Server-wide invariant on every tool: **local validation before `acquire`,
  every RPC after it** — a malformed id is rejected for free, a username resolve is never. Server defaults come from `config::defaults` with a
  desync-guard test; costs above `max_tokens` rejected at config load.
- **Account status is cached and free to read.** `cached_account_status` takes no RPC;
  `refresh_account_status` bounds authorization and best-effort Premium RPCs separately
  by `resolve_secs`, storing independent successful-observation timestamps. Authorization
  errors propagate without overwriting cache; Premium failures preserve the previous
  observation. Startup refreshes once. Transcription refreshes an unknown Premium cache
  only after acquisition; cached false Premium still rejects free. Channel list/basic/full
  info cost one token per operation, including optional enrichment. A channel-list dialog
  walk has one total `history_secs` deadline. The Codex example leaves 120 seconds for startup
  (two default 30-second RPC budgets plus headroom).
- **Timeouts:** three global knobs by call type (`resolve`/`history`/`search`, plus
  `download`), not per-tool; a multi-page `next().await` walk lives inside *one*
  `with_timeout` (budget = total elapsed). No retries — `Error::Timeout` goes to the client,
  which decides.
- **Transcription:** `TranscriptionDeadline` starts after acquisition and is copied
  unchanged through cold account refresh, peer resolution, message validation, the
  initial RPC and polling. Its absolute Tokio instant bounds existing per-RPC caps;
  neither setup nor a poll restarts the budget. Before any state, expiry is a typed
  timeout; afterwards, keep the latest partial. Check expiry before every future
  poll and after completion so a continuation or ready result at expiry cannot
  start another RPC or override the deadline. Cancellation
  drops the awaited future and schedules no further polls.
- **Search latency:** `messages.SearchGlobal` gets `min_date`/`max_date` server-side
  (44.86 s → 0.449 s); window deliberately widened **±1 s** because TL never documents
  inclusive/exclusive — client-side checks re-filter to exact bounds. Don't "fix" the ±1
  out. `[search] deadline_seconds` (default 20) is a backstop: expiry returns a partial
  result with `query_metadata.timed_out`/`partial`, never an error.
- **The three message-fetch loops' decision logic lives in `MessageWalk::step`**
  (`telegram/client/walk.rs`), synchronous and above the DI seam — the loops themselves
  (`get_recent_messages_impl`/`search_in_channel`/`search_global`) sit *below* the seam,
  where `MockTelegramClientTrait` replaces the whole client, so branching placed there stays
  untestable. The three differ only in `WalkConfig`'s five fields (`cutoff_time`, `to_date`,
  `after_bound`, `media_filter`, `below_cutoff`) plus the deadline (history: disabled).
- **Pagination:** `before_id` maps to the RPC's `offset_id`; `after_id` is a client-side
  break (grammers has no `min_id` setter); both exclusive. Message ids are only unique per
  channel, so global search reports `has_more` but never a `next_cursor`. `has_more` means
  "a qualifying message was refused by limit/budget", not "window exhausted". Byte budget
  (`[limits] response_byte_budget`, 40 000) is pop-oldest-until-fits over the fully-shaped
  response, with an at-least-one-message floor (the one documented way a response may exceed
  budget).
- **Albums:** `PostCounter` counts posts against `limit`, not raw messages — an admitted
  album is never split at the limit boundary (a page can exceed `limit` raw messages). A
  lone surviving sibling stays a plain message (`album: None`).
- **Per-item failures are data, not errors:** `get_messages_batch.missing_ids`, fan-out
  `channel_errors`, `resolve_channels` per-identifier `channel` XOR `error`. Batch
  invariant: every requested id lands in exactly one of `messages`/`missing_ids`. Only
  transport-level failure fails the call.
- **Fan-out lives at the MCP layer** over the existing single-channel trait methods
  (`futures::stream::iter(..).buffered(4)`) — client/mock untouched, merge logic pure and
  unit-testable.
- **Leniency at the transport boundary, domain strict** (same split twice): flexible scalar
  coercion via `deserialize_with` (field types unchanged ⇒ advertised schema unchanged —
  schemars ignores `deserialize_with`), and username-vs-numeric identifiers resolved at the
  MCP layer via `resolve_channel_identity` while `SearchParams.channel_id` stays
  numeric-only.
- **Forward attribution requires the response envelope** (`chats`+`users`), which grammers
  hides — so history/search/by-id fetches go through raw TL pagers and
  `EntityLookup::from_envelope`; conversion is a pure function so the zero-extra-call
  invariant is type-enforced. `convert_message` (envelope-less wrapper) was *deleted*, not
  tested around — deleting the weak overload makes the bug unrepresentable. Envelope miss
  degrades to ids-only, never fabricates a name.
- **Never build links from `Channel.username`** — display paths substitute sentinels. Link
  code goes through `ChannelIdentity { username: Option<String> }` /
  `resolve_channel_identity`.
- **`#[schemars(inline)]` on enums referenced from request structs**: rmcp publishes each
  tool's `inputSchema` without `$defs`, so a `$ref` that resolves in the full generator
  output is still dead for clients. `schema_integrity.rs` pins per-tool self-containment.
- **Media sizing:** pick the smallest pre-generated variant whose longest side ≥
  `max_dimension`, else largest (`select_size_candidate`); 20 MiB cap checked on reported
  *and* streamed bytes (reported sizes are untrusted). Already-fitting JPEG passes through
  byte-identical (re-encode measured +29% inflation). Video thumbnails are
  `document.thumbs()` `PhotoSize`s. Batch payload capped by `Base64Budget` (8 MiB default)
  with progressive downscale via the existing shrink loop.
- **Media batch is a client-layer method, not a tool-level loop**: numeric channel
  resolution is a full uncached dialog walk (`resolve_peer`), so N looped calls pay N walks
  regardless of concurrency — hoist resolution out of the loop, resolve+fetch once per
  batch. Live: 4.4–7.2× faster.
- **Test organization:** `#[path]`-extracted test files (no `mod.rs` ever);
  `EnvGuard`/`ENV_LOCK` in `src/config/tests.rs`; shared fixtures in `src/test_helpers.rs`.
  Deterministic time tests use `#[tokio::test(start_paused = true)]` + `time::advance`.

## Gotchas & lessons

- **grammers wraps deleted/never-existed ids in a `MessageEmpty`-backed object, not `None`**
  — blindly converting fabricates an epoch-timestamp message. Every fetch path must route
  through `require_found`/`require_found_raw` (`src/telegram/client/guard.rs`).
- **Video/audio/voice/GIF are all `Media::Document`** — distinguish via `DocumentAttribute`
  variants (`round_message`, `voice`, `Animated`); only photos/stickers get dedicated
  variants.
- **Don't hand-roll `Peer` construction**: `peer::channel::Channel::from_raw` *panics* on
  megagroups (`broadcast == false`, the common case); `Peer::from_raw` does the three-way
  dispatch correctly.
- **When grammers adds an enum variant, grep every `match` on it** — a `_` catch-all
  silently dropped `Peer::Community` (messages visible, channel invisible). Prefer
  exhaustive matches for foreign enums that grow.
- **jiff stays at the boundary**: grammers 0.11 message dates are jiff `Timestamp`; the
  domain stays chrono, converted at the single `message_timestamp` site via `.as_second()`
  (Telegram dates are second-precision).
- **Received usernames use observed-name validation:** nonempty, at most 32 bytes,
  with the existing alphanumeric/underscore policy. Real short names survive channel and
  forward conversion. Input resolution retains its stricter ASCII `is_valid_shape` check.
- **grammers `Peer` IS unit-testable since 0.10**: `MemorySession::default()` + destructured
  `SenderPool::new` (runner never spawned) + `Client::new(handle)` does no I/O; see
  `community_peer()` in `converters/channel.rs` tests.
- **Generated TL structs: check the freshest `target/debug/build/grammers-tl-types` hash dir
  by mtime**, never `head -1` — stale dirs from old pins linger and lie about struct fields.
- **tokio's `test-util` feature must be requested explicitly in `[dev-dependencies]`** for
  `start_paused`/`advance` (no umbrella feature includes it); it was once supplied
  transitively by an "unused" `tokio-test` dep, so removing dead dev-deps can break feature
  unification. The main dep lists its features explicitly (not `full`) — each one is
  annotated with the call site that needs it.
- **`tracing_subscriber`'s `TryInitError` cannot be inspected**: its cause is a private box
  and `source()` forwards to the *cause's* source, so "a subscriber is already installed"
  is indistinguishable by type. It doesn't matter — `try_init`'s only two failure modes
  (`set_global_default`, `LogTracer::init`) both mean already-initialized, so swallowing all
  of them is correct. Real logging-init failures come from `build_file_layer` instead.
- **No `sleep()` in proptest** — it froze the suite; time-dependent behavior belongs in
  paused-clock tokio tests.
- **Owned rolling logs are `telegram-connector.YYYY-MM-DD.log`** with the configured
  daily appender prefix/suffix. Retention matches that exact valid-date pattern and regular
  files before checking age; foreign logs, backups, malformed names and directories survive.
- **`value.clamp(1, max)` panics when a config-driven `max == 0`** — use
  `value.min(max).max(1)`.
- **`defaults.rs` is not the deployment.** Claims about "the current value" are only
  checkable against the running system (`check_mcp_status`). When grep-filtering config
  output to hide secrets, allow-list wanted keys — a `token` deny-filter once swallowed the
  `max_tokens` line and self-concealed the error.
- **Telegram throttles after bursts**: `iter_dialogs` degraded ~2 s → ~18 s right after ten
  media downloads, and `messages.getDialogs` can start failing `RPC_CALL_FAIL 500`.
  Benchmark arms sharing a rate-limited upstream must run cold, in fresh processes, with
  cooldowns.
- **A client-side filter over a server-paginated cursor is unbounded work** whenever the
  filter is more selective than a page. Diagnostic that isolates it: vary `limit` on one
  fixed query (0.27 s at limit 1 vs 44.86 s at limit 20 = the loop is walking, not the
  server computing).
- **One error variant serving two failure modes makes the caller's contract
  unimplementable** (`PayloadCapExceeded` vs `DownloadFailed`) — check whether the callee
  can express the distinction before fixing the call site.
- **A constant's home determines dependency direction** — three layering findings
  (`telegram → mcp`, `config → mcp`, hand-copied defaults) were each one misplaced value;
  check whether the module boundary is the bug before copying across it.
- **`#[path]` resolves relative to the declaring file's directory**, not the module tree.
  The `../tests/` idiom: `converters/message.rs` declares
  `#[path = "../tests/message_tests.rs"]` to reuse `telegram/tests/` instead of a one-file
  `converters/tests/` dir.
- **`EnvGuard` cannot be nested** — `ENV_LOCK` is a plain non-reentrant `Mutex`; exactly one
  guard per test body or self-deadlock.
- **A test-only invariant kept in a recipe/doc ("run serially") is a latent race** — enforce
  it in the test module (lock), not the invocation.
- **`rmcp::RequestContext<RoleServer>` can't be constructed in unit tests** — factor logic
  to take already-decoded values (e.g. `Option<ProtocolVersion>`), leaving only the accessor
  call unverified.
- **Diagnostic logging goes in at `debug!`/`trace!`**, not `info!` — reserve `info!` for
  completions and significant state changes.
- **"PR shows Merged" ≠ "change is on master"** with stacked PRs — retarget children to
  master before merging; verify with `git log origin/master`.
