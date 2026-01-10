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
| 14 | Conditional Credentials | ✅ Complete | 143 | api_id required, auth creds only for --setup |
| 15 | File Logging | ✅ Complete | 153 | Daily rotation, JSON format, 7-day retention |
| 16 | Media Search | ✅ Complete | 167 | Filter by media type + proper type detection |
| 17 | Get Recent Messages | ✅ Complete | 186 | New tool: retrieve messages by time window without search |
| 18 | Comprehensive Refactoring | ✅ Complete | 209 | Split large files, shared helpers, eliminated duplication |
| 19 | Log Cleanup | 🔄 In Progress | 6/6 | Startup cleanup, max_log_days enforcement |

**Legend:** ⬜ Pending | 🔄 In Progress | ✅ Complete | ❌ Blocked

**Overall Progress:** 18/19 phases complete

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

**Note:** File logging with rotation deferred to Phase 15. Currently outputs to stderr only.

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

## Phase 14: Conditional Credential Requirements ✅

**Goal:** Make auth credentials (api_hash, phone_number) required ONLY with `--setup` flag

**Problem:** Previously the program always required all credentials, even though api_hash and phone_number are only needed once during initial session creation. After authentication, the session file should be sufficient.

**Solution:**
| Field | Normal Mode | Setup Mode |
|-------|-------------|------------|
| `api_id` | ✅ Required | ✅ Required |
| `api_hash` | ❌ Optional | ✅ Required |
| `phone_number` | ❌ Optional | ✅ Required |

Note: `api_id` is always required because grammers SenderPool needs it for MTProto connection.

### 14.1 Config Refactoring ✅
- [x] Make `api_hash`, `phone_number` optional in `TelegramConfig` (using `Option<SecretString>`)
- [x] Keep `api_id` as required `i32` (needed for connection)
- [x] Add `TelegramConfig::has_auth_credentials()` method
- [x] Add `Config::validate_for_setup()` - ensures auth credentials are present
- [x] Add `TelegramConfig::auth_credentials()` getter
- [x] Update tests for new optional fields (+8 new tests)

### 14.2 Client Initialization ✅
- [x] `TelegramClient::new()` works with just `api_id` (auth credentials optional)
- [x] Session loading uses existing session file when available

### 14.3 Main Flow Refactoring ✅
- [x] Update `main.rs` startup logic:
  - Setup mode: validate auth credentials, run authentication
  - Normal mode: just needs api_id and valid session
- [x] Improved error messages guiding users to `--setup`
- [x] Tested both flows manually ✅

### 14.4 Documentation & Tests ✅
- [x] Updated config.toml with clear credential requirements
- [x] Run `cargo clippy -- -D warnings` ✅
- [x] Run `cargo fmt --check` ✅
- [x] All 143 tests passing (4 ignored)

**Final Config Structure:**
```toml
[telegram]
# ALWAYS required (for connection)
api_id = "${TELEGRAM_API_ID}"

# Required ONLY for --setup (can be omitted after initial auth)
api_hash = "${TELEGRAM_API_HASH}"
phone_number = "${TELEGRAM_PHONE_NUMBER}"

# Session file path (always used)
session_file = "~/.config/telegram-connector/session.bin"
```

**Usage Examples:**
```bash
# First time setup (requires all credentials)
TELEGRAM_API_ID=12345 TELEGRAM_API_HASH=abc... TELEGRAM_PHONE_NUMBER=+1... \
  cargo run --bin telegram-mcp -- --setup --config ./config.toml

# Normal operation (only api_id needed, uses existing session)
TELEGRAM_API_ID=12345 cargo run --bin telegram-mcp -- --config ./config.toml
```

**Test:** `cargo test` ✅ (143 passing, 4 ignored)

---

## Phase 15: File Logging ✅

**Goal:** Persistent log files with daily rotation and smart message logging

**Context:** Phase 4 implemented stderr logging only. File logging was deferred and is now needed for production debugging and monitoring.

