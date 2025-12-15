# Technical Vision: Telegram MCP Connector

**Version:** 1.0.0
**Status:** Design Document
**Methodology:** TDD + KISS

---

## Table of Contents

1. [Technologies](#1-technologies)
2. [Development Principles](#2-development-principles)
3. [Project Structure](#3-project-structure)
4. [Project Architecture](#4-project-architecture)
5. [Data Model](#5-data-model)
6. [Workflows](#6-workflows)
7. [Configuration Approach](#7-configuration-approach)
8. [Logging Approach](#8-logging-approach)

---

## 1. Technologies

### 1.1 Architecture Pattern

**Library + Binary separation:**

```
telegram-connector/
├── src/
│   ├── lib.rs          # Library crate (core logic, reusable)
│   └── main.rs         # Binary crate (terminal interaction)
```

**Benefits:**
- Core logic is reusable and independently testable
- Terminal/CLI concerns isolated from business logic
- Clean dependency injection for testing

---

### 1.2 Core Dependencies

| Layer | Crate | Version | Purpose |
|-------|-------|---------|---------|
| **Language** | Rust | 2024 Edition | Memory safety, async-first |
| **Async Runtime** | `tokio` | latest, `full` | Async runtime, required by grammers |
| **MCP Protocol** | `rmcp` | 0.8+ | Official MCP server SDK |
| **Telegram Client** | `grammers` | git (main) | MTProto implementation |
| **Config Parsing** | `toml` | 0.8 | TOML file parsing |
| **Serialization** | `serde` | 1.0, `derive` | Struct serialization |
| **JSON** | `serde_json` | 1.0 | MCP JSON-RPC compliance |
| **Logging** | `tracing` | 0.1 | Structured async logging |
| **Log Subscriber** | `tracing-subscriber` | 0.3 | Log output formatting |
| **Errors (types)** | `thiserror` | 1.0 | Custom error definitions |
| **Errors (propagation)** | `anyhow` | 1.0 | Error context & propagation |
| **Date/Time** | `chrono` | 0.4, `serde` | Time-window search filtering |
| **Concurrency** | `dashmap` | 5.5 | Thread-safe rate limiter state |
| **Config Paths** | `directories` | 5.0 | XDG-compliant paths |

---

### 1.3 Testing Dependencies

| Crate | Purpose |
|-------|---------|
| `tokio-test` | Async test utilities |
| `proptest` | Property-based testing for invariants |
| `mockall` | Mocking traits for isolated unit tests |

---

### 1.4 Cargo.toml

```toml
[package]
name = "telegram-connector"
version = "0.1.0"
edition = "2024"

[lib]
name = "telegram_connector"
path = "src/lib.rs"

[[bin]]
name = "telegram-mcp"
path = "src/main.rs"

[dependencies]
# MCP
rmcp = { version = "0.8", features = ["server"] }

# Telegram
grammers-client = { git = "https://github.com/Lonami/grammers", branch = "main" }
grammers-session = { git = "https://github.com/Lonami/grammers", branch = "main" }

# Async
tokio = { version = "1", features = ["full"] }

# Config
toml = "0.8"
serde = { version = "1.0", features = ["derive"] }
directories = "5.0"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }

# Errors
anyhow = "1.0"
thiserror = "1.0"

# Utilities
chrono = { version = "0.4", features = ["serde"] }
serde_json = "1.0"
dashmap = "5.5"

[dev-dependencies]
tokio-test = "0.4"
proptest = "1.4"
mockall = "0.13"
```

---

### 1.5 Build Requirements

- **Rust Toolchain:** stable (2024 edition support)
- **Platform:** macOS (primary), Linux (secondary)
- **External:** Telegram Desktop installed for `tg://` links
- **Credentials:** Telegram API ID & Hash from https://my.telegram.org

---

## 2. Development Principles

### 2.1 TDD Workflow

```
┌─────────────────────────────────────────────────────┐
│  RED → GREEN → REFACTOR                             │
│                                                     │
│  1. Write failing test (RED)                        │
│  2. Write minimal code to pass (GREEN)              │
│  3. Refactor while keeping tests green (REFACTOR)   │
└─────────────────────────────────────────────────────┘
```

**Rules:**
- No production code without a failing test first
- Tests define the API contract
- Each test covers one behavior
- Tests are first-class code (same quality standards)

---

### 2.2 KISS Guidelines

| Principle | Application |
|-----------|-------------|
| **Single Responsibility** | One module = one purpose |
| **No Premature Abstraction** | Extract only when duplication is proven (Rule of Three) |
| **Explicit over Implicit** | Clear function signatures, no magic |
| **Minimal Dependencies** | Each crate must justify its inclusion |
| **Flat over Nested** | Prefer flat module structure where possible |
| **Delete over Comment** | Remove dead code, don't comment it out |

---

### 2.3 Code Style

| Tool | Configuration | Enforcement |
|------|---------------|-------------|
| `rustfmt` | Default settings | Pre-commit hook |
| `clippy` | Warnings as errors | Pre-commit hook |
| `cargo test` | All tests must pass | Pre-commit hook |

**Conventions:**
- Naming: Rust standard (snake_case functions, PascalCase types)
- Documentation: Doc comments (`///`) on public API only
- Line length: 100 characters (rustfmt default)

---

### 2.4 Testing Strategy

| Test Type | Location | Purpose | Tools |
|-----------|----------|---------|-------|
| **Unit** | `#[cfg(test)]` in modules | Test individual functions | `#[test]`, `mockall` |
| **Integration** | `tests/` directory | Test module interactions | `tokio::test` |
| **Property-based** | Within unit/integration | Verify invariants | `proptest` |

**Coverage Target:** 80%+ code coverage

**Mocking Strategy:**
```rust
// Define trait for external dependency
#[cfg_attr(test, mockall::automock)]
trait TelegramClient {
    async fn search(&self, query: &str) -> Result<Vec<Message>>;
}

// Production: real implementation
// Tests: MockTelegramClient generated by mockall
```

---

### 2.5 CI/CD Pipeline

**GitHub Actions** on every push/PR:

```yaml
jobs:
  check:
    - cargo fmt --check      # Formatting
    - cargo clippy           # Linting
    - cargo test             # All tests
    - cargo tarpaulin        # Coverage (80%+ required)
```

**Checks must pass before merge.**

---

### 2.6 Pre-commit Hooks

**Blocking hooks** (commit rejected on failure):

```bash
#!/bin/sh
# .git/hooks/pre-commit

set -e

echo "Running rustfmt..."
cargo fmt --check

echo "Running clippy..."
cargo clippy -- -D warnings

echo "Running tests..."
cargo test

echo "All checks passed!"
```

**Setup:** Hooks installed via project setup script.

---

### 2.7 Git Workflow

| Aspect | Convention |
|--------|------------|
| **Commits** | Small, atomic, one logical change |
| **Messages** | Imperative mood: "Add feature", not "Added feature" |
| **Branch** | `main` always working |
| **Merge** | All CI checks must pass |

**Commit message format:**
```
<type>: <short description>

<optional body>
```

Types: `feat`, `fix`, `test`, `refactor`, `docs`, `chore`

---

## 3. Project Structure

### 3.1 Directory Layout

```
telegram-connector/
├── Cargo.toml                    # Package manifest
├── Cargo.lock                    # Dependency lock (committed)
├── README.md                     # Quick start guide
│
├── src/
│   ├── lib.rs                    # Library root, public API exports
│   ├── main.rs                   # Binary: CLI entry point
│   │
│   ├── config.rs                 # Configuration loading & validation
│   ├── error.rs                  # Custom error types (thiserror)
│   ├── logging.rs                # Tracing subscriber setup
│   ├── rate_limiter.rs           # Token bucket implementation
│   ├── link.rs                   # Deep link generation utility
│   │
│   ├── mcp.rs                    # MCP module declaration
│   ├── mcp/
│   │   ├── server.rs             # rmcp server setup & initialization
│   │   └── tools.rs              # All 6 MCP tools (split when >300 lines)
│   │
│   ├── telegram.rs               # Telegram module declaration
│   └── telegram/
│       ├── client.rs             # Grammers client wrapper
│       ├── auth.rs               # Authentication & 2FA flow
│       └── types.rs              # Message, Channel, SearchResult
│
├── tests/
│   ├── mcp_integration.rs        # MCP server integration tests
│   └── telegram_mock.rs          # Telegram client mock tests
│
└── .github/
    └── workflows/
        └── ci.yml                # CI/CD pipeline
```

---

### 3.2 Module System (Rust 2018+ Style)

**No `mod.rs` files.** Use file-as-module pattern:

```rust
// src/lib.rs - Library root
pub mod config;
pub mod error;
pub mod link;
pub mod logging;
pub mod mcp;
pub mod rate_limiter;
pub mod telegram;

// Re-exports for convenient access
pub use config::Config;
pub use error::Error;
```

```rust
// src/mcp.rs - Module declaration
pub mod server;
pub mod tools;

pub use server::McpServer;
```

```rust
// src/telegram.rs - Module declaration
pub mod auth;
pub mod client;
pub mod types;

pub use client::TelegramClient;
pub use types::{Channel, Message, SearchResult};
```

---

### 3.3 File Responsibilities

| File | Responsibility | Lines (estimate) |
|------|----------------|------------------|
| `lib.rs` | Public API exports, module declarations | ~30 |
| `main.rs` | CLI entry, server startup, signal handling | ~50 |
| `config.rs` | TOML loading, defaults, validation | ~100 |
| `error.rs` | Error enum with thiserror | ~80 |
| `logging.rs` | Tracing subscriber initialization | ~40 |
| `rate_limiter.rs` | Token bucket algorithm | ~100 |
| `link.rs` | `tg://` and `https://t.me` link generation | ~60 |
| `mcp/server.rs` | rmcp server setup, tool registration | ~80 |
| `mcp/tools.rs` | 6 MCP tool implementations | ~250 |
| `telegram/client.rs` | Grammers wrapper, search, channel ops | ~200 |
| `telegram/auth.rs` | Phone auth, 2FA, session persistence | ~150 |
| `telegram/types.rs` | Data structures with serde | ~100 |

**Total estimated:** ~1,200 lines (excluding tests)

---

### 3.4 Scaling Strategy

When a file exceeds ~300 lines:

1. **`mcp/tools.rs`** → Split into individual tool files:
   ```
   mcp/
   ├── server.rs
   ├── tools.rs              # Re-exports only
   └── tools/
       ├── search_messages.rs
       ├── get_channel_info.rs
       └── ...
   ```

2. **`telegram/client.rs`** → Extract to:
   ```
   telegram/
   ├── client.rs             # Core client
   ├── search.rs             # Search operations
   └── channel.rs            # Channel operations
   ```

**Rule:** Split only when complexity demands it, not preemptively.

---

## 4. Project Architecture

### 4.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Comet Browser + Claude                   │
└──────────────────────────┬──────────────────────────────────┘
                           │ JSON-RPC over stdio
┌──────────────────────────▼──────────────────────────────────┐
│                     MCP Server Layer                        │
│                    (rmcp + tools.rs)                        │
└──────────────────────────┬──────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────┐
│                   Application Layer                         │
│         (rate_limiter, link, config, logging)              │
└──────────────────────────┬──────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────┐
│                    Telegram Layer                           │
│              (client, auth, types - grammers)              │
└──────────────────────────┬──────────────────────────────────┘
                           │ MTProto
                           ▼
                   Telegram Cloud API
```

---

### 4.2 Layer Responsibilities

| Layer | Components | Responsibility |
|-------|------------|----------------|
| **MCP** | `server.rs`, `tools.rs` | JSON-RPC handling, tool dispatch, response formatting |
| **Application** | `rate_limiter`, `link`, `config`, `logging` | Cross-cutting concerns, utilities |
| **Telegram** | `client`, `auth`, `types` | Telegram API abstraction, session management |

---

### 4.3 Dependency Flow

```
main.rs
   │
   ├──► Config (loaded first)
   │
   ├──► Logging (initialized)
   │
   ├──► TelegramClient (Arc<T>)
   │         │
   │         ▼
   ├──► RateLimiter (Arc<T>)
   │
   └──► McpServer
            │
            ├──► Arc<TelegramClient>
            └──► Arc<RateLimiter>
```

**Rule:** Dependencies flow downward. Lower layers don't know about upper layers.

---

### 4.4 State Management

**Shared state via `Arc`:**

```rust
// main.rs - Application bootstrap
#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    logging::init(&config.logging)?;

    let telegram_client = Arc::new(TelegramClient::new(&config.telegram).await?);
    let rate_limiter = Arc::new(RateLimiter::new(&config.rate_limiting));

    let server = McpServer::new(
        Arc::clone(&telegram_client),
        Arc::clone(&rate_limiter),
    );

    server.run_stdio().await
}
```

**Why `Arc<T>`:**
- Multiple MCP tools share the same Telegram session
- Rate limiter state shared across concurrent requests
- Clean ownership semantics
- Easy to test with mock implementations

---

### 4.5 Trait-Based Abstraction

For testability, external dependencies are behind traits:

```rust
// telegram/client.rs
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait TelegramClientTrait: Send + Sync {
    async fn search_messages(&self, params: &SearchParams) -> Result<SearchResult>;
    async fn get_channel_info(&self, identifier: &str) -> Result<Channel>;
    async fn get_subscribed_channels(&self, limit: u32, offset: u32) -> Result<Vec<Channel>>;
    async fn is_connected(&self) -> bool;
}

// Real implementation
pub struct TelegramClient { /* grammers client */ }

impl TelegramClientTrait for TelegramClient {
    // ... real implementation
}
```

```rust
// rate_limiter.rs
#[cfg_attr(test, mockall::automock)]
pub trait RateLimiterTrait: Send + Sync {
    async fn acquire(&self, tokens: u32) -> Result<()>;
    fn available_tokens(&self) -> f64;
}
```

---

### 4.6 Error Propagation

```
Tool Error → Application Error → MCP Error Response
     │              │                    │
     ▼              ▼                    ▼
TelegramError → anyhow::Error → JSON-RPC error object
```

**Pattern:**
- Use `thiserror` for typed errors in library
- Use `anyhow` for error context in application
- Convert to MCP error codes at boundary

---

### 4.7 Concurrency Model

| Component | Concurrency | Mechanism |
|-----------|-------------|-----------|
| MCP Server | Single connection, sequential requests | Tokio runtime |
| Telegram Client | Single session, async operations | `Arc<Client>` + async |
| Rate Limiter | Thread-safe state | `DashMap` / `AtomicU64` |

**Note:** MCP over stdio is inherently single-client, but we design for thread-safety to support future HTTP transport.

---

## 5. Data Model

### 5.1 Domain-Driven Design (DDD) Principles

The data model follows DDD principles:

| Principle | Application |
|-----------|-------------|
| **Ubiquitous Language** | Types match Telegram domain terminology |
| **Value Objects** | IDs, timestamps as immutable wrappers |
| **Entities** | `Message`, `Channel` with unique identity |
| **Aggregates** | `SearchResult` as aggregate root |
| **Bounded Context** | Clear separation between Telegram and MCP domains |

---

### 5.2 Type-Safe ID Wrappers

```rust
// telegram/types.rs - Value Objects

/// Telegram channel ID (numeric)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelId(pub i64);

impl ChannelId {
    pub fn new(id: i64) -> Self {
        Self(id)
    }
}

impl fmt::Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Telegram message ID (within channel)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(pub i64);

/// Telegram user ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(pub i64);
```

**Benefits:**
- Compile-time prevention of ID type mixups
- Self-documenting code
- Easy to add validation in constructors

---

### 5.3 Core Entities

```rust
// telegram/types.rs

/// Message entity from Telegram
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub channel_id: ChannelId,
    pub channel_name: String,
    pub channel_username: String,
    pub text: String,
    pub timestamp: DateTime<Utc>,
    pub sender_id: Option<UserId>,
    pub sender_name: Option<String>,
    pub has_media: bool,
    pub media_type: MediaType,
}

/// Channel entity from Telegram
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub name: String,
    pub username: String,
    pub description: Option<String>,
    pub member_count: u64,
    pub is_verified: bool,
    pub is_public: bool,
    pub is_subscribed: bool,
    pub last_message_date: Option<DateTime<Utc>>,
}

/// Media type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaType {
    None,
    Photo,
    Video,
    Document,
    Audio,
    Animation,
}

impl Default for MediaType {
    fn default() -> Self {
        Self::None
    }
}
```

---

### 5.4 Request/Response Types

```rust
// telegram/types.rs

/// Search parameters (input)
#[derive(Debug, Clone)]
pub struct SearchParams {
    pub query: String,
    pub channel_id: Option<ChannelId>,
    pub hours_back: u32,
    pub limit: u32,
}

impl SearchParams {
    pub const DEFAULT_HOURS_BACK: u32 = 48;
    pub const MAX_HOURS_BACK: u32 = 72;
    pub const DEFAULT_LIMIT: u32 = 20;
    pub const MAX_LIMIT: u32 = 100;
}

impl Default for SearchParams {
    fn default() -> Self {
        Self {
            query: String::new(),
            channel_id: None,
            hours_back: Self::DEFAULT_HOURS_BACK,
            limit: Self::DEFAULT_LIMIT,
        }
    }
}

/// Search result aggregate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub messages: Vec<Message>,
    pub total_found: u64,
    pub search_time_ms: u64,
    pub query_metadata: QueryMetadata,
}

/// Query metadata for response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryMetadata {
    pub query: String,
    pub hours_back: u32,
    pub channels_searched: u32,
}
```

---

### 5.5 Link Types

```rust
// link.rs

/// Generated message link
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageLink {
    pub channel_id: ChannelId,
    pub message_id: MessageId,
    pub https_link: String,
    pub tg_protocol_link: String,
}

impl MessageLink {
    pub fn new(channel_id: ChannelId, message_id: MessageId) -> Self {
        let https_link = format!(
            "https://t.me/c/{}/{}?single",
            channel_id, message_id
        );
        let tg_protocol_link = format!(
            "tg://resolve?channel={}&post={}&single",
            channel_id, message_id
        );
        Self {
            channel_id,
            message_id,
            https_link,
            tg_protocol_link,
        }
    }
}
```

---

### 5.6 Configuration Types

```rust
// config.rs

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub telegram: TelegramConfig,
    pub search: SearchConfig,
    pub rate_limiting: RateLimitConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramConfig {
    pub api_id: i32,
    pub api_hash: String,
    pub phone_number: String,
    pub session_file: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    pub default_hours_back: u32,
    pub max_results_default: u32,
    pub max_results_limit: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    pub max_tokens: u32,
    pub refill_rate: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
}
```

---

### 5.7 Serialization Rules

| Type | JSON Representation |
|------|---------------------|
| `ChannelId`, `MessageId`, `UserId` | Number (transparent) |
| `DateTime<Utc>` | ISO 8601 string: `"2025-12-14T16:30:00Z"` |
| `Option<T>` | `null` or value (skipped if None in some contexts) |
| `MediaType` | Lowercase string: `"photo"`, `"video"`, etc. |

---

## 6. Workflows

### 6.1 Application Startup

```
┌─────────────────────────────────────────────────────────────┐
│ 1. Load Configuration                                       │
│    └─► Read ~/.config/telegram-connector/config.toml        │
│    └─► Validate required fields                             │
│    └─► Apply defaults for optional fields                   │
├─────────────────────────────────────────────────────────────┤
│ 2. Initialize Logging                                       │
│    └─► Setup tracing subscriber with configured level       │
├─────────────────────────────────────────────────────────────┤
│ 3. Connect to Telegram                                      │
│    └─► Load existing session OR run first-time auth flow    │
│    └─► Verify connection is active                          │
├─────────────────────────────────────────────────────────────┤
│ 4. Initialize Rate Limiter                                  │
│    └─► Create token bucket with configured parameters       │
├─────────────────────────────────────────────────────────────┤
│ 5. Start MCP Server                                         │
│    └─► Register all 6 tools                                 │
│    └─► Setup signal handlers (SIGTERM, SIGINT)              │
│    └─► Begin listening on stdio                             │
└─────────────────────────────────────────────────────────────┘
```

---

### 6.2 First-Run Authentication Flow

```
                    Binary started
                          │
                          ▼
              ┌───────────────────────┐
              │ Session file exists?  │
              └───────────────────────┘
                    │           │
                   Yes         No
                    │           │
                    ▼           ▼
             Load session   Prompt: "Enter phone number"
                    │           │
                    ▼           ▼
           Session valid?   Connect to Telegram
                    │           │
              ┌─────┴─────┐     ▼
             Yes         No   Send auth code
              │           │     │
              │           └─────┤
              │                 ▼
              │         Prompt: "Enter code from Telegram"
              │                 │
              │                 ▼
              │         ┌───────────────────┐
              │         │ 2FA password      │
              │         │ required?         │
              │         └───────────────────┘
              │               │         │
              │              Yes       No
              │               │         │
              │               ▼         │
              │         Prompt: password│
              │               │         │
              │               └────┬────┘
              │                    │
              │                    ▼
              │            Save session to file
              │                    │
              └────────────────────┤
                                   ▼
                          Ready to serve MCP
```

---

### 6.3 Graceful Shutdown Flow

```
┌─────────────────────────────────────────────────────────────┐
│                   Signal received                           │
│                 (SIGTERM or SIGINT)                         │
└──────────────────────────┬──────────────────────────────────┘
                           │
                           ▼
              ┌────────────────────────┐
              │ Log shutdown initiated │
              └────────────────────────┘
                           │
                           ▼
              ┌────────────────────────┐
              │ Stop accepting new     │
              │ MCP requests           │
              └────────────────────────┘
                           │
                           ▼
              ┌────────────────────────┐
              │ Wait for in-flight     │
              │ requests (with timeout)│
              └────────────────────────┘
                           │
                           ▼
              ┌────────────────────────┐
              │ Disconnect Telegram    │
              │ session cleanly        │
              └────────────────────────┘
                           │
                           ▼
              ┌────────────────────────┐
              │ Log shutdown complete  │
              │ Exit with code 0       │
              └────────────────────────┘
```

**Implementation:**
```rust
// main.rs
use tokio::signal;

async fn shutdown_signal() {
    let ctrl_c = signal::ctrl_c();
    let terminate = signal::unix::signal(signal::unix::SignalKind::terminate())
        .expect("failed to install SIGTERM handler");

    tokio::select! {
        _ = ctrl_c => { tracing::info!("Received SIGINT"); }
        _ = terminate.recv() => { tracing::info!("Received SIGTERM"); }
    }
}
```

---

### 6.4 MCP Tool Request Flow

```
         Comet sends JSON-RPC request
                      │
                      ▼
            ┌─────────────────────┐
            │ Parse JSON-RPC      │
            │ Validate structure  │
            └─────────────────────┘
                      │
                      ▼
            ┌─────────────────────┐
            │ Route to tool       │
            │ by name             │
            └─────────────────────┘
                      │
                      ▼
            ┌─────────────────────┐
            │ Validate tool       │
            │ arguments           │
            └─────────────────────┘
                      │
                      ▼
            ┌─────────────────────┐
            │ Rate limiter:       │
            │ acquire token       │
            └─────────────────────┘
                      │
               ┌──────┴──────┐
               │             │
            Allowed     Rate limited
               │             │
               ▼             ▼
          Execute       Return error:
           tool         { "code": -32000,
               │          "data": { "retry_after_seconds": N } }
               ▼
      Return JSON-RPC response
```

---

### 6.5 Search Messages Flow

```
search_messages(query, channel_id?, hours_back, limit)
                      │
                      ▼
         ┌────────────────────────┐
         │ Validate parameters   │
         │ - query not empty     │
         │ - hours_back ≤ 72     │
         │ - limit ≤ 100         │
         └────────────────────────┘
                      │
                      ▼
         ┌────────────────────────┐
         │ channel_id specified? │
         └────────────────────────┘
              │             │
             Yes           No
              │             │
              ▼             ▼
      Search single    Get all subscribed
        channel          channels
              │             │
              │             ▼
              │      For each channel:
              │      ├─► Search with query
              │      ├─► Apply time filter
              │      └─► Collect results
              │          (continue on error)
              │             │
              └──────┬──────┘
                     │
                     ▼
         ┌────────────────────────┐
         │ Sort by timestamp     │
         │ (newest first)        │
         └────────────────────────┘
                     │
                     ▼
         ┌────────────────────────┐
         │ Apply limit           │
         └────────────────────────┘
                     │
                     ▼
         ┌────────────────────────┐
         │ Generate message      │
         │ links for each result │
         └────────────────────────┘
                     │
                     ▼
         Return SearchResult
```

---

### 6.6 Session Expiration Handling

```
         Any Telegram API call
                   │
                   ▼
         ┌─────────────────────┐
         │ Session expired?    │
         └─────────────────────┘
              │           │
             Yes         No
              │           │
              ▼           ▼
    Return SessionExpired   Continue
    error to MCP client    operation
              │
              ▼
    User must manually
    re-authenticate:
    1. Delete session file
    2. Restart service
    3. Complete 2FA flow
```

**Error response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32001,
    "message": "Session expired. Please re-authenticate.",
    "data": {
      "error_type": "SessionExpired",
      "action_required": "Restart service to re-authenticate"
    }
  }
}
```

---

## 7. Configuration Approach

### 7.1 Configuration File Location

**Primary path:** `~/.config/telegram-connector/config.toml`

**Path resolution order:**
1. `TELEGRAM_MCP_CONFIG` environment variable (if set)
2. XDG config directory via `directories` crate
3. Fallback: `~/.config/telegram-connector/config.toml`

---

### 7.2 Configuration File Format

```toml
# Telegram MCP Connector Configuration
# Location: ~/.config/telegram-connector/config.toml

[telegram]
# Required: Telegram API credentials from https://my.telegram.org
api_id = 12345678
api_hash = "${TELEGRAM_API_HASH}"       # Environment variable reference
phone_number = "+1234567890"

# Optional: Session file location
session_file = "~/.config/telegram-connector/session.bin"

[search]
# Optional: Search defaults
default_hours_back = 48                  # Default: 48
max_results_default = 20                 # Default: 20
max_results_limit = 100                  # Default: 100

[rate_limiting]
# Optional: Token bucket configuration
max_tokens = 50                          # Default: 50 (burst capacity)
refill_rate = 2.0                        # Default: 2.0 tokens/second

[logging]
# Optional: Logging configuration
level = "info"                           # Default: "info"
format = "compact"                       # Default: "compact"
```

---

### 7.3 Environment Variable Support

Sensitive fields support `${VAR_NAME}` syntax for environment variable expansion:

```toml
[telegram]
api_id = "${TELEGRAM_API_ID}"
api_hash = "${TELEGRAM_API_HASH}"
phone_number = "${TELEGRAM_PHONE}"
```

**Implementation:**
```rust
fn expand_env_vars(value: &str) -> Result<String> {
    // Pattern: ${VAR_NAME}
    let re = Regex::new(r"\$\{([^}]+)\}")?;
    let result = re.replace_all(value, |caps: &Captures| {
        std::env::var(&caps[1]).unwrap_or_default()
    });
    Ok(result.to_string())
}
```

**Supported fields:**
- `telegram.api_id`
- `telegram.api_hash`
- `telegram.phone_number`
- `telegram.session_file`

---

### 7.4 Required vs Optional Fields

| Section | Field | Required | Default Value |
|---------|-------|----------|---------------|
| `telegram` | `api_id` | **Yes** | - |
| `telegram` | `api_hash` | **Yes** | - |
| `telegram` | `phone_number` | **Yes** | - |
| `telegram` | `session_file` | No | `~/.config/telegram-connector/session.bin` |
| `search` | `default_hours_back` | No | `48` |
| `search` | `max_results_default` | No | `20` |
| `search` | `max_results_limit` | No | `100` |
| `rate_limiting` | `max_tokens` | No | `50` |
| `rate_limiting` | `refill_rate` | No | `2.0` |
| `logging` | `level` | No | `"info"` |
| `logging` | `format` | No | `"compact"` |

---

### 7.5 Logging Format Options

| Format | Description | Use Case |
|--------|-------------|----------|
| `compact` | Single-line, human-readable | Development, debugging |
| `pretty` | Multi-line, colored output | Interactive terminal use |
| `json` | Structured JSON lines | Production, log aggregation |

**Examples:**

```
# compact
2025-12-14T16:30:00Z INFO telegram_connector::mcp: Search completed query="AI" results=15

# pretty
  2025-12-14T16:30:00Z
  INFO telegram_connector::mcp
    Search completed
    query: "AI"
    results: 15

# json
{"timestamp":"2025-12-14T16:30:00Z","level":"INFO","target":"telegram_connector::mcp","message":"Search completed","query":"AI","results":15}
```

---

### 7.6 Log Levels

| Level | Description |
|-------|-------------|
| `trace` | Very detailed debugging (all internal operations) |
| `debug` | Debugging information (request/response details) |
| `info` | Normal operation events (startup, search completed) |
| `warn` | Warning conditions (rate limited, retries) |
| `error` | Error conditions (API failures, auth errors) |

---

### 7.7 Configuration Loading Implementation

```rust
// config.rs

impl Config {
    pub fn load() -> Result<Self> {
        let path = Self::resolve_config_path()?;
        let content = std::fs::read_to_string(&path)
            .context(format!("Failed to read config: {}", path.display()))?;

        let mut config: Config = toml::from_str(&content)
            .context("Failed to parse config.toml")?;

        // Expand environment variables in sensitive fields
        config.telegram.api_hash = expand_env_vars(&config.telegram.api_hash)?;
        config.telegram.phone_number = expand_env_vars(&config.telegram.phone_number)?;

        // Apply defaults for optional sections
        config.apply_defaults();

        // Validate required fields
        config.validate()?;

        Ok(config)
    }

    fn resolve_config_path() -> Result<PathBuf> {
        // 1. Check environment variable
        if let Ok(path) = std::env::var("TELEGRAM_MCP_CONFIG") {
            return Ok(PathBuf::from(path));
        }

        // 2. Use XDG config directory
        let dirs = directories::ProjectDirs::from("", "", "telegram-connector")
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

        Ok(dirs.config_dir().join("config.toml"))
    }

    fn validate(&self) -> Result<()> {
        if self.telegram.api_id == 0 {
            anyhow::bail!("telegram.api_id is required");
        }
        if self.telegram.api_hash.is_empty() {
            anyhow::bail!("telegram.api_hash is required");
        }
        if self.telegram.phone_number.is_empty() {
            anyhow::bail!("telegram.phone_number is required");
        }
        Ok(())
    }
}
```

---

### 7.8 First-Run Configuration

If config file doesn't exist, display helpful message:

```
Configuration file not found at: ~/.config/telegram-connector/config.toml

Please create the file with the following content:

[telegram]
api_id = YOUR_API_ID           # From https://my.telegram.org
api_hash = "YOUR_API_HASH"     # From https://my.telegram.org
phone_number = "+1234567890"   # Your Telegram phone number

For more information, see: https://github.com/your-repo/telegram-connector#setup
```

---

## 8. Logging Approach

### 8.1 Logging Stack

| Component | Purpose |
|-----------|---------|
| `tracing` | Structured, async-aware logging API |
| `tracing-subscriber` | Log formatting and filtering |
| `tracing-appender` | File output with rotation |

**Additional dependency:**
```toml
tracing-appender = "0.2"
```

---

### 8.2 Output Destinations

| Destination | Purpose | When Used |
|-------------|---------|-----------|
| **stderr** | Real-time logs | Always (MCP uses stdout) |
| **File** | Persistent logs with rotation | Always |

**File location:** `~/.config/telegram-connector/logs/`

---

### 8.3 Log File Rotation

**Strategy:** Size-based rotation with maximum file count

| Parameter | Value |
|-----------|-------|
| Max file size | 10 MB |
| Max files | 5 |
| Naming | `telegram-connector.log`, `telegram-connector.log.1`, etc. |

**Behavior:**
- When `telegram-connector.log` reaches 10 MB, rotate to `.log.1`
- Keep maximum 5 rotated files
- Oldest file (`.log.5`) is deleted when new rotation occurs

---

### 8.4 Configuration Extension

```toml
[logging]
level = "info"
format = "compact"

# File logging
file_enabled = true                                    # Default: true
file_path = "~/.config/telegram-connector/logs/"       # Default
max_file_size_mb = 10                                  # Default: 10
max_files = 5                                          # Default: 5
```

**Updated LoggingConfig:**
```rust
#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
    #[serde(default = "default_true")]
    pub file_enabled: bool,
    #[serde(default = "default_log_path")]
    pub file_path: PathBuf,
    #[serde(default = "default_max_size")]
    pub max_file_size_mb: u64,
    #[serde(default = "default_max_files")]
    pub max_files: u32,
}

fn default_true() -> bool { true }
fn default_max_size() -> u64 { 10 }
fn default_max_files() -> u32 { 5 }
```

---

### 8.5 Sensitive Data Redaction

**Never log these values in plain text:**
- Phone numbers
- API hashes
- 2FA passwords
- Session tokens

**Redaction patterns:**

| Data Type | Original | Redacted |
|-----------|----------|----------|
| Phone number | `+1234567890` | `+1234***890` |
| API hash | `abc123def456` | `abc1***6` |
| Session token | `token_xyz_123` | `[REDACTED]` |

**Implementation:**
```rust
// logging.rs

pub fn redact_phone(phone: &str) -> String {
    if phone.len() <= 6 {
        return "[REDACTED]".to_string();
    }
    let visible_start = 4;  // +123
    let visible_end = 3;    // 890
    format!(
        "{}***{}",
        &phone[..visible_start],
        &phone[phone.len() - visible_end..]
    )
}

pub fn redact_hash(hash: &str) -> String {
    if hash.len() <= 6 {
        return "[REDACTED]".to_string();
    }
    format!("{}***{}", &hash[..4], &hash[hash.len() - 1..])
}

// Use in structured logging
tracing::info!(
    phone = %redact_phone(&config.phone_number),
    "Authenticating user"
);
```

---

### 8.6 What to Log

| Event | Level | Fields |
|-------|-------|--------|
| **Startup** | `info` | version, config_path, log_file_path |
| **Config loaded** | `debug` | search_defaults, rate_limit_config |
| **Telegram connecting** | `info` | phone (redacted) |
| **Telegram connected** | `info` | user_id |
| **Session loaded** | `debug` | session_age_hours |
| **Tool called** | `debug` | tool_name, request_id |
| **Search started** | `debug` | query, channels_count, hours_back |
| **Search completed** | `info` | query, results_count, duration_ms |
| **Rate limited** | `warn` | tokens_available, retry_after_seconds |
| **API error** | `error` | error_type, error_code, message |
| **Session expired** | `error` | action_required |
| **Shutdown initiated** | `info` | signal_received |
| **Shutdown complete** | `info` | uptime_seconds |

---

### 8.7 Implementation

```rust
// logging.rs

use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub fn init(config: &LoggingConfig) -> Result<()> {
    // Build filter
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.level));

    // Create layers
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr);

    // Apply format to stderr
    let stderr_layer = match config.format.as_str() {
        "json" => stderr_layer.json().boxed(),
        "pretty" => stderr_layer.pretty().boxed(),
        _ => stderr_layer.compact().boxed(),
    };

    // File appender with rotation
    let file_appender = if config.file_enabled {
        let appender = RollingFileAppender::builder()
            .rotation(Rotation::NEVER)  // We handle size-based rotation manually
            .max_log_files(config.max_files as usize)
            .filename_prefix("telegram-connector")
            .filename_suffix("log")
            .build(&config.file_path)?;

        Some(
            tracing_subscriber::fmt::layer()
                .with_writer(appender)
                .json()  // Always JSON for file logs (easier to parse)
                .boxed()
        )
    } else {
        None
    };

    // Build subscriber
    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_appender);

    subscriber.init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        log_level = %config.level,
        file_logging = config.file_enabled,
        "Logging initialized"
    );

    Ok(())
}
```

---

### 8.8 Log Examples

**Startup (compact format):**
```
2025-12-14T10:00:00Z INFO telegram_connector: Logging initialized version="0.1.0" log_level="info"
2025-12-14T10:00:00Z INFO telegram_connector::config: Configuration loaded config_path="~/.config/telegram-connector/config.toml"
2025-12-14T10:00:01Z INFO telegram_connector::telegram: Connecting to Telegram phone="+1234***890"
2025-12-14T10:00:02Z INFO telegram_connector::telegram: Connected successfully user_id=123456789
2025-12-14T10:00:02Z INFO telegram_connector::mcp: MCP server started protocol="2025-06-18"
```

**Search operation:**
```
2025-12-14T10:05:00Z DEBUG telegram_connector::mcp: Tool called tool="search_messages" request_id=42
2025-12-14T10:05:00Z DEBUG telegram_connector::telegram: Search started query="AI news" channels=12 hours_back=48
2025-12-14T10:05:01Z INFO telegram_connector::mcp: Search completed query="AI news" results=23 duration_ms=342
```

**Rate limiting:**
```
2025-12-14T10:10:00Z WARN telegram_connector::rate_limiter: Rate limited tokens_available=0.5 retry_after_seconds=2
```

**Error:**
```
2025-12-14T10:15:00Z ERROR telegram_connector::telegram: API error error_type="FloodWait" error_code=420 retry_after=30
```

**Shutdown:**
```
2025-12-14T18:00:00Z INFO telegram_connector: Shutdown initiated signal="SIGTERM"
2025-12-14T18:00:01Z INFO telegram_connector: Shutdown complete uptime_seconds=28801
```

---

### 8.9 Environment Override

Override log level at runtime:
```bash
RUST_LOG=debug ./telegram-mcp
RUST_LOG=telegram_connector::telegram=trace ./telegram-mcp
```

---

*Section 8 complete. Technical Vision document is complete.*

---

## Document Summary

This Technical Vision defines:

1. **Technologies** — Rust 2024, Tokio, rmcp, grammers, tracing
2. **Development Principles** — TDD, KISS, DDD, 80%+ coverage
3. **Project Structure** — Library + Binary, modern module system
4. **Architecture** — Layered (MCP → Application → Telegram), Arc<T> sharing
5. **Data Model** — Type-safe IDs, DDD entities, Option<T> for optionals
6. **Workflows** — Startup, auth, graceful shutdown, search flows
7. **Configuration** — TOML with env var support, sensible defaults
8. **Logging** — Structured logging, file rotation, sensitive data redaction

**Ready for implementation.**
