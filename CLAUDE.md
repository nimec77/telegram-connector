# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Telegram MCP Connector — a Model Context Protocol (MCP) service that enables Claude to search Russian-language Telegram channels and messages in real-time. Built in Rust using the `rmcp` SDK and `grammers` Telegram client.

## Build & Test Commands

```bash
cargo build                            # Debug build
cargo build --release                  # Release build
cargo test                             # All tests (some ignored)
cargo test <module>                    # e.g. cargo test mcp, cargo test types
cargo test <test_fn_name>              # Run a single test by name
cargo test config -- --test-threads=1  # Config tests MUST run serial (env var mutation)
cargo test -- --nocapture              # Show println!/dbg! output during tests
cargo fmt --check                      # Check formatting
cargo clippy -- -D warnings           # Lint (warnings = errors)
cargo run --bin telegram-mcp           # Run the binary

# Pre-commit (all must pass)
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

The same commands are available as `just` recipes (see `justfile`): run `just` to list them, `just check` for the full pre-commit gate.

## Toolchain & Dependencies

- **Rust nightly** (2024 edition) — required for `let chains` and other nightly features; no `rust-toolchain.toml`, nightly is implied by `edition = "2024"`
- **`grammers` from git master** (not crates.io) — API can change between builds; check `grammers-client` docs if compilation fails after update
- **`schemars` v1** (not v0.8) — different derive API; uses `#[derive(JsonSchema)]` from `schemars::JsonSchema`
- **`rmcp` v1.7** — MCP server SDK; `#[tool_router]` and `#[tool(...)]` proc macros. `InitializeResult` uses a builder API (`InitializeResult::new(capabilities).with_server_info(...).with_instructions(...)`).
- **`secrecy`** — `SecretString` wraps sensitive config fields (`api_hash`, `phone_number`); access via `.expose_secret()`

## Architecture

```
MCP Client (Claude) ──JSON-RPC/stdio──► MCP Server Layer (rmcp)
                                        │  src/mcp/server.rs (10 tools)
                                        │  src/mcp/tools/ (helpers + types/{requests,responses,serde_helpers})
                                        │  src/mcp/observability.rs (InstrumentedTransport, SessionMetrics, ResponseBuffer)
                                        ▼
                                      Application Layer
                                        │  config, logging, rate_limiter, link, error, cli
                                        ▼
                                      Telegram Layer (grammers)
                                        │  client.rs, trait_def.rs, converters.rs, auth.rs
                                        │  types/ (ids, names, media, entities, params)
                                        ▼
                                      Telegram Cloud API (MTProto)
```

### Key Patterns

**Generic MCP server with trait-based DI:** `McpServer<T: TelegramClientTrait, R: RateLimiterTrait>` takes `Arc<T>` and `Arc<R>`. In production, T=`TelegramClient`, R=`RateLimiter`. In tests, mockall-generated `MockTelegramClientTrait` and `MockRateLimiterTrait`.

**rmcp tool macros:** All 10 tools live in `server.rs` inside a `#[tool_router] impl` block. Each tool method has a `#[tool(...)` attribute. Tools cannot be split to separate files due to macro constraints. All tools return `Result<String, String>` and serialize responses to JSON, except `get_message_media`, which returns `Result<CallToolResult, String>` because image content blocks cannot be expressed as a JSON string — rmcp's actual constraint is `IntoCallToolResult`. Each `#[tool]` method is a logging wrapper (request-id-correlated `Tool invocation started` / `Tool invocation completed` / `Tool invocation failed`) around a private `*_impl` method; the stdio transport is wrapped by `InstrumentedTransport` (`src/mcp/observability.rs`), which logs every stdout write and feeds `SessionMetrics`/`ResponseBuffer` (configured via the `[observability]` config table).

**Library + Binary split:** `lib.rs` exports all public types/modules. `main.rs` is the CLI entry point with signal handling and setup mode.

**No `mod.rs` files:** File-as-module pattern throughout. `src/mcp.rs` declares submodules, `src/telegram.rs` declares submodules.

**Type-safe domain model (DDD):** Newtype wrappers `ChannelId(i64)`, `MessageId(i64)`, `UserId(i64)`, `Username(String)`, `ChannelName(String)` prevent accidental misuse. JSON schemas via `schemars` derives.

**Config resolution:** `TELEGRAM_MCP_CONFIG` env var → `~/.config/telegram-connector/config.toml`. Supports `${VAR}` env var expansion in TOML values (pure-integer values are auto-unquoted for TOML type compatibility).

**Flexible scalar coercion at the MCP boundary:** Request structs in `src/mcp/tools/types/requests.rs` tolerate cross-type scalars from lenient clients (numeric string `"10"` for an integer, JSON number for a string, `"true"`/`1` for a bool) via `#[serde(deserialize_with = "...")]` helpers in `src/mcp/tools/types/serde_helpers.rs` (`flexible_opt_u32`, `flexible_i64`, `flexible_string`, `flexible_opt_string`, `flexible_opt_bool`). Field *types* and the advertised `JsonSchema` are unchanged — leniency is a deserialization anti-corruption layer at the transport boundary; the domain layer (`params.rs`, newtypes) stays strict. Design: `docs/superpowers/specs/2026-05-31-flexible-scalar-coercion-design.md`.

**Timeout budgets:** `TimeoutConfig` in `src/config.rs` (`[timeouts]` TOML table, plus `shutdown_timeout_seconds`) sets per-call-type timeout budgets applied to `grammers` network operations in `src/telegram/client.rs`, so a hung MTProto call cannot stall the server. All fields have `#[serde(default)]`.

### Test Organization

- Unit tests: `#[cfg(test)]` blocks inline in each module, or in separate files via `#[path = "..."]` attribute (e.g., `config.rs` → `config/tests.rs`)
- MCP tool tests: `src/mcp/tests/{server_core,status,channels,links,search,history,message_by_link,last_responses}.rs`
- Telegram client tests: `src/telegram/tests/client_tests.rs` (mock-based)
- Test fixtures: `src/test_helpers.rs` — `create_test_message()`, `create_test_channel()`, etc.
- Config tests require `--test-threads=1` due to `env::set_var()` race conditions

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

- **brainstorming → writing-plans** — explore intent first, then write the plan. Design docs and implementation plans land in `docs/superpowers/specs/` and `docs/superpowers/plans/` (one dated file per change).
- **test-driven-development** — write the failing test first; no production code without a preceding test.
- **requesting-code-review** — request review before merging a branch.
- **git** — branch, commit, and open PRs freely; use feature branches (and worktrees) to isolate work. History follows conventional-commit style (`feat:`, `fix:`, `chore:`, `docs:`).

**Pre-merge gate (all must pass):**

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

### Project Tracking

- `docs/tasklist.md` — phase checklist and task details
- `docs/memory.md` — patterns, decisions, and lessons learned (local project journal; keep project knowledge here, not in global Claude memory)
- `docs/superpowers/plans/` & `docs/superpowers/specs/` — per-change implementation plans and designs
- `docs/conventions.md` — full coding conventions

**Session start:** skim `docs/tasklist.md` for current phase and open tasks before picking up work.