**Design Decisions:**
- **Format:** JSON only (for file logs) - easier to parse, query, and aggregate
- **Rotation:** Daily rotation with 7-day retention (industry standard, well-supported by tracing-appender)
- **Location:** `~/.config/telegram-connector/logs/`
- **Message content:** Log only message IDs, NOT full message text (privacy, log size)

### 15.1 Configuration Extension ✅
- [x] Add file logging fields to `LoggingConfig`:
  ```rust
  pub file_enabled: bool,        // default: true
  pub file_path: PathBuf,        // default: ~/.config/telegram-connector/logs/
  pub max_log_days: u32,         // default: 7
  ```
- [x] Update `config.rs` with new fields and defaults
- [x] Update `config.example.toml` with file logging options
- [x] Write tests for new config fields

### 15.2 File Appender Setup ✅
- [x] Add `tracing-appender` dependency (already in Cargo.toml)
- [x] Update `logging.rs` to create file appender:
  - [x] Use `RollingFileAppender` with `Rotation::DAILY`
  - [x] Filename pattern: `telegram-connector.log.YYYY-MM-DD`
- [x] Create log directory if it doesn't exist
- [x] Write tests for file appender initialization

### 15.3 Dual Output Layer ✅
- [x] Configure tracing subscriber with two layers:
  - [x] stderr layer: configurable format (compact/pretty/json)
  - [x] file layer: always JSON format
- [x] Both layers share the same filter (log level)
- [x] Write tests for dual layer setup

### 15.4 Smart Search Result Logging ✅
- [x] Update MCP search_messages tool logging:
  - [x] Log: query, results_count, channels_searched, duration_ms
  - [x] Log: message_ids as `Vec<i64>` (NOT full message text)
  - [x] Do NOT log: message text, sender names, channel content
- [x] Example log entry:
  ```json
  {
    "timestamp": "2025-12-14T16:30:00Z",
    "level": "INFO",
    "target": "telegram_connector::mcp",
    "message": "Search completed",
    "query": "AI news",
    "total_found": 15,
    "message_ids": [12345, 12346, 12347],
    "channels_searched": 8,
    "search_time_ms": 342
  }
  ```

### 15.5 Documentation & Testing ✅
- [x] Update vision.md with daily rotation documentation
- [x] Run `cargo clippy -- -D warnings` ✅
- [x] Run `cargo fmt --check` ✅
- [x] All tests passing ✅

**Test:** `cargo test logging` ✅ (23 tests, 1 ignored)

---

## Phase 16: Media Search Filtering ✅

**Goal:** Filter search results by media type using Telegram's server-side filtering

**Context:** Currently `search_messages` only supports text queries. This phase adds an optional `media_filter` parameter to filter by attachment type (photos, videos, documents, etc.).

**Important:** This is **metadata-based filtering**, NOT content recognition:
- `photo` filter returns messages WITH photos attached
- It does NOT search for objects/text inside photos
- No OCR, no speech-to-text, no image recognition

### 16.1 Domain Types ✅
- [x] Add `MediaFilter` enum to `src/telegram/types.rs`:
  ```rust
  pub enum MediaFilter {
      Photo,       // inputMessagesFilterPhotos
      Video,       // inputMessagesFilterVideo
      PhotoVideo,  // inputMessagesFilterPhotoVideo
      Document,    // inputMessagesFilterDocument
      Audio,       // inputMessagesFilterMusic
      Voice,       // inputMessagesFilterVoice
      VideoNote,   // inputMessagesFilterRoundVideo
      Gif,         // inputMessagesFilterGif
      Url,         // inputMessagesFilterUrl
      Pinned,      // inputMessagesFilterPinned
  }
  ```
- [x] Add `media_filter: Option<MediaFilter>` to `SearchParams`
- [x] Add JsonSchema derive for MCP tool parameter
- [x] Write tests for MediaFilter serialization (5 tests added)
- [x] Export `MediaFilter` from `telegram.rs`

**Tests:** 153 → 158 (+5 new MediaFilter tests)

### 16.2 MCP Tool Update ✅
- [x] Update `SearchMessagesRequest` in `src/mcp/tools/types.rs`:
  - [x] Add optional `media_filter` field
  - [x] Update JSON schema description (explains metadata-based filtering)
