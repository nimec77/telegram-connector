# Development Memory - Telegram MCP Connector

**Last Updated:** Refactor roadmap COMPLETE — Phase D data honesty (CQ-4/5), 2026-06-20

---

## Current Status

> **Phase & test counts:** `docs/tasklist.md` is the single source of truth.
> The per-phase list below is a historical journal and may lag the live totals.

- ✅ Phase 1: Project Setup
- ✅ Phase 2: Error Types (11/11 tests)
- ✅ Phase 3: Configuration (15/15 tests + 5 ignored)
- ✅ Phase 4: Logging (19/19 tests)
- ✅ Phase 5: Domain Types (52/52 tests)
- ✅ Phase 6: Link Generation (8/8 tests)
- ✅ Phase 7: Rate Limiter (19/19 tests)
- ✅ Phase 8: Telegram Auth (1/1 test)
- ✅ Phase 9: Telegram Client (18/18 tests)
- ✅ Phase 10: MCP Server (19/19 tests)
- ✅ Phase 11: MCP Tools (4/4 tests)
- ✅ Phase 12: Integration & Polish (real grammers, CLI, signal handling, rmcp tools)
- ✅ Phase 13: Refactoring
- ✅ Phase 14: Conditional Credentials
- ✅ Phase 15: File Logging
- ✅ Phase 16: Media Search Filtering (167 tests)
- ✅ Phase 17: Get Recent Messages (186 tests)
- ✅ Phase 18: Comprehensive Refactoring (209 tests)
- ✅ Phase 19: Log Cleanup (215 tests)
- ✅ Phase 20: Hang Diagnostics & Grammers Timeouts (249 tests)
- ✅ Phase 21: Flexible Scalar Coercion (305 tests)

**Total:** 305 tests passing (5 ignored for CI/CD)

**rmcp Integration:** All 7 MCP tools have `#[tool]` attributes for proper protocol compliance

---

## Phase 3: Configuration (Complete)

### What Was Implemented

1. **Environment Variable Expansion** (src/config.rs:157-171)
   - Simple string operations for `${VAR}` syntax
   - **Decision:** No regex dependency (KISS principle)
   - Handles missing variables gracefully with `unwrap_or_default()`

2. **Config Path Resolution** (src/config.rs:130-143)
   - Priority order:
     1. `TELEGRAM_MCP_CONFIG` environment variable
     2. XDG config directory via `directories` crate
     3. Default: `~/.config/telegram-connector/config.toml`

3. **Validation** (src/config.rs:145-155)
   - Required fields: `api_id`, `api_hash`, `phone_number`
   - Uses `anyhow::bail!` for clear error messages

4. **Serde Default Pattern**
   - All optional fields have `#[serde(default = "function_name")]`
   - Defaults defined at module level (required by serde)
   - Optional config sections use composed default functions

5. **Config::load() orchestration** (src/config.rs:107-128)
   - Load file → Parse TOML → Expand env vars → Validate
   - Uses `anyhow::Context` for error context at each step

### Tests: 16/16 Passing

**Run command:** `cargo test config -- --test-threads=1`

Test coverage:
- 5 env var expansion tests
- 4 validation tests (3 error cases + 1 success)
- 3 file loading tests (valid, invalid, missing)
- 2 env var integration tests
- 2 path resolution tests

### Key Decisions & Rationale

1. **No regex dependency**
   - **Why:** Simple `${VAR}` pattern doesn't need regex complexity
   - **How:** String `find()` + `replace_range()`
   - **Benefit:** Zero dependencies, faster compilation, cleaner code

2. **Serde defaults vs apply_defaults() method**
   - **Choice:** Use serde `#[serde(default)]` attributes
   - **Why:** Automatic, declarative, less code to maintain
   - **Note:** Kept `apply_defaults()` method as no-op for future use

3. **Optional config sections**
   - **Pattern:** Make `search`, `rate_limiting`, `logging` optional
   - **Implementation:** Default functions that construct entire structs
   - **Benefit:** Minimal config file, sensible defaults

### Gotchas & Edge Cases

1. **Environment Variable Race Conditions**
   - **Problem:** `env::set_var()` affects all threads globally
   - **Symptom:** Tests fail randomly when run in parallel
   - **Solution:** Use `cargo test config -- --test-threads=1`
   - **Alternative:** Avoid env var tests, or use test fixtures
   - **Documented in:** tasklist.md, Phase 3 notes

2. **env::set_var() is unsafe**
   - **Reason:** Modifying environment is not thread-safe in Rust
   - **Workaround:** Wrapped in `unsafe {}` blocks in tests
   - **Implication:** Confirms need for serial test execution

3. **Serde default functions must be module-level**
   - **Restriction:** Can't use closures, lambdas, or impl methods
   - **Pattern:** Create `fn default_xxx() -> Type` at module level
   - **Example:** `fn default_log_level() -> String { "info".to_string() }`

4. **XDG config directory creation**
   - **Issue:** `ProjectDirs::from()` can fail if home dir not determinable
   - **Handling:** Return error with `.ok_or_else()`
   - **Note:** Not an issue in practice on macOS/Linux

### Patterns to Reuse

```rust
// Pattern 1: Serde defaults for optional fields
#[derive(Deserialize)]
struct Config {
    #[serde(default = "default_value")]
    field: Type,
}
fn default_value() -> Type { /* ... */ }

// Pattern 2: Error context at each step
let content = std::fs::read_to_string(&path)
    .context(format!("Failed to read config: {}", path.display()))?;

// Pattern 3: Simple env var expansion without regex
while let Some(start) = result.find("${") {
    if let Some(end_offset) = result[start..].find('}') {
        let var_name = &result[start + 2..end];
        let var_value = std::env::var(var_name).unwrap_or_default();
        result.replace_range(start..=end, &var_value);
    }
}

// Pattern 4: Test environment variable handling
unsafe {
    env::set_var("TEST_VAR", "value");
}
// test code
unsafe {
    env::remove_var("TEST_VAR");
}
```

---

## Workflow Adherence

Following docs/workflow.md cycle:
1. ✅ **PROPOSE** - Proposed config implementation approach
2. ✅ **AGREE** - User approved (removed regex dependency)
3. ✅ **IMPLEMENT** - TDD: wrote tests first, then implementation
4. ✅ **VERIFY** - All tests pass, clippy clean
5. ✅ **UPDATE PROGRESS** - Updated tasklist.md
6. ✅ **UPDATE MEMORY** - This file created

---

## Technical Debt / TODOs

None for Phase 3. Clean implementation with 100% test coverage of public API.

---

## Phase 4: Logging (Complete)

### What Was Implemented

1. **Sensitive Data Protection with Secrecy Crate**
   - Added `secrecy = { version = "0.10", features = ["serde"] }` to Cargo.toml
   - Updated `TelegramConfig` to use `SecretString` for `api_hash` and `phone_number`
   - **Decision:** Session file path remains `PathBuf` (not sensitive data)
   - **Reason:** Path itself isn't sensitive; file contents are

2. **Secrecy API Learnings** (src/config.rs:1-90)
   - Version 0.10 uses `SecretString` (alias for `SecretBox<str>`) and `SecretBox<T>`
   - Constructor requires `Box<T>`: `SecretString::new(s.into_boxed_str())`
   - Access via `expose_secret()` method from `ExposeSecret` trait
   - Debug output shows "Secret" instead of actual values
   - **Gotcha:** `PathBuf` doesn't implement `Zeroize`, can't use with `SecretBox`

3. **Redaction Functions** (src/logging.rs:10-41)
   - `redact_phone()`: Shows first 4 + last 3 chars (`+1234567890` → `+123***890`)
   - `redact_hash()`: Shows first 4 + last 1 char (`abc123def456` → `abc1***6`)
   - Both return `"[REDACTED]"` for strings ≤6 characters
   - **Pattern:** Simple string slicing, no regex needed

4. **Tracing Subscriber Initialization** (src/logging.rs:5-35)
   - Added `"json"` feature to `tracing-subscriber` in Cargo.toml
   - Supports three formats: compact (default), pretty, json
   - Uses `try_init()` instead of `init()` to handle already-initialized subscriber
   - **Pattern:** `result.or(Ok(()))` to ignore "already initialized" errors in tests
   - Outputs to stderr only (file logging deferred to Phase 12)

5. **Config Updates for Secrecy**
   - Custom deserializer: `deserialize_secret_string()` converts `String` → `SecretString`
   - Env var expansion: `expand_env_vars_secret()` wraps expanded string in `SecretString`
   - Validation: Uses `.expose_secret()` to check emptiness
   - **Test Count:** Increased from 16 to 18 (added 2 Secret behavior tests)

### Tests: 13/13 Passing

**Run command:** `cargo test logging`

Test coverage:
- 5 phone redaction tests (normal, longer, minimum length, too short, empty)
- 5 hash redaction tests (normal, long, minimum length, too short, empty)
- 3 initialization tests (valid config, different levels, different formats)

### Key Decisions & Rationale

1. **Secrecy for credentials only**
   - **Applied to:** `api_hash`, `phone_number`
   - **Not applied to:** `session_file` (path)
   - **Reason:** Paths aren't credentials; file contents are encrypted separately

2. **Deferred file logging to Phase 12**
   - **Scope reduction:** Phase 4 focuses on core logging + redaction
   - **Reason:** KISS principle - implement basic functionality first
   - **Note:** vision.md §8 describes full file logging with rotation

3. **Try-init pattern for tests**
   - **Problem:** Global subscriber can only be set once
   - **Solution:** Use `try_init()` + `.or(Ok(()))` to ignore re-init errors
   - **Benefit:** Tests can run in any order without failures

### Gotchas & Edge Cases

1. **Secrecy 0.10 API Differences**
   - **Expected:** `Secret<T>` generic type
   - **Actual:** `SecretBox<T>` generic, `SecretString` type alias
   - **Constructor:** Takes `Box<T>`, not `T`
   - **Example:** `SecretString::new("value".to_string().into_boxed_str())`

2. **PathBuf Cannot Be Secret**
   - **Problem:** `SecretBox<T>` requires `T: Zeroize`
   - **Issue:** `PathBuf` doesn't implement `Zeroize`
   - **Solution:** Don't wrap paths in `SecretBox`; they're not sensitive

3. **Tracing Subscriber Features**
   - **Required:** Must enable `"json"` feature for JSON format support
   - **Cargo.toml:** `tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "json"] }`

4. **Global Subscriber in Tests**
   - **Issue:** `init()` panics if subscriber already set
   - **Workaround:** Use `try_init()` which returns `Result`
   - **Pattern:** `.or(Ok(()))` converts "already set" error to success

### Patterns to Reuse

```rust
// Pattern 1: SecretString deserialization
fn deserialize_secret_string<'de, D>(deserializer: D) -> Result<SecretString, D::Error>
where D: serde::Deserializer<'de>
{
    let s = String::deserialize(deserializer)?;
    Ok(SecretString::new(s.into_boxed_str()))
}

// Pattern 2: Redaction helper
pub fn redact_phone(phone: &str) -> String {
    if phone.len() <= 6 {
        return "[REDACTED]".to_string();
    }
    format!("{}***{}", &phone[..4], &phone[phone.len()-3..])
}

// Pattern 3: Tracing initialization with format switching
let result = match config.format.as_str() {
    "json" => tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .json()
        .with_env_filter(filter)
        .try_init(),
    "pretty" => /* ... */,
    _ => /* compact ... */,
};
result.or(Ok(())) // Ignore "already initialized" error

// Pattern 4: SecretString env var expansion
fn expand_env_vars_secret(secret: &SecretString) -> anyhow::Result<SecretString> {
    let value = secret.expose_secret();
    let expanded = expand_env_vars(value)?;
    Ok(SecretString::new(expanded.into_boxed_str()))
}
```

### Documentation Updates

1. **config.example.toml** - Added security notice about SecretString protection
2. **Config tests** - Updated to use SecretString constructors
3. **Test count** - Phase 3 tests increased from 16 to 18 (Secret behavior tests)

---

## Workflow Adherence

Following docs/workflow.md cycle:
1. ✅ **PROPOSE** - Proposed logging + secrecy implementation approach
2. ✅ **AGREE** - User confirmed use of secrecy crate for sensitive data
3. ✅ **IMPLEMENT** - TDD: wrote tests first, then implementation
4. ✅ **VERIFY** - All tests pass (38 total: 18 config + 8 error + 13 logging - 1 overlap)
5. ✅ **UPDATE PROGRESS** - Updated tasklist.md
6. ✅ **UPDATE MEMORY** - This section created

---

## Phase 7: Rate Limiter (Complete)

### What Was Implemented

1. **Enhanced Error Type** (src/error.rs:11-12)
   - Added `retry_after_seconds: u64` field to `Error::RateLimit`
   - Error message now includes: "rate limit exceeded, retry after N seconds"
   - Allows MCP clients to know when to retry

2. **Token Bucket Implementation** (src/rate_limiter.rs)
   - Internal `TokenBucket` struct (52 lines)
   - Public `RateLimiter` struct with `Arc<Mutex<TokenBucket>>`
   - `RateLimiterTrait` for mockability with `#[async_trait]`
   - **Decision:** Used `Mutex` over atomics for simplicity (KISS)

3. **Token Bucket Algorithm** (src/rate_limiter.rs:24-51)
   - On-demand refill calculation (not background task)
   - Refills based on elapsed time: `tokens_to_add = elapsed_seconds * refill_rate`
   - Capped at `max_tokens` (no accumulation beyond capacity)
   - Calculates `retry_after_seconds` when insufficient tokens

4. **Async Trait Implementation** (src/rate_limiter.rs:76-103)
   - `async fn acquire(&self, tokens: u32) -> Result<(), Error>`
   - `fn available_tokens(&self) -> f64`
   - Non-blocking: returns error immediately if insufficient tokens

### Tests: 19/19 Passing

**Run command:** `cargo test rate_limiter` (completes in ~2 seconds)

Test coverage:
- 2 initialization tests
- 4 acquire success tests
- 3 acquire failure tests
- 4 refill over time tests
- 3 edge case tests
- 3 property-based tests (proptest)

**Note:** Removed `prop_refill_eventually_succeeds` test as it used `sleep()` causing tests to freeze/hang

### Key Decisions & Rationale

1. **Mutex<TokenBucket> vs Atomics**
   - **Choice:** `Arc<Mutex<TokenBucket>>`
   - **Why:** Simpler to reason about, easier to maintain
   - **Alternative:** Could use `AtomicU64` + `AtomicI64` for lock-free
   - **Benefit:** KISS principle, can optimize later if profiling shows need

2. **On-demand refill vs Background task**
   - **Choice:** Calculate refill on each `acquire()` call
   - **Why:** More precise, no wasted CPU on background task
   - **Pattern:** `elapsed = now - last_refill; tokens += elapsed * rate`

3. **Non-blocking acquire**
   - **Choice:** Return error immediately if insufficient
   - **Alternative:** Block/sleep until tokens available (semaphore-style)
   - **Why:** MCP tools should fail fast, not block the protocol

4. **Retry metadata calculation**
   - Formula: `retry_after = ceil((tokens_needed - available) / refill_rate)`
   - Example: Need 20 tokens, have 0, rate=5/sec → retry_after = 4 seconds
   - Allows intelligent retry logic in MCP clients

### Gotchas & Edge Cases

1. **Timing Precision in Tests**
   - **Problem:** `Instant::now()` causes microsecond-level refills
   - **Symptom:** Tests expecting exact token counts fail
   - **Solution:** Use approximate equality (e.g., `assert!(x >= 39.9 && x <= 40.1)`)
   - **Example:** After acquiring 10 from 50, might have 40.0001 due to elapsed time

2. **Property Test Performance - Test Removed**
   - **Problem:** `prop_refill_eventually_succeeds` caused tests to freeze (>60s with sleep)
   - **Initial attempt:** Reduced to 10 cases, narrower ranges, higher refill rates
   - **Final solution:** Removed test entirely - `sleep()` in proptest is not practical
   - **Lesson:** Avoid proptest for tests requiring I/O or time delays
   - **Coverage:** Refill behavior adequately tested by regular async tests

3. **Division by Zero with refill_rate=0**
   - **Handling:** `retry_after` calculation returns infinity, casted to `u64::MAX`
   - **Test:** `refill_rate_zero_never_refills` verifies no refill occurs
   - **Valid use case:** Rate limiter that never refills (one-time burst)

4. **Concurrency Safety**
   - **Test:** `concurrent_acquires_are_thread_safe` spawns 10 tasks
   - **Verification:** Exactly 10 successes (100 tokens / 10 per acquire)
   - **Pattern:** `Arc<RateLimiter>` + `Mutex` ensures atomic operations

### Patterns to Reuse

