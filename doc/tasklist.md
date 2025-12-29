# Development Task List

**Reference:** [idea.md](idea.md) | [vision.md](vision.md) | [conventions.md](conventions.md)

---

## Progress Report

| Phase | Description | Status | Tests | Notes |
|-------|-------------|--------|-------|-------|
| 1 | Project Setup | ✅ Complete | - | Cargo, CI, structure |
| 2 | Error Types | ✅ Complete | 11/11 | thiserror definitions |
| 3 | Configuration | ✅ Complete | 15/15 (+4 ignored) | TOML, env vars, secrecy |
| 4 | Logging | ✅ Complete | 13/13 | tracing, redaction |
| 5 | Domain Types | ✅ Complete | 42/42 | Message, Channel, IDs |
| 6 | Link Generation | ✅ Complete | 8/8 | tg://, https://t.me |
| 7 | Rate Limiter | ✅ Complete | 19/19 | Token bucket, proptest |
| 8 | Telegram Auth | ✅ Complete | 1/1 | Interactive auth flow |
| 9 | Telegram Client | ✅ Complete | 12/12 | Real grammers + mocks |
| 10 | MCP Server | ✅ Complete | 19/19 | rmcp setup, stdio, tools |
| 11 | MCP Tools | ✅ Complete | 4/4 | types.rs schemas |
| 12 | Integration | ✅ Complete | 7/7 (CLI) | CLI, grammers, shutdown, rmcp tools |
| 13 | Refactoring | ✅ Complete | - | Split large files, extract tests |
| 14 | Conditional Credentials | ⬜ Pending | - | Credentials only required for --setup |

**Legend:** ⬜ Pending | 🔄 In Progress | ✅ Complete | ❌ Blocked

**Overall Progress:** 13/14 phases complete - Phase 14 pending (conditional credential requirements)

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

## Phase 9: Telegram Client ✅

**Goal:** Channel and message operations

- [x] Define `TelegramClientTrait` with mockall
- [x] Write tests with mock client
- [x] Write tests for channel listing
- [x] Write tests for channel info retrieval
- [x] Write tests for message search
- [x] Implement `src/telegram/client.rs`:
  - [x] `TelegramClient` struct wrapping grammers
  - [x] `new(config)` async constructor
  - [x] `is_connected()` method
  - [x] `get_subscribed_channels(limit, offset)` method
  - [x] `get_channel_info(identifier)` method
  - [x] `search_messages(params)` method
- [x] Verify: all mock tests pass

**Test:** `cargo test client` ✅ PASSED (12/12 tests)

---

## Phase 10: MCP Server ✅

**Goal:** rmcp server setup with stdio transport

- [x] Write tests for server initialization
- [x] Write tests for ServerHandler metadata
- [x] Implement `src/mcp/server.rs`:
  - [x] `McpServer` struct with Arc<T> fields
  - [x] `new(telegram_client, rate_limiter)` constructor
  - [x] `run_stdio()` method with stdio transport
  - [x] ServerHandler trait implementation
- [x] Verify: server compiles and tests pass

**Test:** `cargo test mcp::server` ✅ PASSED (2/2 tests)

**Note:** Tool registration deferred to Phase 11

---

## Phase 11: MCP Tools ✅

**Goal:** All 6 MCP tools implemented

**Status:** Complete - All 6 tools implemented with 21 tests

### 11.0 Foundations ✅
- [x] Add schemars dependency to Cargo.toml
- [x] Create `src/mcp/tools/` module structure
- [x] Implement `types.rs` with all request/response types
- [x] Add JsonSchema derive to all telegram domain types
- [x] Verify: types compile and tests pass (4/4 tests)

### 11.1 check_mcp_status ✅
- [x] Write tests for status response format (2 tests)
- [x] Implement tool handler (server.rs:47-56)
- [x] Verify: returns connection status, rate limit info

### 11.2 get_subscribed_channels ✅
- [x] Write tests for channel list response
- [x] Write tests for pagination (limit, offset)
- [x] Implement tool handler
- [x] Verify: returns channel array

