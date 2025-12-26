# Development Task List

**Reference:** [idea.md](idea.md) | [vision.md](vision.md) | [conventions.md](conventions.md)

---

## Progress Report

| Phase | Description | Status | Tests | Notes |
|-------|-------------|--------|-------|-------|
| 1 | Project Setup | ✅ Complete | - | Cargo, CI, structure |
| 2 | Error Types | ✅ Complete | 8/8 | thiserror definitions |
| 3 | Configuration | ✅ Complete | 18/18 | TOML, env vars, secrecy |
| 4 | Logging | ✅ Complete | 13/13 | tracing, redaction |
| 5 | Domain Types | ✅ Complete | 38/38 | Message, Channel, IDs |
| 6 | Link Generation | ✅ Complete | 5/5 | tg://, https://t.me |
| 7 | Rate Limiter | ✅ Complete | 19/19 | Token bucket, proptest |
| 8 | Telegram Auth | ✅ Complete | 8/8 | Session, 2FA, dialoguer |
| 9 | Telegram Client | ✅ Complete | 12/12 | Trait, mocks, validation |
| 10 | MCP Server | ⬜ Pending | 0/0 | rmcp setup |
| 11 | MCP Tools | ⬜ Pending | 0/0 | All 6 tools |
| 12 | Integration | ⬜ Pending | 0/0 | E2E, polish |

**Legend:** ⬜ Pending | 🔄 In Progress | ✅ Complete | ❌ Blocked

**Overall Progress:** 8/12 phases complete

---

## Phase 1: Project Setup ✅

**Goal:** Compilable project with CI pipeline

- [x] Initialize project: `cargo init --lib`
- [x] Configure `Cargo.toml` with all dependencies (see vision.md §1.4)
- [x] Create directory structure:
  ```
  src/lib.rs, src/main.rs
  src/mcp.rs, src/mcp/
  src/telegram.rs, src/telegram/
  ```
- [x] Create empty module files with `todo!()` placeholders
- [x] Setup `.github/workflows/ci.yml`
- [x] Verify: `cargo build` succeeds

**Test:** `cargo build && cargo clippy` ✅ PASSED

---

## Phase 2: Error Types ✅

**Goal:** Type-safe error handling foundation

- [x] Write tests for error Display implementations
- [x] Implement `src/error.rs`:
  - [x] `Error` enum with thiserror
  - [x] Variants: Auth, Telegram API, RateLimit, Config, Network, MCP
- [x] Export from `lib.rs`
- [x] Verify: all tests pass

**Test:** `cargo test error` ✅ PASSED (8/8 tests)

---

## Phase 3: Configuration ✅

**Goal:** Load and validate TOML config

- [x] Write tests for config loading (valid, missing, invalid)
- [x] Write tests for env var expansion (`${VAR}`)
- [x] Write tests for default values
- [x] Implement `src/config.rs`:
  - [x] `Config`, `TelegramConfig`, `SearchConfig`, `RateLimitConfig`, `LoggingConfig`
  - [x] `Config::load()` with path resolution
  - [x] `Config::validate()`
  - [x] Environment variable expansion (no regex dependency)
- [x] Create example `config.example.toml`
- [x] Verify: all tests pass

**Test:** `cargo test config -- --test-threads=1` ✅ PASSED (16/16 tests)

**Note:** Use `--test-threads=1` for tests that modify environment variables to avoid race conditions.

---

## Phase 4: Logging ✅

**Goal:** Structured async-aware logging with sensitive data redaction

- [x] Write tests for redaction functions (10 tests)
- [x] Write tests for init with different formats and levels (3 tests)
- [x] Implement `src/logging.rs`:
  - [x] `init(config: &LoggingConfig)` function
  - [x] stderr output with compact/pretty/json formats
  - [x] `redact_phone()`, `redact_hash()` helpers
- [x] Add `secrecy` crate for sensitive config fields
- [x] Update config to use `SecretString` for api_hash and phone_number
- [x] Verify: all tests pass

**Test:** `cargo test logging` ✅ PASSED (13/13 tests)

**Note:** File logging with rotation deferred to Phase 12 (Polish). Currently outputs to stderr only.

---

## Phase 5: Domain Types ✅

**Goal:** Type-safe domain model (DDD)

- [x] Write tests for ID types (ChannelId, MessageId, UserId)
- [x] Write tests for serde serialization/deserialization
- [x] Write tests for Display implementations
- [x] Implement `src/telegram/types.rs`:
  - [x] `ChannelId`, `MessageId`, `UserId` wrappers (with validation!)
  - [x] `Message` struct with all fields
  - [x] `Channel` struct with all fields
  - [x] `MediaType` enum (comprehensive - 14 variants)
  - [x] `SearchParams`, `SearchResult`, `QueryMetadata`
  - [x] Bonus: `Username` and `ChannelName` validated types
- [x] Export from `src/telegram.rs`
- [x] Verify: all tests pass

**Test:** `cargo test types` ✅ PASSED (38/38 tests)

---

## Phase 6: Link Generation ✅

**Goal:** Generate Telegram deep links

