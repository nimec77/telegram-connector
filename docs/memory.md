# Project Memory — Telegram MCP Connector

Distilled project knowledge: current state, durable decisions, and hard-won lessons.
Keep this file compact — add only durable facts, dedupe on write, delete what stops being
true. (The full historical journal lives in git history, pre-2026-08-15.)

## Current state

- **v0.22.3** (2026-08-16, brought in by `refactor/audit-stage4-ops-verification`), 16 MCP
  tools (16th: `get_messages_media_batch`). 760 lib tests passing, 5 ignored (up from 726).
  Coverage **78.33% lines** (`cargo llvm-cov --lib`; 76.19% for a full `cargo llvm-cov`
  run), compared approximately to the 75.1% audit-start baseline — that baseline predates
  stages 2–3, so it isn't a clean stage-4-only delta. Near-100% on domain
  types/converters/shaping, `MessageWalk`'s
  decision logic (`walk.rs`) at 95%. The production `TelegramClient` ops layer is still
  mostly 0% — the DI seam swaps exactly that code for mocks; `ops_history`/`ops_message`
  moved off zero (12%/26%), `ops_search` stayed at 0% (thin glue below the seam, as
  designed).
- **Audit 2026-08-15** (spec: `docs/superpowers/specs/2026-08-15-project-audit.md`, 4 staged
  work packages). All four stages shipped (correctness fixes + dead code; module splits +
  test extraction; duplication/KISS refactors; ops-layer coverage via `MessageWalk` — see the
  spec for what landed). Hygiene backlog (11 mechanical fixes) stays its own future PR.

## Open items

- **`Username::new`'s 5-char minimum silently drops real 3–4-char usernames** (e.g. `@mash`)
  from `channel_username` and forward enrichment. Fix belongs in
  `src/telegram/types/names.rs`; needs its own ticket.
- **Raw pagers exist only because grammers pins `Message.peers: PeerMap` `pub(crate)`**
  (verified at pinned rev = upstream HEAD). If a future grammers rev exposes public peer
  resolution, collapse `raw_pager`/`raw_fetch` back to the high-level iterators.
- Channel-scoped `messages.Search` also carries `min_date`/`max_date` but they're
  deliberately not sent (path measured ~1 s). If it ever shows up slow, push the bounds onto
  `RawChannelSearchPager` first — not a redesign.

## Key decisions

- **grammers pinned by `rev` on Codeberg, all three crates bumped together.** GitHub is a
  stale mirror (upstream left Feb 2026); tracking `branch = "master"` would re-create "fresh
  resolve breaks". Never delete `Cargo.lock` casually; prefer targeted `cargo update -p` —
  the lockfile once masked a dead dependency graph (yanked `core2`/`glass_pumpkin` chain)
  for months.
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
- **Rate limiter:** token bucket behind `Arc<Mutex>`, on-demand refill, non-blocking
  `acquire` (fail fast with `retry_after = ceil(deficit/refill_rate)` — MCP tools must not
  block the protocol). Batch tools acquire `cost × ids` up front and refund ids that
  produced no image; `channel_ids` fan-out does one atomic `acquire(N)` for deduped channels
  (never N racing acquires). Server defaults come from `config::defaults` with a
  desync-guard test; costs above `max_tokens` rejected at config load.
- **Timeouts:** three global knobs by call type (`resolve`/`history`/`search`, plus
  `download`), not per-tool; a multi-page `next().await` walk lives inside *one*
  `with_timeout` (budget = total elapsed). No retries — `Error::Timeout` goes to the client,
  which decides.
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
- **jiff stays at the boundary**: grammers 0.10 message dates are jiff `Timestamp`; the
  domain stays chrono, converted at the single `message_timestamp` site via `.as_second()`
  (Telegram dates are second-precision).
- **Any sentinel routed through `Username::new` must be 5–32 chars** — `"user"` (4 chars)
  was a latent panic.
- **grammers `Peer` IS unit-testable since 0.10**: `MemorySession::default()` + destructured
  `SenderPool::new` (runner never spawned) + `Client::new(handle)` does no I/O; see
  `community_peer()` in `converters/channel.rs` tests.
- **Generated TL structs: check the freshest `target/debug/build/grammers-tl-types` hash dir
  by mtime**, never `head -1` — stale dirs from old pins linger and lie about struct fields.
- **tokio's `test-util` feature is excluded from `full`** and must be requested explicitly
  in `[dev-dependencies]` for `start_paused`/`advance`; it was once supplied transitively by
  an "unused" `tokio-test` dep, so removing dead dev-deps can break feature unification.
- **No `sleep()` in proptest** — it froze the suite; time-dependent behavior belongs in
  paused-clock tokio tests.
- **`tracing_appender` names files `app.log.YYYY-MM-DD`** — `path.extension()` returns the
  date; match on `file_name.contains(".log")`.
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