```rust
// Pattern 1: Token bucket with on-demand refill
fn refill(&mut self) {
    let now = Instant::now();
    let elapsed = now.duration_since(self.last_refill).as_secs_f64();
    let tokens_to_add = elapsed * self.refill_rate;
    self.available_tokens = (self.available_tokens + tokens_to_add).min(self.max_tokens);
    self.last_refill = now;
}

// Pattern 2: Calculate retry_after from deficit
let tokens_needed = tokens_f64 - self.available_tokens;
let retry_after = (tokens_needed / self.refill_rate).ceil() as u64;

// Pattern 3: Async trait with mockall
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait RateLimiterTrait: Send + Sync {
    async fn acquire(&self, tokens: u32) -> Result<(), Error>;
    fn available_tokens(&self) -> f64;
}

// Pattern 4: Proptest with custom config
proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]
    #[test]
    fn my_slow_test(value in 10u32..50) {
        // Test with sleep/IO
    }
}

// Pattern 5: Approximate equality for timing tests
let available = limiter.available_tokens();
assert!(available >= 39.9 && available <= 40.1); // ±0.1 tolerance
```

### Documentation Updates

1. **src/error.rs** - Enhanced `RateLimit` variant with field
2. **src/lib.rs** - Exported `RateLimiter` and `RateLimiterTrait`
3. **Test count** - Phase 7: 20 tests (all passing)

---

## Workflow Adherence

Following docs/workflow.md cycle:
1. ✅ **PROPOSE** - Proposed rate limiter with Mutex, non-blocking, retry metadata
2. ✅ **AGREE** - User confirmed all 4 implementation choices
3. ✅ **IMPLEMENT** - TDD: wrote tests first, then implementation
4. ✅ **VERIFY** - All tests pass (20/20), clippy clean, full suite passes (99 tests)
5. ✅ **UPDATE PROGRESS** - Updated tasklist.md
6. ✅ **UPDATE MEMORY** - This section created

---

## Phase 8: Telegram Authentication (Complete)

### What Was Implemented

1. **Session Persistence** (src/telegram/auth.rs:10-36)
   - `save_session(path, bytes)` - Atomic file writes with temp + rename
   - `load_session(path)` - Verifies file permissions before loading
   - File permissions enforced: 0600 (owner read/write only)
   - Parent directory created if missing

2. **Session Loading** (src/telegram/auth.rs:41-67)
   - Checks file exists before reading
   - Validates permissions on Unix (rejects if not 0600)
   - Returns session bytes for use with grammers Client

3. **Session Validity Check** (src/telegram/auth.rs:70-72)
   - `is_session_valid(client)` - Async check using `client.is_authorized()`
   - Returns bool, no exceptions thrown

4. **Interactive Auth Flow** (src/telegram/auth.rs:83-119)
   - `authenticate(client, phone)` - Complete 2FA flow
   - Uses `dialoguer` crate for prompts (Input and Password)
   - Handles: phone → code → 2FA password (if needed)
   - Proper error propagation with context

### Tests: 8/8 Passing

**Run command:** `cargo test telegram::auth`

Test coverage:
- save_session_creates_file
- save_session_creates_parent_directory
- save_session_sets_correct_permissions (Unix only)
- load_session_from_saved_file
- load_session_nonexistent_file_fails
- load_session_rejects_insecure_permissions (Unix only)
- save_and_load_round_trip
- save_overwrites_existing_file

**Note:** `is_session_valid` and `authenticate` require real Telegram client, tested manually

### Key Decisions & Rationale

1. **Raw Bytes vs Session Objects**
   - **Choice:** Work with `&[u8]` instead of grammers Session trait
   - **Why:** Session is a trait in grammers, not a concrete type
   - **Benefit:** Simpler API, caller manages session serialization
   - **Pattern:** `save_session(path, client.session().save())`

2. **Atomic File Writes**
   - **Choice:** Write to temp file, then rename
   - **Why:** Prevents corruption if write fails mid-operation
   - **Pattern:** `write(path.with_extension("tmp"))` → `rename()`
   - **Benefit:** Session file never in half-written state

3. **Permission Enforcement**
   - **Choice:** Error if permissions are not 0600 on load
   - **Why:** Security - session files contain auth tokens
   - **Alternative:** Could auto-fix permissions, but that hides issues
   - **Unix only:** Windows doesn't have Unix permission model

4. **dialoguer for Prompts**
   - **Choice:** Use dialoguer crate instead of raw stdin
   - **Why:** Better UX (validation, hidden password input)
   - **Location:** Prompts in auth.rs (co-located with auth logic)
   - **KISS:** Simple dependency, well-maintained

5. **Error Handling**
   - **Choice:** Keep `Error::Auth(String)` - no new variants
   - **Why:** KISS principle, descriptive messages sufficient
   - **Pattern:** `.map_err(|e| Error::Auth(format!("context: {}", e)))`

### Gotchas & Edge Cases

1. **grammers API Changes**
   - **Problem:** Initial implementation assumed Session was a struct
   - **Reality:** Session is a trait, work with bytes instead
   - **Solution:** Accept `&[u8]`, return `Vec<u8>`
   - **Lesson:** Always check actual API, not assumptions

2. **request_login_code Parameters**
   - **Issue:** Takes 2 arguments (phone + api_hash), not just phone
   - **Fix:** Pass empty string for second parameter for now
   - **Note:** May need to pass actual api_hash in production

3. **Platform-Specific Permissions**
   - **Unix:** File permissions with mode bits (0600)
   - **Windows:** Different permission model
   - **Solution:** `#[cfg(unix)]` for permission checks
   - **Fallback:** Windows doesn't enforce, relies on filesystem ACLs

4. **Temp File Cleanup**
   - **Pattern:** `path.with_extension("tmp")` creates temp file
   - **Cleanup:** Rename to final path (atomic)
   - **Edge case:** If rename fails, temp file may remain
   - **Acceptable:** Temp files are session data with same security

### Patterns to Reuse

```rust
// Pattern 1: Atomic file write
let temp_path = path.with_extension("tmp");
fs::write(&temp_path, data)?;
#[cfg(unix)]
{
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o600))?;
}
fs::rename(&temp_path, path)?;

// Pattern 2: Secure permission check
#[cfg(unix)]
{
    let metadata = fs::metadata(path)?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(...);
    }
}

// Pattern 3: Interactive prompts with dialoguer
let code: String = Input::new()
    .with_prompt("Enter code")
    .interact_text()?;

let password = Password::new()
    .with_prompt("Enter password")
    .interact()?;

// Pattern 4: Grammers 2FA flow
match client.sign_in(&token, &code).await {
    Ok(_) => Ok(()),
    Err(SignInError::PasswordRequired(password_token)) => {
        let password = prompt_password();
        client.check_password(password_token, password).await?;
        Ok(())
    }
    Err(e) => Err(e),
}
```

### Dependencies Added

1. **dialoguer = "0.11"** - Interactive CLI prompts
2. **tempfile = "3.13"** (dev) - Temp directories for tests

### Documentation Updates

1. **src/telegram/auth.rs** - Complete implementation with 8 tests
2. **Cargo.toml** - Added dialoguer and tempfile
3. **Test count** - Phase 8: 8 tests (all passing)

---

## Workflow Adherence

Following docs/workflow.md cycle:
1. ✅ **PROPOSE** - Proposed session persistence + interactive auth
2. ✅ **AGREE** - User confirmed approach (KISS, Error::Auth, 0600, dialoguer)
3. ✅ **IMPLEMENT** - TDD for session I/O, manual test for interactive auth
4. ✅ **VERIFY** - All tests pass (8/8), manual auth flow works
5. ✅ **UPDATE PROGRESS** - Updated tasklist.md
6. ✅ **UPDATE MEMORY** - This section created

---

## Phase 9: Telegram Client (Complete)

### What Was Implemented

1. **TelegramClientTrait Definition** (src/telegram/client.rs:10-29)
   - 4 async methods: `search_messages`, `get_channel_info`, `get_subscribed_channels`, `is_connected`
   - Uses `#[cfg_attr(test, mockall::automock)]` for testing
   - All methods return typed `Result<T, Error>` enum
   - Trait bounds: `Send + Sync` for async compatibility

2. **TelegramClient Struct** (src/telegram/client.rs:31-65)
   - Wraps `Arc<Client>` from grammers
   - `new()` - Stub implementation returning error (deferred to Phase 12)
   - `client()` - Accessor for underlying grammers client (for session saving)
   - **Decision:** Deferred real grammers integration to Phase 12

3. **Trait Implementation** (src/telegram/client.rs:67-146)
   - `is_connected()` - Delegates to `is_session_valid()` from Phase 8
   - `get_subscribed_channels()` - Validates parameters, stub with TODO
   - `get_channel_info()` - Validates identifier (non-empty), stub with TODO
   - `search_messages()` - Validates params (query non-empty, limit > 0), stub with TODO
   - All stubs include detailed implementation pseudocode comments

4. **Test Helpers** (src/telegram/client.rs:153-182)
   - `create_test_channel()` - Constructs Channel with all required fields
   - `create_test_message()` - Constructs Message with all required fields
   - Ensures test data matches actual struct definitions from Phase 5

### Tests: 12/12 Passing

**Run command:** `cargo test client` (118 total tests pass)

Test coverage:
- 2 is_connected tests (returns true, returns false)
- 2 get_subscribed_channels tests (returns list, respects pagination)
- 3 get_channel_info tests (by username, by ID, empty identifier fails)
- 5 search_messages tests (returns results, empty query fails, respects limit, with channel filter, zero limit fails)

**Note:** All tests use mocks - no real Telegram connection required

### Key Decisions & Rationale

1. **Stub Implementation, Not Full Integration**
   - **Choice:** `new()` returns error, trait methods have validation but no real grammers calls
   - **Why:** Phase 9 focuses on API design and testing, not grammers integration
   - **Deferred:** Full grammers connection to Phase 12 (when we have real API credentials)
   - **Benefit:** Can complete Phase 9 without Telegram account, faster iteration

2. **Session Handling in new()**
   - **Initial approach:** Only thought about loading existing session
   - **User correction:** "If we only load the session file in new(), how will the user be able to create it?"
   - **Revised approach:** `new()` handles BOTH first-time (no session) AND returning user (with session)
   - **Flow:** `new()` → check `is_connected()` → if false, call `authenticate()` → `save_session()`
   - **Lesson:** Always consider full user journey (first-time + returning)

3. **Typed Error Enum vs anyhow**
   - **Choice:** Use `Result<T, Error>` everywhere in trait
   - **User asked:** "Explain why you are suggesting this solution?"
   - **Answer:**
     - Type safety for library code
     - Consistent with existing modules (Phases 2-8)
     - Allows pattern matching on error types
     - Self-documenting API
   - **Alternative:** anyhow is great for applications, but not for libraries

4. **Mock-Based Testing**
   - **Choice:** Test via `MockTelegramClientTrait`, not real client
   - **Why:** No Telegram API credentials needed, fast, deterministic
   - **Coverage:** All 4 trait methods + pagination + validation
   - **Pattern:** Separate mock tests from real implementation validation

5. **Search Scope - All Channels vs Specific**
   - **Question:** Search all subscribed channels or require channel_id?
   - **Answer:** Search all subscribed by default, optionally filter by `channel_id`
   - **Rationale:** Matches user expectation ("search my channels")
   - **Implementation:** `if params.channel_id.is_some() { ... } else { ... }`

### Gotchas & Edge Cases

1. **grammers API Assumptions**
   - **Problem:** Initial code assumed `Config` and `InitParams` exist in grammers_client
   - **Reality:** grammers_client has different API structure (no such types)
   - **Error:** `unresolved imports grammers_client::Config, grammers_client::InitParams`
   - **Solution:** Simplified to stub implementation, defer to Phase 12
   - **Lesson:** Don't assume API structure without checking docs/autocomplete

2. **SearchResult Structure Mismatch**
   - **Mistake:** Used `total_count`, `query`, `channels_searched` as direct fields
   - **Actual:** Has nested structure with `QueryMetadata`:
     ```rust
     SearchResult {
         total_found: usize,
         search_time_ms: u64,
         query_metadata: QueryMetadata { query, hours_back, channels_searched },
     }
     ```
   - **Fix:** Updated all test code to match actual types from Phase 5
   - **Lesson:** Always check type definitions before using them

3. **Message and Channel Test Helpers**
   - **Problem:** Used fields like `date`, `views`, `subscriber_count`
   - **Actual:** `timestamp`, `has_media`, `media_type`, `member_count`, `is_verified`, etc.
   - **Error:** "no field `date` on type `Message`", etc.
   - **Fix:** Read actual struct definitions from types.rs and matched all fields
   - **Pattern:** Test helpers must match real struct shape exactly

4. **Unused Imports**
   - **Warning:** `load_session` and `Session` imports unused
   - **Reason:** Stub implementation doesn't need session loading logic
   - **Fix:** Removed unused imports to satisfy clippy
   - **Note:** Will be re-added in Phase 12 when implementing real client

5. **Session Creation Flow Clarification**
   - **User concern:** "How will user create session if new() only loads?"
   - **Clarification:** new() is constructor, auth is separate step
   - **Proper flow:**
     1. `client = TelegramClient::new(config)` - May or may not have session
     2. `if !client.is_connected()` - Check if auth needed
     3. `authenticate(client.client(), phone)` - Interactive 2FA flow
     4. `save_session(path, client.client().session().save())` - Persist
   - **Key insight:** Separation of concerns - construction ≠ authentication

### Patterns to Reuse

```rust
// Pattern 1: Trait with mockall for testing
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait TelegramClientTrait: Send + Sync {
    async fn search_messages(&self, params: &SearchParams) -> Result<SearchResult, Error>;
    async fn is_connected(&self) -> bool;
}

// Pattern 2: Parameter validation before stub
async fn search_messages(&self, params: &SearchParams) -> Result<SearchResult, Error> {
    // Validate parameters first
    if params.query.is_empty() {
        return Err(Error::InvalidInput("Search query cannot be empty".to_string()));
    }
    if params.limit == 0 {
        return Err(Error::InvalidInput("Search limit must be greater than 0".to_string()));
    }

    // Implementation or stub with TODO
    Err(Error::TelegramApi("search_messages not yet fully implemented - Phase 9 TODO".to_string()))
}

// Pattern 3: Test helpers for complex domain types
fn create_test_channel(id: i64, name: &str) -> Channel {
    Channel {
        id: ChannelId::new(id).unwrap(),
        name: ChannelName::new(name).unwrap(),
        username: Username::new("testchannel").unwrap(),
        description: Some("Test channel".to_string()),
        member_count: 1000,
        is_verified: false,
        is_public: true,
        is_subscribed: true,
        last_message_date: None,
    }
}

// Pattern 4: Mock with expectations and predicates
let mut mock = MockTelegramClientTrait::new();
mock.expect_get_subscribed_channels()
    .with(mockall::predicate::eq(10), mockall::predicate::eq(0))
    .times(1)
    .returning(move |_, _| Ok(expected_clone.clone()));

let result = mock.get_subscribed_channels(10, 0).await;
assert_eq!(result.unwrap().len(), 2);

// Pattern 5: Nested result construction
let expected_result = SearchResult {
    messages: expected_messages.clone(),
    total_found: 2,
    search_time_ms: 100,
    query_metadata: QueryMetadata {
        query: "test".to_string(),
        hours_back: 24,
        channels_searched: 1,
    },
};
```

### Dependencies Added

None - used existing dependencies:
- `grammers-client` (already in Cargo.toml)
- `mockall` (already in Cargo.toml, dev dependency)
- `async-trait` (already in Cargo.toml)

### Documentation Updates

1. **src/lib.rs** - Exported `TelegramClient` and `TelegramClientTrait`:
   ```rust
   pub use telegram::client::{TelegramClient, TelegramClientTrait};
   ```

2. **src/telegram/client.rs** - Added detailed implementation notes:
   - Constructor includes note about Phase 12 integration
   - Each stub method has pseudocode comments for future implementation
   - 12 comprehensive mock-based tests

3. **docs/tasklist.md** - Updated Phase 9:
   - Status: "✅ Complete | 12/12 | Trait, mocks, validation"
   - Overall progress: 9/12 phases complete

4. **Test count** - Phase 9: 12 new tests, total: 118 tests passing

---

## Workflow Adherence

Following docs/workflow.md cycle:
1. ✅ **PROPOSE** - Proposed client trait, stub implementation, mock testing
2. ✅ **AGREE** - User corrected session handling approach, confirmed all decisions
3. ✅ **IMPLEMENT** - TDD: wrote mock tests first, then trait and stub implementation
4. ✅ **VERIFY** - All tests pass (12/12 new, 118/118 total), no clippy warnings
5. ✅ **UPDATE PROGRESS** - Updated tasklist.md
6. ✅ **UPDATE MEMORY** - This section created

