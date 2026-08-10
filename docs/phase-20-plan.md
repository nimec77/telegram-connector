# Phase 20: Hang Diagnostics & Grammers Call Timeouts

**Status:** Implemented (see CLAUDE.md "Timeout budgets")
**Trigger:** Recurring MCP request timeouts since 2026-05-20 — server stops responding for 5–10 min, then resumes
**Root cause:** Grammers calls (`resolve_username`, `iter_messages.next()`, `search_messages(...).next()`) are awaited without any timeout. A stalled Telegram response hangs the MCP request indefinitely. Cancellation only arrives when the Claude.ai client hits its own 5-min timeout. Today's tool handlers also only log on completion, so we cannot see which tool or args caused the hang.

---

## Evidence Summary

Examined `~/Library/Application Support/telegram-connector/logs/telegram-connector.2026-05-*.log`.

- First `"Request timed out"` cancellations appear on **2026-05-20** (4 events); none on 05-16…05-19.
- Today (2026-05-22) the canonical incident: line 304 logs successful `@ai_newz` at `15:05:11`; nothing until `15:09:35` when request id 15 is cancelled, then id 16 cancelled at `15:13:20`. A fresh request for `@habr_com` then completes normally at `15:14:38`.
- During the 9 min 27 s gap there are **no log entries at all** — no grammers WARN, no tool entry, no error. The MTProto socket is alive (when it actually dies, grammers emits `marking all N request(s) as failed` within seconds — see line 151, `2026-05-22T07:09:57`).
- Tool handlers in `src/mcp/server.rs` only log on completion (e.g. `server.rs:347`), so the hung request's tool and arguments are unknown.
- `search_messages` is the most likely suspect — observed durations of 22 s / 33 s / 37 s in lines 77–84 with no upper bound. A sparse global search can easily exceed 5 min on Telegram's backend.
- The separate "Connection reset by peer (os error 54)" with `0 request(s)` (line 151) is an unrelated idle-socket symptom; auto-reconnects on next request. Not the bug.

---

## Decisions Already Made

| Decision | Value |
|---|---|
| Scope | Both diagnostics (tool entry logging) + behavior fix (grammers timeouts) in one change |
| Timeout values | Configurable via `config.toml` with conservative defaults (30/60/120 s) |

---

## Implementation Plan

### 1. Config — `src/config.rs`

New optional struct, attached to `TelegramConfig`:

```toml
[telegram.timeouts]
resolve_secs = 30   # resolve_username + iter_dialogs lookup
history_secs = 60   # iter_messages walks (get_recent_messages, get_message_by_id)
search_secs  = 120  # search_messages walks (single-channel and global)
```

- All fields optional. Missing section → defaults (30 / 60 / 120).
- Backwards compatible with existing configs.
- Update default `config.toml` template / setup mode output to document the new keys.

### 2. Error type — `src/error.rs`

Add a typed variant:

```rust
#[error("Operation '{operation}' timed out after {secs}s")]
Timeout { operation: String, secs: u64 },
```

### 3. Grammers call wrapping — `src/telegram/client.rs`

Wrap each network call site in `tokio::time::timeout`:

| Method | Call site | Budget |
|---|---|---|
| `get_channel_info` (line 182) | `resolve_username` (both `@` and bare-username branches) | `resolve_secs` |
| `get_channel_info` (line 182) | full `iter_dialogs` walk in numeric-ID branch | `resolve_secs` |
| `get_recent_messages` (line 380) | `resolve_username` (line 396) | `resolve_secs` |
| `get_recent_messages` (line 380) | `iter_dialogs` fallback walk (line 425) | `resolve_secs` |
| `get_recent_messages` (line 380) | `iter_messages` walk (line 452) | `history_secs` (total budget across all `next().await` iterations) |
| `search_messages` (line 247) | single-channel `iter_dialogs` + `search_iter` walk | `search_secs` |
| `search_messages` (line 247) | global `search_all_messages` walk | `search_secs` |
| `get_message_by_id` (line 507) | message fetch | `history_secs` |