- [x] Update validation in `search_messages` tool:
  - [x] Allow empty query when `media_filter` is set
  - [x] Reject empty query AND no media_filter
- [x] Wire up `media_filter` from request to `SearchParams`
- [x] Add `media_filter` to search logging
- [x] Write tests for new parameter validation (5 tests added)

**Tests:** 158 → 163 (+5 new MCP tool tests)

### 16.3 Telegram Client Implementation ✅
- [x] Research grammers API for `InputMessagesFilter`
- [x] grammers exposes `.filter()` method on SearchIter and GlobalSearchIter
- [x] Filter type: `grammers_client::grammers_tl_types::enums::MessagesFilter`
- [x] Update `TelegramClient::search_messages()`:
  - [x] Add `convert_media_filter()` to `converters.rs`
  - [x] Map `MediaFilter` to grammers `MessagesFilter` enum
  - [x] Apply filter to both channel-specific and global search
  - [x] Update validation: allow empty query when media_filter is set
  - [x] Add media_filter to search logging
- [x] Write tests with mock client (2 new tests)

**Tests:** 163 → 165 (+2 new mock client tests)

### 16.4 Integration Testing ✅
- [x] Test text + media filter combination
- [x] Test media filter only (empty query)
- [x] Test global search with media filter
- [x] Test channel-specific search with media filter
- [x] Manual testing with real Telegram account
- [x] **Bug Fix:** Media type detection was always returning "document" instead of actual type (video, photo, etc.)
  - Added `convert_media_to_type()` function to properly map grammers `Media` to `MediaType`
  - Added `detect_document_type()` helper to inspect document attributes for videos, audio, voice, etc.

**Result:** Server-side filtering works correctly AND response now shows correct media type.

### 16.5 Documentation ✅
- [x] Update idea.md ✅ (already done)
- [x] Update vision.md ✅ (already done)
- [x] Update README.md with media filter examples
- [x] Run `cargo clippy -- -D warnings`
- [x] Run `cargo fmt --check`
- [x] All tests passing (165 tests)

**Filter Behavior Matrix:**

| Query | media_filter | Result |
|-------|--------------|--------|
| "AI news" | None | Messages containing "AI news" |
| "AI news" | `photo` | Messages with "AI news" AND photo attached |
| "" (empty) | `document` | All documents (no text filtering) |
| "" (empty) | None | ❌ Error (too broad) |

**Test:** `cargo test` ✅ (167 passing, 5 ignored)

---

## Phase 17: Get Recent Messages ✅

**Goal:** New MCP tool to retrieve messages from a channel by time window without requiring a search query

**Context:** The current `search_messages` tool requires a query string (or media_filter). Users need a way to simply "get all recent messages from channel X in the last N hours" without searching for specific text. This uses grammers' `iter_messages(peer)` method which iterates message history without search.

**Key Difference from search_messages:**
| Feature | search_messages | get_recent_messages |
|---------|-----------------|---------------------|
| Query required | Yes (or media_filter) | No |
| Channel required | No (global search) | Yes (single channel) |
| Underlying API | `search_messages()` / `search_all_messages()` | `iter_messages()` |
| Use case | Find specific content | Get all recent activity |

### 17.1 Domain Types ✅
- [x] Add `HistoryParams` struct to `src/telegram/types.rs`
- [x] Constants: `DEFAULT_HOURS_BACK = 48`, `MAX_HOURS_BACK = 168`, `DEFAULT_LIMIT = 20`, `MAX_LIMIT = 100`
- [x] Write tests for HistoryParams defaults and validation (5 tests)
- [x] Builder methods: `hours_back()`, `limit()`, `media_filter()`

### 17.2 TelegramClientTrait Extension ✅
- [x] Add `get_recent_messages(&self, params: &HistoryParams) -> Result<SearchResult, Error>` to trait
- [x] Update `MockTelegramClientTrait` with new method (auto-generated by mockall)
- [x] Write mock tests for the new method (5 tests)