### 11.3 get_channel_info ✅
- [x] Write tests for channel metadata response
- [x] Write tests for not found error
- [x] Implement tool handler
- [x] Verify: returns channel details

### 11.4 generate_message_link ✅
- [x] Write tests for link generation response (3 tests)
- [x] Implement tool handler (uses link.rs from Phase 6)
- [x] Verify: returns both link formats

### 11.5 open_message_in_telegram ✅
- [x] Write tests for macOS open command (3 tests)
- [x] Implement tool handler (subprocess with tokio::process::Command)
- [x] Verify: returns response with link used
- [x] Platform-specific: macOS only, returns error on other platforms

### 11.6 search_messages ✅
- [x] Write tests for search response format
- [x] Write tests for parameter validation (empty query)
- [x] Write tests for rate limiting integration
- [x] Write tests for channel filter and limits
- [x] Implement tool handler (5 tests)
- [x] Verify: returns search results

**Test:** `cargo test mcp` ✅ (21/21 tests passing)

---

## Phase 12: Integration & Polish ✅

**Goal:** Production-ready release

- [x] Add ServerConfig with shutdown_timeout_seconds
- [x] Add CLI argument parsing (--setup, --session-file, --config)
- [x] Implement real TelegramClient with grammers connection
  - [x] SqliteSession for persistent storage
  - [x] SenderPool for connection management
  - [x] get_subscribed_channels with iter_dialogs()
  - [x] get_channel_info with resolve_username()
  - [x] search_messages with global/channel-specific search
- [x] Update auth.rs for interactive authentication flow
- [x] Implement main.rs with signal handling (SIGTERM, SIGINT)
- [x] Graceful shutdown with configurable timeout
- [x] Integrate rmcp tool attributes (`#[tool]` macros) for all 6 MCP tools
- [x] Run `cargo clippy -- -D warnings` ✅
- [x] Run `cargo fmt --check` ✅
- [x] All 139 tests passing (4 ignored)
- [x] Update README.md with comprehensive documentation and Comet Browser guide

**Manual Testing:**
- [x] Test with real Telegram account ✅
- [x] Test with MCP client (Comet Browser) ✅
- [x] Create release build: `cargo build --release` ✅

**Test:** `cargo test` ✅ (139 passing, 4 ignored)

---

## Phase 13: Code Refactoring ✅

**Goal:** Split large files for better maintainability

**Context:** server.rs (945 lines) and client.rs (755 lines) exceeded recommended 300-line limit.

**Results:**
- server.rs: 945 → 307 lines (tests extracted to src/mcp/tests/)
- client.rs: 755 → 343 lines (trait + converters extracted)

### 13.1 MCP Server Refactoring ✅
- [x] Create `src/mcp/tests/` directory structure
- [x] Extract tests to `server_core.rs`, `status.rs`, `channels.rs`, `links.rs`, `search.rs`
- [x] Update `src/mcp/server.rs` with `#[path]` attribute for test module
- [x] Verify: all tests pass after refactoring

### 13.2 Telegram Client Refactoring ✅
- [x] Extract `src/telegram/trait_def.rs` - TelegramClientTrait definition
- [x] Extract `src/telegram/converters.rs` - convert_peer_to_channel, convert_message helpers
- [x] Create `src/telegram/tests/client_tests.rs` - extract client tests
- [x] Update `src/telegram.rs` to re-export from new modules
- [x] Verify: all tests pass after refactoring

### 13.3 Code Quality Improvements ✅
- [x] Run `cargo clippy -- -D warnings` ✅
- [x] Run `cargo fmt --check` ✅
- [x] All 139 tests passing (4 ignored)

