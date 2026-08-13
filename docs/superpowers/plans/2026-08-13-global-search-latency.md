# Global Search Latency Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make unscoped `search_messages` predictable by pushing its time window to Telegram instead of enforcing it client-side, and bound every search with a deadline that returns partial results rather than an error.

**Architecture:** `messages.SearchGlobal` already carries `min_date`/`max_date` parameters that the pager hardcodes to `0`, so the client walks the entire global index backwards at 100 messages per round trip and discards everything outside the window. Wiring both bounds removes the walk. A separate `SearchBudget` unit adds a wall-clock deadline plus the page/message counters that make an expensive call legible in the response.

**Tech Stack:** Rust 2024 nightly, `grammers` (pinned Codeberg rev), `rmcp` v3.1, `schemars` v1, `tokio`, `mockall`, `thiserror`/`anyhow`.

**Spec:** `docs/superpowers/specs/2026-08-13-global-search-latency-design.md`

## Global Constraints

- **Pre-merge gate:** `cargo fmt --check && cargo clippy -- -D warnings && cargo test` must pass. Run `cargo fmt --all` after every code change.
- **Config tests run serial:** `cargo test config -- --test-threads=1` (env var mutation races otherwise).
- **Never `unwrap()`** in production code — use `?` or `.context("...")`. `expect()` only in tests.
- **TDD:** the failing test comes first. No production code without a preceding test.
- **Line length 100 chars.**
- **Backward compatible:** no existing field renamed, retyped, or removed. New response fields are omitted when false/absent, so today's responses stay byte-identical.
- **Never log** phone numbers, API hashes, passwords, or session tokens.
- **Deviation from the work order, deliberate:** the work order names the knob `search_deadline_seconds` in `[search]`. This plan uses `deadline_seconds` in `[search]` — the table already supplies the `search.` prefix, and `search.search_deadline_seconds` stutters. Same knob, same default, same table.

---

### Task 1: Server-side date bounds on the global pager

**This task is the empirical gate for the whole plan.** Step 8 runs the live probe. If `document`/24h/limit-20 does not drop from ~45 s to under a second, **stop and report** — the fallback is the `continue` → `break` change described in the spec, and the rest of this plan changes shape.

**Files:**
- Modify: `src/telegram/client/raw_pager.rs` (add `window_bounds` free fn + `RawGlobalSearchPager::window`; pager struct at `:349`, `new()` at `:361`)
- Modify: `src/telegram/client/ops_search.rs:167` (global branch pager construction)
- Test: `src/telegram/client/raw_pager.rs` (`mod tests` at `:485`)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `fn window_bounds(from: DateTime<Utc>, to: Option<DateTime<Utc>>) -> (i32, i32)` returning `(min_date, max_date)`; `RawGlobalSearchPager::window(self, from: DateTime<Utc>, to: Option<DateTime<Utc>>) -> Self`.

**Why a free function:** `RawGlobalSearchPager::new` requires a live grammers `Client`, which cannot be constructed in a unit test. The existing tests in this file (`input_peer_for_message`, `advance_search_offsets`) test free functions for exactly this reason. Keep the date arithmetic pure and testable; `window()` becomes a trivial two-field assignment.

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `src/telegram/client/raw_pager.rs`:

```rust
#[test]
fn window_bounds_maps_both_ends() {
    let from = DateTime::from_timestamp(1_700_000_000, 0).expect("valid ts");
    let to = DateTime::from_timestamp(1_700_086_400, 0).expect("valid ts");
    let (min_date, max_date) = window_bounds(from, Some(to));
    assert_eq!(min_date, 1_700_000_000);
    assert_eq!(max_date, 1_700_086_400);
}

#[test]
fn window_bounds_open_upper_end_is_unbounded_sentinel() {
    let from = DateTime::from_timestamp(1_700_000_000, 0).expect("valid ts");
    let (min_date, max_date) = window_bounds(from, None);
    assert_eq!(min_date, 1_700_000_000);
    // 0 is the protocol's "no upper bound", not "the epoch".
    assert_eq!(max_date, 0);
}

#[test]
fn window_bounds_clamps_pre_epoch_lower_end_to_unbounded() {
    let from = DateTime::from_timestamp(-86_400, 0).expect("valid ts");
    let (min_date, max_date) = window_bounds(from, None);
    // A degraded bound costs latency; a rejected search costs the caller
    // their result. Degrade.
    assert_eq!(min_date, 0);
    assert_eq!(max_date, 0);
}

#[test]
fn window_bounds_clamps_beyond_i32_range() {
    // Past 2038: saturates instead of wrapping into a negative i32, which
    // would silently widen the window to everything.
    let from = DateTime::from_timestamp(i32::MAX as i64 + 1_000, 0).expect("valid ts");
    let (min_date, _) = window_bounds(from, None);
    assert_eq!(min_date, i32::MAX);
}
```

Add `use chrono::DateTime;` to the test module's imports if not already present.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib raw_pager::tests::window_bounds`
Expected: FAIL — `cannot find function 'window_bounds' in this scope`

- [ ] **Step 3: Write the implementation**

Add near the other free helpers in `src/telegram/client/raw_pager.rs` (alongside `advance_search_offsets`), and add `use chrono::{DateTime, Utc};` to the file's imports if absent:

```rust
/// Map a time window onto `messages.SearchGlobal`'s `min_date`/`max_date`.
///
/// The TL schema types both as `int` (i32 unix seconds) and treats `0` as
/// "unbounded". Out-of-range instants clamp rather than error: a degraded
/// bound costs a slower search, a rejected one costs the caller their result.
/// The client-side window filter in `ops_search` stays in place either way.
fn window_bounds(from: DateTime<Utc>, to: Option<DateTime<Utc>>) -> (i32, i32) {
    let clamp = |ts: i64| ts.clamp(0, i32::MAX as i64) as i32;
    (clamp(from.timestamp()), to.map_or(0, |t| clamp(t.timestamp())))
}
```

Then add the builder to `impl RawGlobalSearchPager`, next to `query()` and `filter()`:

```rust
/// Bound the search server-side. Without this the pager walks the entire
/// global index backwards discarding out-of-window results — measured at
/// 44.86 s for a rare media filter over a 24 h window.
pub(super) fn window(mut self, from: DateTime<Utc>, to: Option<DateTime<Utc>>) -> Self {
    let (min_date, max_date) = window_bounds(from, to);
    self.request.min_date = min_date;
    self.request.max_date = max_date;
    self
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib raw_pager::tests::window_bounds`
Expected: PASS (4 tests)

- [ ] **Step 5: Wire it into the global search branch**

In `src/telegram/client/ops_search.rs`, replace the pager construction at `:167`:

```rust
let mut pager = RawGlobalSearchPager::new(&self.client).query(&params.query);
```

with:

```rust
// Bound the search server-side. The client-side window checks below are
// retained as defense in depth: they cost nothing once the server honors
// these bounds, and keep the result correct if it ever does not.
let mut pager = RawGlobalSearchPager::new(&self.client)
    .query(&params.query)
    .window(cutoff_time, params.to_date);
```

**Do not touch the `continue` statements at `:178-185`.** Removing them is a separate, riskier change the spec explicitly declines.

- [ ] **Step 6: Run the full gate**

Run: `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all pass, no test count regression.

- [ ] **Step 7: Build the probe binary**

Run: `cargo build --release`
Expected: builds `target/release/telegram-mcp`. It reads the same config and authenticated session as the deployed binary (`~/Library/Application Support/telegram-connector/`), so no extra setup is needed. Run it only when no other instance is live — they share one SQLite session file.

- [ ] **Step 8: GATE — verify Telegram honors the bounds**

Run:

```bash
python3 scripts/mcp_probe.py target/release/telegram-mcp scripts/probes/search-latency.json
```

Compare against the pre-fix baseline recorded in the spec:

| case | before | target |
|---|---|---|
| global `document` 24h, limit 20 | 44.86 s | **< 1 s** |
| global `voice` 72h, limit 20 | 8.07 s | < 1 s |
| global `video_note` 24h, limit 20 | 12.93 s | < 1 s |
| global `url` 24h, limit 20 | 0.46 s | ~unchanged |

Also confirm the result *sets* did not change: `document` 24h must still return 6 messages, `video_note` 24h still 1. A faster search that returns different results is a correctness regression, not a win.

**If latency does not improve, STOP.** Telegram is not honoring `min_date`; report the probe output and revisit the spec's fallback before continuing.

- [ ] **Step 9: Commit**

```bash
git add src/telegram/client/raw_pager.rs src/telegram/client/ops_search.rs
git commit -m "fix: bound global search server-side with min_date/max_date

messages.SearchGlobal carries min_date/max_date and the pager hardcoded
both to 0, so the time window was enforced entirely client-side while
Telegram streamed the global index backwards at 100/page. A rare media
filter over a narrow window walked to exhaustion: document/24h/limit-20
measured 44.86 s to return 6 messages.

Client-side window checks are retained as defense in depth."
```

---

### Task 2: `SearchBudget` — deadline and work counters

**Files:**
- Create: `src/telegram/client/search_budget.rs`
- Modify: `src/telegram/client.rs` (add `mod search_budget;` beside the other `client/` submodule declarations)

**Interfaces:**
- Consumes: nothing.
- Produces: `pub(crate) struct SearchBudget` with `new(deadline_secs: u64) -> Self`, `expired(&mut self) -> bool`, `record_page(&mut self, messages_in_page: usize)`, `pages_fetched(&self) -> u32`, `messages_scanned(&self) -> u64`, `timed_out(&self) -> bool`.

**Why `tokio::time::Instant`:** `tokio::time::pause()` controls it, so deadline tests are deterministic and never sleep — the same technique `src/telegram/tests/timeout_tests.rs` already uses. `std::time::Instant` is not affected by pause and would force real sleeps.

**Why `expired(&mut self)`:** checking the deadline is what latches `timed_out`, so the caller cannot forget to set the flag it just acted on.

- [ ] **Step 1: Write the failing tests**

Create `src/telegram/client/search_budget.rs` containing only the test module for now:

```rust
//! Wall-clock budget and work counters for a search accumulation loop.

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time;

    #[tokio::test(start_paused = true)]
    async fn budget_is_not_expired_within_its_window() {
        let mut budget = SearchBudget::new(20);
        time::advance(Duration::from_secs(19)).await;
        assert!(!budget.expired());
        assert!(!budget.timed_out());
    }

    #[tokio::test(start_paused = true)]
    async fn budget_expires_and_latches_timed_out() {
        let mut budget = SearchBudget::new(20);
        time::advance(Duration::from_secs(21)).await;
        assert!(budget.expired());
        assert!(budget.timed_out(), "checking expiry must latch the flag");
    }

    #[tokio::test(start_paused = true)]
    async fn timed_out_stays_latched_after_a_later_non_expired_check() {
        // Guards against a caller re-checking and clearing the flag.
        let mut budget = SearchBudget::new(20);
        time::advance(Duration::from_secs(21)).await;
        assert!(budget.expired());
        assert!(budget.timed_out());
        assert!(budget.timed_out());
    }

    #[test]
    fn counters_accumulate_across_pages() {
        let mut budget = SearchBudget::new(20);
        budget.record_page(100);
        budget.record_page(37);
        assert_eq!(budget.pages_fetched(), 2);
        assert_eq!(budget.messages_scanned(), 137);
    }

    #[test]
    fn fresh_budget_reports_no_work_done() {
        let budget = SearchBudget::new(20);
        assert_eq!(budget.pages_fetched(), 0);
        assert_eq!(budget.messages_scanned(), 0);
        assert!(!budget.timed_out());
    }

    #[tokio::test(start_paused = true)]
    async fn zero_deadline_is_treated_as_disabled_not_instantly_expired() {
        // Config validation rejects 0, but a disabled budget must never
        // return zero results if one ever reaches here.
        let mut budget = SearchBudget::new(0);
        time::advance(Duration::from_secs(3600)).await;
        assert!(!budget.expired());
    }
}
```

Register the module — in `src/telegram/client.rs`, beside the existing `mod raw_pager;` / `mod ops_search;` declarations:

```rust
mod search_budget;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib search_budget`
Expected: FAIL — `cannot find type 'SearchBudget' in this scope`

- [ ] **Step 3: Write the implementation**

Add above the test module in `src/telegram/client/search_budget.rs`:

```rust
use std::time::Duration;
use tokio::time::Instant;