---

## Technical Debt / TODOs

- File logging with rotation - deferred to Phase 12 (Polish)
- Log file size limits and cleanup - deferred to Phase 12
- Manual integration test for full auth flow - deferred to Phase 12
- Full grammers client integration - deferred to Phase 12 (requires real Telegram API credentials)
- **NEW:** Tool registration/implementation - Phase 11 (rmcp SDK tool patterns)

---

## Phase 10: MCP Server (Complete)

### What Was Implemented

1. **McpServer Generic Struct** (src/mcp/server.rs:7-13)
   - Generic over `TelegramClientTrait + 'static` and `RateLimiterTrait + 'static`
   - Fields: `Arc<TelegramClient>` and `Arc<RateLimiter>` for shared state
   - Fields marked `#[allow(dead_code)]` (used in Phase 11)

2. **Constructor** (src/mcp/server.rs:16-21)
   - Simple `new(telegram_client, rate_limiter)` pattern
   - Takes ownership of `Arc<T>` clones

3. **ServerHandler Trait Implementation** (src/mcp/server.rs:40-59)
   - Implements `rmcp::ServerHandler` trait
   - `get_info()` returns `InitializeResult` with:
     - Protocol version: `ProtocolVersion::default()` (MCP 2024-11-05)
     - Server info: `Implementation { name, version, title, icons, website_url }`
     - Instructions: Description for Claude

4. **stdio Transport** (src/mcp/server.rs:23-36)
   - `run_stdio()` async method
   - Uses `tokio::io::{stdin, stdout}` as transport
   - Calls `.serve()` via `ServiceExt` trait
   - Blocks on `.waiting()` until shutdown

### Tests: 2/2 Passing

**Run command:** `cargo test mcp::server --lib`

Test coverage:
- `server_new_creates_instance_with_valid_dependencies` - Arc refcounting verification
- `server_handler_provides_server_info` - Metadata validation

**Total project tests:** 122 (all passing)

### Key Decisions & Rationale

1. **Generic over Traits, not Concrete Types**
   - **Choice:** `McpServer<T: TelegramClientTrait, R: RateLimiterTrait>`
   - **Why:** Allows testing with mocks, maintains testability from previous phases
   - **Benefit:** Same pattern as Phases 7-9, consistent architecture

2. **'static Lifetime Bounds Required**
   - **Choice:** Added `'static` to all generic bounds
   - **Why:** rmcp's `.serve()` requires owned types that live for program lifetime
   - **Error encountered:** "parameter type `T` may not live long enough"
   - **Solution:** `impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static>`

3. **No tool_box Macro (Yet)**
   - **Initial attempt:** Used `#[tool(tool_box)]` macro based on documentation
   - **Error:** "Unknown field: `tool_box`. Available values: ..."
   - **Reason:** rmcp 0.12.0 API differs from examples found online
   - **Decision:** Plain trait impl for Phase 10, defer tool registration to Phase 11
   - **Benefit:** KISS - implement one thing at a time

4. **anyhow::Result for run_stdio()**
   - **Choice:** Application-level error handling with `anyhow`
   - **Why:** Consistent with vision.md pattern for main.rs integration
   - **Alternative:** Could add `Error::McpServer` variant, but unnecessary

### Gotchas & Edge Cases

1. **rmcp 0.12.0 API Structure Complexity**
   - **Problem:** Expected `ServerInfo` as return type
   - **Reality:** `get_info()` returns `InitializeResult` with nested structure:
     ```rust
     InitializeResult {
         protocol_version: ProtocolVersion,
         capabilities: ServerCapabilities,
         server_info: Implementation,  // <-- nested!
         instructions: Option<String>,
     }
     ```
   - **Implementation { ... }** requires: `title`, `icons`, `website_url` (all `Option<T>`)
   - **Lesson:** Always check actual API structure in docs, not examples

2. **ServiceExt Trait Not Auto-Imported**
   - **Error:** "no method named `serve` found ... method is available but not in scope"
   - **Cause:** `.serve()` is in `ServiceExt` trait, not `ServerHandler`
   - **Fix:** `use rmcp::{ServerHandler, ServiceExt};`
   - **Lesson:** Trait methods require trait to be in scope

3. **tool_box Macro Not Available in 0.12.0**
   - **Documentation showed:** `#[tool(tool_box)]` for tool registration
   - **Actual error:** "Unknown field: `tool_box`"
   - **Root cause:** rmcp 0.12.0 has different macro API than examples
   - **Workaround:** Plain trait impl, defer tooling to Phase 11
   - **Future:** Will research correct tool registration pattern in Phase 11

4. **Dead Code Warnings on Fields**
   - **Warning:** "fields `telegram_client` and `rate_limiter` are never read"
   - **Reason:** Phase 10 only sets up server, tools use fields in Phase 11
   - **Solution:** `#[allow(dead_code)]` with explanatory comment
   - **Clean approach:** Better than suppressing with `_prefix` which hides intent

### Patterns to Reuse

```rust
// Pattern 1: Generic server with trait bounds and 'static lifetime
pub struct McpServer<T: TelegramClientTrait, R: RateLimiterTrait> {
    telegram_client: Arc<T>,
    rate_limiter: Arc<R>,
}

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static>
    McpServer<T, R>
{
    pub fn new(telegram_client: Arc<T>, rate_limiter: Arc<R>) -> Self {
        Self { telegram_client, rate_limiter }
    }
}

// Pattern 2: ServerHandler implementation with InitializeResult
impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static>
    ServerHandler for McpServer<T, R>
{
    fn get_info(&self) -> InitializeResult {
        InitializeResult {
            protocol_version: ProtocolVersion::default(),
            capabilities: Default::default(),
            server_info: Implementation {
                name: "server-name".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some("Description here".to_string()),
        }
    }
}

// Pattern 3: stdio transport setup
pub async fn run_stdio(self) -> anyhow::Result<()> {
    use tokio::io::{stdin, stdout};

    let transport = (stdin(), stdout());
    let server = self.serve(transport).await?;
    server.waiting().await?;

    Ok(())
}

// Pattern 4: Allow dead code with explanatory comment
pub struct Server {
    #[allow(dead_code)]  // Used in next phase
    field: Type,
}
```

### Documentation Updates

1. **src/mcp/server.rs** - Complete implementation (111 lines total):
   - McpServer struct with generics
   - ServerHandler trait impl
   - run_stdio() method with stdio transport
   - 2 comprehensive tests

2. **docs/tasklist.md** - Updated Phase 10:
   - Status: "✅ Complete | 2/2 | rmcp setup, stdio"
   - Overall progress: 10/12 phases complete
   - Noted tool registration deferred to Phase 11

3. **Test count** - Phase 10: 2 new tests, total: 122 tests passing

---

## Workflow Adherence

Following docs/workflow.md cycle:
1. ✅ **PROPOSE** - Proposed server structure, traits, stdio transport
2. ✅ **AGREE** - User confirmed all 4 questions (macro usage, error handling, metadata, scope)
3. ✅ **IMPLEMENT** - TDD: wrote tests first, then implementation, fixed compilation errors iteratively
4. ✅ **VERIFY** - All tests pass (2/2 new, 122/122 total), clippy clean, full build succeeds
5. ✅ **UPDATE PROGRESS** - Updated tasklist.md with Phase 10 completion
6. ✅ **UPDATE MEMORY** - This section created

---

## Phase 11: MCP Tools (In Progress - 30% Complete)

### What Was Implemented (Session 1)

**Completed:** Foundations + Tool 1 (check_mcp_status)

1. **Dependencies Added** (Cargo.toml:33)
   - `schemars = { version = "0.8", features = ["derive", "chrono"] }`
   - Enables JSON schema generation for MCP tool parameters
   - `chrono` feature needed for `DateTime<Utc>` support

2. **Module Structure** (src/mcp/tools/)
   - Created subdirectory organization per user preference (Option B)
   - `types.rs` - All 6 tool request/response types (172 lines)
   - Re-exports via `src/mcp/tools.rs`

3. **Tool Request/Response Types** (src/mcp/tools/types.rs)
   - `StatusResponse` - check_mcp_status response
   - `GetChannelsRequest` / `ChannelsResponse` - get_subscribed_channels
   - `GetChannelInfoRequest` - get_channel_info (returns Channel)
   - `GenerateLinkRequest` / `MessageLinkResponse` - generate_message_link
   - `OpenMessageRequest` / `OpenMessageResponse` - open_message_in_telegram
   - `SearchRequest` - search_messages (returns SearchResult)
   - All types derive: `Debug, Clone, Serialize/Deserialize, JsonSchema`
   - Field-level `#[schemars(description = "...")]` for documentation

4. **Domain Type Updates** (src/telegram/types.rs)
   - Added `JsonSchema` derive to ALL types:
     - ChannelId, MessageId, UserId
     - Username, ChannelName
     - MediaType, Message, Channel
     - SearchResult, QueryMetadata
   - Required for tool response schemas

5. **Tool 1: check_mcp_status** ✅ (src/mcp/server.rs:41-50)
   - Signature: `async fn check_mcp_status(&self) -> Result<Json<StatusResponse>, String>`
   - Returns connection status, rate limiter tokens, server version
   - 2/2 tests passing:
     - `check_status_returns_connection_info`
     - `check_status_reports_disconnected`
   - Pattern: Uses mocks (`MockTelegramClientTrait`, `MockRateLimiterTrait`)

### Tests: 6/6 Passing (4 type tests + 2 tool tests)

**Run command:** `cargo test mcp` or `cargo test tools`

Test coverage:
- Type serialization/deserialization (4 tests)
- Tool 1 functionality (2 tests)

**Total project tests:** 129 (was 122, +7 new)

### Key Decisions & Rationale

1. **schemars for JSON Schemas**
   - **Choice:** Use schemars crate with derive macros
   - **Why:** Standard approach for MCP tools, automatic schema generation
   - **Alternative:** Manual schema definition - rejected (too verbose)
   - **Benefit:** Type-safe, self-documenting, less code

2. **Tool Module Organization**
   - **Question:** Single file vs subdirectory?
   - **User preference:** Subdirectory structure (Option B)
   - **Structure:**
     ```
     src/mcp/
     ├── server.rs (ServerHandler + tool methods)
     ├── tools.rs (re-exports)
     └── tools/
         └── types.rs (all request/response types)
     ```
   - **Benefit:** Cleaner organization, easier to navigate

3. **Error Handling: Result<Json<T>, String>**
   - **Choice:** Use String for error messages in tools
   - **Why:** rmcp tools support this pattern, simple conversion from `Error`
   - **Pattern:** `.map_err(|e| e.to_string())` converts our Error to String
   - **Benefit:** Leverages thiserror Display implementations

4. **Tool Implementation Order**
   - **Order:** check_mcp_status → get_subscribed_channels → get_channel_info → generate_message_link → open_message_in_telegram → search_messages
   - **Rationale:** Simple to complex, build confidence with easy wins
   - **Status:** 1/6 complete (check_mcp_status ✅)

5. **macOS-Specific Tool**
   - **Question:** open_message_in_telegram platform support?
   - **Decision:** macOS-only for Phase 11 (Option A)
   - **Implementation:** Will return error on non-macOS platforms
   - **Future:** Linux support (xdg-open) in Phase 12+

### Gotchas & Edge Cases

1. **schemars chrono Feature Required**
   - **Problem:** Compilation error - `DateTime<Utc>` doesn't implement JsonSchema
   - **Error:** "the trait `JsonSchema` is not implemented for `DateTime<Utc>`"
   - **Cause:** Message/Channel types use `DateTime<Utc>` from chrono
   - **Solution:** Add `features = ["chrono"]` to schemars dependency
   - **Lesson:** Check feature flags when using external types with derives

2. **Mock Expectations for Async Methods**
   - **Initial attempt:** `.returning(|| Box::pin(async { true }))`
   - **Error:** "expected `bool`, found `Pin<Box<...>>`"
   - **Cause:** mockall handles async automatically with `#[async_trait]`
   - **Solution:** Use `.return_once(|| value)` for simple returns
   - **Pattern:**
     ```rust
     mock_client.expect_is_connected().return_once(|| true);
     mock_limiter.expect_available_tokens().return_once(|| 45.5);
     ```

3. **Unused Import Warning**
   - **Warning:** `unused import: Message` in tools/types.rs
   - **Cause:** Message not directly used in type definitions
   - **Fix:** Remove unused import (SearchResult already in scope)
   - **Note:** May be re-added when implementing tool handlers

4. **Dead Code Warnings Removed**
   - **Before:** `telegram_client` and `rate_limiter` marked `#[allow(dead_code)]`
   - **After:** Warnings gone - fields now used by check_mcp_status
   - **Lesson:** TDD approach validates design decisions

### Patterns to Reuse

```rust
// Pattern 1: Tool request type with schemars
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetChannelsRequest {
    #[schemars(description = "Maximum number of channels to return")]
    pub limit: Option<u32>,
}

// Pattern 2: Tool response type
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StatusResponse {
    #[schemars(description = "Whether Telegram client is connected")]
    pub telegram_connected: bool,
    pub rate_limiter_tokens: f64,
}

// Pattern 3: Tool method signature
pub async fn check_mcp_status(&self) -> Result<Json<StatusResponse>, String> {
    let connected = self.telegram_client.is_connected().await;
    Ok(Json(StatusResponse { ... }))
}

// Pattern 4: Tool test with mocks
#[tokio::test]
async fn check_status_returns_connection_info() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_is_connected().return_once(|| true);

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_available_tokens().return_once(|| 45.5);

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server.check_mcp_status().await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.telegram_connected, true);
}
```

### Files Modified/Created

1. **Created:**
   - `src/mcp/tools/types.rs` (172 lines) - All tool request/response types

2. **Modified:**
   - `Cargo.toml` - Added schemars dependency
   - `src/mcp/tools.rs` - Updated to re-export types module
   - `src/mcp/server.rs` - Added check_mcp_status method + 2 tests
   - `src/telegram/types.rs` - Added JsonSchema derive to all types

3. **Test Count:**
   - Phase 11 Session 1: 6 tests (4 types + 2 tools)
   - Total: 129 tests

### Tool 2: get_subscribed_channels ✅ (Session 2)

**What Was Implemented** (src/mcp/server.rs:52-76)

1. **Tool Method**
   - Signature: `async fn get_subscribed_channels(&self, request: GetChannelsRequest) -> Result<Json<ChannelsResponse>, String>`
   - Extracts `limit` (default 20) and `offset` (default 0) from request
   - Delegates to `self.telegram_client.get_subscribed_channels(limit, offset)`
   - Calculates `has_more` based on result count vs limit: `total >= limit as usize`
   - Returns `ChannelsResponse { channels, total, has_more }`

2. **Error Handling**
   - Simple `.map_err(|e| e.to_string())` conversion (Option B pattern agreed)
   - RateLimit errors will get structured JSON treatment in search_messages (Tool 6)

3. **Tests** (2/2 passing)
   - `get_subscribed_channels_returns_list` - Default pagination (20, 0), verifies response structure
   - `get_subscribed_channels_respects_pagination` - Custom limit/offset (10, 5), verifies passthrough

**Test Count:**
- Phase 11 Session 2: +2 tests (total 8/20 Phase 11 tests)
- Total: 131 tests

**Key Decision:**
- **has_more calculation:** Simple heuristic `total >= limit` - if we got as many as we asked for, there might be more
- **Response structure:** Uses `total` (count returned) and `has_more` (boolean), not `limit`/`offset` in response

**Pattern Established:**
```rust
// Tool with defaults
let limit = request.limit.unwrap_or(20);
let offset = request.offset.unwrap_or(0);

// Delegate to client
let channels = self.telegram_client
    .get_subscribed_channels(limit, offset)
    .await
    .map_err(|e| e.to_string())?;

// Calculate pagination metadata
let total = channels.len();
let has_more = total >= limit as usize;

// Wrap in response
Ok(Json(ChannelsResponse { channels, total, has_more }))
```

### Tool 3: get_channel_info ✅ (Session 2)

**What Was Implemented** (src/mcp/server.rs:79-90)

1. **Tool Method**
   - Signature: `async fn get_channel_info(&self, request: GetChannelInfoRequest) -> Result<Json<Channel>, String>`
   - Extracts `channel_identifier` from request
   - Delegates to `self.telegram_client.get_channel_info(&request.channel_identifier)`
   - Returns `Channel` directly wrapped in `Json` (no wrapper struct needed)
   - Simple passthrough with error mapping

2. **Error Handling**
   - Simple `.map_err(|e| e.to_string())` conversion
   - Consistent with Option B pattern

3. **Tests** (2/2 passing)
   - `get_channel_info_returns_channel_details` - Happy path, verifies all channel fields
   - `get_channel_info_handles_error` - Error case when channel not found

