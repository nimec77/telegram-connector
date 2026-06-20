# 03 — Conventions & Quality

Smaller, mostly-mechanical items: places where the code drifts from the project's
own stated rules, plus dead code, unused dependencies, and data-honesty nits.
Individually minor; collectively they keep the codebase honest to `CLAUDE.md` and
`docs/conventions.md`.

---

## CQ-1 — `.unwrap()` in production code 🟡

**Effort:** S · **Risk:** Low · **Impact:** Medium (rule compliance)

### The rule
`CLAUDE.md` / `docs/conventions.md`: **"Never `unwrap()` in production code — use
`?` or `.context(...)`. `expect()` is allowed only in tests or truly impossible
situations."**

### Evidence — violations
- `converters.rs` — five `Username::new("…").unwrap()` fallbacks:
  - `:230` `Username::new("unknown").unwrap()`
  - `:251` `Username::new("group").unwrap()`
  - `:379` `Username::new("unknown").unwrap()`
  - `:386` `Username::new("group").unwrap()`
  - `:393` `Username::new("user").unwrap()`
- `rate_limiter.rs` — three `self.bucket.lock().unwrap()`:
  - `:70`, `:90`, `:99`

### Assessment
Both are *arguably* "truly impossible": the `Username` literals are statically
valid, and a poisoned mutex only happens if a holder panicked. But the rule is
written to forbid bare `.unwrap()` regardless — `expect()` with a message is the
sanctioned form, and it turns a future regression (e.g. someone tightens
`Username` validation) into a clear panic message instead of a bare unwrap.

### Fix sketch
- For the `Username` fallbacks, define the defaults once and validate them once:

  ```rust
  // converters/channel.rs
  fn fallback_username(kind: &'static str) -> Username {
      Username::new(kind).expect("static fallback username is always valid")
  }
  ```

  Better still, pair with AD-4 so there's a single fallback site, not five.
- For the mutex, use `.expect("rate limiter mutex poisoned")` (or migrate to
  `parking_lot::Mutex`, whose `lock()` doesn't return a `Result` — but that's a
  new dependency; `expect` is the minimal fix).

### Note
A `grep -rn "\.unwrap()" src/ --include=*.rs` outside `#[cfg(test)]` blocks is the
acceptance check. Test code may use `unwrap`/`expect` freely (allowed by the rule).

---

## CQ-2 — `apply_defaults()` is a documented no-op 🟢

**Effort:** S · **Risk:** Low · **Impact:** Low

### The rule
`docs/conventions.md`: **"Delete over Comment — remove dead code, don't comment
it."**

### Evidence
```rust
// config.rs:417
fn apply_defaults(&mut self) {
    // Defaults are handled by serde with #[serde(default)] attributes
    // This method is kept for potential future use
}
```
Called once (`config.rs:367`). It does nothing and is explicitly "kept for the
future" — the exact pattern the convention says to delete.

### Fix sketch
Delete the method and its call site. If a post-load normalization hook is wanted
later, add it when there's an actual default to apply (YAGNI).

---

## CQ-3 — Unused dependencies 🟢

**Effort:** S · **Risk:** Low · **Impact:** Low (build time, supply-chain surface)

### Evidence
- **`dashmap = "6.2.1"`** (`Cargo.toml:55`) — no `dashmap` / `DashMap` reference
  anywhere in `src/` (source scan: 0 hits). The concurrent maps in use are
  `tokio::sync::RwLock` (premium flag) and `std::sync::Mutex` (rate limiter).
- **`tokio-test = "0.4.5"`** (`Cargo.toml:65`, dev-dependency) — no `tokio_test`
  reference in `src/` (0 hits).

`proptest` (`rate_limiter.rs`), `tempfile`, `mockall`, and `filetime`
(`logging_tests.rs`) *are* used — keep those.

### Fix sketch
Remove the two unused lines from `Cargo.toml`, run
`cargo build && cargo test` to confirm, and commit the updated `Cargo.lock`.
(If `dashmap` was added in anticipation of a concurrency feature, that's YAGNI —
re-add it when the feature lands.)

> Optional: add `cargo-machete` or `cargo +nightly udeps` to the `just check`
> recipe to catch this class of drift automatically.

---

## CQ-4 — `Channel` reports `member_count = 0` and `description = None` unconditionally 🟡

**Effort:** M · **Risk:** Low · **Impact:** Medium (response honesty)

