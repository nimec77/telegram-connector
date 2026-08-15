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

## Stage 1 — Correctness fixes + dead code ✅ (merged)

Done: `ENV_LOCK` + `EnvGuard` serialize env-mutating config tests (plain `cargo test` is
safe); char-aware `redact_phone` used by auth (panic path removed); shared `wire_message_id`
helper replaced the silent i64→i32 truncations; dead code removed (`sign_in`,
`parse_optional_channel_id`, `matches_media_filter`, `PostCounter::overflowed` gated,
placeholder auth test).

## Stage 2 — Module splits & test extraction ✅ (merged via PR #40)

Done: `raw_pager.rs` split into `raw_page.rs`/`raw_fetch.rs`; oversized in-file
`#[cfg(test)]` modules moved to `#[path]`-included siblings (`mcp/tests/`,
`telegram/tests/`, `config/tests/`); mock-only client tests pruned; shared fixtures
consolidated into `test_helpers.rs`. Accepted overshoots: `server.rs` (macro-bound),
`test_helpers.rs`, `telegram/tests/message_tests.rs`.

## Stage 3 — Duplication / KISS refactors ✅ (merged via PR #42)

Done: shared `parse_cursor_bounds`/`resolve_max_text_length` + `fanout::run` scaffold
(MCP list-tool prologues/fan-out); telegram `cursor_wire_bounds` + `PageAccumulator`
(triplicated admit/limit/`has_more`); `search_messages_impl` split into channel/global
paths; serde copy-paste pairs → `flexible_opt_int`/`flexible_opt_enum`; client-wide
`validate_channel_identifier` (7 inline guards replaced); `unique_channel_count`;
media-batch per-outcome fn; explicit imports in the nine server submodules; rate-limiter
poison recovery + `available_tokens` dedup. Three deliberate behavior deltas documented
in PR #42 (guard wording, `search_global` debug-log timing scope, enum-coercion error
text).

## Stage 4 — Coverage (75.1% overall; the distribution is the story)

Designed: `docs/superpowers/specs/2026-08-15-audit-stage4-design.md`. The goal there is
*verified ops behavior*, not a coverage percentage. Scope is the message-fetch core
(`ops_search`, `ops_history`, `ops_message`, `resolve`, `channels`); `ops_media`,
`ops_stats`, `ops_transcribe`, `lifecycle`, `client/auth` are deferred.

- Production `TelegramClient` ops layer at **0%**: `ops_search` / `ops_media` /
  `ops_history` / `ops_message` / `ops_stats` / `ops_transcribe` / `lifecycle` /
  `client/auth`; `resolve.rs` 12%, `channels.rs` 45%, `raw_pager.rs` 61%. Structural: the
  DI seam sits above these files. Remedy proven in-repo: keep extracting pure decision
  logic (as `albums.rs`, `search_budget.rs`, paging math already were).
- `telegram/auth.rs` effectively untested (placeholder only, removed in stage 1).
- `config.rs` 69.6% despite 861 test lines — file-loading error branches untested.
- `serde_helpers.rs` 81.5%, `mcp/server.rs` 79.6% (tool wrapper log paths — needs a
  `RequestContext<RoleServer>`, which is not constructible in unit tests; deferred).
- Stage 3 follow-ups (from the stage-3 final review): add `#[must_use]` to
  `PageAccumulator::push`; add a collapse=false album-sibling `into_messages` test
  (albums.rs unit tests are the only pin on `PageAccumulator` — the ops loops sit below
  the DI seam); rename the global-search tracing field `page` to `page_no`
  (`ops_search.rs` — clashes with the `page` accumulator local).

## Hygiene backlog (batch into any stage's PR)

Line numbers re-verified at `5013672`; corrections marked. Scheduled into stage 4 — see
that design doc's Section 5.

`config/defaults.rs:14,56` `ProjectDirs…expect()` panics where `config.rs:426` errors;
`config.rs:117,121` `auth_credentials()` `expect()` → return `Option`; `logging.rs`
`result.or(Ok(()))` swallows all init errors, not just double-init; `main.rs:69`
`process::exit(0)` skips destructors (comment or return); `impl_status.rs:105` base64 size
underflow → `saturating_sub`; trait docs leak internal names (`trait_def.rs:57,64` — the
leak is `raw_fetch::fetch_messages_by_id`, *not* `raw_pager`); `tokio` `features=["full"]`;
`cli.rs:35` `parse_args` wrapper (was cited as `:33`); "Tool 16" doc-comment numbering
drift (`server.rs:426`, between Tool 10 at `:405` and Tool 11 at `:447` — was cited as
`:441`); `impl_message_batch.rs` vs `impl_media.rs` 6-line precheck duplication (barely
worth it — deliberately skipped); Vec-element scalars get no flexible coercion (deliberate
scalar scope — document in the coercion design doc); `redact_phone`'s ≤6-char threshold
leaves a 7-char phone fully visible (`logging.rs:85` — first 4 + last 3 is the whole
string; documented by an existing test, but the threshold should arguably be higher).

**Done, not outstanding:** `rate_limiter.rs` `lock().unwrap()` ×4 and the verbatim-
duplicated `available_tokens` were fixed in `2072a67` (stage 3).