**Test Count:**
- Phase 11 Session 2: +2 tests (total 10/20 Phase 11 tests)
- Total: 133 tests

**Key Learnings:**
- **Field naming:** Use `channel_identifier` not `identifier` (check type definitions first)
- **ChannelId comparison:** Compare structs directly, field is private
- **Error assertion pattern:** Use `if let Err(error_msg) = result` to avoid Debug trait requirement on success type
- **Simplest tool yet:** Direct passthrough with minimal logic

**Pattern:**
```rust
// Direct passthrough tool
pub async fn get_channel_info(
    &self,
    request: GetChannelInfoRequest,
) -> Result<Json<Channel>, String> {
    let channel = self
        .telegram_client
        .get_channel_info(&request.channel_identifier)
        .await
        .map_err(|e| e.to_string())?;

    Ok(Json(channel))
}
```

### Remaining Work for Phase 11

**3 Tools to Implement:** (Estimated ~250 lines code + ~200 lines tests)

3. **generate_message_link** (~20 lines + 2 tests)
   - Uses `link::MessageLink` from Phase 6
   - Parses channel_id to i64
   - Returns both https and tg:// links

4. **open_message_in_telegram** (~40 lines + 2 tests)
   - Platform-specific: macOS only
   - Uses `tokio::process::Command::new("open").arg(link)`
   - Returns success/failure in `OpenMessageResponse`

5. **search_messages** (~60 lines + 4 tests)
   - Most complex: validation + rate limiting + client call
   - Validates query non-empty, limit bounds
   - Calls `self.rate_limiter.acquire(tokens)`
   - Delegates to `self.telegram_client.search_messages(params)`
   - Handles rate limit errors specially

**Estimated completion:** 1-2 more sessions for quality TDD implementation

---

## Workflow Adherence

Following docs/workflow.md cycle:
1. ✅ **PROPOSE** - Proposed tool structure, types, dependencies
2. ✅ **AGREE** - User confirmed all 5 questions (schemars, error mapping, order, macOS-only, module organization)
3. ✅ **IMPLEMENT** - TDD: types → tests → implementation for Tool 1
4. ✅ **VERIFY** - All tests pass (6/6 new, 129/129 total), clippy clean
5. ✅ **UPDATE PROGRESS** - Updated tasklist.md with Phase 11 progress
6. ✅ **UPDATE MEMORY** - This section created

---

## Technical Debt / TODOs

- File logging with rotation - deferred to Phase 12 (Polish)
- Log file size limits and cleanup - deferred to Phase 12
- Manual integration test for full auth flow - deferred to Phase 12
- Full grammers client integration - deferred to Phase 12 (requires real Telegram API credentials)

---

## Phase 11: MCP Tools (Complete - Session 3)

### What Was Implemented (Session 3 - Tools 4-6)

**Completed:** Tools 4-6 (generate_message_link, open_message_in_telegram, search_messages)

1. **Tool 4: generate_message_link** (src/mcp/server.rs:98-131)
   - Parses channel_id string to i64, creates type-safe ChannelId
   - Uses existing `MessageLink::new()` from link.rs
   - Returns `MessageLinkResponse` with https and optional tg:// links
   - Respects `include_tg_protocol` flag (defaults to true)
   - 3 tests: both formats, without tg protocol, invalid channel_id

2. **Tool 5: open_message_in_telegram** (src/mcp/server.rs:133-195)
   - Platform-specific: macOS only using `tokio::process::Command::new("open")`
   - Uses `#[cfg(target_os = "macos")]` for conditional compilation
   - Returns graceful error on non-macOS platforms
   - Returns `OpenMessageResponse` with success, message, link_used, app_opened
   - 3 tests: invalid channel_id, tg:// by default, https when requested

3. **Tool 6: search_messages** (src/mcp/server.rs:197-259)
   - Validates query (non-empty after trim)
   - Parses optional channel_id
   - Applies defaults and limits (hours_back: 48/72, limit: 20/100)
   - Acquires rate limiter token before search
   - Delegates to TelegramClient.search_messages()
   - 5 tests: returns results, empty query fails, rate limited, channel filter, applies limits

### Tests: 21/21 Passing (Phase 11 Total)

**Run command:** `cargo test mcp`

Test breakdown:
- 2 server tests (instance creation, handler info)
- 2 check_mcp_status tests
- 2 get_subscribed_channels tests
- 2 get_channel_info tests
- 3 generate_message_link tests
- 3 open_message_in_telegram tests
- 5 search_messages tests
- 4 types tests (from tools/types.rs)

**Total project tests:** 140/140 passing

### Key Decisions & Rationale