### Evidence
`convert_peer_to_channel` (`converters.rs:236–241`, `:256–261`) hardcodes:

```rust
description: None,   // Not available from basic chat info
member_count: 0,     // Would need additional API call
```

These ship to the MCP client as real values. `member_count: 0` is indistinguish-
able from "a channel with zero members," and `description: None` looks like "no
description" rather than "not fetched."

### Why it matters
The connector's purpose is to feed accurate channel data to a model. A hardcoded
`0` is a silent inaccuracy the model may reason over (e.g. "this channel is
empty"). This is a data-model honesty issue, not just cosmetics.

### Fix sketch (pick one)
- **Cheapest / honest:** make the fields optional —
  `member_count: Option<u64>` (→ `None` = "not fetched"), keep `description:
  Option<String>`. Update `Channel`'s schema + any response mapping. The model can
  then distinguish "unknown" from "zero/empty."
- **Complete:** fetch full channel info (`channels.getFullChannel`) in
  `get_channel_info` (the single-channel path where the extra call is justified)
  and populate real values; keep the cheap list path (`get_subscribed_channels`)
  as `None` to avoid N extra calls.

Recommend the optional-fields change first (small, honest), and the full-fetch
only for `get_channel_info` if the data is actually wanted.

---

## CQ-5 — `has_more` over-reports a next page 🟢

**Effort:** S · **Risk:** Low · **Impact:** Low

### Evidence
```rust
// server.rs:139–140 (get_subscribed_channels_impl)
let total = channels.len();
let has_more = total >= limit as usize;
```
When the channel count is an exact multiple of `limit`, `has_more` is `true` even
though the next page is empty. The client then makes one wasted call that returns
zero channels.

### Why it matters
Minor pagination inaccuracy — one redundant round-trip at the boundary. Not a
correctness bug (the empty next page is harmless), but the signal is misleading.

### Fix sketch
The robust fix is to fetch `limit + 1` from the client and report
`has_more = fetched > limit` (then truncate to `limit`). That requires the client
method to accept an over-fetch, so it's slightly more than a one-liner. If not
worth it, document the heuristic ("`has_more` may be optimistic at exact page
boundaries") so consumers don't treat it as exact.

---

## CQ-6 — Documentation drift 🟡

**Effort:** S · **Risk:** Low · **Impact:** Medium (onboarding / accuracy)

### Evidence
The code has **11** MCP tools (`server.rs` "Tool 11"; `mcp/tools.rs:3` "all 11
MCP tools"; `transcribe_voice_message` is the 11th). But:

- `CLAUDE.md` (Architecture) states **"src/mcp/server.rs (10 tools)"** and
  **"All 10 tools live in `server.rs`"**.
- `docs/memory.md` header says **"Phase 22 … 305 tests"** and lists **21/21
  phases**, while `docs/tasklist.md` lists **23/23 phases / 408 tests**. The two
  trackers disagree, and both lag the actual count.

### Why it matters
`CLAUDE.md` is the file these instructions tell every contributor (and Claude) to
treat as authoritative. A wrong tool count there is the kind of small inaccuracy
that erodes trust in the doc and misleads onboarding.

### Fix sketch
- Update `CLAUDE.md`: "10 tools" → "11 tools" (two occurrences), and confirm the
  tool inventory in the Architecture section mentions `get_message_media` and
  `transcribe_voice_message`.
- Reconcile `docs/memory.md` with `docs/tasklist.md` (single source of truth for
  phase/test counts), or add a note that `tasklist.md` is authoritative.
- Consider a tiny CI check that asserts the tool count in `CLAUDE.md` matches the
  number of `#[tool(` attributes in `server.rs` (cheap drift guard).

---

## Summary

| ID | Finding | Sev | Effort | Risk |
|----|---------|-----|--------|------|
| CQ-1 | Replace `.unwrap()` in prod with `expect`/shared fallback | 🟡 | S | Low |
| CQ-2 | Delete no-op `apply_defaults()` | 🟢 | S | Low |
| CQ-3 | Drop unused deps `dashmap`, `tokio-test` | 🟢 | S | Low |
| CQ-4 | Make `member_count`/`description` honest (Optional or fetched) | 🟡 | M | Low |
| CQ-5 | Fix/`document` `has_more` boundary over-report | 🟢 | S | Low |
| CQ-6 | Fix CLAUDE.md "10 tools" → 11; reconcile memory/tasklist | 🟡 | S | Low |
