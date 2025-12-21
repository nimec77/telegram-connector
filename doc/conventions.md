# Development Conventions

**Reference:** See [vision.md](vision.md) for complete architecture, data model, and workflows.

---

## TDD Workflow

```
RED → GREEN → REFACTOR
```

1. Write a failing test first
2. Write minimal code to pass
3. Refactor while tests stay green

**No production code without a failing test.**

---

## KISS Principles

| Principle | Rule |
|-----------|------|
| Single Responsibility | One module = one purpose |
| No Premature Abstraction | Extract only after Rule of Three |
| Explicit over Implicit | Clear signatures, no magic |
| Flat over Nested | Prefer flat module structure |
| Delete over Comment | Remove dead code, don't comment it |

---

## Code Style

- **Format:** `cargo fmt` (default settings)
- **Lint:** `cargo clippy -- -D warnings`
- **Naming:** snake_case functions, PascalCase types
- **Docs:** `///` on public API only
- **Line length:** 100 characters

---

## Module System

**No `mod.rs` files.** Use file-as-module pattern:

```rust
// src/mcp.rs (not src/mcp/mod.rs)
pub mod server;
pub mod tools;
```

---

## Error Handling

| Layer | Crate | Usage |
|-------|-------|-------|
| Library | `thiserror` | Typed error definitions |
| Application | `anyhow` | Error context & propagation |

### Production Code Rules

- **NEVER use `unwrap()`** in production code (library or application)
- **Use `?` operator** for propagating errors
- **Use `expect()` with clear messages** only in tests or truly impossible situations
- **Add context** with `.context()` for better error messages

```rust
// Library errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("session expired")]
    SessionExpired,
}

// Application errors
fn load() -> anyhow::Result<Config> {
    // Use .context() for error context
}
```

---

## Trait-Based Abstraction

External dependencies behind traits for testability:

```rust
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait TelegramClientTrait: Send + Sync {
    async fn search_messages(&self, params: &SearchParams) -> Result<SearchResult>;
}
```

---

## State Management

Shared state via `Arc<T>`:

```rust
let client = Arc::new(TelegramClient::new(&config).await?);
let server = McpServer::new(Arc::clone(&client));
```

---

## Testing

| Type | Location | Tools |
|------|----------|-------|
| Unit | `#[cfg(test)]` in modules | `#[test]`, `mockall` |
| Integration | `tests/` directory | `tokio::test` |
| Property-based | Within unit/integration | `proptest` |

**Coverage target:** 80%+

---

## Logging

- Use `tracing` for structured async logging
- **Never log:** phone numbers, API hashes, passwords, session tokens
- Use redaction helpers for sensitive data

```rust
tracing::info!(
    phone = %redact_phone(&phone),
    "Authenticating"
);
```

---

## Git Commits

- Small, atomic, one logical change
- Imperative mood: "Add feature", not "Added feature"
- Format: `<type>: <description>`
- Types: `feat`, `fix`, `test`, `refactor`, `docs`, `chore`

---

## Pre-Commit Checklist

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

All checks must pass before commit.

---

## How to Write Code

### Start with Tests (TDD)

```rust
// 1. Write the test FIRST
#[test]
fn redact_phone_hides_middle_digits() {
    assert_eq!(redact_phone("+1234567890"), "+123***890");
}

// 2. Write minimal implementation
pub fn redact_phone(phone: &str) -> String {
    format!("{}***{}", &phone[..4], &phone[phone.len()-3..])
}

// 3. Refactor if needed (tests must stay green)
```

### Use Domain Types (DDD)

```rust
// Wrap primitives in meaningful types
pub struct ChannelId(pub i64);
pub struct MessageId(pub i64);

// Now the compiler prevents mistakes
fn get_message(channel: ChannelId, message: MessageId) -> Message;

// This won't compile - types are different
get_message(message_id, channel_id); // ERROR
```

### Keep Functions Small and Focused (KISS)