1. **Numeric channel_id only for link generation**
   - `MessageLink` in link.rs uses numeric ChannelId
   - Username format (https://t.me/username/msgid) would need different URL pattern
   - Kept simple for Phase 11, can extend later if needed

2. **Platform-specific compilation for open_message**
   - Used `#[cfg(target_os = "macos")]` instead of runtime check
   - Returns graceful error response on non-macOS (not Err)
   - Allows MCP client to see what happened

3. **Rate limiting before search**
   - Acquire 1 token per search operation
   - Rate limit error propagates to MCP client with retry info
   - Query validation happens BEFORE rate limiting (fail fast)

4. **Limit capping instead of error**
   - Values exceeding MAX_HOURS_BACK/MAX_LIMIT are silently capped
   - Alternative: Return error for out-of-bounds values
   - Decision: More forgiving UX, similar to pagination patterns

### Patterns Established

```rust
// Pattern 1: String to ChannelId parsing
let channel_id_num: i64 = request.channel_id.parse()
    .map_err(|_| format!("Invalid channel_id: '{}' is not a valid number", request.channel_id))?;
let channel_id = ChannelId::new(channel_id_num)
    .map_err(|e| format!("Invalid channel_id: {}", e))?;

// Pattern 2: Platform-specific code
#[cfg(target_os = "macos")]
let result = tokio::process::Command::new("open")
    .arg(link_to_open)
    .output()
    .await;

#[cfg(not(target_os = "macos"))]
let result: Result<std::process::Output, std::io::Error> = Err(std::io::Error::new(
    std::io::ErrorKind::Unsupported,
    "open_message_in_telegram is only supported on macOS",
));

// Pattern 3: Limit capping
let hours_back = request.hours_back
    .unwrap_or(SearchParams::DEFAULT_HOURS_BACK)
    .min(SearchParams::MAX_HOURS_BACK);

// Pattern 4: Rate limiter integration
self.rate_limiter.acquire(1).await.map_err(|e| e.to_string())?;
```

### Files Modified

1. **src/mcp/server.rs** - Added 3 tool methods + 11 tests (~200 lines)
2. **docs/tasklist.md** - Marked Phase 11 complete
3. **docs/memory.md** - Added this section
4. **CLAUDE.md** - Updated status and test counts

---

## Phase 12: Integration & Polish (Complete)

### What Was Implemented

1. **ServerConfig with Shutdown Timeout** (src/config.rs)
   - Added `ServerConfig` struct with `shutdown_timeout_seconds` field
   - Default: 5 seconds
   - Added `load_from()` method for custom config path
   - Added `apply_cli_overrides()` for CLI session file override

2. **CLI Argument Parsing** (src/cli.rs - NEW)
   - Used `clap` with derive macros
   - `--setup` / `-s` flag for interactive authentication
   - `--session-file` for session path override
   - `--config` / `-c` for custom config file path
   - 7 comprehensive tests for CLI parsing

3. **Real Grammers Client Integration** (src/telegram/client.rs)
   - **SqliteSession** for persistent session storage (grammers-session)
   - **SenderPool** for connection management (grammers-mtsender)
   - Spawns runner in background task
   - Implements all `TelegramClientTrait` methods with real Telegram API:
     - `is_connected()` - Delegates to `client.is_authorized()`
     - `get_subscribed_channels()` - Uses `iter_dialogs()` with `Peer::Channel` filter
     - `get_channel_info()` - Uses `resolve_username()` or dialog iteration
     - `search_messages()` - Uses `search_messages()` or `search_all_messages()`
   - Added `request_login_code()`, `sign_in()`, `check_password()` for auth
   - Peer type conversion helpers for Channel, Group, User

4. **Interactive Authentication** (src/telegram/auth.rs)
   - Simplified to single `interactive_auth()` function
   - Uses dialoguer for Input (code) and Password (2FA)
   - Handles 2FA flow with password hint display
   - Removed old session save/load functions (SqliteSession handles persistence)

5. **Main Entry Point** (src/main.rs)
   - Signal handling for SIGTERM and SIGINT (Ctrl+C)
   - Uses `tokio::select!` for concurrent shutdown monitoring
   - Setup mode (`--setup`) for initial authentication
   - Normal MCP server mode for authenticated sessions
   - Graceful shutdown with configurable timeout
   - Clear error messages for unauthenticated state

### Key Decisions & Rationale

1. **grammers Master Branch API**
   - **Decision:** Use git dependency pointing to master branch
   - **Why:** Stable crates.io version is outdated, master has SqliteSession
   - **Trade-off:** API may change, but current implementation is well-documented

2. **SqliteSession vs Memory Session**
   - **Choice:** SqliteSession for all environments
   - **Why:** Automatic persistence, no manual save/load required
   - **Benefit:** Session survives crashes, no explicit save_session() needed

3. **SenderPool Architecture**
   - **Pattern:** Create pool → Create client → Spawn runner in background
   - **Why:** grammers uses actor pattern with runner handling network I/O
   - **Implementation:** `tokio::spawn(pool.runner.run())`

4. **Peer Type Handling**
   - **Pattern:** Match on `Peer` enum variants (User, Group, Channel)
   - **Decision:** Include Groups in channel listings (they behave similarly)
   - **Filtering:** Skip Users in `get_subscribed_channels()`

5. **Signal Handling**
   - **Unix:** Both SIGTERM and SIGINT (Ctrl+C)
   - **Windows:** Only Ctrl+C (SIGTERM not available)
   - **Pattern:** `tokio::select!` with shutdown channel

### Gotchas & Edge Cases

1. **grammers API Differences from Docs**
   - **Problem:** Documentation showed `Config`, `InitParams` types
   - **Reality:** Uses `SenderPool::new(session, api_id)` pattern
   - **Lesson:** Always check actual source code, not just examples

2. **request_login_code Signature**
   - **Expected:** `(api_id, api_hash, phone)`
   - **Actual:** `(phone, api_hash)` - api_id from SenderPool
   - **Lesson:** Check method signatures carefully

3. **Peer ID Methods**
   - **User:** Has `bare_id()` method directly
   - **Channel/Group:** Has `bare_id()` method
   - **Message sender:** Returns `Peer` enum, need to match

4. **ChannelId.get() vs .value()**
   - **Our types:** Use `.get()` method
   - **Easy mistake:** Assume `.value()` exists
   - **Lesson:** Check own type definitions

5. **Collapsible if with let chains**
   - **Clippy warning:** Can collapse nested `if let` statements
   - **Rust 2024:** Uses `if let && let` pattern (let chains)
   - **Example:** `if let Ok(peer) = msg.peer() && let Some(conv) = convert(...)`

### Patterns to Reuse

```rust
// Pattern 1: SqliteSession with SenderPool
let session = Arc::new(SqliteSession::open(&config.session_file)?);
let pool = SenderPool::new(Arc::clone(&session), config.api_id);
let client = Client::new(&pool);
tokio::spawn(async move { pool.runner.run().await });

// Pattern 2: Signal handling with tokio::select!
tokio::select! {
    result = server.run_stdio() => { result?; }
    _ = shutdown_rx => {
        tracing::info!("Graceful shutdown initiated");
    }
}

// Pattern 3: CLI with clap derive
#[derive(Parser)]
struct Cli {
    #[arg(long, short = 's')]
    setup: bool,

    #[arg(long, value_name = "FILE")]
    session_file: Option<PathBuf>,
}

// Pattern 4: Peer type matching
match peer {
    Peer::Channel(ch) => {
        let id = ChannelId::new(ch.bare_id())?;
        // ...
    }
    Peer::Group(g) => { /* similar */ }
    Peer::User(_) => None, // skip
}

// Pattern 5: Let chains (Rust 2024)
if let Ok(peer) = msg.peer()
    && let Some(converted) = convert_message(&msg, peer)
{
    messages.push(converted);
}
```

### Dependencies Added

1. **grammers-mtsender** - Direct dependency for SenderPool
   ```toml
   grammers-mtsender = { git = "https://github.com/Lonami/grammers", branch = "master" }
   ```

### Files Modified/Created

1. **Created:**
   - `src/cli.rs` (100 lines) - CLI argument parsing with clap

2. **Modified:**
   - `Cargo.toml` - Added grammers-mtsender, clap
   - `src/lib.rs` - Added cli module, exported Cli
   - `src/config.rs` - Added ServerConfig, load_from(), apply_cli_overrides()
   - `src/telegram/client.rs` - Complete rewrite with real grammers
   - `src/telegram/auth.rs` - Simplified to interactive_auth()
   - `src/main.rs` - Full implementation with signal handling

3. **Test Count:**
   - Phase 12: 7 CLI tests added
   - Total: 139 tests passing (4 ignored)

---

## Usage Instructions

### First-Time Setup
```bash
# Create config file
mkdir -p ~/.config/telegram-connector
cat > ~/.config/telegram-connector/config.toml << EOF
[telegram]
api_id = YOUR_API_ID
api_hash = "YOUR_API_HASH"
phone_number = "+1234567890"
EOF

# Run setup to authenticate
cargo run --bin telegram-mcp -- --setup
```

### Running MCP Server
```bash
# After authentication, run normally
cargo run --bin telegram-mcp

# Or with custom config
cargo run --bin telegram-mcp -- --config /path/to/config.toml
```

### CLI Options
```
telegram-mcp [OPTIONS]

Options:
  -s, --setup                Run interactive setup to authenticate
      --session-file <FILE>  Path to session file (overrides config)
  -c, --config <FILE>        Path to configuration file
  -h, --help                 Print help
  -V, --version              Print version
```

---

## Iteration 14: rmcp Tool Attributes Integration (Complete)

### What Was Implemented

1. **rmcp Tool Macros Added**
   - Added `#[tool]` attributes to all 6 MCP tool methods in `server.rs`
   - Each tool now has proper rmcp integration with description and parameter handling
   - Uses `tool_handler!` macro pattern for request/response handling

2. **Documentation Updates**
   - Enhanced README.md with detailed Comet Browser configuration guide
   - Added step-by-step instructions for MCP client setup
   - Included JSON-RPC examples for manual testing

### Tools with rmcp Integration

All 6 tools now have proper `#[tool]` attributes:
1. `check_mcp_status` - Health check and diagnostics
2. `get_subscribed_channels` - List channels with pagination
3. `get_channel_info` - Get channel metadata
4. `generate_message_link` - Create Telegram deep links
5. `open_message_in_telegram` - Open message in Telegram Desktop (macOS)
6. `search_messages` - Full-text search with rate limiting

---

## Project Status: COMPLETE ✅

### What's Complete
- ✅ All 12 development phases complete
- ✅ 139 tests passing (4 ignored for CI/CD compatibility)
- ✅ Real grammers integration with SqliteSession
- ✅ CLI with --setup, --session-file, --config options
- ✅ Signal handling (SIGTERM, SIGINT) for graceful shutdown
- ✅ rmcp tool attributes for MCP protocol compliance
- ✅ Comprehensive README.md with Comet Browser guide
- ✅ Manual testing with real Telegram account - PASSED
- ✅ Manual testing with MCP client (Comet Browser) - PASSED
- ✅ Release build created - `target/release/telegram-mcp`

---

## Iteration 13: Documentation & Rules Update (Complete)

### What Was Implemented

1. **Critical Rules Added**
   - Added "NEVER create git commits" rule to CLAUDE.md, conventions.md, workflow.md
   - User manages all git operations; Claude only writes code and documentation

2. **Comprehensive README.md Created**
   - Project overview with features list
   - Architecture diagram (ASCII art)
   - Prerequisites and installation guide
   - Configuration examples with environment variables
   - Detailed MCP Tools Reference (all 6 tools)
   - Manual Testing Guide with JSON-RPC examples
   - Troubleshooting section
   - Development guide

### Files Modified/Created

1. **Created:**
   - `README.md` - Comprehensive documentation (~500 lines)

2. **Modified:**
   - `CLAUDE.md` - Added Critical Rules section
   - `docs/conventions.md` - Added git commit restriction
   - `docs/workflow.md` - Added Critical Rules section
   - `docs/tasklist.md` - Marked README task complete
   - `docs/memory.md` - This section

### Key Decisions

1. **MCP Client Choice:** Documented Claude Desktop as the primary MCP client
2. **Manual Testing:** Included JSON-RPC examples for testing via stdin
3. **Tool Documentation:** Full parameter tables with types, defaults, and descriptions

### Completed Tasks

- [x] Test with real Telegram account ✅
- [x] Test with MCP client (Comet Browser) ✅
- [x] Create release build: `cargo build --release` ✅

**Note:** All tasks complete. Project is production-ready. Binary at `target/release/telegram-mcp`.

---

## Phase 13: Code Refactoring (Complete)

### What Was Implemented

1. **MCP Server Test Extraction** (Phase 13.1)
   - Created `src/mcp/tests/` directory with organized test files
   - Extracted tests into: `server_core.rs`, `status.rs`, `channels.rs`, `links.rs`, `search.rs`
   - Used `#[cfg(test)] #[path = "tests/mod.rs"] mod tests;` for test module path
   - **Result:** server.rs reduced from 945 → 307 lines

2. **Telegram Client Module Extraction** (Phase 13.2)
   - Created `src/telegram/trait_def.rs` - TelegramClientTrait definition with mockall
   - Created `src/telegram/converters.rs` - convert_peer_to_channel, convert_message helpers
   - Created `src/telegram/tests/` directory with client_tests.rs
   - Updated `src/telegram.rs` to re-export from new modules
   - **Result:** client.rs reduced from 755 → 343 lines

### Key Decisions & Rationale

1. **Test Extraction vs Tool Extraction**
   - **Choice:** Extract tests only, keep tool implementations in server.rs
   - **Why:** rmcp `#[tool_router]` macro requires all tool methods in single impl block
   - **Benefit:** Maintains macro compatibility while reducing file size

2. **Path Attribute for Test Modules**
   - **Pattern:** `#[cfg(test)] #[path = "tests/mod.rs"] mod tests;`
   - **Why:** Allows tests to live in separate directory while still being part of module
   - **Alternative:** Inline tests in same file (what we had before)

3. **Re-exports for Backward Compatibility**
   - **Pattern:** `pub use trait_def::TelegramClientTrait;` in telegram.rs
   - **Why:** External code can still import from `crate::telegram::TelegramClientTrait`
   - **Test imports:** Updated to use new paths for MockTelegramClientTrait

### Gotchas & Edge Cases

1. **Import Path Updates Required**
   - **Problem:** After moving TelegramClientTrait, existing imports broke
   - **Error:** `use crate::telegram::client::TelegramClientTrait` - trait is private
   - **Solution:** Re-export from `telegram.rs` and update all import paths
   - **Files affected:** server.rs, all MCP test files

2. **Mock Trait Methods Require Trait in Scope**
   - **Problem:** MockTelegramClientTrait methods not found
   - **Error:** "no method named `is_connected` found for struct `MockTelegramClientTrait`"
   - **Cause:** Mock implements trait, so trait must be imported to call methods
   - **Solution:** `use crate::telegram::trait_def::{MockTelegramClientTrait, TelegramClientTrait};`

3. **Unused Import Warning**
   - **Warning:** `unused import: ChannelId` in client.rs after extraction
   - **Cause:** ChannelId used in converters.rs, not client.rs anymore
   - **Solution:** Remove unused import from client.rs

4. **File-as-Module Pattern with #[path] Attribute**
   - **Problem:** Using `#[path = "tests.rs"]` in server.rs resolves submodules relative to parent
   - **Error:** `file not found for module` when submodules declared without explicit paths
   - **Solution:** Use `#[path = "tests/xxx.rs"]` for each submodule in tests.rs
   - **Key rule:** Never use `mod.rs` files - always use file-as-module pattern

### Patterns to Reuse

```rust
// Pattern 1: External test directory with file-as-module pattern
// In server.rs:
#[cfg(test)]
#[path = "tests.rs"]
mod tests;

// In tests.rs (NOT mod.rs!):
#[path = "tests/channels.rs"]
mod channels;
#[path = "tests/links.rs"]
mod links;

// Pattern 2: Conditional re-export for test mocks
#[cfg(test)]
pub use trait_def::MockTelegramClientTrait;

// Pattern 3: Import trait with mock for testing
use crate::telegram::trait_def::{MockTelegramClientTrait, TelegramClientTrait};

// Pattern 4: Module structure for extracted components
// src/telegram.rs
pub mod auth;
pub mod client;
pub mod converters;  // NEW
pub mod trait_def;   // NEW
pub mod types;

pub use client::TelegramClient;
pub use trait_def::TelegramClientTrait;
```

### Files Created/Modified

**Created:**
- `src/mcp/tests.rs` - Test module declarations (file-as-module pattern)
- `src/mcp/tests/server_core.rs` - Server creation tests
- `src/mcp/tests/status.rs` - Status tool tests
- `src/mcp/tests/channels.rs` - Channel tool tests
- `src/mcp/tests/links.rs` - Link tool tests
- `src/mcp/tests/search.rs` - Search tool tests
- `src/telegram/trait_def.rs` - TelegramClientTrait definition
- `src/telegram/converters.rs` - Type conversion helpers
- `src/telegram/tests.rs` - Test module declarations (file-as-module pattern)
- `src/telegram/tests/client_tests.rs` - Client mock tests

**Modified:**
- `src/mcp/server.rs` - Removed inline tests, added path attribute
- `src/telegram/client.rs` - Removed trait definition, converters, and tests
- `src/telegram.rs` - Added new module declarations and re-exports

---

## Project Status: ALL PHASES COMPLETE ✅

### Summary
- ✅ All 13 development phases complete
- ✅ 140 tests passing (4 ignored for CI/CD compatibility)
- ✅ Real grammers integration with SqliteSession
- ✅ CLI with --setup, --session-file, --config options
- ✅ Signal handling (SIGTERM, SIGINT) for graceful shutdown
- ✅ rmcp tool attributes for MCP protocol compliance
- ✅ Code refactoring complete (server.rs: 307 lines, client.rs: 343 lines)
- ✅ Manual testing with real Telegram account - PASSED
- ✅ Manual testing with MCP client (Comet Browser) - PASSED
- ✅ Release build created - `target/release/telegram-mcp`

---

## Bugfix: Environment Variable Expansion for Numeric Fields (2025-12-29)

### Problem
Config file with `api_id = "${TELEGRAM_API_ID}"` failed to parse:
```
invalid type: string "${TELEGRAM_API_ID}", expected i32
```

### Root Cause
Environment variable expansion happened AFTER TOML parsing, but `api_id` is typed as `i32`. The TOML parser saw a string `"${VAR}"` and failed before expansion could occur.

### Solution
1. **Pre-parse expansion**: Expand env vars in raw TOML content BEFORE parsing
2. **Smart numeric unquoting**: When a quoted TOML value is ONLY an env var (e.g., `"${VAR}"`) and the expanded value is purely numeric (digits only), remove the quotes so TOML parses it as an integer

### Implementation (src/config.rs:214-244)
```rust
fn expand_env_vars(value: &str) -> anyhow::Result<String> {
    // ...
    // Check if quoted value is ONLY an env var: "= \"${VAR}\""
    let is_quoted_only_env_var = start >= 1
        && result.as_bytes().get(start - 1) == Some(&b'"')
        && result.as_bytes().get(end + 1) == Some(&b'"');

    // Only unquote if value is purely digits (no +/- signs)
    // This ensures phone numbers like "+1234567890" stay as strings
    let is_pure_integer = !var_value.is_empty()
        && var_value.chars().all(|c| c.is_ascii_digit());

    if is_quoted_only_env_var && is_pure_integer {
        // Replace including quotes: "12345" -> 12345
        result.replace_range((start - 1)..=(end + 1), &var_value);
    } else {
        result.replace_range(start..=end, &var_value);
    }
}
```

### Key Behavior
| Config Value | Env Var Value | Result | Reason |
|-------------|---------------|--------|--------|
| `"${API_ID}"` | `12345` | `12345` | Pure digits → unquoted integer |
| `"${PHONE}"` | `+1234567890` | `"+1234567890"` | Has `+` → stays quoted string |
| `"${HASH}"` | `abc123` | `"abc123"` | Not numeric → stays quoted string |

### Tests Added
- `test_expand_env_vars_numeric_unquoting` - verifies pure numbers are unquoted, phone numbers stay quoted

**Test count:** 139 → 140 tests

---

## Phase 14: Conditional Credential Requirements (2025-12-29)

### Problem
Program required all credentials (api_id, api_hash, phone_number) on every run, but they're only needed during initial setup. After authentication, the session file should be sufficient.

### Solution
Made `api_hash` and `phone_number` optional, while keeping `api_id` required (needed by grammers SenderPool for MTProto connection).

| Field | Normal Mode | Setup Mode |
|-------|-------------|------------|
| `api_id` | ✅ Required | ✅ Required |
| `api_hash` | ❌ Optional | ✅ Required |
| `phone_number` | ❌ Optional | ✅ Required |

### Key Changes

1. **config.rs** - TelegramConfig with optional auth fields:
   ```rust
   pub struct TelegramConfig {
       pub api_id: i32,  // Always required
       pub api_hash: Option<SecretString>,  // Only for --setup
       pub phone_number: Option<SecretString>,  // Only for --setup
       pub session_file: PathBuf,
   }
   ```

2. **config.rs** - New methods:
   - `TelegramConfig::has_auth_credentials()` - checks if api_hash and phone_number are present
   - `TelegramConfig::auth_credentials()` - returns (&str, &str) tuple
   - `Config::validate_for_setup()` - validates auth credentials are present

3. **main.rs** - Flow branching:
   - Setup mode: `config.validate_for_setup()` → create client → authenticate
   - Normal mode: create client → check `is_connected()` → start MCP server

### Custom Deserializer for Optional SecretString
```rust
fn deserialize_optional_secret_string<'de, D>(deserializer: D) -> Result<Option<SecretString>, D::Error> {
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.is_empty()).map(|s| SecretString::new(s.into_boxed_str())))
}
```

### Test Count
140 → 143 tests (+3 new credential tests)

### Usage
```bash
# Setup (requires all credentials)
TELEGRAM_API_ID=... TELEGRAM_API_HASH=... TELEGRAM_PHONE_NUMBER=... \
  cargo run -- --setup --config ./config.toml

# Normal operation (only api_id needed)
TELEGRAM_API_ID=... cargo run -- --config ./config.toml
```

---

## Phase 15: File Logging (2025-12-30)

### Problem
Phase 4 implemented stderr logging only. File logging was deferred and is now needed for production debugging and monitoring. Concerns about logging full message text (size bloat, privacy issues).

### Solution
Implemented dual-layer logging with daily rotation:
- **stderr**: Configurable format (compact/pretty/json)
- **file**: Always JSON format, daily rotation, 7-day retention
- **Search logging**: Message IDs only (not full message text)

### Key Changes

1. **config.rs** - Extended LoggingConfig:
   ```rust
   pub struct LoggingConfig {
       pub level: String,
       pub format: String,
       #[serde(default = "default_file_enabled")]
       pub file_enabled: bool,  // default: true
       #[serde(default = "default_log_path")]
       pub file_path: PathBuf,  // default: ~/.config/telegram-connector/logs/
       #[serde(default = "default_max_log_days")]
       pub max_log_days: u32,   // default: 7
   }
   ```

2. **logging.rs** - Dual layer subscriber:
   - `build_stderr_layer()` - configurable format (compact/pretty/json)
   - `build_file_layer()` - always JSON, daily rotation via `RollingFileAppender`
   - Auto-creates log directory if it doesn't exist

3. **mcp/server.rs** - Smart search logging (line 276-289):
   ```rust
   tracing::info!(
       query = %params.query,
       channel_id = ?params.channel_id.map(|c| c.get()),
       hours_back = params.hours_back,
       limit = params.limit,
       total_found = result.total_found,
       messages_returned = message_ids.len(),
       message_ids = ?message_ids,  // IDs only, NOT full text
       search_time_ms = result.search_time_ms,
       channels_searched = result.query_metadata.channels_searched,
       "Search completed"
   );
   ```

### Design Decisions

1. **Daily rotation vs size-based** - Chose daily rotation (industry standard, well-supported by tracing-appender, easier to correlate logs by date)

2. **Message IDs only** - Log only message IDs, NOT full message text:
   - Prevents log file bloat (messages can be very long)
   - Privacy protection (message content not persisted)
   - Message IDs are sufficient for debugging (can look up if needed)

3. **JSON format for files** - Always JSON for file logs (easier to parse, query, aggregate)

4. **File logging enabled by default** - Sensible defaults for production use

### Files Modified

1. **src/config.rs** - Added file logging fields + 4 new tests
2. **src/logging.rs** - Dual layer implementation + 6 new tests
3. **src/mcp/server.rs** - Search result logging (IDs only)
4. **config.example.toml** - Added file logging options
5. **docs/vision.md** - Updated §8.3-8.4 for daily rotation
6. **docs/tasklist.md** - Marked Phase 15 complete

### Test Count
143 → 153 tests (+10 new logging tests)

### Example Log Entry
```json
{
  "timestamp": "2025-12-30T16:30:00Z",
  "level": "INFO",
  "target": "telegram_connector::mcp::server",
  "message": "Search completed",
  "query": "AI news",
  "total_found": 15,
  "message_ids": [12345, 12346, 12347],
  "channels_searched": 8,
  "search_time_ms": 342
}
```

---

## Phase 16: Media Search Filtering (In Progress)

### Goal
Add optional `media_filter` parameter to `search_messages` tool for server-side filtering by media type.

### Important Context
**Metadata-based filtering, NOT content recognition:**
- `photo` filter returns messages WITH photos attached
- Does NOT search for objects/text inside photos
- No OCR, no speech-to-text, no image recognition

### 16.1 Domain Types (Complete - 2025-12-31)

**What Was Implemented:**

1. **MediaFilter Enum** (src/telegram/types.rs:183-212)
   - 10 variants matching Telegram's InputMessagesFilter:
     - `Photo`, `Video`, `PhotoVideo`, `Document`, `Audio`
     - `Voice`, `VideoNote`, `Gif`, `Url`, `Pinned`
   - Derives: `Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema`
   - Uses `#[serde(rename_all = "snake_case")]` for JSON serialization

2. **SearchParams Update** (src/telegram/types.rs:262-289)
   - Added `media_filter: Option<MediaFilter>` field
   - Updated `new()` and `Default` to initialize with `None`
   - Added documentation comments

3. **Module Exports** (src/telegram.rs)
   - Added `MediaFilter` to public re-exports

4. **Placeholder in MCP Server** (src/mcp/server.rs:267)
   - Added `media_filter: None` with TODO comment for Phase 16.2

**Tests Added (5 new tests):**
- `media_filter_serializes_to_snake_case` - Verifies `PhotoVideo` → `"photo_video"`
- `media_filter_deserializes_from_snake_case` - Verifies reverse
- `media_filter_all_variants_serialize` - Tests all 10 variants
- `media_filter_roundtrip` - Serialization/deserialization cycle
- `search_params_with_media_filter` - SearchParams with filter

**Test Count:** 153 → 158 (+5)

### Patterns Established

```rust
// Pattern 1: MediaFilter enum with snake_case serialization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaFilter {
    Photo,       // "photo"
    PhotoVideo,  // "photo_video"
    VideoNote,   // "video_note"
    // ...
}

// Pattern 2: Optional filter in SearchParams
pub struct SearchParams {
    pub query: String,
    pub channel_id: Option<ChannelId>,
    pub hours_back: u32,
    pub limit: u32,
    pub media_filter: Option<MediaFilter>,  // NEW
}

// Pattern 3: Filter behavior matrix
// Query     | media_filter | Result
// "AI news" | None         | Messages containing "AI news"
// "AI news" | Photo        | Messages with "AI news" AND photo attached
// ""        | Document     | All documents (no text filtering)
// ""        | None         | ❌ Error (too broad)
```

### 16.2 MCP Tool Update (Complete - 2025-12-31)

**What Was Implemented:**

1. **SearchRequest Update** (src/mcp/tools/types.rs:134-155)
   - Added `media_filter: Option<MediaFilter>` field
   - Added `Default` derive for easier test construction
   - Comprehensive schemars description explaining metadata-based filtering

2. **Validation Logic Update** (src/mcp/server.rs:223-229)
   - Allow empty query when `media_filter` is set
   - Reject empty query AND no media_filter (too broad)
   - Updated error message to explain media_filter option

3. **Wiring** (src/mcp/server.rs:270)
   - `media_filter` passed from request to `SearchParams`
   - Added to search completion logging

**Tests Added (5 new tests):**
- `search_request_with_media_filter_deserializes`
- `search_request_media_filter_snake_case`
- `search_request_all_media_filters_deserialize`
- `search_allows_empty_query_with_media_filter`
- `search_passes_media_filter_to_params`

**Test Count:** 158 → 163 (+5)

### 16.3 Telegram Client Implementation (Complete - 2026-01-01)

**What Was Implemented:**

1. **grammers API Research**
   - grammers exposes `.filter()` method on `SearchIter` and `GlobalSearchIter`
   - Filter type: `grammers_client::grammers_tl_types::enums::MessagesFilter`
   - Builder pattern: `.query("text").filter(filter_type)`

2. **Converter Function** (src/telegram/converters.rs:8-22)
   ```rust
   pub fn convert_media_filter(filter: &MediaFilter) -> tl::enums::MessagesFilter {
       match filter {
           MediaFilter::Photo => tl::enums::MessagesFilter::InputMessagesFilterPhotos,
           MediaFilter::Video => tl::enums::MessagesFilter::InputMessagesFilterVideo,
           MediaFilter::PhotoVideo => tl::enums::MessagesFilter::InputMessagesFilterPhotoVideo,
           MediaFilter::Document => tl::enums::MessagesFilter::InputMessagesFilterDocument,
           MediaFilter::Audio => tl::enums::MessagesFilter::InputMessagesFilterMusic,
           MediaFilter::Voice => tl::enums::MessagesFilter::InputMessagesFilterVoice,
           MediaFilter::VideoNote => tl::enums::MessagesFilter::InputMessagesFilterRoundVideo,
           MediaFilter::Gif => tl::enums::MessagesFilter::InputMessagesFilterGif,
           MediaFilter::Url => tl::enums::MessagesFilter::InputMessagesFilterUrl,
           MediaFilter::Pinned => tl::enums::MessagesFilter::InputMessagesFilterPinned,
       }
   }
   ```

3. **Client Update** (src/telegram/client.rs:227-342)
   - Updated validation: allow empty query when `media_filter` is set
   - Applied filter to channel-specific search (line 264-267)
   - Applied filter to global search (line 295-298)
   - Added `media_filter` to search logging (line 337)

**Key Pattern - Applying Filter to Search:**
```rust
// Channel-specific search
let mut search_iter = self.client.search_messages(peer).query(&params.query);
if let Some(ref media_filter) = params.media_filter {
    search_iter = search_iter.filter(convert_media_filter(media_filter));
}

// Global search
let mut search_iter = self.client.search_all_messages().query(&params.query);
if let Some(ref media_filter) = params.media_filter {
    search_iter = search_iter.filter(convert_media_filter(media_filter));
}
```

**Gotcha - Import Path:**
- ❌ `use grammers_tl_types as tl;` - Error: no external crate
- ✅ `use grammers_client::grammers_tl_types as tl;` - Works (re-exported)

**Tests Added (2 new tests):**
- `mock_search_messages_with_media_filter_photo` - Empty query with photo filter
- `mock_search_messages_with_media_filter_document` - Query + document filter

**Test Count:** 163 → 165 (+2)

### 16.3.1 Media Type Detection Fix (2026-01-01)

**Bug Discovered:**
During manual testing (16.4), searching with `media_filter: "video"` returned messages with `media_type: "document"` instead of `"video"`. The server-side filter worked correctly (only videos returned), but the response incorrectly labeled the media type.

**Root Cause:**
In `src/telegram/converters.rs:119-123`, the `convert_message` function always defaulted to `MediaType::Document` whenever any media was present, instead of properly detecting the actual type:

```rust
// BUG: Always returned "document" for any media
let (has_media, media_type) = if msg.media().is_some() {
    (true, MediaType::Document) // <-- BUG HERE
} else {
    (false, MediaType::None)
};
```

**Fix Implemented:**

1. **Added `convert_media_to_type()` function** (src/telegram/converters.rs:25-41)
   - Maps grammers `Media` enum variants to our `MediaType`:
   - `Media::Photo` → `MediaType::Photo`
   - `Media::Sticker` → `MediaType::Sticker`
   - `Media::Contact` → `MediaType::Contact`
   - `Media::Poll` → `MediaType::Poll`
   - `Media::Geo` / `Media::GeoLive` → `MediaType::Location`
   - `Media::Venue` → `MediaType::Venue`
   - `Media::Dice` → `MediaType::Dice`
   - `Media::WebPage` → `MediaType::None` (not considered media)
   - `Media::Document` → Delegates to `detect_document_type()`

2. **Added `detect_document_type()` helper** (src/telegram/converters.rs:44-89)
   - Inspects `DocumentAttribute` to determine document subtype:
   - `DocumentAttributeVideo` + `round_message: true` → `MediaType::VideoNote`
   - `DocumentAttributeVideo` → `MediaType::Video`
   - `DocumentAttributeAudio` + `voice: true` → `MediaType::Voice`
   - `DocumentAttributeAudio` → `MediaType::Audio`
   - `DocumentAttributeAnimated` → `MediaType::Animation`
   - MIME type `image/gif` → `MediaType::Animation`
   - Default → `MediaType::Document`

3. **Updated `convert_message()`** (src/telegram/converters.rs:185-189)
   ```rust
   // FIX: Properly detect media type
   let (has_media, media_type) = match msg.media() {
       Some(media) => (true, convert_media_to_type(&media)),
       None => (false, MediaType::None),
   };
   ```

**Key Insight - grammers Media Structure:**
- Videos, audio, GIFs, voice messages are all wrapped in `Media::Document`
- Must inspect `DocumentAttribute` variants to distinguish them
- Photos and stickers have dedicated `Media::Photo` and `Media::Sticker` variants

**Pattern - Detecting Document Subtype:**
```rust
fn detect_document_type(doc: &Document) -> MediaType {
    let raw_doc = match &doc.raw.document {
        Some(tl::enums::Document::Document(d)) => d,
        _ => return MediaType::Document,
    };

    for attr in &raw_doc.attributes {
        match attr {
            tl::enums::DocumentAttribute::Video(v) => {
                return if v.round_message { MediaType::VideoNote } else { MediaType::Video };
            }
            tl::enums::DocumentAttribute::Audio(a) => {
                return if a.voice { MediaType::Voice } else { MediaType::Audio };
            }
            tl::enums::DocumentAttribute::Animated => return MediaType::Animation,
            _ => {}
        }
    }
    MediaType::Document
}
```

**Test Count:** 165 → 167 (+2 from earlier additions, fix verified with existing tests)

### Remaining Work

- **16.4 Integration Testing** - ✅ Bug found and fixed during manual testing
- **16.5 Documentation** - Update README with media filter examples (already done per CLAUDE.md)

---

## Phase 17: Get Recent Messages (Complete)

**Date:** 2026-01-03

### Problem Statement

The current `search_messages` tool requires a query string (or `media_filter`) to work. Empty queries are rejected because Telegram's search API doesn't support them. However, users need a way to retrieve "all recent messages from channel X in the last N hours" without searching for specific text.

### Solution Implemented

**grammers API Methods Used:**
- `iter_messages(peer)` - Iterates message history in reverse chronological order without requiring a search query

### What Was Built

1. **HistoryParams** (src/telegram/types.rs:297-345)
   - `channel_id: ChannelId` - Required channel
   - `hours_back: u32` - Default: 48, max: 168 (7 days)
   - `limit: u32` - Default: 20, max: 100
   - `media_filter: Option<MediaFilter>` - Client-side filtering
   - Builder methods: `hours_back()`, `limit()`, `media_filter()`

2. **TelegramClientTrait Extension** (src/telegram/trait_def.rs:15-19)
   ```rust
   async fn get_recent_messages(&self, params: &HistoryParams) -> Result<SearchResult, Error>;
   ```

3. **TelegramClient Implementation** (src/telegram/client.rs:358-445)
   - Finds channel in dialogs by ID
   - Uses `iter_messages(peer)` for history iteration
   - Time filter: breaks when `msg.date() < cutoff_time`
   - Client-side media filtering via `matches_media_filter()`
   - Returns `SearchResult` for consistency

4. **matches_media_filter() helper** (src/telegram/converters.rs:91-123)
   - Matches grammers `Message` against `MediaFilter`
   - Handles all media types including Url and Pinned

5. **GetRecentMessagesRequest** (src/mcp/tools/types.rs:196-213)
   - `channel_id: String` - Required (ID or username)
   - `hours_back: Option<u32>`
   - `limit: Option<u32>`
   - `media_filter: Option<MediaFilter>`

6. **MCP Tool** (src/mcp/server.rs:300-378)
   - `#[tool]` attribute for rmcp compliance
   - Username resolution via `get_channel_info()`
   - Rate limiting integration
   - Delegates to `TelegramClient::get_recent_messages()`

### Key Implementation Patterns

**Client-side media filtering:**
```rust
if params
    .media_filter
    .as_ref()
    .is_some_and(|filter| !matches_media_filter(&msg, filter))
{
    continue;
}
```

**Username resolution in MCP tool:**
```rust
let channel_id = if let Ok(id_num) = request.channel_id.parse::<i64>() {
    ChannelId::new(id_num)?
} else {
    // Username provided - resolve via get_channel_info
    let channel = self.telegram_client.get_channel_info(&request.channel_id).await?;
    channel.id
};
```

### Tests Added (19 new tests)

- **5 HistoryParams tests** (types.rs)
- **5 mock client tests** (client_tests.rs)
- **3 GetRecentMessagesRequest tests** (tools/types.rs)
- **6 MCP tool tests** (tests/history.rs)

**Test count:** 167 → 186 (+19)

---

## Phase 18: Comprehensive Refactoring (Complete)

### What Was Implemented

**Goal:** Split large files into focused modules, eliminate duplication, create shared test helpers.

### 18.1 Shared Test Helpers

Created `src/test_helpers.rs` with factory functions for test fixtures:

```rust
// Factory functions reduce test code duplication
pub fn create_test_message(id: i64, text: &str, channel_id: i64) -> Message
pub fn create_test_message_with_media(id: i64, text: &str, channel_id: i64, media: MediaType) -> Message
pub fn create_test_message_with_sender(id: i64, text: &str, channel_id: i64, sender: &str) -> Message
pub fn create_test_channel(id: i64, username: &str) -> Channel
pub fn create_test_channel_detailed(id: i64, username: &str, title: &str, members: i64, verified: bool) -> Channel
pub fn create_test_search_result(messages: Vec<Message>) -> SearchResult
pub fn create_empty_search_result() -> SearchResult
```

### 18.2 Telegram Types Extraction

Split `telegram/types.rs` (865 lines) into 5 focused submodules:

| File | Lines | Contents |
|------|-------|----------|
| `types/ids.rs` | 185 | ChannelId, MessageId, UserId with validation |
| `types/names.rs` | 168 | Username, ChannelName validated strings |
| `types/media.rs` | 152 | MediaType enum, MediaFilter enum |
| `types/entities.rs` | 131 | Message, Channel domain entities |
| `types/params.rs` | 235 | SearchParams, HistoryParams, SearchResult, QueryMetadata |
| `types.rs` | 24 | Module declarations + re-exports |

**Pattern:** Each submodule contains its types + inline tests for that type.

### 18.3 MCP Tools Types Extraction

Split `mcp/tools/types.rs` (366 lines) into 3 focused submodules:

| File | Lines | Contents |
|------|-------|----------|
| `types/requests.rs` | 224 | 6 request structs with JsonSchema |
| `types/responses.rs` | 109 | 4 response structs |
| `types/serde_helpers.rs` | 82 | Custom deserializer for empty string handling |
| `types.rs` | 18 | Module declarations + re-exports |

### 18.4 MCP ID Parsing Helpers

Created `src/mcp/tools/helpers.rs` to eliminate duplicated ID parsing in server.rs:

```rust
pub fn parse_channel_id(id_str: &str) -> Result<ChannelId, String>
pub fn parse_message_id(id: i64) -> Result<MessageId, String>
pub fn parse_optional_channel_id(id_str: &Option<String>) -> Result<Option<ChannelId>, String>
```

**Before:** ID parsing duplicated in 4 tool methods
**After:** Single source of truth with tests

### Key Design Decisions

1. **Keep tests inline with submodules**
   - Each submodule has its own `#[cfg(test)] mod tests`
   - Keeps tests close to implementation
   - No separate test file needed

2. **Re-export pattern for API stability**
   - Parent module re-exports all public types
   - External code uses same imports as before
   - Zero breaking changes

3. **No mod.rs files**
   - Maintained file-as-module pattern
   - e.g., `types.rs` declares submodules, not `types/mod.rs`

### Files Created

| File | Lines | Purpose |
|------|-------|---------|
| `src/test_helpers.rs` | 206 | Test fixture factories |
| `src/telegram/types/ids.rs` | 185 | ID wrapper types |
| `src/telegram/types/names.rs` | 168 | Name validated types |
| `src/telegram/types/media.rs` | 152 | Media enums |
| `src/telegram/types/entities.rs` | 131 | Domain entities |
| `src/telegram/types/params.rs` | 235 | Search/History params |
| `src/mcp/tools/helpers.rs` | 121 | ID parsing helpers |
| `src/mcp/tools/types/requests.rs` | 224 | Request types |
| `src/mcp/tools/types/responses.rs` | 109 | Response types |
| `src/mcp/tools/types/serde_helpers.rs` | 82 | Custom deserializers |

### Results

| Metric | Before | After |
|--------|--------|-------|
| Largest file | 865 lines | 381 lines |
| telegram/types.rs | 865 lines | 24 lines |
| mcp/tools/types.rs | 366 lines | 18 lines |
| ID parsing duplication | 4x | 1x |
| Test count | 186 | 209 (+23) |

### Lessons Learned

1. **Re-export pattern is essential** - Changing module structure without breaking public API requires careful re-exports at each level.

2. **Inline tests work well for submodules** - No need to create separate test files when submodules are small and focused.

3. **Test helpers reduce duplication** - Factory functions like `create_test_message()` make tests more readable and DRY.

4. **File-as-module pattern scales** - Even with nested submodules, avoiding `mod.rs` keeps structure clear.

---

## Phase 19: Log Cleanup (Complete - 2026-01-10)

### What Was Implemented

1. **cleanup_old_logs() Function** (src/logging.rs:122-158)
   - Removes log files older than `max_log_days` configuration
   - Called on application startup after logging initialization
   - Returns count of files removed

2. **Startup Integration** (src/main.rs:26-31)
   - Called after `logging::init()`
   - Uses let chains for clean conditional: `if let Ok(removed) = ... && removed > 0`
   - Only logs when files are actually removed (avoids noise)

3. **Documentation Updates**
   - README.md: Updated feature description to mention automatic cleanup
   - config.example.toml: Added "(old logs cleaned on startup)" note

### Final Implementation

```rust
// src/logging.rs
pub fn cleanup_old_logs(config: &LoggingConfig) -> anyhow::Result<usize> {
    if !config.file_enabled || config.max_log_days == 0 {
        return Ok(0);
    }

    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(u64::from(config.max_log_days) * 86400);

    let entries = match std::fs::read_dir(&config.file_path) {
        Ok(e) => e,
        Err(_) => return Ok(0),
    };

    let mut removed = 0;

    for entry in entries.flatten() {
        let path = entry.path();

        // Match files containing ".log" (handles telegram-connector.log.YYYY-MM-DD)
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !file_name.contains(".log") {
            continue;
        }

        if let Ok(metadata) = entry.metadata()
            && let Ok(modified) = metadata.modified()
            && modified < cutoff
            && std::fs::remove_file(&path).is_ok()
        {
            removed += 1;
        }
    }

    Ok(removed)
}
```

### Key Decisions & Rationale

1. **Startup cleanup vs background task**
   - **Choice:** Cleanup on startup only
   - **Why:** KISS principle - simple, catches most cases, no complex shutdown handling
   - **Trade-off:** Won't clean if app crashes repeatedly before startup completes

2. **File matching: contains(".log") vs extension check**
   - **Initial:** Used `path.extension() == Some("log")`
   - **Problem:** tracing_appender names files `telegram-connector.log.YYYY-MM-DD` where extension is the date
   - **Fix:** Changed to `file_name.contains(".log")` to match the pattern

3. **Let chains for nested conditions**
   - **Pattern:** `if let Ok(x) = ... && let Ok(y) = ... && condition`
   - **Why:** Clippy requires collapsing nested if-let statements in Rust 2024
   - **Benefit:** Cleaner, more idiomatic code

### Gotchas & Edge Cases

1. **tracing_appender file naming**
   - **Problem:** Expected files like `app.log`, got `telegram-connector.log.2025-01-01`
   - **Issue:** `.extension()` returns `"2025-01-01"`, not `"log"`
   - **Solution:** Use `file_name.contains(".log")` instead

2. **Clippy collapsible_if warning**
   - **Error:** Nested `if let` statements must be collapsed
   - **Solution:** Use let chains with `&&`

### Tests Added (6 tests)

- `cleanup_skipped_when_file_disabled` - Returns 0 when file_enabled=false
- `cleanup_skipped_when_max_days_zero` - Returns 0 when max_log_days=0
- `cleanup_handles_missing_directory` - Returns 0, doesn't error
- `cleanup_removes_old_log_files` - Removes old files, keeps recent
- `cleanup_ignores_non_log_files` - Only removes .log files
- `cleanup_handles_empty_directory` - Returns 0 for empty dir

### Dependencies Added

- `filetime = "0.2"` (dev-dependency) - For setting file modification times in tests

### Patterns to Reuse

```rust
// Pattern 1: Let chains for multiple conditions
if let Ok(metadata) = entry.metadata()
    && let Ok(modified) = metadata.modified()
    && modified < cutoff
{
    // action
}

// Pattern 2: Graceful directory iteration
let entries = match std::fs::read_dir(&path) {
    Ok(e) => e,
    Err(_) => return Ok(0),
};

// Pattern 3: Filename pattern matching
let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
if !file_name.contains(".log") {
    continue;
}
```

### Results

| Metric | Value |
|--------|-------|
| Tests added | 6 |
| Total tests | 215 (5 ignored) |
| Files modified | 6 |
| Phases complete | 19/19 |

### Status

✅ Complete - All subtasks done, documentation updated

---

## Logging Test Extraction (2026-01-10)

### Problem

`src/logging.rs` had grown to 561 lines, with ~400 lines being tests. This violated the project's pattern of keeping source files focused and extracting tests to separate files.

### Solution

Following the established pattern from Phase 13 (MCP server and Telegram client refactoring), extracted all tests to a separate file using the `#[path]` attribute.

### What Was Done

1. **Created `src/logging_tests.rs`** (299 lines)
   - All 25 logging tests organized by category:
     - Phone Number Redaction Tests (5)
     - API Hash Redaction Tests (5)
     - Initialization Tests (3)
     - File Layer Tests (6)
     - Log Cleanup Tests (6)

2. **Updated `src/logging.rs`** (162 lines, down from 561)
   - Replaced inline test module with path attribute:
   ```rust
   #[cfg(test)]
   #[path = "logging_tests.rs"]
   mod tests;
   ```

### Results

| File | Before | After |
|------|--------|-------|
| `src/logging.rs` | 561 lines | 162 lines |
| `src/logging_tests.rs` | (new) | 299 lines |

### Pattern Used

```rust
// In logging.rs - reference external test file
#[cfg(test)]
#[path = "logging_tests.rs"]
mod tests;

// In logging_tests.rs - import from parent module
use super::*;

#[test]
fn test_name() {
    // test code
}
```

### Verification

- All 215 tests pass (5 ignored)
- Clippy clean (no warnings)
- Consistent with project patterns from Phase 13

---

## Debug Log Cleanup in telegram/client.rs (2026-02-14)

### Problem

During the `structuredContent` / Cyrillic debugging session, extensive `tracing::info!` logging was added to `src/telegram/client.rs` to trace every step of channel resolution. The root cause was fixed (`String` vs `Json<T>` serialization), but the diagnostic logs remained — creating noisy output on every normal operation.

### What Was Done

Cleaned up `src/telegram/client.rs` — removed 11 verbose `info!` traces, kept all `error!`/`warn!` logs:

**`get_subscribed_channels`** (3 changes):
- Removed entry-point `info!` log ("Starting get_subscribed_channels")
- Removed `total_dialogs` counter (declaration + increment) — only existed for diagnostic logging
- Downgraded completion log from `info!` to `debug!`, removed `total_dialogs` field

**`get_channel_info`** (5 removals):
- Removed 5 step-by-step `info!` traces: entry point, `@` prefix resolution, numeric ID resolution, bare username resolution, peer-resolved intermediate step
- Kept all `error!`/`warn!` logs (fire only on failures)

**`get_recent_messages`** (3 removals):
- Removed 3 step-by-step `info!` traces: username resolution entry, username resolved success, dialog search entry
- Kept `warn!` logs for fallback paths, `error!` for dialog iteration failure
- Kept `identifier` field on the completion `info!` log (useful context, not diagnostic noise)

### Lesson Learned

**Log level discipline:** When adding diagnostic logging to debug an issue, use `debug!` or `trace!` level — not `info!`. This avoids the need for cleanup after the fix. Reserve `info!` for operation completions and significant state changes. Reserve `error!`/`warn!` for failures and fallbacks.

### Verification

- All 215 tests pass (5 ignored)
- `cargo fmt --check` clean
- `cargo clippy -- -D warnings` clean
- No behavioral changes — only log output reduced

---

## Phase 20: Hang Diagnostics & Grammers Timeouts (Complete)

### Trigger

Recurring MCP request timeouts since 2026-05-20 — server stops responding for 5–10 min, then resumes. Logs showed a successful `@ai_newz` call at 15:05:11, then no log entries at all until id 15 was cancelled at 15:09:35 (Claude.ai's own 5-min timeout). The MTProto socket was alive — grammers would have logged `marking all N request(s) as failed` if it had died — so a single in-flight grammers call (`resolve_username` / `iter_messages.next()` / `search_iter.next()`) was stalling without being bounded. Tool handlers only logged on completion, so the hung request's tool and arguments were invisible.

### What Was Implemented

1. **`TimeoutConfig`** (`src/config.rs`)
   - New struct attached to `TelegramConfig` as `timeouts: TimeoutConfig`.
   - Three fields: `resolve_secs` (30), `history_secs` (60), `search_secs` (120). All optional with serde defaults.
   - `validate()` rejects zero values; called from `Config::load_from` after parsing.
   - TOML key: `[telegram.timeouts]`.

2. **`Error::Timeout { operation, secs }`** (`src/error.rs`)
   - New typed variant. Display: `operation '{op}' timed out after {secs}s`.

3. **`with_timeout` helper** (`src/telegram/client.rs:28`)
   - `pub(crate) async fn with_timeout<F, T>(operation: &str, secs: u64, fut: F) -> Result<T, Error>` where `F: Future<Output = Result<T, Error>>`.
   - Wraps `tokio::time::timeout`; on elapsed returns `Error::Timeout` carrying the call-site operation name.
   - **Test pattern (TDD):** unit tests in `src/telegram/tests/timeout_tests.rs` use `#[tokio::test(start_paused = true)]` + `tokio::time::advance` to drive the timeout deterministically. No real network. Three tests: completes-in-budget, propagates-inner-error, elapsed-returns-typed-error.

4. **All grammers call sites wrapped** — every site identified in `docs/phase-20-plan.md` §3, plus the resolve/dialog-walk paths inside `get_message_by_id` (same hang exposure as the other tools; strict superset of the plan):
   - `get_channel_info`: `resolve_username` (@ + bare) → `resolve_secs`; numeric-ID `iter_dialogs` → `resolve_secs`.
   - `search_messages`: single-channel `iter_dialogs` + `search_iter` walk → `search_secs`; global `search_all_messages` → `search_secs`.
   - `get_recent_messages`: `resolve_username` → `resolve_secs`; `iter_dialogs` fallback → `resolve_secs`; `iter_messages` walk → `history_secs`.
   - `get_message_by_id`: numeric-ID `iter_dialogs` + `resolve_username` → `resolve_secs`; `get_messages_by_id` fetch → `history_secs`.
   - For multi-iteration walks the budget is **total elapsed time across all `next().await` iterations**, not per-iteration — the entire `while let Some(...)` block lives inside one `with_timeout`.
   - `get_subscribed_channels` is **not** wrapped — internal pagination only, not user-driven. (Re-evaluate if it shows up in future hang logs.)

5. **Tool entry logging** (`src/mcp/server.rs`)
   - `tracing::info!(tool = "...", ...args, "Tool invocation started")` at the top of all 8 `#[tool]` methods. Args mirror the existing completion logs. Deliberately `info!` (not `debug!`) so next time something hangs the entry log is visible without changing config.
   - Trade-off: log volume roughly doubles (entry + completion per call). Acceptable; 7-day rotation already enforced.

### Patterns & Decisions

- **All grammers calls bounded by `tokio::time::timeout` via `with_timeout`.** Budgets live in `TimeoutConfig` keyed by call type (resolve / history / search), not per-tool. Three global knobs, no tool-specific overrides — keeps the surface small.
- **No retries.** Timeout → return `Error::Timeout` to MCP client. Claude can decide whether to re-invoke. Retries would mask the underlying problem and burn rate-limiter tokens.
- **`TelegramClient` owns its `timeouts: TimeoutConfig`** (cloned in `new()`), so call sites can read budgets without passing them through every method signature.
- **TDD discipline applied:** every change had a failing test first — `default_timeout_config` returning 30/60/120, `[telegram.timeouts]` partial/full override via `toml::from_str::<TelegramConfig>`, zero-value validation (one test per field), `Error::Timeout` Display format, and the three `with_timeout` behaviours.
- **Did not wrap `get_subscribed_channels`'s internal `iter_dialogs`.** Trade-off: it's only called from one tool with bounded pagination (`limit` + `offset`). If it shows up as a hang source in future logs, wrap with `resolve_secs`.
- **Connection-reset (`os error 54`) cosmetic log noise is still out of scope.** Grammers auto-reconnects on next request; this phase only addresses request-level hangs, not idle-socket churn.

### Tests: +34 (215 → 249)

- 8 new `config` tests (defaults, partial/full override, validation).
- 1 new `error::tests::test_timeout_error_display`.
- 3 new `telegram::tests::timeout_tests` (`with_timeout` behaviour under `tokio::time::pause()`).
- Existing 215 still pass unchanged — tool entry logging is side-effect-only and no test depended on log absence.
- Total: **249 passing, 5 ignored.** Run with `cargo test`; config tests serial via `cargo test config -- --test-threads=1`.

### Verification

- `cargo fmt --check` clean
- `cargo clippy --all-targets -- -D warnings` clean
- `cargo test` → 249/249
- `cargo test config -- --test-threads=1` → 36/36
- Manual hang simulation deferred to first real-world incident (per plan §"Verification Checklist"): a future hang now surfaces as `Error::Timeout { operation, secs }` within the configured budget, with the tool name + args visible in the entry log.

### Files Changed

- `src/config.rs` — `TimeoutConfig`, defaults, `validate()`, `TelegramConfig.timeouts` field, validation wired into `Config::load_from`.
- `src/config/tests.rs` — 8 new tests + `create_test_config` helper updated to populate `timeouts`.
- `src/error.rs` — `Error::Timeout` variant + display test.
- `src/telegram/client.rs` — `with_timeout` helper; `TelegramClient.timeouts` field; all 4 trait methods updated to wrap their grammers calls.
- `src/telegram/tests.rs` + `src/telegram/tests/timeout_tests.rs` — new test module.
- `src/mcp/server.rs` — entry log at top of all 8 `#[tool]` methods.
- `config.example.toml` — documented `[telegram.timeouts]` section.
- `CHANGELOG.md`, `docs/tasklist.md`, `docs/memory.md` — Phase 20 entries.

---

## Phase 21: Flexible Scalar Coercion (Complete)

### What Was Implemented

Some MCP clients send scalar arguments in the "wrong" JSON type — a numeric string `"10"` where a `u32` is expected, a JSON number `123` where a string is expected, or `"true"`/`1` where a bool is expected. Strict serde deserialization rejected these payloads *before* any tool code ran, surfacing as an opaque invalid-params error. Phase 21 makes every cross-type scalar field on the request structs tolerant of the alternate JSON form, without changing field types or the advertised JSON schema.

1. **Five reusable `deserialize_with` helpers** (`src/mcp/tools/types/serde_helpers.rs`) — added alongside the pre-existing `deserialize_optional_media_filter`, reusing the same `#[serde(untagged)]` inner-enum technique:
   - `flexible_opt_u32` → `Option<u32>`: JSON number or trimmed numeric string; empty/whitespace/`null`/missing → `None`; float/negative/out-of-range/garbage → error.
   - `flexible_i64` → `i64` (required): JSON number or trimmed numeric string; empty/garbage/missing/`null` → error.
   - `flexible_string` → `String` (required): JSON string as-is (incl. `""`), or integer number stringified (`123` → `"123"`); float → error.
   - `flexible_opt_string` → `Option<String>`: string or integer-stringified; empty/whitespace/`null`/missing → `None`.
   - `flexible_opt_bool` → `Option<bool>`: real bool, `1`/`0`, or `"true"`/`"false"`/`"1"`/`"0"` (case-insensitive, trimmed); empty/`null`/missing → `None`; other ints/strings → error.

2. **Wired onto 17 fields across 7 request structs** (`src/mcp/tools/types/requests.rs`) via `#[serde(deserialize_with = "...")]`. Optional fields also get `#[serde(default)]` (missing → `None`); required fields get no `default` (missing → error). `media_filter` fields left on `deserialize_optional_media_filter`.

### Patterns & Decisions

- **Leniency lives at the transport boundary, not the domain.** Chose boundary deserializers (a serde anti-corruption layer) over a DDD newtype wrapper. Rationale: these values (`limit`/`offset`/`hours_back`/`message_id`) are transport params immediately unwrapped and validated downstream in `params.rs`; the real domain types (`ChannelId`, `MessageId`) already exist deeper in and are built in the tool body via `parse_*`. A newtype would have forced a hand-written `JsonSchema` impl per wrapper and edits to every tool body + test.
- **Field types unchanged ⇒ JSON schema unchanged.** `schemars` derives from the field *type*, ignoring `#[serde(deserialize_with)]`. Numeric fields still advertise `integer`, strings `string`, bools `boolean` — we *tolerate* the alternate form without *inviting* it. This was an explicit goal and is verified by the diff containing no field-type or `#[schemars]` changes.
- **`#[serde(default)]` required for optional `deserialize_with` fields.** Adding `deserialize_with` makes serde call the helper for present values including `null`; without `default`, a *missing* field would error. Optional fields therefore pair `default` + `deserialize_with` (same as the existing `media_filter`). Required fields deliberately omit `default` so missing → error.
- **Untagged-enum variant order matters.** `flexible_string`/`flexible_opt_string` declare `Str` before `Int`; `flexible_opt_bool` declares `Bool, Int, Str`. serde_json's typed tokens never cross-match (a JSON string never deserializes as `i64`, etc.), so ordering is safe, but the order is kept "primary type first" for readability.
- **Pragmatic coercion semantics** (chosen over strict): trim numeric strings; empty string → `None` for optional / error for required; bool accepts `1`/`0` and `"true"/"false"/"1"/"0"`; String fields stringify an integer number; JSON floats for integer fields → error (consistent with the advertised `integer` schema — a known, accepted limitation).
- **Server tool bodies and the domain layer were not touched.** `server.rs` still reads `request.limit.unwrap_or(20)` etc. at the same types.
- **TDD discipline:** every helper and every wiring point landed test-first (helper compile-fail → green), reviewed per-task with two-stage (spec + quality) review.

### Tests: +56 (249 → 305)

- ~46 helper unit tests in `serde_helpers.rs` (per helper: native type, numeric/alternate string, whitespace trim, empty→None/error, `null`, missing, float/garbage/negative → error, all bool forms).
- 13 struct-level wiring tests in `requests.rs` exercising the full deserialization stack for each request type (`"limit":"10"`, `channel_id` as number, `message_id` as string, bool as string, etc.). Pre-existing `requests` tests (plain number/string forms) still pass unchanged.
- Total: **305 passing, 5 ignored.**

### Verification

- `cargo fmt --check` clean
- `cargo clippy --all-targets -- -D warnings` clean
- `cargo test -- --test-threads=1` → 305/305 (5 ignored)
- Diff audited to confirm no field type or `#[schemars(description=...)]` line changed ⇒ advertised JSON schema preserved.

### Files Changed

- `src/mcp/tools/types/serde_helpers.rs` — 5 new `flexible_*` helpers + their unit tests.
- `src/mcp/tools/types/requests.rs` — `#[serde(deserialize_with=...)]` on 17 fields across 7 structs; grouped helper import; struct-level wiring tests.
- `CHANGELOG.md`, `docs/tasklist.md`, `docs/memory.md` — Phase 21 entries.
- `docs/superpowers/specs/2026-05-31-flexible-scalar-coercion-design.md`, `docs/superpowers/plans/2026-05-31-flexible-scalar-coercion.md` — design spec + implementation plan.

## Dependency Audit & Update (2026-06-12)

### Outcome

- Audited every direct dependency against crates.io (`max_stable_version`). All were already at their latest stable versions except **chrono 0.4.44 → 0.4.45** (patch bump applied to `Cargo.toml` + `Cargo.lock`).
- grammers git pin (`master#fa7692e4`) was already the latest upstream master commit — nothing to update there.

### Lesson: full `cargo update` is currently BROKEN — use targeted `cargo update -p <crate>` instead

- `grammers-crypto` (git master) requires `glass_pumpkin ^1.7.0`. The lockfile holds `glass_pumpkin 1.10.0`, which has since been **yanked**; every non-yanked `glass_pumpkin` ≤1.9.0 depends on `core2 ^0.4`, and **all versions of `core2` are yanked** from crates.io.
- Consequence: any operation that freshly re-resolves the grammers subgraph (blanket `cargo update`, deleting `Cargo.lock`) fails with `failed to select a version for the requirement core2 = "^0.4"`. The build only works because Cargo honors the already-locked (yanked) `glass_pumpkin 1.10.0`.
- **Do not delete `Cargo.lock`.** Update individual crates with `cargo update -p <name>` until upstream ships a fix (e.g. a re-published glass_pumpkin without `core2`, or `2.0` stable + a grammers bump).
- `cargo update -p thiserror` is ambiguous (lockfile has transitive 1.0.69 + our 2.0.18); use `thiserror@2.0.18` if ever needed.

### Verification

- `cargo fmt --check` clean; `cargo clippy -- -D warnings` clean; `cargo test` → 331 passed, 0 failed.

---

## Phase 22: Get Message Media (Complete)

### Decision: get_message_media returns Result<CallToolResult, String>

**Date:** 2026-06-12

The project convention "all tools return `Result<String, String>`" is a project-level convention, **not** an rmcp constraint. rmcp's actual requirement is that the return type implements `IntoCallToolResult`. The standard `Result<T, E>` composition holds: `Err(String)` still becomes an `is_error: true` text content block via the trait. `get_message_media` returns `Result<CallToolResult, String>` because an MCP image content block (base64-encoded JPEG bytes) cannot be expressed as a plain JSON string — it must be a `ContentBlock::Image` inside a `CallToolResult`.

### Server-side size selection algorithm

Telegram stores each photo in several pre-generated sizes (grammers `PhotoSize`). `size_candidates()` (converters.rs) extracts downloadable variants (`Size`/`Cached`/`Progressive`; `Empty`/`Stripped`/`Path` are skipped) into the grammers-free `SizeCandidate` struct, and the pure `select_size_candidate()` picks the smallest variant whose longest side >= `max_dimension`, falling back to the largest available. This minimizes download bytes while guaranteeing the requested resolution is met. Ties on longest side pick the first candidate (pinned by test).

For video/animation/video notes: the same rule runs over `document.thumbs()` (also `PhotoSize` values — there is no separate thumbnail type). No usable variant → `Error::DownloadFailed("no downloadable size variant available")`.

The selected variant is refused if its reported byte size exceeds 20 MB (`Error::MediaTooLarge`), and the streaming loop re-checks accumulated bytes against the same cap because reported sizes are untrusted input. `PhotoSize::Cached` needs no network call — grammers `iter_download` short-circuits via `to_data()`.

### ResponseBuffer stub for large payloads

Responses larger than `[observability] max_buffered_payload_bytes` (default 262144 = 256 KiB) are stored in the `get_last_responses` ring buffer with their payload replaced by `OVERSIZED_PAYLOAD_STUB` (`{"omitted":"payload exceeded max_buffered_payload_bytes"}` — valid JSON so the tool can embed it); `size_bytes` keeps the real wire size. This prevents megabyte-sized image responses from pinning memory in the ring buffer and from being replayed as text by `get_last_responses`.

### Tests: +39 (331 → 370)

- Selector tests in `converters.rs` (smallest-sufficient, fallback-largest, empty, portrait, tie-break, single candidate).
- Image pipeline tests in `src/mcp/tools/image.rs` (`process_image`: downscale dimensions, no-upscale guard, base64/JPEG round-trip, payload-cap shrink loop, decode error) using the `create_test_jpeg` fixture in `test_helpers.rs`.
- New `src/mcp/tests/media.rs` (10 mock-based tool tests: photo blocks, video thumbnail, no-media error, oversize, both clamp bounds, cost charging, rate-limit short-circuit, decode error, metadata/size consistency); mock conformance test in `src/telegram/tests/client_tests.rs`; stub integration test in `src/mcp/tests/last_responses.rs`; config defaults/parse/validation tests.
- Total: **370 lib tests passing, 5 ignored.**

### Files Changed

- `Cargo.toml` — added `image` (0.25, jpeg-only features) and `base64` (0.22).
- `src/error.rs` — new `Error::MediaTooLarge`, `Error::NoVisualMedia`, `Error::DownloadFailed` variants.
- `src/config.rs` — `[rate_limiting] media_download_cost` (default 5); `[telegram.timeouts] download_secs` (default 120, validated > 0); `[observability] max_buffered_payload_bytes` (default 262144).
- `src/telegram/types/media.rs` — `MediaDownload`, `SizeCandidate` domain types.
- `src/telegram/converters.rs` — `size_candidates()`, `select_size_candidate()`.
- `src/telegram/trait_def.rs` — `download_message_media` method on `TelegramClientTrait`.
- `src/telegram/client.rs` — `download_message_media` implementation; `resolve_peer` extracted from `get_message_by_id` and shared; `download_secs` timeout applied.
- `src/mcp/tools/image.rs` — `process_image`/`ProcessedImage`: decode, Lanczos3 downscale, JPEG q80 re-encode, base64, iterative shrink under `MAX_BASE64_LEN` (1.5 MB).
- `src/mcp/tools/types/requests.rs` — `GetMessageMediaRequest` (flexible scalar coercion).
- `src/mcp/tools/types/responses.rs` — `GetMessageMediaResponse` (12-field metadata block).
- `src/mcp/server.rs` — `get_message_media` tool (tool 10); `media_download_cost` field + `with_media_download_cost` builder; `log_tool_outcome` generalized to `<T>`.
- `src/mcp/observability.rs` — `ResponseBuffer::push` stubs payloads above `max_buffered_payload_bytes` (two-arg constructor).
- `src/main.rs` — `.with_media_download_cost(config.rate_limiting.media_download_cost)` wired in.

## Refactor: architecture & duplication (AD-2 … AD-6)

Followed `docs/refactoring/02-architecture-and-duplication.md`; AD-1 (one
`resolve_peer`) and the LM-* splits were already done. One commit per finding,
gate green per step. Tests 432 → 437.

- **AD-5** — `json_response<T: Serialize>` in `mcp/tools/helpers.rs` (re-exported via `mcp/tools`) replaces ~11 `serde_json::to_string(..).map_err(|e| e.to_string())` tails in the server impl modules.
- **AD-4 / CQ-1** — `peer_identity(&Peer) -> Option<(ChannelId, ChannelName, Username)>` in `converters/channel.rs` shared by `convert_peer_to_channel` and `convert_message`; one `fallback_username(kind)` using `.expect()` replaces five bare `Username::new(..).unwrap()`.
- **AD-2** — `get_recent_messages` no longer double-resolves usernames. Server stops the `get_channel_info` pre-call and hands the raw identifier through; `HistoryParams.channel_id` is now `Option<ChannelId>` (`None` for a username). Client owns resolution and derives the id from the resolved peer for logging.
- **AD-3** — tool-wrapper boilerplate collapsed into a `ToolInvocation` guard (`start`/`finish`), not a macro.
- **AD-6** — `[telegram] max_download_bytes` (20 MiB) and `[transcription] {default,max}_timeout_seconds` (30/120) are now config, `#[serde(default)]`. Internal consts (JPEG_QUALITY, POLL_INTERVAL_SECS, base64 cap, image dimensions) stay; the 500-char preview cap is now `LINK_PREVIEW_DESCRIPTION_MAX_CHARS`.

### Lessons (non-obvious)

- **`Username::new` requires 5–32 chars, so the `"user"` fallback sentinel (4 chars) was a latent panic.** `convert_message`'s `Peer::User` branch did `Username::new("user").unwrap()` — it would panic for any user-peer message without a public username. CQ-1's premise that "the `Username` literals are statically valid" is false. Resolution: a user with no username now reuses the valid `"unknown"` sentinel (`"unknown"`/`"group"` unchanged). Any sentinel routed through `Username::new` must satisfy the 5–32-char rule.
- **AD-3's declarative-macro option cannot compose with rmcp.** A `macro_rules!` emitting a `#[tool]` method in *item position* inside the `impl` expands *after* `#[tool_router]` (an attribute macro) has already scanned the impl body, so the generated tool never registers in `list_tools`/`call_tool`. The non-fragile guard object was chosen instead.
- **Configurable clamp bound:** with a config-driven max, `value.clamp(1, max)` panics if `max == 0` (violates `min <= max`). Use `value.min(max).max(1)` instead.

## Refactor: Phase A hygiene (CQ-2, CQ-3, CQ-6) — 2026-06-20

Closed out the remaining Phase A items from `docs/refactoring/04-roadmap.md`
(CQ-1 and AD-5 were done in the earlier AD sweep; LM-1 covered Phase B). One
commit per finding, gate green per step, test count unchanged at 437.

- **CQ-3** — removed `dashmap` (zero usages) and `tokio-test` from deps.
- **CQ-2** — deleted the empty `apply_defaults()` and its `load_from` call; `config` no longer needs `mut`.
- **CQ-6** — `CLAUDE.md` "10 tools" → 11 (×2); same drift fixed in the gitignored local instruction files (`.claude/rules/ast-index.md`, `.claude/skills/project-conventions/SKILL.md`); declared `tasklist.md` the single source of truth for phase/test counts and reframed memory.md's per-phase list as historical.

### Lessons (non-obvious)

- **`tokio-test` was unused as a crate but was transitively supplying tokio's `test-util` feature.** Five tests (`tests/timeout_tests.rs` ×4, `transcription.rs` ×2) use `#[tokio::test(start_paused = true)]` / `time::advance`, which require `test-util`. That feature is **intentionally excluded from tokio's `full`** (it swaps in a pausable clock), so it must be requested explicitly. Naively deleting `tokio-test` broke the build with `no method named start_paused on Builder`. Fix: drop the unused crate and add `tokio = { features = ["test-util"] }` to `[dev-dependencies]` — Cargo unifies it into test builds while release builds (which skip dev-deps) stay clean. Lesson: before removing a "dead" dev-dep, check whether it's feeding a feature into a shared crate; `cargo check` won't warn, only a full test build reveals it.
- **`.claude/` is gitignored here**, so edits to `.claude/rules/*` and `.claude/skills/*` take effect locally but can never be committed — don't claim them in a commit body, and don't `git add -f` against the team's deliberate ignore policy.

## Refactor: Phase D data honesty (CQ-4, CQ-5) — 2026-06-20

Closed the final two roadmap items, completing the whole refactoring roadmap
(`docs/refactoring/04-roadmap.md`: every LM/AD/CQ finding now done). TDD,
one commit per finding. Tests 439 → 441.

- **CQ-4** — `Channel.member_count` is now `Option<u64>` (None → JSON null). The converter (`convert_peer_to_channel`) emitted a hardcoded `0`, indistinguishable from an empty channel; it now emits `None`. Scope: optional-fields-only (user chose this over the full `getFullChannel` fetch in `get_channel_info`, which adds untestable network surface). **Breaking** schema change — documented in CHANGELOG + README.
- **CQ-5** — `get_subscribed_channels` over-fetches `limit + 1` and sets `has_more = returned > limit`, then truncates. Replaces `has_more = len >= limit`, which falsely advertised a next page at an exact-multiple boundary.

### Lessons / notes (non-obvious)

- **`convert_peer_to_channel` is effectively un-unit-testable**: it takes a grammers `peer::Peer`, which can't be fabricated in a unit test (no public constructor; that's *why* the whole stack is mock-driven at the trait boundary). So CQ-4's converter behavior (`None`) is guarded only indirectly — a type-level serialization test (`entities.rs`) and a mock-boundary test through `get_channel_info` (`channels.rs`). The converter line itself rides on the compiler. Don't reach for a converter unit test here; there's no Peer fixture.
- **`description` was *already* always `null`** in practice (the converter hardcodes `description: None` on every path), so the README example showing a description string was pre-existing drift. CQ-4 was the moment to make the README honest for both fields, not just `member_count`.
- **Over-fetch is a server-boundary fix, not a client change.** The trait method already accepts an arbitrary `limit`; passing `limit + 1` + truncating keeps the client/trait/mock signatures untouched. Cost: the two existing pinned-arg mock tests (`.with(eq(20))` / `eq(10)`) had to move to `eq(21)` / `eq(11)` — that's the new contract, not test-fudging. Use `saturating_add(1)` so `limit == u32::MAX` can't overflow.

## Dependency rescue: grammers → Codeberg, yanked core2/glass_pumpkin — 2026-08-10

Bare `cargo update` had been silently broken for the whole project; only the
committed `Cargo.lock` kept builds alive. Root cause was a two-yank collision
outside our tree, fixed by moving the grammers git deps to their new upstream.
Gate green, tests 441 (no count change — pure dependency/boundary work).

- **The yank chain:** grammers-crypto 0.8 requires `glass_pumpkin ^1.7` (used in
  exactly one line: `safe_prime::check` for SRP 2FA). glass_pumpkin 1.7–1.9
  depend on `core2 ^0.4`, and **every version of core2 is yanked**. 1.10.0
  dropped core2 but was itself yanked (it broke semver by bumping rand_core in a
  minor) and was republished as 2.0.0-rc0 — outside `^1.7`. Net: no resolvable
  version existed; any fresh resolve failed, while the lock pinning the yanked
  1.10.0 still built. A lockfile can mask a dead dependency graph for months.
- **grammers left GitHub in Feb 2026.** `github.com/Lonami/grammers` is a stale
  mirror (last commit fa7692e, "Migrate off GitHub") and "may be deleted".
  Real upstream: `https://codeberg.org/Lonami/grammers` — master already had the
  glass_pumpkin fix (grammers-crypto 0.10). **Never point the git deps back at
  github.com**, and don't assume a git dep's GitHub URL is alive just because
  cargo still fetches it.
- **Fallback bridge exists:** `nimec77/grammers` (GitHub fork of the mirror,
  branch `fix/glass-pumpkin-yanked-core2`) carries the minimal one-line fix at
  the old commit — useful only if Codeberg master ever regresses badly; safe to
  delete once 0.10 has soaked.
- **grammers 0.8.1 → 0.10.0 API churn survived at the boundary** (domain model
  untouched): `Peer::to_ref` now `Result<Option<PeerRef>, _>` (map_err +
  ok_or_else, five call sites); `PeerId::bare_id` now `Option<i64>` (`None` only
  for the self-user sentinel — converters `?` it away, comparisons wrap in
  `Some(..)`); message dates are jiff `Timestamp` — **domain stays chrono**,
  compare/convert via `.as_second()` so jiff never becomes a direct dependency
  (Telegram dates are second-precision; nothing lost); new `Peer::Community`
  kind mapped like a group (no username accessor → `fallback_username("group")`);
  `User::is_premium` removed — read `u.premium` by matching the now-public
  `me.raw` TL enum.
- **glass_pumpkin 2.0.0-rc0 is a pre-release pinned by grammers**, not by us; it
  will move to `"2.0.0"` upstream when stable. Nothing to do on our side.

### Post-review round (same day)

- **grammers deps are now pinned by `rev`, not `branch = "master"`.** Review
  caught that the old master-tracking policy silently relied on the GitHub
  mirror being frozen (a de-facto pin); Codeberg master is active and shipped
  two breaking bumps in five months, so tracking it would re-create the
  "fresh resolve breaks" failure this change fixed. Upgrades = deliberate rev
  bump of all three crates together (see Cargo.toml comment + CLAUDE.md).
- **The June "Peer is un-unit-testable" lesson is stale as of grammers 0.10:**
  `MemorySession::default()` + destructured `SenderPool::new` (runner never
  spawned) + `Client::new(handle)` does no I/O, and `Community::from_raw` /
  TL structs are public — see `community_peer()` in
  `src/telegram/converters/channel.rs` tests. Converter tests are writable now;
  don't cite the old excuse.
- **The Community bug was a two-match trap:** `peer_identity` got a Community
  arm but `convert_peer_to_channel`'s separate flags-match had a `_` catch-all
  that silently dropped it (messages visible, channel invisible). The catch-all
  is now an explicit `Peer::User(_)` arm so the next new grammers peer kind
  fails to compile instead of vanishing at runtime. Lesson: when a dependency
  adds an enum variant, grep every `match` on that enum — and prefer exhaustive
  matches over `_` for foreign enums that grow.
- **Boundary helpers own the churn now:** `peer_to_ref` (client.rs) is the
  single `to_ref` error-mapping site (distinguishes session error vs.
  cache-miss in logs), and `message_timestamp` (converters/message.rs) is the
  single jiff→chrono site — cutoff comparisons are back to full-precision
  chrono ordering, undoing the ≤1 s window widening the first pass introduced.