/// Wall-clock budget for a search accumulation loop, plus the work counters
/// reported back to the caller.
///
/// Bounds the *loop*, not an individual round trip — a single hung MTProto
/// call is `[telegram.timeouts] search_secs`' job. On expiry the loop returns
/// what it gathered; it never turns a slow-but-working search into an error,
/// because partial results are strictly more useful than a failure.
///
/// Uses `tokio::time::Instant` so `tokio::time::pause()` drives it in tests.
pub(crate) struct SearchBudget {
    /// `None` disables the deadline entirely.
    deadline: Option<Instant>,
    timed_out: bool,
    pages_fetched: u32,
    messages_scanned: u64,
}

impl SearchBudget {
    pub(crate) fn new(deadline_secs: u64) -> Self {
        Self {
            deadline: (deadline_secs > 0)
                .then(|| Instant::now() + Duration::from_secs(deadline_secs)),
            timed_out: false,
            pages_fetched: 0,
            messages_scanned: 0,
        }
    }

    /// True once the budget is spent. Latches `timed_out`, so a caller cannot
    /// act on expiry without the response reporting it.
    pub(crate) fn expired(&mut self) -> bool {
        if self.deadline.is_some_and(|d| Instant::now() >= d) {
            self.timed_out = true;
        }
        self.timed_out
    }

    /// Record one round trip and the messages it returned.
    pub(crate) fn record_page(&mut self, messages_in_page: usize) {
        self.pages_fetched = self.pages_fetched.saturating_add(1);
        self.messages_scanned = self.messages_scanned.saturating_add(messages_in_page as u64);
    }

    pub(crate) fn pages_fetched(&self) -> u32 {
        self.pages_fetched
    }

    pub(crate) fn messages_scanned(&self) -> u64 {
        self.messages_scanned
    }