```rust
// Each function does ONE thing
pub fn validate_query(query: &str) -> Result<()> {
    if query.is_empty() {
        return Err(Error::EmptyQuery);
    }
    Ok(())
}

pub fn validate_limit(limit: u32) -> Result<u32> {
    Ok(limit.min(MAX_LIMIT))
}

pub fn search(&self, params: SearchParams) -> Result<SearchResult> {
    validate_query(&params.query)?;
    let limit = validate_limit(params.limit)?;
    self.client.search(&params.query, limit).await
}
```

### Extract When Duplicated Three Times (DRY)

```rust
// First occurrence - just write it
let url1 = format!("https://t.me/c/{}/{}", channel_id1, msg_id1);

// Second occurrence - still fine
let url2 = format!("https://t.me/c/{}/{}", channel_id2, msg_id2);

// Third occurrence - NOW extract
fn make_message_url(channel: ChannelId, message: MessageId) -> String {
    format!("https://t.me/c/{}/{}", channel, message)
}
```

### Use Explicit Error Context

```rust
// Add context at each layer
let content = std::fs::read_to_string(&path)
    .context(format!("Failed to read config: {}", path.display()))?;

let config: Config = toml::from_str(&content)
    .context("Failed to parse TOML")?;
```

### Prefer Composition Over Inheritance

```rust
// Compose behaviors via traits
struct McpServer<T: TelegramClientTrait, R: RateLimiterTrait> {
    client: Arc<T>,
    limiter: Arc<R>,
}

// Easy to test with mocks
let server = McpServer::new(Arc::new(MockTelegramClient::new()), ...);
```

---

## Anti-Patterns (What NOT to Do)

### Violates TDD: Code Before Test

```rust
// WRONG: Writing implementation first
pub fn complex_search_logic() -> Vec<Message> {
    // 200 lines of untested code...
}

// Then "adding tests later" (often never happens)
```

### Violates KISS: Premature Abstraction

```rust
// WRONG: Abstract factory for one use case
trait MessageFormatterFactory {
    fn create_formatter(&self) -> Box<dyn MessageFormatter>;
}

trait MessageFormatter {
    fn format(&self, msg: &Message) -> String;
}

struct DefaultFormatterFactory;
impl MessageFormatterFactory for DefaultFormatterFactory { ... }

// RIGHT: Just a function
fn format_message(msg: &Message) -> String {
    format!("[{}] {}", msg.timestamp, msg.text)
}
```

### Violates KISS: Over-Engineering

```rust
// WRONG: Generic where not needed
pub struct Config<S: Storage, L: Logger, V: Validator> {
    storage: S,
    logger: L,
    validator: V,
}

// RIGHT: Concrete types until proven otherwise
pub struct Config {
    pub telegram: TelegramConfig,
    pub logging: LoggingConfig,
}
```

### Violates DRY: Copy-Paste Code

```rust
// WRONG: Same validation in multiple places
fn search_messages(query: &str, limit: u32) {
    if query.is_empty() { return Err(...); }
    if limit > 100 { return Err(...); }
    // ...
}

fn search_channel(query: &str, channel: &str, limit: u32) {
    if query.is_empty() { return Err(...); }  // Duplicated!
    if limit > 100 { return Err(...); }       // Duplicated!
    // ...
}

// RIGHT: Extract shared validation
fn validate_search_params(query: &str, limit: u32) -> Result<()> { ... }
```

### Violates DDD: Primitive Obsession

```rust
// WRONG: Raw primitives everywhere
fn get_message(channel_id: i64, message_id: i64) -> Message;

// Easy to mix up arguments - no compiler help
get_message(msg_id, channel_id);  // Compiles but WRONG!

// RIGHT: Type-safe wrappers
fn get_message(channel: ChannelId, message: MessageId) -> Message;
```

### Violates DDD: Anemic Domain Model

```rust
// WRONG: Data struct + separate logic
pub struct Message {
    pub channel_id: i64,
    pub message_id: i64,
}

// Logic scattered elsewhere
fn make_link(msg: &Message) -> String { ... }
fn is_recent(msg: &Message) -> bool { ... }

// RIGHT: Behavior with data
impl Message {
    pub fn link(&self) -> MessageLink {
        MessageLink::new(self.channel_id, self.message_id)
    }

    pub fn is_recent(&self, hours: u32) -> bool {
        self.timestamp > Utc::now() - Duration::hours(hours as i64)
    }
}
```