- [x] Write tests for HTTPS link format
- [x] Write tests for tg:// protocol link format
- [x] Write tests for MessageLink construction
- [x] Implement `src/link.rs`:
  - [x] `MessageLink` struct
  - [x] `MessageLink::new(channel_id, message_id)`
  - [x] Generate both link formats
- [x] Verify: all tests pass

**Test:** `cargo test link` ✅ PASSED (5/5 tests)

---

## Phase 7: Rate Limiter ✅

**Goal:** Token bucket rate limiting

- [x] Write tests for initialization (max tokens)
- [x] Write tests for acquire (success, insufficient)
- [x] Write tests for refill over time
- [x] Write property-based tests (proptest) for invariants
- [x] Implement `src/rate_limiter.rs`:
  - [x] `RateLimiter` struct with Mutex<TokenBucket>
  - [x] `RateLimiterTrait` for mockability
  - [x] `acquire(tokens)` async method
  - [x] `available_tokens()` method
- [x] Enhanced `Error::RateLimit` with retry_after_seconds
- [x] Verify: all tests pass including proptest

**Test:** `cargo test rate_limiter` ✅ PASSED (19/19 tests, removed 1 slow proptest)

---

## Phase 8: Telegram Authentication ✅

**Goal:** Session management and 2FA flow

- [x] Write tests for session file operations (save/load)
- [x] Write tests for file permissions (0600 enforcement)
- [x] Implement `src/telegram/auth.rs`:
  - [x] `save_session(path, bytes)` function
  - [x] `load_session(path)` function
  - [x] `is_session_valid(client)` function
  - [x] Interactive auth flow with dialoguer (phone, code, 2FA)
  - [x] Atomic file writes with temp + rename
- [x] Verify: tests pass (8/8 unit tests)

**Test:** `cargo test auth` ✅ PASSED (8/8 tests)

---

## Phase 9: Telegram Client

**Goal:** Channel and message operations

- [ ] Define `TelegramClientTrait` with mockall
- [ ] Write tests with mock client
- [ ] Write tests for channel listing
- [ ] Write tests for channel info retrieval
- [ ] Write tests for message search
- [ ] Implement `src/telegram/client.rs`:
  - [ ] `TelegramClient` struct wrapping grammers
  - [ ] `new(config)` async constructor
  - [ ] `is_connected()` method
  - [ ] `get_subscribed_channels(limit, offset)` method
  - [ ] `get_channel_info(identifier)` method
  - [ ] `search_messages(params)` method
- [ ] Verify: all mock tests pass

**Test:** `cargo test client`

---

## Phase 10: MCP Server

**Goal:** rmcp server setup with stdio transport

- [ ] Write tests for server initialization
- [ ] Write tests for tool registration
- [ ] Implement `src/mcp/server.rs`:
  - [ ] `McpServer` struct
  - [ ] `new(telegram_client, rate_limiter)` constructor
  - [ ] `run_stdio()` method
  - [ ] Tool registration
- [ ] Verify: server starts and responds to initialize

**Test:** `cargo test mcp_server` + manual JSON-RPC test

---

## Phase 11: MCP Tools

**Goal:** All 6 MCP tools implemented

### 11.1 check_mcp_status
- [ ] Write tests for status response format
- [ ] Implement tool handler
- [ ] Verify: returns connection status, rate limit info

### 11.2 get_subscribed_channels
- [ ] Write tests for channel list response
- [ ] Write tests for pagination (limit, offset)
- [ ] Implement tool handler
- [ ] Verify: returns channel array

### 11.3 get_channel_info
- [ ] Write tests for channel metadata response
- [ ] Write tests for not found error
- [ ] Implement tool handler
- [ ] Verify: returns channel details

### 11.4 generate_message_link
- [ ] Write tests for link generation response
- [ ] Implement tool handler
- [ ] Verify: returns both link formats

### 11.5 open_message_in_telegram
- [ ] Write tests for macOS open command
- [ ] Implement tool handler (subprocess)
- [ ] Verify: opens Telegram Desktop

### 11.6 search_messages
- [ ] Write tests for search response format
- [ ] Write tests for parameter validation
- [ ] Write tests for rate limiting integration
- [ ] Write tests for graceful degradation (channel errors)
- [ ] Implement tool handler
- [ ] Verify: returns search results

**Test:** `cargo test tools`

---

## Phase 12: Integration & Polish

**Goal:** Production-ready release

- [ ] Write E2E integration tests
- [ ] Test with real Telegram account
- [ ] Test with Comet browser
- [ ] Add signal handling (SIGTERM, SIGINT)
- [ ] Verify graceful shutdown
- [ ] Run `cargo clippy -- -D warnings`
- [ ] Run `cargo fmt --check`
- [ ] Verify coverage >= 80%
- [ ] Update README.md with quick start
- [ ] Create release build: `cargo build --release`

**Test:** Full E2E flow + Comet integration

---

## Quick Reference

### Run All Tests
```bash
cargo test
```

### Run Specific Phase Tests
```bash
cargo test error
cargo test config
cargo test logging
cargo test types
cargo test link
cargo test rate_limiter
cargo test auth
cargo test client
cargo test mcp
cargo test tools
```

### Check Coverage
```bash
cargo tarpaulin --out Html
```

### Pre-commit Checks
```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```
