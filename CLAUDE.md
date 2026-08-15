# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Telegram MCP Connector — a Model Context Protocol (MCP) service that enables Claude to search Russian-language Telegram channels and messages in real-time. Built in Rust using the `rmcp` SDK and `grammers` Telegram client.

## Build & Test Commands

```bash
# Pre-commit (all must pass)
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

The same commands are available as `just` recipes (see `justfile`): run `just` to list them, `just check` for the full pre-commit gate.

## Toolchain & Dependencies

- **Rust nightly** (2024 edition) — required for `let chains` and other nightly features; no `rust-toolchain.toml`, nightly is implied by `edition = "2024"`
- **`grammers` from Codeberg git, pinned by rev** (not crates.io; upstream left GitHub in Feb 2026 — never point the deps back at the stale github.com mirror). Bump the pinned rev deliberately, all three crates together, and expect to absorb API churn when you do
- **`schemars` v1** (not v0.8) — different derive API; uses `#[derive(JsonSchema)]` from `schemars::JsonSchema`
- **`rmcp` v3.1** — MCP server SDK targeting MCP 2026-07-28 (stateless lifecycle + `server/discover`), with automatic per-client fallback to the legacy `initialize` handshake; `#[tool_router]` and `#[tool(...)]` proc macros. `InitializeResult` uses a builder API (`InitializeResult::new(capabilities).with_server_info(...).with_instructions(...)`). Tool content uses `ContentBlock` (a plain enum since v3 — no `Annotated`/`RawContent` wrapper).
- **`secrecy`** — `SecretString` wraps sensitive config fields (`api_hash`, `phone_number`); access via `.expose_secret()`

## Architecture

### Key Patterns

**Generic MCP server with trait-based DI:** `McpServer<T: TelegramClientTrait, R: RateLimiterTrait>` takes `Arc<T>` and `Arc<R>`. In production, T=`TelegramClient`, R=`RateLimiter`. In tests, mockall-generated `MockTelegramClientTrait` and `MockRateLimiterTrait`.

**rmcp tool macros:** All 16 tools live in `server.rs` inside a `#[tool_router] impl` block. Each tool method has a `#[tool(...)` attribute. Tools cannot be split to separate files due to macro constraints. All tools return `Result<String, String>` and serialize responses to JSON, except `get_message_media` and `get_messages_media_batch`, which return `Result<CallToolResult, String>` because image content blocks cannot be expressed as a JSON string — rmcp's actual constraint is `IntoCallToolResult`. Each `#[tool]` method is a logging wrapper (request-id-correlated `Tool invocation started` / `Tool invocation completed` / `Tool invocation failed`) around a private `*_impl` method; the stdio transport is wrapped by `InstrumentedTransport` (`src/mcp/observability.rs`), which logs every stdout write and feeds `SessionMetrics`/`ResponseBuffer` (configured via the `[observability]` config table).

**Library + Binary split:** `lib.rs` exports all public types/modules. `main.rs` is the CLI entry point with signal handling and setup mode.

**No `mod.rs` files:** File-as-module pattern throughout. `src/mcp.rs` declares submodules, `src/telegram.rs` declares submodules.

**Type-safe domain model (DDD):** Newtype wrappers `ChannelId(i64)`, `MessageId(i64)`, `UserId(i64)`, `Username(String)`, `ChannelName(String)` prevent accidental misuse. JSON schemas via `schemars` derives.

**Config resolution:** `TELEGRAM_MCP_CONFIG` env var → `~/.config/telegram-connector/config.toml`. Supports `${VAR}` env var expansion in TOML values (pure-integer values are auto-unquoted for TOML type compatibility).

**Flexible scalar coercion at the MCP boundary:** Request structs in `src/mcp/tools/types/requests.rs` tolerate cross-type scalars from lenient clients (numeric string `"10"` for an integer, JSON number for a string, `"true"`/`1` for a bool) via `#[serde(deserialize_with = "...")]` helpers in `src/mcp/tools/types/serde_helpers.rs` (`flexible_opt_u32`, `flexible_i64`, `flexible_opt_i64`, `flexible_string`, `flexible_opt_string`, `flexible_opt_bool`). Field *types* and the advertised `JsonSchema` are unchanged — leniency is a deserialization anti-corruption layer at the transport boundary; the domain layer (`params.rs`, newtypes) stays strict. (Vec-element scalars deliberately get no coercion — scalar-only scope.)

**Timeout budgets:** `TimeoutConfig` in `src/config.rs` (`[timeouts]` TOML table, plus `shutdown_timeout_seconds`) sets per-call-type timeout budgets applied to `grammers` network operations in `src/telegram/client.rs`, so a hung MTProto call cannot stall the server. All fields have `#[serde(default)]`.

### Test Organization

- Env-mutating config tests self-serialize through the `EnvGuard` drop-guard (`ENV_LOCK` in `src/config/tests.rs`), which also restores variables on panic; plain `cargo test` is safe

## Coding Conventions

Full detail in `docs/conventions.md`. Key rules:

**Error handling:**
- Library code uses `thiserror` (typed errors in `src/error.rs`); application code uses `anyhow` (context + propagation)
- **Never `unwrap()`** in production code — use `?` or `.context("...")`
- `expect()` is allowed only in tests or truly impossible situations

**Logging:** Use `tracing`. Never log phone numbers, API hashes, passwords, or session tokens.

**Style:** Line length 100 chars. Run `cargo fmt --all` after every code change (not just `--check`).

**TDD:** Write the failing test first; no production code without a preceding test.

## Workflow

This repo is set up for in-house team development using the **superpowers** skill-driven flow — there is no manual approval gate, and git operations (branch, commit, PR) are expected as part of normal work.

- **brainstorming → writing-plans** — explore intent first, then write the plan. Design docs and implementation plans land in `docs/superpowers/specs/` and `docs/superpowers/plans/` (one dated file per change). **Delete-on-merge:** once a change is merged, delete its plan/spec files — git history is the archive; only active (unmerged or still-pending) documents stay in the tree.
- **test-driven-development** — write the failing test first; no production code without a preceding test.
- **requesting-code-review** — request review before merging a branch.
- **git** — branch, commit, and open PRs freely; use feature branches (and worktrees) to isolate work. History follows conventional-commit style (`feat:`, `fix:`, `chore:`, `docs:`).

**Pre-merge gate (all must pass):**

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

### Project Tracking

- `docs/memory.md` — distilled project knowledge: current state, open items, durable decisions, gotchas (keep project knowledge here, not in global Claude memory; add only durable facts, dedupe on write, delete what stops being true)
- `docs/superpowers/plans/` & `docs/superpowers/specs/` — active-only per-change plans and designs (delete-on-merge; merged ones live in git history)
- `docs/conventions.md` — full coding conventions

**Session start:** skim `docs/memory.md` ("Current state" and "Open items") before picking up work.