For multi-iteration walks (`iter_messages.next()`, `search_iter.next()`), the budget is total elapsed time, not per-iteration. Pattern: wrap the whole `while let Some(...)` block in `tokio::time::timeout(Duration::from_secs(budget), async { ... }).await`.

Timeouts surface as `Error::Timeout { operation, secs }` and propagate through the existing `?` chain back to the MCP tool as a `String` error.

### 4. Tool entry logging — `src/mcp/server.rs`

Add `tracing::info!` at the top of each of the 8 `#[tool]` methods. Include the tool name and the key argument(s). Example for `get_recent_messages`:

```rust
tracing::info!(
    tool = "get_recent_messages",
    channel_id = %request.channel_id,
    hours_back = ?request.hours_back,
    limit = ?request.limit,
    media_filter = ?request.media_filter,
    "Tool invocation started"
);
```

Logging at `info!` (not `debug!`) is deliberate — the whole point is that next time something hangs, the entry log is visible without changing config.

Trade-off: log volume roughly doubles (entry + completion per call). Acceptable; 7-day rotation already enforced.

### 5. Tests (TDD — failing test first)

**`src/config/tests.rs`** (or new `src/config/timeouts_tests.rs`):
- Default values when `[telegram.timeouts]` absent.
- Partial override (e.g. only `search_secs` set, others default).
- Full override.
- Invalid values rejected (e.g. zero).
- Must run with `--test-threads=1` (existing config-test constraint).

**`src/telegram/tests/client_tests.rs`** (mock-based via `MockTelegramClientTrait`):
- Currently the trait is what we mock — but the timeouts live *inside* `TelegramClient`'s impl of the trait, not in the trait surface. So we need a different test point. Two options:
  - (a) Add unit tests inside `src/telegram/client.rs` that drive `tokio::time::timeout` via `tokio::time::pause()` + a hand-rolled future that never resolves. Doesn't touch grammers.
  - (b) Skip mock layer for these — add small synchronous tests that verify the timeout wrappers' error mapping logic.
- Recommend (a): smallest surface, deterministic, no real network.

**MCP server tests** (`src/mcp/tests/*.rs`):
- Existing tests verify response structure — should pass unchanged after we add entry logging (logging is side-effect-only).
- Add no new tests for entry logging unless behavior depends on it.

### 6. Documentation

- Update `docs/memory.md`: append entry under "Patterns & Decisions" — "All grammers calls now bounded by `tokio::time::timeout`; budgets in `TimeoutConfig`."
- Update `docs/tasklist.md`: add Phase 20 row with status / test count.
- Update `CLAUDE.md` "Toolchain & Dependencies" / "Critical Rules" if any newly enforced invariant emerges.
- Update `CHANGELOG.md` under unreleased section.

---

## Out of Scope (Deliberately)

- **Idle-connection RST suppression** (symptom A): the periodic `Connection reset by peer (os error 54)` with `0 request(s)`. Cosmetic log noise; grammers reconnects on next request. Could be fixed separately by sending app-level keepalives during idle, but not in this phase.
- **Retries.** Timeout → return error to MCP client. No automatic retry; Claude can decide whether to re-invoke.
- **Per-tool timeout overrides.** All grammers calls share three global budgets keyed by call type. No tool-specific knobs.

---

## Verification Checklist

Before declaring Phase 20 done:

- [ ] `cargo fmt --check`
- [ ] `cargo clippy -- -D warnings`
- [ ] `cargo test` (full suite)
- [ ] `cargo test config -- --test-threads=1`
- [ ] Manual smoke: run `cargo run --bin telegram-mcp`, exercise each tool via Claude.ai client, confirm entry + completion logs appear in `~/Library/Application Support/telegram-connector/logs/`
- [ ] Manual hang simulation: in a scratch branch, hard-code a `tokio::time::sleep(10 min)` into one grammers call, confirm `Error::Timeout` surfaces within the configured budget instead of hanging
- [ ] Memory + tasklist updated
- [ ] No new `unwrap()` / `expect()` outside test modules
