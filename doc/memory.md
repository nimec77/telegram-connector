# Development Memory - Telegram MCP Connector

**Last Updated:** Phase 3 Complete (2025-12-16)

---

## Current Status

**Progress:** 3/12 phases complete
- ✅ Phase 1: Project Setup
- ✅ Phase 2: Error Types (8/8 tests)
- ✅ Phase 3: Configuration (16/16 tests)
- ⬜ Phase 4: Logging (next)

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

Following doc/workflow.md cycle:
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

## Next Phase: Phase 4 - Logging

**Goal:** Structured async logging with sensitive data redaction

**Key Components:**
- `tracing` subscriber setup
- stderr + file output with rotation
- `redact_phone()`, `redact_hash()` helpers
- Log levels and formats from config

**Estimated Complexity:** Medium (existing patterns in vision.md §8)