    pub(crate) fn timed_out(&self) -> bool {
        self.timed_out
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib search_budget`
Expected: PASS (6 tests)

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add src/telegram/client/search_budget.rs src/telegram/client.rs
git commit -m "feat: SearchBudget deadline and work counters

Bounds the search accumulation loop rather than an individual round trip,
and carries the page/message counters that make an expensive search
legible in its own response. tokio::time::Instant so pause() drives the
deadline tests deterministically."
```

---

### Task 3: `[search] deadline_seconds` config

**Files:**
- Modify: `src/config.rs` (`SearchConfig` at `:141`; validation call site at `:290`)
- Modify: `src/config/defaults.rs` (defaults live here, referenced via `use defaults::*`)
- Modify: `src/telegram/client.rs:65-75` (struct field), `src/telegram/client/lifecycle.rs:14,48-56` (constructor)
- Modify: `src/main.rs:42,51` (both `TelegramClient::new` call sites)
- Modify: `config.example.toml` (`[search]` table at `:37`)
- Test: `src/config/tests.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `SearchConfig::deadline_seconds: u64` (default 20); `SearchConfig::validate(&self) -> anyhow::Result<()>`; `TelegramClient::new(config: &TelegramConfig, search: &SearchConfig)`; private field `TelegramClient::search_deadline_secs: u64`.

**Threading note:** `TelegramClient::new` currently takes only `&TelegramConfig`, which cannot see the `[search]` table. The signature gains a second parameter rather than moving the knob into `[telegram.timeouts]` — the work order specifies `[search]`, and that is where a caller tuning search behavior will look.

**The default of 20 is the work order's estimate, not a measurement.** It ships configurable and is hardcoded nowhere.

- [ ] **Step 1: Write the failing tests**

Add to `src/config/tests.rs`, following the `limits_config_rejects_zero_budget` idiom at `:712`:

```rust
#[test]
fn search_deadline_defaults_to_twenty_seconds() {
    let config: Config =
        toml::from_str("[telegram]\napi_id = 123\n").expect("parse");
    assert_eq!(config.search.deadline_seconds, 20);
}

#[test]
fn search_config_rejects_zero_deadline() {
    let config: Config =
        toml::from_str("[telegram]\napi_id = 123\n\n[search]\ndeadline_seconds = 0\n")
            .expect("parse");
    assert!(config.search.validate().is_err());
}

#[test]
fn search_config_accepts_explicit_deadline() {
    let config: Config =
        toml::from_str("[telegram]\napi_id = 123\n\n[search]\ndeadline_seconds = 45\n")
            .expect("parse");
    assert_eq!(config.search.deadline_seconds, 45);
    assert!(config.search.validate().is_ok());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test config -- --test-threads=1`
Expected: FAIL — no field `deadline_seconds` on `SearchConfig`

- [ ] **Step 3: Add the config field, default, and validation**

In `src/config/defaults.rs`, beside `default_max_results_limit`:

```rust
pub(crate) fn default_search_deadline_seconds() -> u64 {
    20
}
```

In `src/config.rs`, extend `SearchConfig` (`:141`):

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    #[serde(default = "default_hours_back")]
    pub default_hours_back: u32,
    #[serde(default = "default_max_results_default")]
    pub max_results_default: u32,
    #[serde(default = "default_max_results_limit")]
    pub max_results_limit: u32,
    /// Wall-clock budget for a search's accumulation loop. On expiry the
    /// search returns the results gathered so far with `timed_out`/`partial`
    /// set — never an error. Must stay below `[telegram.timeouts] search_secs`
    /// (default 120) to be reachable.
    #[serde(default = "default_search_deadline_seconds")]
    pub deadline_seconds: u64,
}

impl SearchConfig {
    /// Reject a zero deadline — it would end every search before its first page.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.deadline_seconds == 0 {
            anyhow::bail!("search.deadline_seconds must be > 0");
        }
        Ok(())
    }
}
```

In `src/config.rs`, add to the validation chain after the `limits` block (`:290-293`):

```rust
config
    .search
    .validate()
    .context("invalid search configuration")?;
```

- [ ] **Step 4: Thread it to the client**

In `src/telegram/client.rs`, add the field beside `max_download_bytes` (`:71`):

```rust
/// Wall-clock budget for a search accumulation loop (`[search] deadline_seconds`).
search_deadline_secs: u64,
```

In `src/telegram/client/lifecycle.rs`, change the signature at `:14` and the struct literal at `:48`:

```rust
pub async fn new(config: &TelegramConfig, search: &SearchConfig) -> Result<Self, Error> {
```

```rust
Ok(Self {
    client,
    session,
    session_path: config.session_file.clone(),
    timeouts: config.timeouts.clone(),
    max_download_bytes: config.max_download_bytes,
    search_deadline_secs: search.deadline_seconds,
    premium: tokio::sync::RwLock::new(None),
    _runner_handle: runner_handle,
})
```

Add `SearchConfig` to the config imports in `src/telegram/client.rs` so `use super::*` carries it into `lifecycle.rs`.

Update both call sites in `src/main.rs` (`:42` and `:51`):

```rust
let telegram_client = TelegramClient::new(&config.telegram, &config.search)
```

- [ ] **Step 5: Document it in `config.example.toml`**

Replace the `[search]` block at `:37`:

```toml
[search]
# Optional: Search defaults
# default_hours_back = 48                  # Default: 48
# max_results_default = 20                 # Default: 20
# max_results_limit = 100                  # Default: 100

# Wall-clock budget for a search's accumulation loop. On expiry the search
# returns the results gathered so far with "timed_out": true and
# "partial": true in query_metadata — never an error, because partial
# results beat a failed workflow. Must be > 0.
#
# Keep this BELOW [telegram.timeouts] search_secs (default 120), or the hard
# timeout fires first and the call fails instead of degrading. The two are not
# cross-validated: each table is independently overridable, and a hard coupling
# would surprise anyone tuning one without the other.
#
# CONSERVATIVE STARTING POINT, NOT A MEASURED VALUE — chosen to sit below
# typical MCP client timeouts. Tune against your own traffic.
# deadline_seconds = 20                    # Default: 20
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test config -- --test-threads=1` then `cargo test`
Expected: PASS. Config tests **must** be serial — they mutate env vars.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all && cargo clippy -- -D warnings
git add src/config.rs src/config/defaults.rs src/config/tests.rs \
        src/telegram/client.rs src/telegram/client/lifecycle.rs \
        src/main.rs config.example.toml
git commit -m "feat: [search] deadline_seconds config knob

Default 20s — the work order's estimate, chosen to sit below typical MCP
client timeouts rather than derived from measurement. Configurable,
validated non-zero, hardcoded nowhere. TelegramClient::new gains a
&SearchConfig parameter: the knob belongs in [search], where someone
tuning search behavior will look, and [telegram] cannot see that table."
```

---

### Task 4: `QueryMetadata` gains the four reporting fields

**Files:**
- Modify: `src/telegram/types/params.rs:176-188` (`QueryMetadata`)
- Modify: `src/test_helpers.rs:209-227` (`create_test_search_result`)
- Modify: `src/mcp/tools/fanout.rs:39-51,86-92` (`merge_results`)
- Test: `src/telegram/types/params.rs` (`mod tests` at `:191`), `src/mcp/tests/multi_channel.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `QueryMetadata.timed_out: bool`, `.partial: bool`, `.pages_fetched: u32`, `.messages_scanned: u64`; `fn is_false(b: &bool) -> bool` (serde skip helper, private to `params.rs`).

**Serialization contract:** both bools carry `#[serde(default, skip_serializing_if = "is_false")]`. `skip_serializing_if` keeps today's responses byte-identical; `default` keeps them deserializable when absent and marks them optional in the generated JSON schema. `std::ops::Not::not` cannot be used — serde passes `&bool`, and `not` takes `bool` by value.

**Why both flags:** `timed_out` is the cause, `partial` the consequence. They co-occur today; they stay distinct because `partial` is the field a caller checks generically, leaving room for byte-budget truncation to set it later without falsely claiming a timeout.

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `src/telegram/types/params.rs`:

```rust
#[test]
fn query_metadata_omits_false_flags_from_json() {
    let meta = QueryMetadata {
        query: "test".to_string(),
        window_from: Utc::now(),
        window_to: None,
        channels_scanned: None,
        channels_in_results: 0,
        timed_out: false,
        partial: false,
        pages_fetched: 3,
        messages_scanned: 300,
    };
    let json = serde_json::to_string(&meta).expect("serializes");
    assert!(!json.contains("timed_out"), "false flags stay off the wire");
    assert!(!json.contains("partial"), "false flags stay off the wire");
    assert!(json.contains("\"pages_fetched\":3"));
    assert!(json.contains("\"messages_scanned\":300"));
}

#[test]
fn query_metadata_emits_true_flags() {
    let meta = QueryMetadata {
        query: "test".to_string(),
        window_from: Utc::now(),
        window_to: None,
        channels_scanned: None,
        channels_in_results: 0,
        timed_out: true,
        partial: true,
        pages_fetched: 9,
        messages_scanned: 900,
    };
    let json = serde_json::to_string(&meta).expect("serializes");
    assert!(json.contains("\"timed_out\":true"));
    assert!(json.contains("\"partial\":true"));
}

#[test]
fn query_metadata_deserializes_without_the_new_fields() {
    // A payload written by an older server must still parse.
    let json = r#"{"query":"q","window_from":"2026-08-13T00:00:00Z",
                   "channels_scanned":null,"channels_in_results":2}"#;
    let meta: QueryMetadata = serde_json::from_str(json).expect("parses");
    assert!(!meta.timed_out);
    assert!(!meta.partial);
    assert_eq!(meta.pages_fetched, 0);
    assert_eq!(meta.messages_scanned, 0);
}
```

Add to `src/mcp/tests/multi_channel.rs`:

```rust
#[test]
fn fanout_merge_sums_counters_and_ors_flags() {
    use crate::mcp::tools::fanout::{ChannelFetchOutcome, merge_results};

    let mut clean = create_test_search_result(vec![], "q", 0);
    clean.query_metadata.pages_fetched = 2;
    clean.query_metadata.messages_scanned = 150;

    let mut degraded = create_test_search_result(vec![], "q", 0);
    degraded.query_metadata.pages_fetched = 5;
    degraded.query_metadata.messages_scanned = 500;
    degraded.query_metadata.timed_out = true;
    degraded.query_metadata.partial = true;

    let merged = merge_results(
        vec![
            ChannelFetchOutcome { channel: "a".into(), result: Ok(clean) },
            ChannelFetchOutcome { channel: "b".into(), result: Ok(degraded) },
        ],
        20,
        "q".to_string(),
        Utc::now(),
        None,
    )
    .expect("merge succeeds");

    // Summed, not dropped — a caller must see the whole fan-out's cost.
    assert_eq!(merged.query_metadata.pages_fetched, 7);
    assert_eq!(merged.query_metadata.messages_scanned, 650);
    // One degraded channel degrades the merged result.
    assert!(merged.query_metadata.timed_out);
    assert!(merged.query_metadata.partial);
}
```

Add whatever `use` lines that file needs (`chrono::Utc`, `crate::test_helpers::create_test_search_result`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib query_metadata fanout_merge_sums`
Expected: FAIL — `struct 'QueryMetadata' has no field named 'timed_out'`

- [ ] **Step 3: Extend the struct**

In `src/telegram/types/params.rs`, replace `QueryMetadata` (`:175-188`):

```rust
/// The window and scope a query actually executed with (work-order B6/B7).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryMetadata {
    pub query: String,
    /// Effective window start actually applied (from_date, or now - hours_back).
    pub window_from: DateTime<Utc>,
    /// Effective upper bound; omitted when the window is open-ended.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub window_to: Option<DateTime<Utc>>,
    /// Channels the search actually scanned; `null` when unknowable
    /// (server-side global search).
    pub channels_scanned: Option<u32>,
    /// Distinct channels present in `messages`.
    pub channels_in_results: u32,
    /// The search hit `[search] deadline_seconds` and stopped early. Omitted
    /// when false, so unaffected responses are unchanged on the wire.
    #[serde(default, skip_serializing_if = "is_false")]
    pub timed_out: bool,
    /// The result set is known-incomplete. Today only the deadline sets this;
    /// it is distinct from `timed_out` so future truncation causes can report
    /// incompleteness without claiming a timeout.
    #[serde(default, skip_serializing_if = "is_false")]
    pub partial: bool,
    /// Round trips issued to Telegram for this search.
    #[serde(default)]
    pub pages_fetched: u32,
    /// Raw messages walked, including those filtered out. Together with
    /// `pages_fetched` this is what makes an expensive call legible: a caller
    /// who cannot see a cost cannot budget for it.
    #[serde(default)]
    pub messages_scanned: u64,
}

/// serde `skip_serializing_if` helper. `std::ops::Not::not` cannot be used
/// here — serde hands the predicate a `&bool`.
fn is_false(b: &bool) -> bool {
    !*b
}
```

- [ ] **Step 4: Fix the construction sites the compiler flags**

`cargo test` will now fail to compile everywhere `QueryMetadata` is built literally. Add the four fields to each.

`src/test_helpers.rs:218-224`:

```rust
query_metadata: QueryMetadata {
    query: query.to_string(),
    window_from: Utc::now() - chrono::Duration::hours(48),
    window_to: None,
    channels_scanned: Some(channels_in_results),
    channels_in_results,
    timed_out: false,
    partial: false,
    pages_fetched: 0,
    messages_scanned: 0,
},
```

`src/mcp/tools/fanout.rs` — accumulate in the loop at `:39-51`:

```rust
let mut has_more = false;
let mut search_time_ms = 0u64;
let mut pages_fetched = 0u32;
let mut messages_scanned = 0u64;
let mut timed_out = false;
let mut partial = false;

for outcome in outcomes {
    match outcome.result {
        Ok(result) => {
            has_more |= result.has_more;
            search_time_ms = search_time_ms.max(result.search_time_ms);
            // Summed, not maxed: these are the fan-out's total cost.
            pages_fetched = pages_fetched.saturating_add(result.query_metadata.pages_fetched);
            messages_scanned =
                messages_scanned.saturating_add(result.query_metadata.messages_scanned);
            timed_out |= result.query_metadata.timed_out;
            partial |= result.query_metadata.partial;
            messages.extend(result.messages.into_iter().map(MessageResponse::from));
        }
        Err(error) => errors.push(ChannelFetchError {
            channel: outcome.channel,
            error,
        }),
    }
}
```

and the construction at `:86-92`:

```rust
query_metadata: QueryMetadata {
    query,
    window_from,
    window_to,
    channels_scanned: Some(attempted),
    channels_in_results: unique.len() as u32,
    timed_out,
    partial,
    pages_fetched,
    messages_scanned,
},
```

Then fix every remaining literal the compiler reports (test modules across `src/mcp/tests/`). Fill `false`/`false`/`0`/`0` unless the test is specifically about these fields.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS, including the new tests and every previously passing test.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy -- -D warnings
git add -A
git commit -m "feat: query_metadata reports timed_out/partial/pages_fetched/messages_scanned

pages_fetched and messages_scanned replace the work order's suggested
dialogs_scanned: this path issues one paginated searchGlobal and sweeps no
dialogs, so that name would describe work the code does not do. Round
trips and messages walked are the work actually performed.

Flags are omitted when false, so existing responses are byte-identical.
The channel_ids fan-out sums the counters and ORs the flags rather than
dropping them."
```

---

### Task 5: Wire the budget into `search_messages_impl`

**Files:**
- Modify: `src/telegram/client/ops_search.rs` (both branches; global loop at `:173-208`, channel loop at `:88-134`, result construction at `:243-255`)
- Modify: `src/telegram/client/raw_pager.rs` (expose per-page size from both search pagers)

**Interfaces:**
- Consumes: `SearchBudget` (Task 2), `TelegramClient::search_deadline_secs` (Task 3), the four `QueryMetadata` fields (Task 4).
- Produces: no new public surface.

**Counting note:** `record_page` must be called once per *round trip*, not once per message. Both pagers buffer a page and hand out messages one at a time, so the pagers need to report when they refilled and with how many. Add to each pager a `pub(super) fn take_last_page_size(&mut self) -> Option<usize>` that returns `Some(n)` exactly once after a refill and `None` otherwise; call it after each `next()` and feed a `Some` into `record_page`. Counting messages yielded instead would undercount the walk — the discarded messages are the cost being reported.

**Honest note on test coverage for this task.** Both pagers own a live grammers `Client`, so `next()` and `take_last_page_size()` cannot be exercised in a unit test — there is no seam to inject a fake response at. Do **not** write a test that asserts `Option::take()` returns `Some` then `None`; that tests the standard library, not this code, and would give false confidence in the one piece of accounting most likely to be wrong.

The real verification is behavioral and happens twice:
- Task 7 Step 3 greps the debug log for one "page fetched" event per round trip.
- Task 8 Step 5 asserts `pages_fetched` is far smaller than `messages_scanned` in the live response. Per-message counting — the plausible bug here — would make them equal.

The budget arithmetic they feed is unit-tested in Task 2.

- [ ] **Step 1: Add page-size reporting to both search pagers**

In `src/telegram/client/raw_pager.rs`, add a field to **both** `RawGlobalSearchPager` and `RawChannelSearchPager`:

```rust
/// Messages in the page just fetched, taken exactly once by the caller so a
/// round trip is counted once rather than once per yielded message.
last_page_size: Option<usize>,
```

Initialize it to `None` in each `new()`. In each `next()`, immediately after the buffer is filled from `page.messages`, set it — for the global pager that is right after the `for message in messages` loop:

```rust
self.last_page_size = Some(self.buffer.len());
```

For the channel pager, set it at the equivalent point after its buffer fill. Then add to both impls:

```rust
pub(super) fn take_last_page_size(&mut self) -> Option<usize> {
    self.last_page_size.take()
}
```

- [ ] **Step 2: Wire the budget into the global branch**

In `src/telegram/client/ops_search.rs`, inside the `with_timeout("search_all_messages", ...)` closure, create the budget alongside the existing locals (`:159-161`):

```rust
let mut budget = SearchBudget::new(self.search_deadline_secs);
```

Change the loop head at `:173` to check the deadline before each fetch, and record each page after it:

```rust
while !budget.expired()
    && let Some((raw_msg, entities, chat_peer)) = pager
        .next()
        .await
        .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?
{
    if let Some(page_size) = pager.take_last_page_size() {
        budget.record_page(page_size);
    }
```

Leave the loop body unchanged. After the loop, return the budget with the results:

```rust
Ok((messages, has_more, budget))
```

and update the destructuring at `:157` and `:213` to carry it out to the outer scope.

- [ ] **Step 3: Wire the budget into the channel branch**

Apply the same three changes inside the `with_timeout("search_messages_channel", ...)` closure: construct a `SearchBudget` beside `counter` (`:61`), guard the inner `while let` at `:88` with `!budget.expired() &&`, call `record_page` from `pager.take_last_page_size()` at the top of the body, and thread the budget out through the closure's return tuple and the `.map(...)` at `:142`.

The channel path is fast today (measured 0.97–1.30 s across every shape), but a deadline guarding only the path we just fixed is the wrong shape.

Add the import at the top of `ops_search.rs`:

```rust
use super::search_budget::SearchBudget;
```

- [ ] **Step 4: Populate the metadata**

Replace the `SearchResult` construction at `:243-255`:

```rust
Ok(SearchResult {
    returned,
    has_more,
    search_time_ms,
    query_metadata: QueryMetadata {
        query: params.query.clone(),
        window_from: cutoff_time,
        window_to: params.to_date,
        channels_scanned,
        channels_in_results,
        timed_out: budget.timed_out(),
        // Today the deadline is the only thing that truncates a result set
        // without `has_more` already saying so.
        partial: budget.timed_out(),
        pages_fetched: budget.pages_fetched(),
        messages_scanned: budget.messages_scanned(),
    },
    messages,
})
```

Extend the existing `tracing::info!("Search completed")` call at `:233-241` with the new fields:

```rust
pages_fetched = budget.pages_fetched(),
messages_scanned = budget.messages_scanned(),
timed_out = budget.timed_out(),
```

- [ ] **Step 5: Run the full gate**

Run: `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/telegram/client/ops_search.rs src/telegram/client/raw_pager.rs
git commit -m "feat: bound both search branches with SearchBudget

On expiry the accumulation loop returns what it gathered with
timed_out/partial set — never an error, because partial results beat a
failed workflow. Pages are counted per round trip via take_last_page_size
rather than per yielded message: the discarded messages are precisely the
cost being reported."
```

---

### Task 6: MCP-layer deadline test

**Files:**
- Test: `src/mcp/tests/search.rs`

**Interfaces:**
- Consumes: `QueryMetadata` fields (Task 4).
- Produces: nothing.

**Why this is separate from Task 2's tests:** the deadline itself lives inside `TelegramClient`, which needs a real grammers `Client` and cannot be mocked — Task 2 covers that logic as a pure unit. This task covers the other half: that a degraded `SearchResult` crossing the trait boundary reaches the caller as JSON flags **and not as an error**. That is the spec's "no error propagates" requirement, and it is a distinct failure mode from the budget arithmetic.

- [ ] **Step 1: Write the failing test**

Add to `src/mcp/tests/search.rs`:

`SearchRequest` has no `Default` impl in this file's tests — every field is listed explicitly. `RequestId` is a tuple struct. Both match `search_messages_returns_results` at `:17`.

```rust
/// All-`None` request body; only `query` varies in these two tests.
fn search_request(query: &str) -> SearchRequest {
    SearchRequest {
        query: query.to_string(),
        channel_id: None,
        channel_ids: None,
        hours_back: None,
        limit: None,
        media_filter: None,
        from_date: None,
        to_date: None,
        collapse_albums: None,
        before_id: None,
        after_id: None,
        max_text_length: None,
        format: None,
    }
}

#[tokio::test]
async fn timed_out_search_returns_partial_results_not_an_error() {
    let mut mock_client = MockTelegramClientTrait::new();
    let mut degraded =
        create_test_search_result(vec![create_test_message(1, "partial hit", 123)], "rare", 1);
    degraded.query_metadata.timed_out = true;
    degraded.query_metadata.partial = true;
    degraded.query_metadata.pages_fetched = 41;
    degraded.query_metadata.messages_scanned = 4100;

    mock_client
        .expect_search_messages()
        .returning(move |_| Ok(degraded.clone()));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let result = server
        .search_messages(
            Parameters(search_request("rare")),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let body = result.expect("a slow-but-working search must not surface as an error");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(parsed["query_metadata"]["timed_out"], true);
    assert_eq!(parsed["query_metadata"]["partial"], true);
    assert_eq!(parsed["query_metadata"]["pages_fetched"], 41);
    assert_eq!(parsed["query_metadata"]["messages_scanned"], 4100);
    // The whole point: results survive the deadline.
    assert_eq!(parsed["returned"], 1);
}

#[tokio::test]
async fn healthy_search_omits_the_degradation_flags() {
    let mut mock_client = MockTelegramClientTrait::new();
    let expected =
        create_test_search_result(vec![create_test_message(1, "hit", 123)], "common", 1);
    mock_client
        .expect_search_messages()
        .returning(move |_| Ok(expected.clone()));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let result = server
        .search_messages(
            Parameters(search_request("common")),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let body = result.expect("search succeeds");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert!(parsed["query_metadata"].get("timed_out").is_none());
    assert!(parsed["query_metadata"].get("partial").is_none());
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib timed_out_search healthy_search_omits`
Expected: **PASS (2 tests)**.

These are characterization tests over behavior Tasks 4 and 5 already built, so they pass on the first run — unlike every other test in this plan, they are not test-first, and pretending otherwise would be dishonest bookkeeping.

- [ ] **Step 3: Prove the tests can actually fail**

A test that has never failed has not been shown to test anything. Temporarily delete `skip_serializing_if = "is_false"` from `QueryMetadata::timed_out` in `src/telegram/types/params.rs`, re-run, and confirm `healthy_search_omits_the_degradation_flags` fails. Then restore it and confirm both pass again.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add src/mcp/tests/search.rs
git commit -m "test: deadline degradation surfaces as flags, never as an error"
```

---

### Task 7: Instrumentation spans

**Files:**
- Modify: `src/telegram/client/ops_search.rs` (both `with_timeout` closures)

**Interfaces:**
- Consumes: `SearchBudget` (Task 2).
- Produces: nothing.

**Why this ships permanently:** the counters it feeds are already in the response (Task 4), so measurement and reporting are one implementation. The next regression of this kind shows up in the response body instead of needing a bespoke probe.

- [ ] **Step 1: Add the span and per-page events**

In `src/telegram/client/ops_search.rs`, wrap the global branch's accumulation in a span. Inside the `with_timeout("search_all_messages", ...)` closure, before the loop:

```rust
let span = tracing::debug_span!(
    "search_global",
    query = %params.query,
    media_filter = ?params.media_filter,
    window_from = %cutoff_time,
);
let _guard = span.enter();
let mut mtproto_nanos: u128 = 0;
```

Time each fetch and emit a per-page event where `record_page` is called:

```rust
let fetch_start = Instant::now();
let next = pager.next().await;
mtproto_nanos += fetch_start.elapsed().as_nanos();
```

then, at the `record_page` site:

```rust
if let Some(page_size) = pager.take_last_page_size() {
    budget.record_page(page_size);
    tracing::debug!(
        page = budget.pages_fetched(),
        messages_in_page = page_size,
        messages_scanned = budget.messages_scanned(),
        kept = messages.len(),
        "Global search page fetched"
    );
}
```

Restructure the `while` head to use the timed `next` binding rather than awaiting inline.

After the loop, log the split the work order asked for:

```rust
tracing::debug!(
    pages = budget.pages_fetched(),
    messages_scanned = budget.messages_scanned(),
    mtproto_ms = (mtproto_nanos / 1_000_000) as u64,
    total_ms = start_time.elapsed().as_millis() as u64,
    "Global search finished"
);
```

Conversion time is `total_ms - mtproto_ms`; it is not tracked separately because the measurement showed it is not a factor (six messages converted in a 44.86 s call), and a third timer would cost more than it reveals.

- [ ] **Step 2: Run the gate**

Run: `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all pass. Spans are debug-level, so default `info` logging is unchanged.

- [ ] **Step 3: Verify the spans actually emit**

Run:

```bash
cargo build --release
RUST_LOG=telegram_connector=debug python3 scripts/mcp_probe.py \
    target/release/telegram-mcp scripts/probes/search-latency.json
grep -c "Global search page fetched" probe-stderr.log
```

Expected: a non-zero count, and `pages` in the "Global search finished" line now in the low single digits for the document case rather than the hundreds implied by the pre-fix timing.

- [ ] **Step 4: Commit**

```bash
git add src/telegram/client/ops_search.rs
git commit -m "feat: tracing spans for global search round trips

Debug-level per-page events plus an MTProto-vs-total split, so the next
regression of this kind is visible in the logs instead of needing a
bespoke probe."
```

---

### Task 8: Documentation, tool description, and live acceptance

**Files:**
- Modify: `src/mcp/server.rs:290-292` (tool description)
- Modify: `README.md` (tool reference + response examples)
- Modify: `CHANGELOG.md` (`[Unreleased]`)
- Modify: `docs/tasklist.md` (progress table + new phase)
- Modify: `docs/memory.md` (patterns and lessons)

**Interfaces:**
- Consumes: everything above.
- Produces: nothing.

- [ ] **Step 1: Update the tool description**

In `src/mcp/server.rs:290`:

```rust
#[tool(
    description = "Search messages across subscribed Telegram channels with optional \
                   filters. Scoping with channel_id or channel_ids is cheaper than an \
                   unscoped global search and supports cursor pagination \
                   (before_id/after_id); global searches support neither, because there \
                   is no per-channel offset to resume from. A search that exceeds its \
                   configured deadline returns the results gathered so far with \
                   query_metadata.timed_out and .partial set, never an error."
)]
```

Written against post-fix behavior. Warning about a 35-second cliff this change removes would steer callers away from a shape that is no longer expensive.

- [ ] **Step 2: Update README.md**

In the `search_messages` tool reference, document the four new `query_metadata` fields and note that the flags are omitted when false. Add `deadline_seconds` to the `[search]` configuration section. Update any `query_metadata` response example to include `pages_fetched` and `messages_scanned`.

- [ ] **Step 3: Update CHANGELOG.md**

Under `## [Unreleased]` — the work order requires the before/after measurements ship with the change:

```markdown
### Fixed
- Unscoped `search_messages` no longer walks Telegram's entire global message
  index to enforce its own time window. `messages.SearchGlobal` carries
  `min_date`/`max_date` parameters that the pager hardcoded to `0`, so the
  window was applied client-side while Telegram streamed results backwards at
  100 per round trip — a rare media filter over a narrow window paged to
  exhaustion. Both bounds are now sent with the request. Measured on a live
  session, limit 20:

  | search | before | after |
  |---|---|---|
  | global `document`, 24h | 44.86 s | _(fill in from Task 1 Step 8)_ |
  | global `video_note`, 24h | 12.93 s | _(fill in)_ |
  | global `voice`, 72h | 8.07 s | _(fill in)_ |
  | global `url`, 24h | 0.46 s | _(fill in)_ |

  The client-side window checks are retained as defense in depth. Result sets
  are unchanged; only the work done to produce them is.

### Added
- `[search] deadline_seconds` (default 20) bounds a search's accumulation loop.
  On expiry the search returns the results gathered so far with
  `query_metadata.timed_out` and `query_metadata.partial` set — never an error,
  because partial results beat a failed workflow. Keep it below
  `[telegram.timeouts] search_secs` (default 120), which still fails the call.
  The default is a conservative starting point, not a measured value.
- `query_metadata.pages_fetched` and `query_metadata.messages_scanned` report
  the round trips issued and raw messages walked, so an expensive search is
  legible to its caller. These replace the work order's suggested
  `dialogs_scanned`: the global path issues one paginated `searchGlobal` and
  sweeps no dialogs. Both flags are omitted from JSON when false, so existing
  responses are byte-identical.
```

**Replace every `_(fill in)_` with the Step 5 numbers before committing.** A changelog table with placeholders is worse than no table.

- [ ] **Step 4: Update tracking docs**

`docs/tasklist.md`: add Phase 35 to the progress table (description "Global search latency (work order B)", status ✅, the final test count) and bump "Overall Progress" to 35/35.

`docs/memory.md`: record the finding that survives this change — a client-side filter over a server-paginated cursor is unbounded work whenever the filter is more selective than the page, and the measurement that isolates it is varying `limit` rather than the filter. Note that `min_date`/`max_date` exist on `messages.Search` too, so the channel path could take the same treatment if it ever shows up slow (it measured 0.97–1.30 s here, so it was deliberately left alone).

- [ ] **Step 5: Live acceptance run**

Run:

```bash
cargo build --release
python3 scripts/mcp_probe.py target/release/telegram-mcp scripts/probes/search-latency.json
```

Confirm and record:
- `document` 24h/limit 20 is **under 1 s**, still returning **6** messages.
- `pages_fetched` is in the low single digits for that case.
- **`pages_fetched` is far smaller than `messages_scanned`** (single digits vs hundreds). If they are equal, the counter is firing per message instead of per round trip — this is the check that stands in for the unit test Task 5 could not have.
- `url` 24h is unchanged (~0.46 s) — the fix must not have slowed the fast path.
- No response contains `timed_out` (nothing should hit a 20 s deadline once the walk is gone).

Paste the actual numbers into the CHANGELOG table from Step 3.

- [ ] **Step 6: Run the full gate**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test` and `cargo test config -- --test-threads=1`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add src/mcp/server.rs README.md CHANGELOG.md docs/tasklist.md docs/memory.md
git commit -m "docs: global search latency — README, changelog, tasklist, memory

Changelog carries the measured before/after table the work order requires."
```

- [ ] **Step 8: Request code review**

Use the `superpowers:requesting-code-review` skill before merging.

---

## Verification Summary

| Spec requirement | Task |
|---|---|
| Push `min_date`/`max_date` server-side | 1 |
| Retain client-side guards; no `continue` → `break` | 1 (Step 5) |
| Empirical verification before building on the assumption | 1 (Step 8, gate) |
| `[search] deadline_seconds`, default 20, validated `> 0` | 3 |
| Deadline bounds the loop, not a round trip | 2, 5 |
| Deadline applies to both branches | 5 |
| Partial results, never an error | 2, 5, 6 |
| `timed_out` / `partial`, omitted when false | 4 |
| `pages_fetched` / `messages_scanned` | 4, 5 |
| Fan-out sums counters and ORs flags | 4 |
| No `next_cursor` on global (documented) | 8 (Step 1) |
| Tracing spans, MTProto vs total split | 7 |
| Tool description | 8 (Step 1) |
| Docs: config.example.toml, README, CHANGELOG | 3 (Step 5), 8 |
| Before/after measurements in CHANGELOG | 8 (Steps 3, 5) |

**Out of scope, per spec:** result caching, ordering or filter-semantics changes, lowering the default `limit`, the channel path's termination logic.