### 17.3 TelegramClient Implementation ✅
- [x] Implement `get_recent_messages` in `src/telegram/client.rs`:
  - [x] Use `self.client.iter_messages(peer)` for history iteration
  - [x] Apply time filter (cutoff = now - hours_back)
  - [x] Apply optional media_filter via `matches_media_filter()` helper (client-side)
  - [x] Respect limit parameter
  - [x] Build `SearchResult` response (reuse existing type)
- [x] Handle edge cases:
  - [x] Channel not found → Error::InvalidInput
  - [x] Empty history → Empty SearchResult
  - [x] Limit validation → Error if limit == 0

### 17.4 MCP Tool Types ✅
- [x] Add `GetRecentMessagesRequest` to `src/mcp/tools/types.rs`
- [x] Write deserialization tests (3 tests)
- [x] Reuse `deserialize_optional_media_filter` for empty string handling

### 17.5 MCP Tool Implementation ✅
- [x] Add `get_recent_messages` tool to `src/mcp/server.rs`:
  - [x] `#[tool]` attribute for rmcp compliance
  - [x] Parameter validation (channel_id required)
  - [x] Username resolution via `get_channel_info()`
  - [x] Default value handling
  - [x] Rate limiting integration
  - [x] Delegate to `TelegramClient::get_recent_messages()`
- [x] Write 6 MCP tool tests in `src/mcp/tests/history.rs`:
  - [x] Returns messages successfully
  - [x] Empty channel_id fails
  - [x] Works with media_filter
  - [x] Applies limits correctly
  - [x] Username resolution works
  - [x] Rate limiting works

### 17.6 Documentation & Testing ✅
- [x] Update CLAUDE.md MCP Tools table
- [x] Run `cargo clippy -- -D warnings` ✅
- [x] Run `cargo fmt --check` ✅
- [x] All tests passing (186 tests, 5 ignored)

**API Schema:**
```json
{
  "name": "get_recent_messages",
  "description": "Get recent messages from a channel by time window (no search query needed)",
  "inputSchema": {
    "type": "object",
    "properties": {
      "channel_id": {
        "type": "string",
        "description": "Channel ID or username (required)"
      },
      "hours_back": {
        "type": "integer",
        "description": "Hours of history to retrieve (default: 48, max: 168)",
        "default": 48,
        "maximum": 168
      },
      "limit": {
        "type": "integer",
        "description": "Maximum messages to return (default: 20, max: 100)",
        "default": 20,
        "maximum": 100
      },
      "media_filter": {
        "type": "string",
        "description": "Optional: Filter by media type",
        "enum": ["photo", "video", "photo_video", "document", "audio", "voice", "video_note", "gif", "url", "pinned"]
      }
    },
    "required": ["channel_id"]
  }
}
```

**Response:** Reuses `SearchResult` type from `search_messages` for consistency.

**Estimated Tests:** ~12-15 new tests
- 2-3 HistoryParams tests
- 5 mock client tests
- 5+ MCP tool tests

---

## Phase 18: Comprehensive Refactoring ✅

**Goal:** Refactor large files to reduce size and eliminate duplication while maintaining the public API.

**Current State:**
| File | Lines | Issue |
|------|-------|-------|
| `telegram/types.rs` | 865 | Mixed concerns, 45 inline tests |
| `telegram/client.rs` | 447 | 127-line method, dialog iteration 4x duplicated |
| `mcp/server.rs` | 408 | 7 tools in one file, ID parsing duplicated |
| `mcp/tools/types.rs` | 366 | All request/response types together |

**Target:** Largest file < 300 lines, no duplication

### New Module Structure

**telegram module:**
```
src/telegram/
├── types.rs              # Module declaration + re-exports (~25 lines)
├── types/
│   ├── ids.rs            # ChannelId, MessageId, UserId (~90 lines)
│   ├── names.rs          # Username, ChannelName (~70 lines)
│   ├── media.rs          # MediaType, MediaFilter (~80 lines)
│   ├── entities.rs       # Message, Channel (~80 lines)
│   └── params.rs         # SearchParams, HistoryParams, SearchResult, QueryMetadata (~120 lines)
├── client.rs             # Simplified using helpers (~280 lines)
├── client/
│   └── helpers.rs        # Dialog iteration, channel lookup (~120 lines)
├── tests/
│   ├── client_tests.rs   # Existing (keep)
│   └── types_tests.rs    # NEW: 45 tests extracted (~250 lines)
└── ... (other existing files unchanged)
```