**Final Structure:**
```
src/mcp/
├── server.rs           # Core server, tool routing (307 lines)
├── tools.rs            # Re-exports types module
├── tools/
│   └── types.rs        # Request/response types
├── tests.rs            # Test module declarations (file-as-module pattern)
└── tests/
    ├── server_core.rs  # Server creation tests
    ├── status.rs       # check_mcp_status tests
    ├── channels.rs     # channel tool tests
    ├── links.rs        # link generation tool tests
    └── search.rs       # search_messages tests

src/telegram/
├── client.rs           # Core client implementation (343 lines)
├── trait_def.rs        # TelegramClientTrait + MockTelegramClientTrait
├── converters.rs       # Type conversion helpers
├── auth.rs             # Authentication
├── types.rs            # Domain types
├── tests.rs            # Test module declarations (file-as-module pattern)
└── tests/
    └── client_tests.rs # Client mock tests
```

**Test:** `cargo test` ✅ (140 passing, 4 ignored)

---

## Phase 14: Conditional Credential Requirements ⬜

**Goal:** Make credentials (api_id, api_hash, phone_number) required ONLY with `--setup` flag

**Problem:** Currently the program always requires credentials in config, even though they're only needed once during initial session creation. After authentication, the session file should be sufficient.

**Expected Behavior:**
| Mode | Credentials Required | Session Required | Action |
|------|---------------------|------------------|--------|
| `--setup` | ✅ Yes | ❌ No (will create) | Authenticate and create session |
| Normal (MCP) | ❌ No | ✅ Yes (must exist) | Use existing session, start MCP server |

### 14.1 Config Refactoring ⬜
- [ ] Make `api_id`, `api_hash`, `phone_number` optional in `TelegramConfig`
  - Change from required fields to `Option<T>`
- [ ] Add `TelegramConfig::has_credentials()` method
  - Returns true if all three credential fields are present
- [ ] Split validation into two modes:
  - `validate_for_setup()` - ensures credentials are present
  - `validate_for_operation()` - only validates non-credential fields
- [ ] Update tests for new optional fields

### 14.2 Client Initialization Refactoring ⬜
- [ ] Create `TelegramClient::from_session(session_path)` constructor
  - Loads existing session without requiring credentials
  - Returns error if session doesn't exist or is invalid
- [ ] Update `TelegramClient::new()` to require full config (for setup)
- [ ] Add `TelegramClient::is_session_valid()` static method
  - Checks if session file exists and can be loaded

### 14.3 Main Flow Refactoring ⬜
- [ ] Update `main.rs` startup logic:
  ```
  if --setup flag:
      require credentials in config
      create client with full config
      run authentication flow
      save session
  else:
      load session from file (no credentials needed)
      if session invalid/missing:
          error: "Session not found. Run with --setup to authenticate."
      start MCP server
  ```
- [ ] Update error messages to guide users to `--setup`
- [ ] Test both flows manually

### 14.4 Documentation & Tests ⬜
- [ ] Update README.md with clear setup vs. operation instructions
- [ ] Update config.example.toml to show credentials are optional for normal use
- [ ] Add integration tests for:
  - Setup mode with valid credentials
  - Normal mode with existing session
  - Normal mode with missing session (should fail gracefully)
- [ ] Run `cargo clippy -- -D warnings` ✅
- [ ] Run `cargo fmt --check` ✅
- [ ] All tests passing

**Target Config Structure:**
```toml
[telegram]
# Required ONLY for --setup (initial authentication)
# Can be omitted or empty for normal MCP operation
api_id = "${TELEGRAM_API_ID}"        # Optional for normal use
api_hash = "${TELEGRAM_API_HASH}"    # Optional for normal use
phone_number = "${TELEGRAM_PHONE_NUMBER}"  # Optional for normal use

# Always used - path to session file
session_file = "~/.config/telegram-connector/session.bin"
```

**Usage Examples:**
```bash
# First time setup (requires credentials in config)
TELEGRAM_API_ID=12345 TELEGRAM_API_HASH=abc... TELEGRAM_PHONE_NUMBER=+1... \
  cargo run --bin telegram-mcp -- --setup --config ./config.toml

# Normal operation (uses existing session, no credentials needed)
cargo run --bin telegram-mcp -- --config ./config.toml
```

**Test:** `cargo test` (all tests must pass)

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
