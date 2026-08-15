# Project Audit — Findings & Refactoring Roadmap (2026-08-15)

Full-codebase audit at v0.22.1 (26.4k lines, 103 files). Three parallel review passes
(telegram layer, MCP layer, tests + cross-cutting) plus an instrumented coverage run
(`cargo llvm-cov`, 75.1% line coverage overall). All findings below were verified at the
cited lines.

**Verdict:** architecture is sound — layering holds in both directions (zero `grammers`
imports in `src/mcp/`, zero `crate::mcp` imports in `src/telegram/`), `server.rs` tool
extraction is complete, response types are well shared, no production `unwrap()` outside
four rate-limiter mutex locks. The work splits into four stages, each its own plan/branch.

---

## Stage 1 — Correctness fixes + dead code (plan: `../plans/2026-08-15-audit-stage1-correctness.md`)

1. **Pre-merge gate races itself.** `just check` → plain parallel `cargo test`, but
   `src/config/tests.rs` mutates env vars (`TELEGRAM_MCP_CONFIG`, `TEST_PHONE`, …) at 17
   sites with no lock; the serial-run guarantee lives only in the separate `test-config`
   recipe and doc notes. Fix: a shared `ENV_LOCK` mutex inside the test module; retire the
   special-case invocation everywhere it is documented (`CLAUDE.md:12,50`, `justfile:19`,
   `README.md:1618`, `docs/workflow.md:78`).
2. **Panic path + hand-rolled redaction in auth.** `src/telegram/auth.rs:21` logs
   `&phone[..4]` — panics on phones < 4 bytes or non-ASCII boundaries, and bypasses
   `logging::redact_phone`. `redact_phone` itself (`logging.rs:85-98`) byte-slices too and
   has the same latent multibyte panic. Fix both: char-aware `redact_phone`, used by auth.
3. **Silent i64→i32 id truncation.** `impl_media.rs:38` (get_message_media),
   `impl_media.rs:279` (transcribe), `impl_search.rs:465` (get_message_by_link — id comes
   from a caller-supplied t.me link) all do `message_id.get() as i32`. The batch path
   (`helpers.rs:74`) already uses `MessageId::as_i32()` with a proper error. Fix: shared
   `wire_message_id` helper at all four places.
4. **Dead code:** `TelegramClient::sign_in` (`client/auth.rs:21-39`, zero callers —
   `interactive_auth` bypasses it via `client.client()` because the wrapper discards the
   2FA token); `parse_optional_channel_id` (`helpers.rs:91` + tests + `tools.rs:18`
   re-export); `matches_media_filter` (`converters/media.rs:320` + `converters.rs:17`
   re-export; only the `_raw` twin is used); `PostCounter::overflowed`
   (`albums.rs:50`, `#[allow(dead_code)]`, exercised only by tests → gate `#[cfg(test)]`);
   placeholder `test_auth_module_compiles` (`auth.rs:71`).

## Stage 2 — Module splits & test extraction (mechanical, no behavior change)

Most >500-line files overshoot because of in-file `#[cfg(test)]` modules; the repo's
`#[path]`-included test-file convention is the remedy. Only one production file needs a
real split.

| File | Lines | Action |
|---|---|---|
| `telegram/client/raw_pager.rs` | 976 | Real split: `raw_page.rs` (envelope interpretation ~215: `RawPage`, `unpack_page`, `channel_access_hash`, `input_peer_for_message`, `fill_buffer`, `chat_peer_for_message`), `raw_fetch.rs` (~75: `GetMessagesRequest`, `get_messages_request`, `index_messages`, `fetch_messages_by_id`; update `ops_message.rs:6`), pagers stay; 409-line test block → `client/tests/` |
| `telegram/converters/message.rs` | 843 | Production is cohesive — do NOT split; move 522-line test module → `telegram/tests/message_tests.rs` |
| `telegram/client/channels.rs` | 518 | Test extraction alone → ~310 lines (discovery-vs-subscription production split exists but KISS says skip) |
| `mcp/server.rs` | 649 | Already fine (macro-bound boilerplate); optional: `ToolInvocation` (~50 lines) → `server/invocation.rs` |
| `mcp/tests/search.rs` | 1217 | → `search_core.rs` / `search_dates.rs` / `search_shaping.rs` |
| `mcp/tests/history.rs` | 962 | → `history_core.rs` / `history_dates.rs` / `history_paging.rs`; delete local `create_test_message` duplicate (:17-44) |
| `config/tests.rs` | 861 | → `tests/env_tests.rs` / `load_tests.rs` (all env-mutating loaders in one auditable place) / `validation_tests.rs` / `defaults_tests.rs`. Fold in an `EnvGuard` drop-guard (stage-1 review): `remove_var` cleanup currently runs only on the success path, so a failing assertion leaks vars into subsequent locked tests |
| `telegram/tests/converters_tests.rs` | 847 | → thumb/forward, av, doc/poll files |
| `telegram/tests/client_tests.rs` | 710 | **Prune, don't split**: 21 `mock_*` tests assert on the mockall mock itself (zero production code under test, ~550 lines); keep the 6 `username_to_resolve` tests |
| `mcp/tests/media_batch.rs` | 649 | → core/budget + shared fixtures (`permissive_limiter` → `test_helpers`) |
| `mcp/tests/channels.rs` | 511 | 2% over — leave |