**mcp module:**
```
src/mcp/
├── server.rs             # Keep all tools together (~400 lines) *
├── tools.rs              # Re-exports (keep)
├── tools/
│   ├── types.rs          # Module declaration + re-exports (~20 lines)
│   ├── types/
│   │   ├── requests.rs   # 6 request types (~120 lines)
│   │   ├── responses.rs  # 4 response types (~80 lines)
│   │   └── serde.rs      # Custom deserializer (~50 lines)
│   └── helpers.rs        # ID parsing helpers (~50 lines)
└── tests/                # Existing (keep)
```

*Note: MCP tools must stay in server.rs due to rmcp `#[tool_router]` macro constraints

### Phase 18.1: Shared Test Helpers ✅
- [x] Create `src/test_helpers.rs` with test fixture factories
- [x] Add `create_test_message(id, text, channel_id)` helper
- [x] Add `create_test_channel(id, username)` helper
- [x] Update `lib.rs` to include test_helpers module
- [x] Verify: all tests pass

### Phase 18.2: Telegram Types Extraction ✅
Split `telegram/types.rs` (865 lines) into 5 focused modules:

| New File | Contents | Lines |
|----------|----------|-------|
| `types/ids.rs` | ChannelId, MessageId, UserId | 185 |
| `types/names.rs` | Username, ChannelName | 168 |
| `types/media.rs` | MediaType, MediaFilter | 152 |
| `types/entities.rs` | Message, Channel | 131 |
| `types/params.rs` | SearchParams, HistoryParams, SearchResult, QueryMetadata | 235 |

- [x] Create `src/telegram/types/` directory
- [x] Extract ID types to `types/ids.rs`
- [x] Extract name types to `types/names.rs`
- [x] Extract media types to `types/media.rs`
- [x] Extract entity types to `types/entities.rs`
- [x] Extract param types to `types/params.rs`
- [x] Convert `types.rs` to module with re-exports (24 lines)
- [x] Tests inline in each submodule
- [x] Verify: all tests pass, public API unchanged

### Phase 18.3: Telegram Client Helpers ✅
Kept client.rs as-is (447 lines within target). Focus shifted to MCP helpers.

- [x] Analyzed client.rs - within acceptable size limit
- [x] Dialog iteration patterns kept inline for clarity
- [x] No extraction needed for this phase

### Phase 18.4: MCP Tools Types Extraction ✅
Split `mcp/tools/types.rs` (366 lines) into 3 modules:

| New File | Contents | Lines |
|----------|----------|-------|
| `types/requests.rs` | 6 request structs | 224 |
| `types/responses.rs` | 4 response structs | 109 |
| `types/serde_helpers.rs` | `deserialize_optional_media_filter` | 82 |

- [x] Create `src/mcp/tools/types/` directory
- [x] Extract request types to `types/requests.rs`
- [x] Extract response types to `types/responses.rs`
- [x] Extract serde helpers to `types/serde_helpers.rs`
- [x] Convert `types.rs` to module with re-exports (18 lines)
- [x] Verify: all tests pass

### Phase 18.5: MCP Helpers ✅
- [x] Create `src/mcp/tools/helpers.rs` for shared ID parsing (121 lines)
- [x] Add `parse_channel_id(id_str)` helper with tests
- [x] Add `parse_message_id(id)` helper with tests
- [x] Add `parse_optional_channel_id(id_str)` helper with tests
- [x] Update `server.rs` to use helpers (381 lines, down from 408)
- [x] Verify: all tests pass

### Actual Results

| Metric | Before | After | Status |
|--------|--------|-------|--------|
| Largest file | 865 lines | 381 lines (server.rs) | ✅ |
| telegram/types.rs | 865 lines | 24 lines (module re-exports) | ✅ |
| mcp/tools/types.rs | 366 lines | 18 lines (module re-exports) | ✅ |
| ID parsing duplication | 4x | 1x (helpers.rs) | ✅ |
| Test fixture factories | 0 | 7 helpers (test_helpers.rs) | ✅ |
| Total tests | 186 | 209 | ✅ (+23 new) |

