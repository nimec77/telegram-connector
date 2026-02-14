# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Telegram MCP Connector — a Model Context Protocol (MCP) service that enables Claude to search Russian-language Telegram channels and messages in real-time. Built in Rust using the `rmcp` SDK and `grammers` Telegram client.

**Status:** 19 phases complete, 215 tests passing (5 ignored), 7 MCP tools.

## Build & Test Commands

```bash
cargo build                            # Debug build
cargo build --release                  # Release build
cargo test                             # All 215 tests (5 ignored)
cargo test <module>                    # e.g. cargo test mcp, cargo test types
cargo test config -- --test-threads=1  # Config tests MUST run serial (env var mutation)
cargo fmt --check                      # Check formatting
cargo clippy -- -D warnings            # Lint (warnings = errors)
cargo run --bin telegram-mcp           # Run the binary

# Pre-commit (all must pass)
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

## Architecture

```
MCP Client (Comet) ──JSON-RPC/stdio──► MCP Server Layer (rmcp)
                                        │  src/mcp/server.rs (7 tools)
                                        │  src/mcp/tools/ (types, helpers)
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

**rmcp tool macros:** All 7 tools live in `server.rs` inside a `#[tool_router] impl` block. Each tool method has a `#[tool(...)` attribute. Tools cannot be split to separate files due to macro constraints.

**Library + Binary split:** `lib.rs` exports all public types/modules. `main.rs` is the CLI entry point with signal handling and setup mode.

**No `mod.rs` files:** File-as-module pattern throughout. `src/mcp.rs` declares submodules, `src/telegram.rs` declares submodules.

**Type-safe domain model (DDD):** Newtype wrappers `ChannelId(i64)`, `MessageId(i64)`, `UserId(i64)`, `Username(String)`, `ChannelName(String)` prevent accidental misuse. JSON schemas via `schemars` derives.

### Test Organization

- Unit tests: `#[cfg(test)]` blocks inline in each module, or in sibling `tests/` directories
- MCP tool tests: `src/mcp/tests/{server_core,status,channels,links,search,history}.rs`
- Telegram client tests: `src/telegram/tests/client_tests.rs` (mock-based)
- Test fixtures: `src/test_helpers.rs` — `create_test_message()`, `create_test_channel()`, etc.
- Config tests require `--test-threads=1` due to `env::set_var()` race conditions

## MCP Tools

| Tool | Description |
|------|-------------|
| `check_mcp_status` | Connection status, rate limiter tokens |
| `get_subscribed_channels` | List channels with pagination |
| `get_channel_info` | Channel metadata by username or ID |
| `generate_message_link` | Generate tg:// and https://t.me links |
| `open_message_in_telegram` | Open in Telegram Desktop (macOS) |
| `search_messages` | Search with rate limiting, optional media filter |
| `get_recent_messages` | Channel messages by time window (no query needed) |

## Error Handling

- **Library layer:** `thiserror` for typed error definitions in `error.rs`
- **Application layer:** `anyhow` with `.context()` for error propagation
- **NEVER use `unwrap()` in production code** — use `?` or `expect()` with clear messages

## Configuration

Config file: `~/.config/telegram-connector/config.toml`

- `${VAR}` syntax for env var expansion in ALL fields
- `api_id` always required; `api_hash` and `phone_number` only required with `--setup` flag
- Numeric fields auto-unquoted if value is pure digits

## Logging

- `tracing` for structured async logging; dual output (stderr + file)
- Daily rotation in `~/.config/telegram-connector/logs/`, JSON format for files
- Old logs cleaned on startup via `cleanup_old_logs()` based on `max_log_days`
- Never log sensitive data — use `redact_phone()` and `redact_hash()`

## Critical Rules

1. **NEVER create git commits** — The user manages all git operations. Only write code and documentation.
2. **Always use LOCAL memory file** `docs/memory.md`, NOT global Claude memory.
3. **Wait for user approval** before implementing any proposed changes.

## Workflow

See `docs/workflow.md` for the iteration cycle: PROPOSE → AGREE → IMPLEMENT → VERIFY → UPDATE PROGRESS → UPDATE MEMORY

Progress tracked in:
- `docs/tasklist.md` — phase checklist and task details
- `docs/memory.md` — patterns, decisions, and lessons learned
