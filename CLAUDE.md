# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Telegram MCP Connector - a Model Context Protocol (MCP) service that enables Claude to search Russian-language Telegram channels and messages in real-time. Built in Rust using the `rmcp` SDK and `grammers` Telegram client.

## Build & Test Commands

```bash
# Build
cargo build
cargo build --release

# Run all tests
cargo test

# Run tests for specific module
cargo test error
cargo test config -- --test-threads=1  # Serial execution for env var tests
cargo test types
cargo test link
cargo test rate_limiter

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
│              src/mcp/server.rs, tools.rs            │
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
- Traits with `mockall` for testability (e.g., `TelegramClientTrait`)
- No `mod.rs` files - use file-as-module pattern (e.g., `src/mcp.rs` declares submodules)

## Module Structure

| Module | Purpose |
|--------|---------|
| `error.rs` | Error types with thiserror |
| `config.rs` | TOML config loading, env var expansion |
| `logging.rs` | tracing subscriber setup, sensitive data redaction |
| `rate_limiter.rs` | Token bucket rate limiting |
| `link.rs` | Telegram deep link generation (tg://, https://t.me) |
| `mcp/server.rs` | rmcp server setup |
| `mcp/tools.rs` | MCP tool implementations |
| `telegram/client.rs` | Grammers client wrapper |
| `telegram/auth.rs` | Session management, 2FA flow |
| `telegram/types.rs` | Domain types (Message, Channel, IDs) |

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

```rust
// Library errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("session expired")]
    SessionExpired,
}

// Application errors - use .context()
let config = Config::load().context("Failed to load config")?;
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