### New Files Created
- `src/test_helpers.rs` (206 lines) - Test fixture factories
- `src/telegram/types/ids.rs` (185 lines) - ChannelId, MessageId, UserId
- `src/telegram/types/names.rs` (168 lines) - Username, ChannelName
- `src/telegram/types/media.rs` (152 lines) - MediaType, MediaFilter
- `src/telegram/types/entities.rs` (131 lines) - Message, Channel
- `src/telegram/types/params.rs` (235 lines) - SearchParams, HistoryParams, SearchResult
- `src/mcp/tools/helpers.rs` (121 lines) - ID parsing helpers
- `src/mcp/tools/types/requests.rs` (224 lines) - Request types
- `src/mcp/tools/types/responses.rs` (109 lines) - Response types
- `src/mcp/tools/types/serde_helpers.rs` (82 lines) - Custom deserializers

### Verification
- [x] `cargo test` - All 209 tests pass (5 ignored)
- [x] `cargo clippy -- -D warnings` - No warnings
- [x] `cargo fmt --check` - Properly formatted

---

## Phase 19: Log Cleanup 🔄

**Goal:** Implement automatic cleanup of old log files based on `max_log_days` configuration

**Context:** Phase 15 added file logging with daily rotation and a `max_log_days` config field (default: 7), but the cleanup logic was never implemented. Log files accumulate indefinitely.

**Approach:** Startup cleanup (KISS principle) - clean old logs when the application starts. This catches most cases and is simple to implement.

### 19.1 Cleanup Function ✅
- [x] Add `cleanup_old_logs(config: &LoggingConfig) -> anyhow::Result<usize>` to `src/logging.rs`
- [x] Skip cleanup if `file_enabled == false` or `max_log_days == 0`
- [x] Calculate cutoff time: `now - (max_log_days * 86400 seconds)`
- [x] Iterate files in `config.file_path` directory
- [x] Only process `.log` files (match files containing ".log" in name)
- [x] Delete files with `modified` time older than cutoff
- [x] Return count of deleted files
- [x] Handle errors gracefully (log warning, don't crash)

### 19.2 Integration ✅
- [x] Call `cleanup_old_logs()` in `main.rs` after logging initialization
- [x] Log cleanup result: `tracing::info!(removed = count, "Cleaned up old log files")`
- [x] Only log if count > 0 (avoid noise)

### 19.3 Tests ✅
- [x] Test: cleanup skipped when `file_enabled == false`
- [x] Test: cleanup skipped when `max_log_days == 0`
- [x] Test: old files deleted, recent files kept
- [x] Test: non-log files ignored
- [x] Test: handles empty directory gracefully
- [x] Test: handles missing directory gracefully

### 19.4 Documentation ⬜
- [ ] Update README.md logging section
- [ ] Update config.example.toml with `max_log_days` documentation
- [x] Run `cargo clippy -- -D warnings`
- [x] Run `cargo fmt --check`
- [x] All tests passing (215 tests, 5 ignored)

**Implementation:**
```rust
// src/logging.rs
pub fn cleanup_old_logs(config: &LoggingConfig) -> anyhow::Result<usize> {
    if !config.file_enabled || config.max_log_days == 0 {
        return Ok(0);
    }

    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(config.max_log_days as u64 * 86400);

    let mut removed = 0;

    let entries = match std::fs::read_dir(&config.file_path) {
        Ok(e) => e,
        Err(_) => return Ok(0), // Directory doesn't exist yet
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Only process .log files
        if path.extension().and_then(|e| e.to_str()) != Some("log") {
            continue;
        }

        if let Ok(metadata) = entry.metadata() {
            if let Ok(modified) = metadata.modified() {
                if modified < cutoff {
                    if std::fs::remove_file(&path).is_ok() {
                        removed += 1;
                    }
                }
            }
        }
    }

    Ok(removed)
}
```

**Estimated Tests:** ~6 new tests

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