### Violates Single Responsibility

```rust
// WRONG: God function
fn process_request(req: Request) -> Response {
    // Validate input (50 lines)
    // Check rate limit (30 lines)
    // Query database (40 lines)
    // Transform results (60 lines)
    // Format response (30 lines)
    // Log everything (20 lines)
}

// RIGHT: Composed small functions
fn process_request(req: Request) -> Response {
    let params = validate(req)?;
    rate_limiter.acquire()?;
    let results = query(&params)?;
    format_response(results)
}
```

### Violates Explicit Over Implicit

```rust
// WRONG: Magic defaults hidden in implementation
fn search(query: &str) -> Vec<Message> {
    let limit = 50;        // Where does 50 come from?
    let hours = 48;        // Why 48?
    // ...
}

// RIGHT: Explicit with named constants
const DEFAULT_LIMIT: u32 = 50;
const DEFAULT_HOURS_BACK: u32 = 48;

fn search(query: &str, limit: Option<u32>, hours: Option<u32>) -> Vec<Message> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    let hours = hours.unwrap_or(DEFAULT_HOURS_BACK);
    // ...
}
```

### Violates Delete Over Comment

```rust
// WRONG: Commented-out code
fn search() {
    // let old_impl = deprecated_search();
    // if USE_OLD { return old_impl; }

    new_search()
}

// RIGHT: Just delete it (git has history)
fn search() {
    new_search()
}
```

### Violates Error Handling Conventions

```rust
// WRONG: Using unwrap() - can panic at runtime
fn load_config() -> Config {
    std::fs::read_to_string("config.toml")
        .unwrap()  // ❌ Panic if file missing!
}

fn get_user_id(user: &Option<User>) -> UserId {
    user.as_ref().unwrap().id  // ❌ Panic if None!
}

// WRONG: Swallowing errors
fn load_config() -> Config {
    std::fs::read_to_string("config.toml")
        .unwrap_or_default()  // Silent failure!
        .parse()
        .unwrap()             // Panic on error!
}

// WRONG: Stringly-typed errors
fn connect() -> Result<(), String> {
    Err("connection failed".to_string())
}

// RIGHT: Proper error propagation with context
fn load_config() -> Result<Config> {
    let content = std::fs::read_to_string("config.toml")
        .context("Failed to read config file")?;
    toml::from_str(&content)
        .context("Failed to parse config")
}

// RIGHT: Handle Option properly
fn get_user_id(user: &Option<User>) -> Result<UserId> {
    user.as_ref()
        .map(|u| u.id)
        .context("User not found")
}
```

### Violates Flat Over Nested

```rust
// WRONG: Deep nesting
if let Some(user) = get_user() {
    if let Some(channel) = user.channel() {
        if let Some(message) = channel.last_message() {
            if message.is_recent() {
                process(message);
            }
        }
    }
}

// RIGHT: Early returns / guard clauses
let Some(user) = get_user() else { return };
let Some(channel) = user.channel() else { return };
let Some(message) = channel.last_message() else { return };
if !message.is_recent() { return };
process(message);
```

### Violates Testing: Testing Implementation Details

```rust
// WRONG: Testing private internals
#[test]
fn test_internal_cache_structure() {
    let cache = Cache::new();
    assert_eq!(cache.internal_map.len(), 0);  // Breaks on refactor
}

// RIGHT: Test public behavior
#[test]
fn cache_returns_none_for_missing_key() {
    let cache = Cache::new();
    assert!(cache.get("missing").is_none());
}
```

### Violates Minimal Dependencies

```rust
// WRONG: Adding crate for trivial operation
// Cargo.toml: some-string-utils = "1.0"
use some_string_utils::is_empty_or_whitespace;

// RIGHT: Just write it
fn is_empty_or_whitespace(s: &str) -> bool {
    s.trim().is_empty()
}
