# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Telegram MCP Connector - a Model Context Protocol (MCP) service that enables Claude to search Russian-language Telegram channels and messages in real-time. Built in Rust using the `rmcp` SDK and `grammers` Telegram client.

**Current Status:** Phase 11 complete (6/6 MCP tools implemented), 140 tests passing.

## Build & Test Commands

```bash
# Build
cargo build
cargo build --release

# Run all tests (140 tests)
cargo test

# Run tests for specific module
cargo test error           # 9 tests
cargo test config -- --test-threads=1  # 18 tests (serial for env var tests)
cargo test logging         # 13 tests
cargo test types           # 38 tests
cargo test link            # 5 tests
cargo test rate_limiter    # 19 tests
cargo test auth            # 8 tests
cargo test client          # 12 tests
cargo test mcp             # 21 tests (server + all 6 tools)

# Linting and formatting
cargo fmt --check
cargo clippy -- -D warnings

# Pre-commit check (all must pass)
cargo fmt --check && cargo clippy -- -D warnings && cargo test

# Run the binary
cargo run --bin telegram-mcp
```

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                 MCP Client (Comet)                  │
└─────────────────────────┬───────────────────────────┘
                          │ JSON-RPC over stdio
┌─────────────────────────▼───────────────────────────┐
│              MCP Server Layer (rmcp)                │
│         src/mcp/server.rs, tools/, tools/types.rs   │
└─────────────────────────┬───────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────┐
│              Application Layer                       │
│     config, logging, rate_limiter, link, error      │
└─────────────────────────┬───────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────┐
│              Telegram Layer (grammers)              │
│        src/telegram/client.rs, auth.rs, types.rs    │
└─────────────────────────┬───────────────────────────┘
                          │ MTProto
                          ▼
                   Telegram Cloud API
```

**Key design patterns:**
- Library + Binary separation (`lib.rs` for core logic, `main.rs` for CLI)
- Shared state via `Arc<T>` for Telegram client and rate limiter
- Traits with `mockall` for testability (e.g., `TelegramClientTrait`, `RateLimiterTrait`)
- No `mod.rs` files - use file-as-module pattern (e.g., `src/mcp.rs` declares submodules)
- JSON schemas via `schemars` for MCP tool parameters

## Module Structure

| Module | Purpose |
|--------|---------|
| `error.rs` | Error types with thiserror (RateLimit includes retry_after) |
| `config.rs` | TOML config loading, env var expansion, SecretString for sensitive data |
| `logging.rs` | tracing subscriber setup, sensitive data redaction |
| `rate_limiter.rs` | Token bucket rate limiting with retry_after calculation |
| `link.rs` | Telegram deep link generation (tg://, https://t.me) |
| `mcp/server.rs` | rmcp ServerHandler + MCP tool methods |
| `mcp/tools.rs` | Re-exports tools module |
| `mcp/tools/types.rs` | MCP tool request/response types with JsonSchema |
| `telegram/client.rs` | TelegramClientTrait + mock-based implementation |
| `telegram/auth.rs` | Session persistence (atomic writes, 0600 perms), 2FA flow |
| `telegram/types.rs` | Domain types (Message, Channel, IDs) with JsonSchema |

## MCP Tools (Phase 11 Complete)

| Tool | Status | Description |
|------|--------|-------------|
| `check_mcp_status` | ✅ | Connection status, rate limiter tokens |
| `get_subscribed_channels` | ✅ | List user's Telegram channels with pagination |
| `get_channel_info` | ✅ | Get channel metadata by username or ID |
| `generate_message_link` | ✅ | Generate tg:// and https://t.me links |
| `open_message_in_telegram` | ✅ | Open message in Telegram Desktop (macOS) |
| `search_messages` | ✅ | Search messages with rate limiting |

## Development Methodology

**TDD Workflow:** RED → GREEN → REFACTOR
- Write failing test first
- Write minimal code to pass
- Refactor while tests stay green

**KISS Principles:**
- Single Responsibility: one module = one purpose
- No premature abstraction: extract only after Rule of Three
- Explicit over implicit: clear function signatures
- Delete over comment: remove dead code

## Error Handling

- **Library layer:** `thiserror` for typed error definitions
- **Application layer:** `anyhow` for error context and propagation
- **NEVER use `unwrap()` in production code** - use `?` or `expect()` with clear messages

```rust
// Library errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("session expired")]
    SessionExpired,
}

// Application errors - use .context()
let config = Config::load().context("Failed to load config")?;

// NEVER use unwrap() in production code
// let value = option.unwrap();  // ❌ WRONG - can panic
let value = option.context("Expected value to be present")?;  // ✅ RIGHT
```

## Domain Types (DDD)

Type-safe ID wrappers prevent accidental misuse:
```rust
pub struct ChannelId(pub i64);
pub struct MessageId(pub i64);
```

## Configuration

Config file: `~/.config/telegram-connector/config.toml`

Supports `${VAR}` syntax for environment variable expansion in sensitive fields.

## Logging

- Use `tracing` for structured async logging
- Never log sensitive data (phone numbers, API hashes, passwords, session tokens)
- Use `redact_phone()` and `redact_hash()` helpers

## Workflow

See `doc/workflow.md` for the iteration cycle:
1. PROPOSE - describe solution with tests and API
2. AGREE - wait for user confirmation
3. IMPLEMENT - tests first, then code
4. VERIFY - wait for confirmation
5. UPDATE PROGRESS - update `doc/tasklist.md`
6. UPDATE MEMORY - after completing each phase/iteration, update `doc/memory.md` (LOCAL file) to record:
   - Progress made (what was completed)
   - Patterns applied (design decisions, architectural choices)
   - Lessons learned (gotchas, edge cases discovered)
   - Code patterns to reuse in future phases

**IMPORTANT:** Always use LOCAL memory file `doc/memory.md`, NOT global Claude memory.

Current progress tracked in:
- `doc/tasklist.md` - checklist of phases and tasks
- `doc/memory.md` - detailed notes, patterns, and lessons learned

## Development Progress

| Phase | Description | Status | Tests |
|-------|-------------|--------|-------|
| 1 | Project Setup | ✅ | - |
| 2 | Error Types | ✅ | 9/9 |
| 3 | Configuration | ✅ | 18/18 |
| 4 | Logging | ✅ | 13/13 |
| 5 | Domain Types | ✅ | 38/38 |
| 6 | Link Generation | ✅ | 5/5 |
| 7 | Rate Limiter | ✅ | 19/19 |
| 8 | Telegram Auth | ✅ | 8/8 |
| 9 | Telegram Client | ✅ | 12/12 |
| 10 | MCP Server | ✅ | 2/2 |
| 11 | MCP Tools | ✅ | 21/21 |
| 12 | Integration | ⬜ | - |

**Overall:** 11/12 phases complete, ready for Phase 12 (Integration & Polish)