Also: fixture duplication `test_helpers.rs` should own (`create_test_message`/
`create_test_channel` re-implemented locally in 2 files each; all-`None` request literal
repeated 22×/20× in search/history tests; `expect_acquire().returning(|_| Ok(()))` ~60×).

## Stage 3 — Duplication / KISS refactors

1. `impl_search.rs`: ~150 structurally identical lines between `search_messages_impl`
   (:9-230) and `get_recent_messages_impl` (:245-440) — extract `parse_cursor_bounds`,
   `resolve_max_text_length`, and a `fanout::run` scaffold.
2. Telegram mirror: 18-line cursor→i32 block duplicated (`ops_search.rs:35` /
   `ops_history.rs:84`); ~20-line album-admit/limit/`has_more` block **triplicated**
   (`ops_search.rs:137,252`, `ops_history.rs:159`) — accumulator next to `PostCounter`.
3. `ops_search::search_messages_impl` is 337 lines — split channel path vs global path.
4. `serde_helpers.rs`: two copy-paste deserializer pairs (~60 lines via one generic each).
5. Empty-reference guard hand-inlined in 7 client files with wording drift — promote
   `channels.rs::validate_channel_identifier` (:239) to a client-wide guard.
6. Smaller: `shaping.rs:106` vs `fanout.rs:84` channel-count recompute; `server.rs:1-42`
   implicit prelude for nine submodules (each `impl_*.rs` should import its own deps);
   `impl_media.rs:71-248` batch loop body (~90 lines) → private per-id fn.

## Stage 4 — Coverage (75.1% overall; the distribution is the story)

- Production `TelegramClient` ops layer at **0%**: `ops_search` / `ops_media` /
  `ops_history` / `ops_message` / `ops_stats` / `ops_transcribe` / `lifecycle` /
  `client/auth`; `resolve.rs` 12%, `channels.rs` 45%, `raw_pager.rs` 61%. Structural: the
  DI seam sits above these files. Remedy proven in-repo: keep extracting pure decision
  logic (as `albums.rs`, `search_budget.rs`, paging math already were).
- `telegram/auth.rs` effectively untested (placeholder only, removed in stage 1).
- `config.rs` 69.6% despite 861 test lines — file-loading error branches untested.
- `serde_helpers.rs` 81.5%, `mcp/server.rs` 79.6% (tool wrapper log paths).

## Hygiene backlog (batch into any stage's PR)

`rate_limiter.rs:81,114,124,129` `lock().unwrap()` (use
`unwrap_or_else(PoisonError::into_inner)`) + verbatim-duplicated `available_tokens`
(:80-85 vs :128-132); `config/defaults.rs:12-16,54-58` `ProjectDirs…expect()` panics where
`config.rs:426` errors; `config.rs:113-124` `auth_credentials()` `expect()` → return
`Option`; `logging.rs:26-33` swallows all init errors, not just double-init;
`main.rs:69` `process::exit(0)` skips destructors (comment or return); `impl_status.rs:99`
base64 size underflow → `saturating_sub`; trait docs leak `raw_pager` names
(`trait_def.rs:57,64`); `tokio` `features=["full"]`; `cli.rs:33` `parse_args` wrapper;
"Tool 16" doc-comment numbering drift (`server.rs:441`); `impl_message_batch.rs:14-19` vs
`impl_media.rs:75-80` 6-line precheck duplication (barely worth it); Vec-element scalars
get no flexible coercion (deliberate scalar scope — document in the coercion design doc);
`redact_phone`'s ≤6-char threshold leaves a 7-char phone fully visible (first 4 + last 3 is the
whole string — documented by an existing test, but the threshold should arguably be higher).
