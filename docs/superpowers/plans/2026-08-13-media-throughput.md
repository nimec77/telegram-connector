# Media Throughput Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `get_messages_media_batch` — up to 10 images in one call, one peer resolution, one message-fetch RPC, bounded-concurrent downloads under a total payload cap — and retune the media rate-limit costs.

**Architecture:** The per-message selection logic is extracted out of `download_message_media_impl` into a shared `media_download_from_message` helper, so the single and batch client methods cannot drift. A new `download_messages_media` trait method resolves the peer once, issues one `get_messages_by_id` for all ids, and runs downloads through `futures::stream…buffered(FANOUT_CONCURRENCY)`. At the MCP layer a pure `Base64Budget` allocates the total payload cap greedily in request order, handing each image its remaining allowance to the existing `process_image_with_cap`, which already downscales to fit.

**Tech Stack:** Rust 2024 nightly, `rmcp` v3.1 (`#[tool_router]` / `#[tool]`), `grammers` (pinned Codeberg rev), `schemars` v1, `mockall`, `futures`, `tokio`.

**Spec:** `docs/superpowers/specs/2026-08-13-media-throughput-design.md`

## Global Constraints

- Pre-merge gate, all must pass: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
- Run `cargo fmt --all` after every code change, not just `--check`.
- Config tests must run serial: `cargo test config -- --test-threads=1`
- **Never `unwrap()`** in production code — use `?` or `.context("...")`. `expect()` only in tests.
- Line length 100 chars.
- TDD: the failing test comes first; no production code without a preceding test. Exceptions are called out explicitly per task and justified.
- No `mod.rs` files — file-as-module. New modules are declared in the parent module file.
- Backward compatible: no existing response field renamed, retyped, or removed.
- Newtypes (`MessageId`, `ChannelId`) stay strict; leniency lives only in `serde_helpers.rs` at the MCP boundary.
- Never log phone numbers, API hashes, passwords, or session tokens.
- Commit style: conventional commits (`feat:`, `fix:`, `test:`, `refactor:`, `docs:`, `chore:`).
- Search the codebase with `ast-index`, not `grep`, per `.claude/rules/ast-index.md`. Run `ast-index update` after checking out the branch.

## File Structure

| File | Disposition | Responsibility |
|---|---|---|
| `src/rate_limiter.rs` | Modify | `refund` on `TokenBucket`, `RateLimiter`, `RateLimiterTrait` |
| `src/rate_limiter/tests.rs` | Modify | Refund unit tests |
| `src/config/defaults.rs` | Modify | `max_tokens` 50→60, `media_download_cost` 5→3, new `default_media_batch_max_total_bytes` |
| `src/config.rs` | Modify | `LimitsConfig::media_batch_max_total_bytes` + validation |
| `src/config/tests.rs` | Modify | Default and validation tests |
| `config.example.toml` | Modify | Three documented keys, flagged as estimates |
| `src/mcp/tools/image.rs` | Modify | `process_image_with_cap` private → `pub(crate)` |
| `src/mcp/tools/media_budget.rs` | **Create** | Pure `Base64Budget` allocator |
| `src/mcp/tools.rs` | Modify | Declare `media_budget` |
| `src/telegram/types/media.rs` | Modify | `MediaFetchOutcome` |
| `src/telegram/client/ops_media.rs` | Modify | Extract helper; add batch impl |
| `src/telegram/trait_def.rs` | Modify | `download_messages_media` |
| `src/telegram/client.rs` | Modify | Trait delegator |
| `src/telegram.rs` | Modify | Re-export `MediaFetchOutcome` |
| `src/mcp/tools/types/requests.rs` | Modify | `GetMessagesMediaBatchRequest` |
| `src/mcp/tools/types/responses.rs` | Modify | `MediaBatchSummary`, `MediaBatchFailure`, `MediaLimits` |
| `src/mcp/server/impl_media.rs` | Modify | `get_messages_media_batch_impl`, shared dimension constants |
| `src/mcp/server/impl_status.rs` | Modify | Populate `MediaLimits` |
| `src/mcp/server.rs` | Modify | `#[tool]` wrapper, `media_batch_max_total_bytes` field + builder |
| `src/mcp/tests/media_batch.rs` | **Create** | Batch tool tests |
| `src/mcp/tests.rs` | Modify | Declare `media_batch` |
| `src/mcp/tests/status.rs` | Modify | `media` block test |
| `src/main.rs` | Modify | Wire the new config value |
| `README.md`, `CHANGELOG.md`, `docs/tasklist.md`, `docs/memory.md` | Modify | Documentation |

---

## Task 1: Rate limiter refund and retuned defaults

**Files:**
- Modify: `src/rate_limiter.rs`
- Test: `src/rate_limiter/tests.rs`
- Modify: `src/config/defaults.rs:34-36` (`default_max_tokens`), `src/config/defaults.rs:84-86` (`default_media_download_cost`)
- Test: `src/config/tests.rs`
- Modify: `config.example.toml:57-60`

**Interfaces:**
- Consumes: nothing.
- Produces: `RateLimiterTrait::refund(&self, tokens: u32)`. Consumed by Task 7. `MockRateLimiterTrait` gains `expect_refund()`.

**Context:** The bucket already clamps to `max_tokens` inside `refill()` (`src/rate_limiter.rs:29`), so an over-refund cannot inflate capacity provided `refund` routes through the same clamp.

- [ ] **Step 1: Write the failing refund tests**

Append to `src/rate_limiter/tests.rs` (the file already has `fn test_config(max_tokens: u32, refill_rate: f64) -> RateLimitConfig`):

```rust
#[tokio::test]
async fn refund_returns_tokens_to_the_bucket() {
    let limiter = RateLimiter::new(&test_config(30, 0.0));
    limiter.acquire(15).await.expect("bucket starts full");
    assert_eq!(limiter.available_tokens(), 15.0);

    limiter.refund(6);

    assert_eq!(limiter.available_tokens(), 21.0);
}

#[tokio::test]
async fn refund_cannot_exceed_capacity() {
    let limiter = RateLimiter::new(&test_config(30, 0.0));
    limiter.acquire(5).await.expect("bucket starts full");

    limiter.refund(500);

    assert_eq!(
        limiter.available_tokens(),
        30.0,
        "refund must clamp at capacity, never inflate the bucket"
    );
}

#[tokio::test]
async fn refund_of_zero_is_a_no_op() {
    let limiter = RateLimiter::new(&test_config(30, 0.0));
    limiter.acquire(10).await.expect("bucket starts full");

    limiter.refund(0);

    assert_eq!(limiter.available_tokens(), 20.0);
}
```

`refill_rate` is 0.0 so wall-clock refill cannot perturb the assertions.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test rate_limiter::tests::refund`
Expected: FAIL — `no method named 'refund' found for struct 'RateLimiter'`.

- [ ] **Step 3: Implement refund**

In `src/rate_limiter.rs`, add to `impl TokenBucket` (after `try_acquire`, before `available`):

```rust
    /// Return previously-acquired tokens. Clamped at capacity, so returning
    /// more than was taken can never inflate the bucket.
    fn refund(&mut self, tokens: u32) {
        self.refill();
        self.available_tokens = (self.available_tokens + f64::from(tokens)).min(self.max_tokens);
    }
```

Add to `pub trait RateLimiterTrait` (after `acquire`):

```rust
    /// Return tokens taken by an `acquire` whose work did not happen.
    ///
    /// Batch tools acquire pessimistically for every requested item, then
    /// refund the items that produced nothing, so admission control stays real
    /// while the net charge matches the work actually performed.
    fn refund(&self, tokens: u32);
```

Add to `impl RateLimiterTrait for RateLimiter`:

```rust
    fn refund(&self, tokens: u32) {
        let mut bucket = self.bucket.lock().unwrap();
        bucket.refund(tokens);
    }
```

Note: `.lock().unwrap()` matches the three existing methods in this file. A poisoned mutex here is unrecoverable and the established local convention is to panic; do not diverge.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test rate_limiter`
Expected: PASS, all existing rate-limiter tests still green.

- [ ] **Step 5: Write the failing defaults test**

Append to `src/config/tests.rs`:

```rust
#[test]
fn retuned_media_rate_limit_defaults() {
    let config = default_rate_limit_config();
    assert_eq!(config.max_tokens, 60, "burst capacity raised for batch media");
    assert_eq!(config.media_download_cost, 3, "per-image cost lowered");
    assert_eq!(config.refill_rate, 2.0, "refill rate is deliberately unchanged");
}
```

If `default_rate_limit_config` is not already in scope in that file, import it: `use crate::config::defaults::default_rate_limit_config;`.

- [ ] **Step 6: Run test to verify it fails**

Run: `cargo test config -- --test-threads=1`
Expected: FAIL — `assertion left == right failed: left: 50, right: 60`.

- [ ] **Step 7: Retune the defaults**

`src/config/defaults.rs` — change the two function bodies:

```rust
pub(crate) fn default_max_tokens() -> u32 {
    60
}
```

```rust
pub(crate) fn default_media_download_cost() -> u32 {
    3
}
```

Leave `default_refill_rate` at `2.0`.

- [ ] **Step 8: Run test to verify it passes**

Run: `cargo test config -- --test-threads=1`
Expected: PASS.

- [ ] **Step 9: Document the retune in config.example.toml**

Replace the `[rate_limiting]` block (currently lines 57-60):

```toml
[rate_limiting]
# Optional: Token bucket configuration.
#
# These are CONSERVATIVE ESTIMATES, not values calibrated against Telegram's
# real flood thresholds. Tune them against your own traffic.
# max_tokens = 60                          # Default: 60 (burst capacity)
# refill_rate = 2.0                        # Default: 2.0 tokens/second
# media_download_cost = 3                  # Default: 3 tokens per image
# transcription_cost = 5                   # Default: 5 (Telegram's weekly quota is scarce)
```

At 60 / 2.0 / 3 that is 20 images in a burst, then one per 1.5 s.

- [ ] **Step 10: Verify and commit**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: all green.

```bash
git add src/rate_limiter.rs src/rate_limiter/tests.rs src/config/defaults.rs src/config/tests.rs config.example.toml
git commit -m "feat: rate-limiter refund and retuned media defaults"
```

---

## Task 2: Base64 payload budget

**Files:**
- Create: `src/mcp/tools/media_budget.rs`
- Modify: `src/mcp/tools.rs`
- Modify: `src/mcp/tools/image.rs:44` (visibility only)
- Modify: `src/config.rs` (`LimitsConfig`), `src/config/defaults.rs`
- Test: `src/config/tests.rs`
- Modify: `config.example.toml` (`[limits]`)

**Interfaces:**
- Consumes: `image::MAX_BASE64_LEN` (existing, `1_572_864`).
- Produces:
  - `media_budget::Base64Budget` with `new(total: usize) -> Self`, `allowance(&self) -> Option<usize>`, `consume(&mut self, actual: usize)`
  - `media_budget::MIN_IMAGE_BASE64_BYTES: usize` = `32_768`
  - `image::process_image_with_cap(bytes: &[u8], max_dimension: u32, max_base64_len: usize) -> Result<ProcessedImage, Error>` now `pub(crate)`
  - `LimitsConfig::media_batch_max_total_bytes: u64`, default `8_388_608`

  All consumed by Task 6.

- [ ] **Step 1: Write the failing budget tests**

Create `src/mcp/tools/media_budget.rs`:

```rust
//! Total-payload budget for multi-image responses.
//!
//! Counts **base64 characters** — the quantity that actually lands in the
//! client's context, and 4/3 the size of the underlying JPEG bytes. Pure: no
//! I/O, no image decoding.

use crate::mcp::tools::image::MAX_BASE64_LEN;

/// Floor below which an image would be downscaled past usefulness. Once the
/// remaining budget drops under this, the batch stops rather than emitting
/// unreadable thumbnails.
pub(crate) const MIN_IMAGE_BASE64_BYTES: usize = 32_768;

/// Greedy allocator over a batch's total base64 payload budget.
#[derive(Debug)]
pub(crate) struct Base64Budget {
    remaining: usize,
}

impl Base64Budget {
    pub(crate) fn new(total: usize) -> Self {
        Self { remaining: total }
    }

    /// Base64 bytes the next image may occupy, or `None` when the budget is
    /// spent. Never exceeds the per-image cap, however much budget is left.
    pub(crate) fn allowance(&self) -> Option<usize> {
        if self.remaining < MIN_IMAGE_BASE64_BYTES {
            return None;
        }
        Some(self.remaining.min(MAX_BASE64_LEN))
    }

    /// Record what an image actually used. Saturates at zero.
    pub(crate) fn consume(&mut self, actual: usize) {
        self.remaining = self.remaining.saturating_sub(actual);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowance_is_capped_by_the_per_image_limit() {
        let budget = Base64Budget::new(100 * MAX_BASE64_LEN);
        assert_eq!(budget.allowance(), Some(MAX_BASE64_LEN));
    }

    #[test]
    fn allowance_shrinks_to_what_is_left() {
        let mut budget = Base64Budget::new(MAX_BASE64_LEN + 100_000);
        budget.consume(MAX_BASE64_LEN);
        assert_eq!(budget.allowance(), Some(100_000));
    }

    #[test]
    fn allowance_is_none_below_the_floor() {
        let mut budget = Base64Budget::new(MIN_IMAGE_BASE64_BYTES + 10);
        assert!(budget.allowance().is_some());
        budget.consume(11);
        assert_eq!(
            budget.allowance(),
            None,
            "below the floor the batch must stop, not emit an unreadable image"
        );
    }

    #[test]
    fn consuming_more_than_remains_saturates_at_zero() {
        let mut budget = Base64Budget::new(50_000);
        budget.consume(usize::MAX);
        assert_eq!(budget.allowance(), None);
    }

    #[test]
    fn allowance_is_none_when_the_budget_starts_below_the_floor() {
        let budget = Base64Budget::new(MIN_IMAGE_BASE64_BYTES - 1);
        assert_eq!(budget.allowance(), None);
    }
}
```

- [ ] **Step 2: Declare the module and widen image visibility**

`src/mcp/tools.rs` — add after the `image` declaration:

```rust
pub(crate) mod media_budget;
```

`src/mcp/tools/image.rs:44` — change the signature from `fn` to `pub(crate) fn`:

```rust
pub(crate) fn process_image_with_cap(
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test media_budget`
Expected: PASS (5 tests). The module is pure, so the tests go green as soon as it compiles — this is the module's own definition, not implementation-before-test.

The invariant "a failed image leaves the budget untouched" is deliberately NOT tested here: at this layer it would be a tautology (not calling `consume` obviously changes nothing). It is a property of the *caller* and is covered by Task 6's cap tests, which assert `returned + failed == requested`.

- [ ] **Step 4: Write the failing config test**

Append to `src/config/tests.rs`:

```rust
#[test]
fn media_batch_payload_cap_defaults_to_eight_mib() {
    let limits = default_limits_config();
    assert_eq!(limits.media_batch_max_total_bytes, 8_388_608);
}

#[test]
fn zero_media_batch_payload_cap_is_rejected() {
    let limits = LimitsConfig {
        response_byte_budget: 40_000,
        media_batch_max_total_bytes: 0,
    };
    let err = limits.validate().expect_err("a zero cap returns no images at all");
    assert!(err.to_string().contains("media_batch_max_total_bytes"));
}
```

Import `default_limits_config` and `LimitsConfig` if not already in scope.

- [ ] **Step 5: Run test to verify it fails**

Run: `cargo test config -- --test-threads=1`
Expected: FAIL — `struct 'LimitsConfig' has no field named 'media_batch_max_total_bytes'`.

- [ ] **Step 6: Add the config field**

`src/config.rs`, inside `pub struct LimitsConfig` after `response_byte_budget`:

```rust
    /// Cap (bytes of base64 payload, as sent to the client) on the total image
    /// payload of one `get_messages_media_batch` call. Base64 is 4/3 the size
    /// of the underlying JPEG, and base64 is what consumes client context, so
    /// the budget is counted in base64 bytes.
    #[serde(default = "default_media_batch_max_total_bytes")]
    pub media_batch_max_total_bytes: u64,
```

Extend `impl LimitsConfig::validate`, after the existing `response_byte_budget` check:

```rust
        if self.media_batch_max_total_bytes == 0 {
            anyhow::bail!("limits.media_batch_max_total_bytes must be > 0");
        }
```

`src/config/defaults.rs` — add the default function next to `default_response_byte_budget`:

```rust
pub(crate) fn default_media_batch_max_total_bytes() -> u64 {
    8 * 1024 * 1024
}
```

and extend `default_limits_config`:

```rust
pub(crate) fn default_limits_config() -> LimitsConfig {
    LimitsConfig {
        response_byte_budget: default_response_byte_budget(),
        media_batch_max_total_bytes: default_media_batch_max_total_bytes(),
    }
}
```

Import `default_media_batch_max_total_bytes` in `src/config.rs` alongside the other `default_*` imports.

- [ ] **Step 7: Run test to verify it passes**

Run: `cargo test config -- --test-threads=1`
Expected: PASS.

- [ ] **Step 8: Document the key**

`config.example.toml`, replace the `[limits]` block (currently lines 62-65):

```toml
[limits]
# Byte cap on a serialized message-stream response. When a page would exceed
# it, trailing messages are dropped and has_more/next_cursor are set.
# response_byte_budget = 40000

# Cap on the TOTAL image payload of one get_messages_media_batch call, counted
# in bytes of base64 as sent to the client (base64 is 4/3 the JPEG size, and
# base64 is what consumes context). Images are downscaled progressively to fit;
# ids that still do not fit are reported in `failed` with reason
# payload_cap_reached.
#
# This is a CONSERVATIVE ESTIMATE, not a measured limit.
# media_batch_max_total_bytes = 8388608    # Default: 8388608 (8 MiB)
```

- [ ] **Step 9: Verify and commit**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: all green.

```bash
git add src/mcp/tools/media_budget.rs src/mcp/tools.rs src/mcp/tools/image.rs src/config.rs src/config/defaults.rs src/config/tests.rs config.example.toml
git commit -m "feat: base64 payload budget and media_batch_max_total_bytes"
```

---

## Task 3: Extract the per-message download helper

**Files:**
- Modify: `src/telegram/client/ops_media.rs:9-155`

**Interfaces:**
- Consumes: nothing.
- Produces: `TelegramClient::media_download_from_message(&self, msg: grammers_client::message::Message, channel_ref: &str, message_id: i32, max_dimension: u32) -> Result<MediaDownload, Error>` — an inherent `pub(super)` method. Consumed by Task 4.

**This task is a pure refactor with no behaviour change.** TDD does not apply: no new behaviour means no new test. The guard is that the existing `get_message_media` suite (`src/mcp/tests/media.rs`) must pass byte-identically before and after. Establish green first, then refactor, then confirm still green. If any assertion changes, the extraction was wrong — revert, do not amend the test.

- [ ] **Step 1: Establish the baseline is green**

Run: `cargo test media`
Expected: PASS. Record the test count from the output; it must be identical in Step 4.

- [ ] **Step 2: Extract the helper**

In `src/telegram/client/ops_media.rs`, split `download_message_media_impl`. Everything from `let media = msg.media().ok_or_else(...)` (currently line 50) through the closing `Ok(MediaDownload { ... })` moves verbatim into a new method. The result:

```rust
impl TelegramClient {
    pub(super) async fn download_message_media_impl(
        &self,
        channel_ref: &str,
        message_id: i32,
        max_dimension: u32,
    ) -> Result<MediaDownload, Error> {
        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }

        let peer = self.resolve_peer(channel_ref).await?;
        let peer_ref = peer_to_ref(&peer).await?;

        let messages = with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
            self.client
                .get_messages_by_id(peer_ref, &[message_id])
                .await
                .map_err(|e| {
                    tracing::error!(
                        channel_ref = %channel_ref,
                        message_id,
                        error = %e,
                        "Failed to get message for media download"
                    );
                    Error::TelegramApi(format!("Failed to get message: {}", e))
                })
        })
        .await?;

        let msg = require_found(
            messages.into_iter().next().flatten(),
            channel_ref,
            message_id,
        )?;

        self.media_download_from_message(msg, channel_ref, message_id, max_dimension)
            .await
    }

    /// Select and download a message's visual media: the photo itself, or the
    /// server-side thumbnail for video-like media.
    ///
    /// Shared by the single-message and batch entry points so the
    /// photo-vs-thumbnail rules, the size-variant selection and the
    /// `max_download_bytes` enforcement exist in exactly one place. Takes an
    /// already-fetched message, so it performs no resolve and no fetch.
    pub(super) async fn media_download_from_message(
        &self,
        msg: grammers_client::message::Message,
        channel_ref: &str,
        message_id: i32,
        max_dimension: u32,
    ) -> Result<MediaDownload, Error> {
        // Hard cap on a single download (`[telegram] max_download_bytes`, AD-6).
        // Hoisted to a Copy local so the streaming closure captures the value.
        let max_download_bytes = self.max_download_bytes;

        // ... everything from the original `let media = msg.media()...`
        //     through `Ok(MediaDownload { ... })`, moved unchanged ...
    }
}
```

Move the `let max_download_bytes = self.max_download_bytes;` binding into the helper — that is where it is used. Do not otherwise alter a line of the moved code: not the tracing fields, not the error strings, not the ordering.

`require_found` already returns `grammers_client::message::Message` and is already imported in this file, so if `Message` resolves unambiguously via `use super::*` use the bare name and drop the path qualification. Check the existing imports in `src/telegram/client.rs` before choosing.

- [ ] **Step 3: Verify the refactor compiles**

Run: `cargo check`
Expected: clean.

- [ ] **Step 4: Verify behaviour is unchanged**

Run: `cargo test`
Expected: PASS with the same test count as Step 1. No test file is edited in this task — if one needs editing, the extraction changed behaviour and must be redone.

- [ ] **Step 5: Verify and commit**

Run: `cargo fmt --all && cargo clippy -- -D warnings`
Expected: clean.

```bash
git add src/telegram/client/ops_media.rs
git commit -m "refactor: extract media_download_from_message from the single-message path"
```

---

## Task 4: Client-layer batch download

**Files:**
- Modify: `src/telegram/types/media.rs` (after `MediaDownload`, currently ends line 182)
- Modify: `src/telegram/trait_def.rs:85-90` (after `download_message_media`)
- Modify: `src/telegram/client.rs:144-152` (after the `download_message_media` delegator)
- Modify: `src/telegram/client/ops_media.rs`
- Modify: `src/telegram.rs` (re-export)

**Interfaces:**
- Consumes: `TelegramClient::media_download_from_message` (Task 3), `crate::mcp::tools::fanout::FANOUT_CONCURRENCY` (existing, `= 4`).
- Produces:
  - `MediaFetchOutcome { pub message_id: i32, pub result: Result<MediaDownload, Error> }`
  - `TelegramClientTrait::download_messages_media(&self, channel_ref: &str, message_ids: &[i32], max_dimension: u32) -> Result<Vec<MediaFetchOutcome>, Error>`

  Both consumed by Tasks 5–7. `MockTelegramClientTrait` gains `expect_download_messages_media()`.

**No unit test at this layer, and that is deliberate.** `src/telegram/tests/client_tests.rs` exercises `MockTelegramClientTrait`, not `TelegramClient` — no `*_impl` method in `src/telegram/client/` has a unit test today, because constructing a live `grammers` client in-process is not feasible. This method inherits that. Its verification is: the compiler, the Task 5–7 MCP tests against the mocked trait, and the live acceptance run in Task 10. Do not fabricate a test that only exercises the mock and call it coverage.

- [ ] **Step 1: Add the outcome type**

`src/telegram/types/media.rs`, after `MediaDownload`:

```rust
/// Why one id in a batch produced no image.
///
/// A typed enum rather than a bare `Error` because the MCP layer must emit a
/// stable, machine-readable reason token per id, and the not-found case is
/// otherwise indistinguishable: `guard::not_found` returns
/// `Error::InvalidInput` with the reason only in its message text, and
/// sniffing that string would couple the wire contract to prose.
#[derive(Debug)]
pub enum MediaFetchError {
    /// Deleted, never existed, or returned as the `MessageEmpty` placeholder.
    NotFound,
    /// The message exists but carries nothing renderable as an image.
    NoVisualMedia { media_type: String },
    /// Anything else: oversize, decode failure, RPC error.
    Failed(crate::error::Error),
}

/// Per-message result of a batch media download.
///
/// The batch call's own `Result` is reserved for channel-level failures — an
/// unresolvable channel, a failed fetch — where no id could have succeeded.
/// Anything id-specific lands here instead, so one deleted message cannot fail
/// a batch of ten. Mirrors `fanout::ChannelFetchOutcome`, which models the same
/// partial-success shape for the multi-channel search fan-out.
#[derive(Debug)]
pub struct MediaFetchOutcome {
    pub message_id: i32,
    pub result: Result<MediaDownload, MediaFetchError>,
}
```

Neither type derives `Clone` or `PartialEq`: `Error` is not `Clone`, and both are
consumed once.

- [ ] **Step 2: Re-export it**

`src/telegram.rs`, add `MediaFetchError` and `MediaFetchOutcome` to the `pub use types::{...}` list, keeping alphabetical order — both go before the existing `MediaFilter`.

- [ ] **Step 3: Declare the trait method**

`src/telegram/trait_def.rs`, immediately after `download_message_media`:

```rust
    /// Download the visual media of several messages from ONE channel.
    ///
    /// Resolves the peer once and issues a single `get_messages_by_id` for all
    /// ids, then downloads with bounded concurrency — so N images cost one
    /// dialog walk and one fetch RPC rather than N of each.
    ///
    /// `Err` means the whole call failed (empty reference, channel not found,
    /// fetch RPC error). Per-id failures — deleted message, no visual media,
    /// oversize — are reported in the returned `MediaFetchOutcome`s, one per
    /// requested id, in request order.
    async fn download_messages_media(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
        max_dimension: u32,
    ) -> Result<Vec<MediaFetchOutcome>, Error>;
```

Import `MediaFetchOutcome` in that file alongside `MediaDownload`.

- [ ] **Step 4: Add the delegator**

`src/telegram/client.rs`, after the `download_message_media` delegator:

```rust
    async fn download_messages_media(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
        max_dimension: u32,
    ) -> Result<Vec<MediaFetchOutcome>, Error> {
        self.download_messages_media_impl(channel_ref, message_ids, max_dimension)
            .await
    }
```

Add `MediaFetchOutcome` to that file's imports.

- [ ] **Step 5: Implement the batch download**

`src/telegram/client/ops_media.rs`, in the same `impl TelegramClient` block:

```rust
    pub(super) async fn download_messages_media_impl(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
        max_dimension: u32,
    ) -> Result<Vec<MediaFetchOutcome>, Error> {
        use futures::StreamExt as _;

        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }

        // One resolve and one fetch for the whole batch — the point of this
        // method. A numeric channel_ref costs a full dialog walk, so doing it
        // per id is what made the naive loop slow.
        let peer = self.resolve_peer(channel_ref).await?;
        let peer_ref = peer_to_ref(&peer).await?;

        let messages = with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
            self.client
                .get_messages_by_id(peer_ref, message_ids)
                .await
                .map_err(|e| {
                    tracing::error!(
                        channel_ref = %channel_ref,
                        requested = message_ids.len(),
                        error = %e,
                        "Failed to get messages for batch media download"
                    );
                    Error::TelegramApi(format!("Failed to get messages: {}", e))
                })
        })
        .await?;

        // grammers returns one slot per requested id, in request order; a None
        // slot is a deleted or inaccessible message.
        let slots: Vec<(i32, Option<_>)> = message_ids
            .iter()
            .copied()
            .zip(messages.into_iter().chain(std::iter::repeat_with(|| None)))
            .collect();

        let outcomes = futures::stream::iter(slots.into_iter().map(
            |(message_id, slot)| async move {
                // require_found also rejects the MessageEmpty placeholder, so
                // both flavours of "deleted" collapse to NotFound here exactly
                // as they do on the single-message path.
                let result = match require_found(slot, channel_ref, message_id) {
                    Err(_) => Err(MediaFetchError::NotFound),
                    Ok(msg) => self
                        .media_download_from_message(msg, channel_ref, message_id, max_dimension)
                        .await
                        .map_err(|e| match e {
                            Error::NoVisualMedia { media_type } => {
                                MediaFetchError::NoVisualMedia { media_type }
                            }
                            other => MediaFetchError::Failed(other),
                        }),
                };
                MediaFetchOutcome { message_id, result }
            },
        ))
        .buffered(crate::mcp::tools::fanout::FANOUT_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

        tracing::info!(
            channel_ref = %channel_ref,
            requested = outcomes.len(),
            succeeded = outcomes.iter().filter(|o| o.result.is_ok()).count(),
            "Batch media download complete"
        );

        Ok(outcomes)
    }
```

Three details, already verified against the current source:

1. **`require_found`** is `pub(super) fn require_found(fetched: Option<grammers_client::message::Message>, channel_ref: &str, message_id: i32) -> Result<grammers_client::message::Message, Error>` (`src/telegram/client/guard.rs:32`). It is already imported at the top of `ops_media.rs` (`use super::guard::require_found;`). Reusing it — rather than testing `slot.is_none()` — is what makes the batch treat Telegram's `MessageEmpty` placeholder as deleted, exactly as the single path does.
2. **There is no `Error::MessageNotFound` variant.** `guard::not_found` returns `Error::InvalidInput` with the reason in its message text (`guard.rs:18-27`). That is precisely why `MediaFetchError::NotFound` exists: the discrimination is made here, where the information is structural, instead of by string-matching downstream. Do not add a new `Error` variant for this.
3. **The `.chain(repeat_with(|| None))`** guards against grammers returning a short vector. Harmless belt-and-braces if it always returns `message_ids.len()` slots; keep it and keep the comment.

`futures` is already a dependency (used by `impl_search.rs`). `FANOUT_CONCURRENCY` is `pub(crate)`, so it is reachable from `src/telegram/`.

- [ ] **Step 6: Verify it compiles and nothing regressed**

Run: `cargo test`
Expected: PASS. Existing tests are unaffected — `MockTelegramClientTrait` gains a method, but mockall only fails on *called* methods without expectations, and no test calls this one yet.

- [ ] **Step 7: Verify and commit**

Run: `cargo fmt --all && cargo clippy -- -D warnings`
Expected: clean.

```bash
git add src/telegram/types/media.rs src/telegram/trait_def.rs src/telegram/client.rs src/telegram/client/ops_media.rs src/telegram.rs
git commit -m "feat: download_messages_media resolves once and fans out downloads"
```

---

## Task 5: Batch tool — types, validation, happy path

**Files:**
- Modify: `src/mcp/tools/types/requests.rs` (after `GetMessagesBatchRequest`, ends ~line 276)
- Modify: `src/mcp/tools/types/responses.rs`
- Modify: `src/mcp/server/impl_media.rs`
- Modify: `src/mcp/server.rs` (`#[tool]` wrapper after `get_message_media` at line 384; struct field + builder)
- Create: `src/mcp/tests/media_batch.rs`
- Modify: `src/mcp/tests.rs`
- Modify: `src/mcp/tests/server_core.rs:71,99,106` and `src/mcp/tests/schema_integrity.rs:42` (tool count 15 → 16)

**Interfaces:**
- Consumes: `MediaFetchOutcome`, `TelegramClientTrait::download_messages_media` (Task 4).
- Produces:
  - `GetMessagesMediaBatchRequest { channel_id: String, message_ids: Vec<i64>, max_dimension: Option<u32> }`
  - `MediaBatchSummary`, `MediaBatchFailure { id: i64, reason: String }`
  - `McpServer::get_messages_media_batch` tool + `get_messages_media_batch_impl`
  - `impl_media::{DEFAULT_MAX_DIMENSION, MIN_DIMENSION, MAX_DIMENSION, MAX_MEDIA_BATCH_IDS}`
  - `McpServer::media_batch_max_total_bytes` field + `with_media_batch_max_total_bytes(bytes: u64) -> Self`

  Consumed by Tasks 6–8.

This task delivers the tool with the payload cap **reported but not yet enforced**: each image goes through the plain `process_image`, and the summary reports the configured cap truthfully. Task 6 adds enforcement and the config wiring. The server field ships here rather than in Task 6 specifically so no placeholder value ever exists — a summary reporting `max_total_bytes: 0` would be a knowingly-wrong response passing review.

- [ ] **Step 1: Write the failing tests**

Create `src/mcp/tests/media_batch.rs`:

```rust
//! Tests for get_messages_media_batch (work-order C).

use crate::error::Error;
use crate::mcp::server::McpServer;
use crate::mcp::tools::{GetMessagesMediaBatchRequest, GetMessageMediaResponse, MediaBatchSummary};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{MediaDownload, MediaFetchError, MediaFetchOutcome, MediaType};
use crate::test_helpers::create_test_jpeg;
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ContentBlock, NumberOrString};
use std::sync::Arc;

fn photo_download(width: u32, height: u32) -> MediaDownload {
    let bytes = create_test_jpeg(width, height);
    let source_size_bytes = bytes.len() as u64;
    MediaDownload {
        bytes,
        media_type: MediaType::Photo,
        is_thumbnail: false,
        caption: Some("benchmark chart".to_string()),
        width: Some(width),
        height: Some(height),
        source_size_bytes,
        video_info: None,
        largest_width: None,
        largest_height: None,
    }
}

fn ok_outcome(message_id: i32, width: u32, height: u32) -> MediaFetchOutcome {
    MediaFetchOutcome {
        message_id,
        result: Ok(photo_download(width, height)),
    }
}

fn err_outcome(message_id: i32, error: MediaFetchError) -> MediaFetchOutcome {
    MediaFetchOutcome {
        message_id,
        result: Err(error),
    }
}

fn no_media(message_id: i32) -> MediaFetchOutcome {
    err_outcome(
        message_id,
        MediaFetchError::NoVisualMedia {
            media_type: "document".to_string(),
        },
    )
}

fn not_found(message_id: i32) -> MediaFetchOutcome {
    err_outcome(message_id, MediaFetchError::NotFound)
}

fn request(channel: &str, ids: Vec<i64>) -> GetMessagesMediaBatchRequest {
    GetMessagesMediaBatchRequest {
        channel_id: channel.to_string(),
        message_ids: ids,
        max_dimension: None,
    }
}

/// A limiter that accepts anything — charging is Task 7's subject.
fn permissive_limiter() -> MockRateLimiterTrait {
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().returning(|_| Ok(()));
    limiter.expect_refund().return_const(());
    limiter
}

fn summary_of(content: &[ContentBlock]) -> MediaBatchSummary {
    let ContentBlock::Text(text) = content.last().expect("summary block") else {
        panic!("last content block must be the summary text block");
    };
    serde_json::from_str(&text.text).expect("summary must be valid JSON")
}

#[tokio::test]
async fn mixed_batch_returns_images_and_reports_failures() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .withf(|channel, ids, max_dim| {
            channel == "news" && ids == [10, 11, 12, 13] && *max_dim == 1280
        })
        .return_once(|_, _, _| {
            Ok(vec![
                ok_outcome(10, 200, 100),
                no_media(11),
                ok_outcome(12, 160, 160),
                not_found(13),
            ])
        });

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11, 12, 13])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("a batch with per-id failures must still succeed");

    // Two image/metadata pairs, then the summary.
    assert_eq!(result.content.len(), 5);
    assert!(matches!(result.content[0], ContentBlock::Image(_)));
    assert!(matches!(result.content[1], ContentBlock::Text(_)));
    assert!(matches!(result.content[2], ContentBlock::Image(_)));
    assert!(matches!(result.content[3], ContentBlock::Text(_)));

    let summary = summary_of(&result.content);
    assert_eq!(summary.requested, 4);
    assert_eq!(summary.returned, 2);
    assert_eq!(summary.failed.len(), 2);
    assert_eq!(summary.failed[0].id, 11);
    assert_eq!(summary.failed[0].reason, "no_visual_media");
    assert_eq!(summary.failed[1].id, 13);
    assert_eq!(summary.failed[1].reason, "not_found");
}

#[tokio::test]
async fn metadata_blocks_are_adjacent_to_their_images_in_request_order() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .return_once(|_, _, _| Ok(vec![ok_outcome(10, 200, 100), ok_outcome(11, 160, 160)]));

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("tool should succeed");

    let ContentBlock::Text(first) = &result.content[1] else {
        panic!("block 1 must be metadata");
    };
    let first: GetMessageMediaResponse =
        serde_json::from_str(&first.text).expect("metadata must be valid JSON");
    assert_eq!(first.message_id, 10);

    let ContentBlock::Text(second) = &result.content[3] else {
        panic!("block 3 must be metadata");
    };
    let second: GetMessageMediaResponse =
        serde_json::from_str(&second.text).expect("metadata must be valid JSON");
    assert_eq!(second.message_id, 11);
}

#[tokio::test]
async fn channel_level_failure_fails_the_call() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_download_messages_media().return_once(|_, _, _| {
        Err(Error::InvalidInput("Channel not found: nope".to_string()))
    });

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
    let result = server
        .get_messages_media_batch(
            Parameters(request("nope", vec![10, 11])),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let err = result.expect_err("an unresolvable channel is not a per-id failure");
    assert!(err.contains("Channel not found"));
}

#[tokio::test]
async fn empty_message_ids_is_rejected_without_a_network_call() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_download_messages_media().never();

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![])),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    assert!(result.expect_err("empty ids").contains("at least one id"));
}

#[tokio::test]
async fn more_than_ten_ids_is_rejected() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_download_messages_media().never();

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", (1..=11).collect())),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let err = result.expect_err("11 ids exceeds the cap");
    assert!(err.contains("at most 10"), "error must state the cap: {err}");
}

#[tokio::test]
async fn duplicate_ids_are_deduped_preserving_first_seen_order() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .withf(|_, ids, _| ids == [12, 10])
        .return_once(|_, _, _| Ok(vec![ok_outcome(12, 80, 80), ok_outcome(10, 80, 80)]));

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![12, 10, 12])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("tool should succeed");

    assert_eq!(summary_of(&result.content).requested, 2);
}

#[tokio::test]
async fn max_dimension_is_clamped_to_the_supported_range() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .withf(|_, _, max_dim| *max_dim == 2048)
        .return_once(|_, _, _| Ok(vec![ok_outcome(10, 80, 80)]));

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
    let mut req = request("news", vec![10]);
    req.max_dimension = Some(99_999);
    let result = server
        .get_messages_media_batch(Parameters(req), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn batch_of_one_matches_the_single_tool_metadata() {
    use crate::mcp::tools::GetMessageMediaRequest;

    let mut batch_client = MockTelegramClientTrait::new();
    batch_client
        .expect_download_messages_media()
        .return_once(|_, _, _| Ok(vec![ok_outcome(42, 200, 100)]));
    let batch_server = McpServer::new(Arc::new(batch_client), Arc::new(permissive_limiter()));
    let batch = batch_server
        .get_messages_media_batch(
            Parameters(request("news", vec![42])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("batch should succeed");

    let mut single_client = MockTelegramClientTrait::new();
    single_client
        .expect_download_message_media()
        .return_once(|_, _, _| Ok(photo_download(200, 100)));
    let single_server = McpServer::new(Arc::new(single_client), Arc::new(permissive_limiter()));
    let single = single_server
        .get_message_media(
            Parameters(GetMessageMediaRequest {
                channel_id: "news".to_string(),
                message_id: 42,
                max_dimension: None,
            }),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("single should succeed");

    let (ContentBlock::Image(batch_img), ContentBlock::Image(single_img)) =
        (&batch.content[0], &single.content[0])
    else {
        panic!("both must lead with an image block");
    };
    assert_eq!(batch_img.data, single_img.data, "image payload must be identical");

    let (ContentBlock::Text(batch_meta), ContentBlock::Text(single_meta)) =
        (&batch.content[1], &single.content[1])
    else {
        panic!("both must follow with a metadata block");
    };
    assert_eq!(
        batch_meta.text, single_meta.text,
        "batch-of-1 metadata must be byte-identical to the single tool's"
    );

    // The batch adds a summary; that is the only permitted difference.
    assert_eq!(batch.content.len(), 3);
    assert_eq!(single.content.len(), 2);
}
```

Declare the module in `src/mcp/tests.rs`, keeping alphabetical order (after `media`):

```rust
#[path = "tests/media_batch.rs"]
mod media_batch;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test media_batch`
Expected: FAIL to compile — `GetMessagesMediaBatchRequest` and `MediaBatchSummary` do not exist.

- [ ] **Step 3: Add the request type**

`src/mcp/tools/types/requests.rs`, after `GetMessagesBatchRequest`:

```rust
/// Request for get_messages_media_batch tool (work-order C)
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetMessagesMediaBatchRequest {
    #[schemars(description = "Channel ID or username (required). All ids must belong to it.")]
    #[serde(deserialize_with = "flexible_string")]
    pub channel_id: String,

    #[schemars(
        description = "Message IDs to fetch media for in one call (1-10). Ids with no visual media, deleted ids, and ids dropped at the payload cap are reported per-id in `failed`, not as an error."
    )]
    pub message_ids: Vec<i64>,

    #[schemars(
        description = "Optional: longest-side pixel limit per image (default: 1280, clamped 64-2048). Images may be downscaled below this to fit the batch payload cap."
    )]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub max_dimension: Option<u32>,
}
```

- [ ] **Step 4: Add the response types**

`src/mcp/tools/types/responses.rs`, after `GetMessageMediaResponse`:

```rust
/// Trailing summary block of a get_messages_media_batch response (work-order C).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MediaBatchSummary {
    #[schemars(description = "Channel the messages belong to (as passed in the request)")]
    pub channel_id: String,

    #[schemars(description = "Ids requested, after de-duplication")]
    pub requested: usize,

    #[schemars(description = "Images actually returned as content blocks")]
    pub returned: usize,

    #[schemars(description = "Ids that produced no image, with a machine-readable reason each")]
    pub failed: Vec<MediaBatchFailure>,

    #[schemars(description = "Total base64 payload returned, in bytes")]
    pub total_base64_bytes: usize,

    #[schemars(description = "Configured cap on total base64 payload, in bytes")]
    pub max_total_bytes: u64,
}

/// One id that produced no image.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MediaBatchFailure {
    #[schemars(description = "The requested message id")]
    pub id: i64,

    #[schemars(
        description = "Why no image was returned: not_found, no_visual_media, payload_cap_reached, or download_failed: <detail>"
    )]
    pub reason: String,
}
```

- [ ] **Step 5: Implement the tool**

`src/mcp/server/impl_media.rs` — hoist the three dimension constants out of `get_message_media_impl` to module scope so both tools share them, and add the batch impl:

```rust
/// Longest-side pixel limit applied when a request omits `max_dimension`.
pub(super) const DEFAULT_MAX_DIMENSION: u32 = 1280;
pub(super) const MIN_DIMENSION: u32 = 64;
pub(super) const MAX_DIMENSION: u32 = 2048;
/// Hard cap on ids per batch media call. Smaller than `MAX_BATCH_IDS` (50)
/// because each id costs a download, not just a row in a response.
pub(super) const MAX_MEDIA_BATCH_IDS: usize = 10;
```

Delete the three `const` lines from inside `get_message_media_impl`; its body otherwise stays as-is.

Add:

```rust
    pub(super) async fn get_messages_media_batch_impl(
        &self,
        request: GetMessagesMediaBatchRequest,
    ) -> Result<CallToolResult, String> {
        if request.channel_id.trim().is_empty() {
            return Err("channel_id is required".to_string());
        }
        if request.message_ids.is_empty() {
            return Err("message_ids must contain at least one id".to_string());
        }

        // Dedupe silently, preserving first-seen order (same rule as
        // get_messages_batch).
        let mut seen = std::collections::HashSet::new();
        let unique: Vec<i64> = request
            .message_ids
            .iter()
            .copied()
            .filter(|id| seen.insert(*id))
            .collect();
        if unique.len() > MAX_MEDIA_BATCH_IDS {
            return Err(format!(
                "message_ids accepts at most {MAX_MEDIA_BATCH_IDS} ids per call, got {}",
                unique.len()
            ));
        }

        let mut wire_ids = Vec::with_capacity(unique.len());
        for id in &unique {
            let parsed = parse_message_id(*id)?;
            wire_ids.push(
                parsed.as_i32().ok_or_else(|| {
                    format!("message_id {} exceeds Telegram's message id range", id)
                })?,
            );
        }

        let max_dimension = request
            .max_dimension
            .unwrap_or(DEFAULT_MAX_DIMENSION)
            .clamp(MIN_DIMENSION, MAX_DIMENSION);

        let outcomes = self
            .telegram_client
            .download_messages_media(&request.channel_id, &wire_ids, max_dimension)
            .await
            .map_err(|e| e.to_string())?;

        let mut content = Vec::new();
        let mut failed = Vec::new();
        let mut total_base64_bytes = 0usize;

        for outcome in outcomes {
            let id = i64::from(outcome.message_id);
            let download = match outcome.result {
                Ok(download) => download,
                Err(e) => {
                    failed.push(MediaBatchFailure {
                        id,
                        reason: failure_reason(&e),
                    });
                    continue;
                }
            };

            let processed = match process_image(&download.bytes, max_dimension) {
                Ok(processed) => processed,
                Err(e) => {
                    failed.push(MediaBatchFailure {
                        id,
                        reason: format!("download_failed: {e}"),
                    });
                    continue;
                }
            };

            total_base64_bytes += processed.base64_jpeg.len();
            let metadata = GetMessageMediaResponse {
                channel_id: request.channel_id.clone(),
                message_id: id,
                media_type: download.media_type,
                is_thumbnail: download.is_thumbnail,
                caption: download.caption,
                source_variant_width: download.width,
                source_variant_height: download.height,
                source_variant_size_bytes: download.source_size_bytes,
                largest_available_width: download.largest_width,
                largest_available_height: download.largest_height,
                returned_width: processed.width,
                returned_height: processed.height,
                returned_size_bytes: processed.encoded_size_bytes,
                mime_type: "image/jpeg".to_string(),
                video_info: download.video_info,
            };
            content.push(ContentBlock::image(processed.base64_jpeg, "image/jpeg"));
            content.push(ContentBlock::text(json_response(&metadata)?));
        }

        let returned = content.len() / 2;
        tracing::info!(
            channel = %request.channel_id,
            requested = unique.len(),
            returned,
            failed = failed.len(),
            total_base64_bytes,
            "Messages media batch results"
        );

        let summary = MediaBatchSummary {
            channel_id: request.channel_id,
            requested: unique.len(),
            returned,
            failed,
            total_base64_bytes,
            max_total_bytes: self.media_batch_max_total_bytes as u64,
        };
        content.push(ContentBlock::text(json_response(&summary)?));

        Ok(CallToolResult::success(content))
    }
```

And, outside the `impl` block at the end of the file:

```rust
/// Map a per-id download failure to a stable, machine-readable reason.
///
/// Callers branch on these tokens, so they are deliberately not the `Display`
/// text of the underlying error — that text is free to change. The match is
/// total, so a new `MediaFetchError` variant is a compile error here rather
/// than a silent fall-through to `download_failed`.
fn failure_reason(error: &MediaFetchError) -> String {
    match error {
        MediaFetchError::NotFound => "not_found".to_string(),
        MediaFetchError::NoVisualMedia { .. } => "no_visual_media".to_string(),
        MediaFetchError::Failed(inner) => format!("download_failed: {inner}"),
    }
}
```

- [ ] **Step 6: Add the tool wrapper**

`src/mcp/server.rs`, after `get_message_media` (which ends at line 398):

```rust
    /// Tool 16: get_messages_media_batch - Return several messages' images in one call
    #[tool(
        description = "Get the photos (or video/animation/video-note thumbnails) of up to 10 messages from ONE channel in a single call, as image blocks the model can see, each followed by its JSON metadata and a trailing batch summary. Far cheaper than N get_message_media calls: one channel resolution and one fetch round trip for the whole batch. Ids with no visual media, deleted ids, and ids dropped at the total payload cap are reported in the summary's `failed` array rather than failing the call. Charged media_download_cost tokens per image actually returned."
    )]
    pub async fn get_messages_media_batch(
        &self,
        Parameters(request): Parameters<GetMessagesMediaBatchRequest>,
        id: RequestId,
    ) -> Result<CallToolResult, String> {
        let inv = ToolInvocation::start("get_messages_media_batch", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            channel_id = %request.channel_id,
            requested = request.message_ids.len(),
            max_dimension = ?request.max_dimension,
            "Tool invocation started"
        );
        inv.finish(self.get_messages_media_batch_impl(request).await)
    }
```

Update the tool count in the `src/mcp/tools.rs` doc comment ("all 15 MCP tools" → 16) and in `CLAUDE.md`'s "All 15 tools live in `server.rs`" line.

- [ ] **Step 6b: Add the server field and builder**

`src/mcp/server.rs` — add to `pub struct McpServer` after `response_byte_budget`:

```rust
    media_batch_max_total_bytes: usize,
```

Initialize it in `new()` after `response_byte_budget`:

```rust
            media_batch_max_total_bytes: 8 * 1024 * 1024,
```

Add the builder next to `with_response_byte_budget`:

```rust
    /// Set the total base64 payload cap for `get_messages_media_batch`
    /// (`[limits] media_batch_max_total_bytes`, default 8 MiB).
    pub fn with_media_batch_max_total_bytes(mut self, bytes: u64) -> Self {
        self.media_batch_max_total_bytes = bytes as usize;
        self
    }
```

The field is reported in the summary from this task on, so no response ever
carries a placeholder. Task 6 adds the enforcement that makes it binding.

- [ ] **Step 6c: Update the tool-count assertions**

A new tool legitimately changes these. Update 15 → 16 at:
`src/mcp/tests/server_core.rs:71`, `:99`, `:106` (and the comment at `:70`),
and `src/mcp/tests/schema_integrity.rs:42` (including its message text).

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test media_batch`
Expected: PASS, 8 tests.

- [ ] **Step 8: Verify and commit**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: all green, including the updated tool-count assertions.

```bash
git add src/mcp/tools/types/requests.rs src/mcp/tools/types/responses.rs src/mcp/server/impl_media.rs src/mcp/server.rs src/mcp/tests/media_batch.rs src/mcp/tests.rs src/mcp/tests/server_core.rs src/mcp/tests/schema_integrity.rs src/mcp/tools.rs CLAUDE.md
git commit -m "feat: get_messages_media_batch returns up to 10 images per call"
```

---

## Task 6: Apply the payload cap

**Files:**
- Modify: `src/mcp/server.rs` (field + builder)
- Modify: `src/mcp/server/impl_media.rs`
- Modify: `src/main.rs:107-115`
- Test: `src/mcp/tests/media_batch.rs`

**Interfaces:**
- Consumes: `Base64Budget`, `MIN_IMAGE_BASE64_BYTES`, `process_image_with_cap` (Task 2); `get_messages_media_batch_impl` (Task 5).
- Produces: `McpServer::with_media_batch_max_total_bytes(bytes: u64) -> Self`.

- [ ] **Step 1: Write the failing cap tests**

Append to `src/mcp/tests/media_batch.rs`:

```rust
#[tokio::test]
async fn payload_cap_downscales_then_reports_cap_reached() {
    // Three sizeable photos against a cap that fits roughly one of them.
    let mut client = MockTelegramClientTrait::new();
    client.expect_download_messages_media().return_once(|_, _, _| {
        Ok(vec![
            ok_outcome(10, 1200, 1200),
            ok_outcome(11, 1200, 1200),
            ok_outcome(12, 1200, 1200),
        ])
    });

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()))
        .with_media_batch_max_total_bytes(400_000);
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11, 12])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("hitting the cap is not an error");

    let summary = summary_of(&result.content);
    assert!(
        summary.total_base64_bytes <= 400_000,
        "cap must hold: {} bytes returned",
        summary.total_base64_bytes
    );
    assert_eq!(summary.max_total_bytes, 400_000);
    assert!(summary.returned >= 1, "at least one image must come back");
    assert!(
        summary.failed.iter().any(|f| f.reason == "payload_cap_reached"),
        "ids dropped at the cap must say so: {:?}",
        summary.failed
    );
    assert_eq!(
        summary.returned + summary.failed.len(),
        3,
        "every requested id must be accounted for"
    );
}

#[tokio::test]
async fn cap_reached_ids_are_reported_in_request_order() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_download_messages_media().return_once(|_, _, _| {
        Ok(vec![
            ok_outcome(10, 1200, 1200),
            ok_outcome(11, 1200, 1200),
            ok_outcome(12, 1200, 1200),
        ])
    });

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()))
        .with_media_batch_max_total_bytes(400_000);
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11, 12])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("tool should succeed");

    let summary = summary_of(&result.content);
    let capped: Vec<i64> = summary
        .failed
        .iter()
        .filter(|f| f.reason == "payload_cap_reached")
        .map(|f| f.id)
        .collect();
    let mut sorted = capped.clone();
    sorted.sort_unstable();
    assert_eq!(capped, sorted, "cap failures follow request order");
}

#[tokio::test]
async fn a_generous_cap_returns_every_image() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .return_once(|_, _, _| Ok(vec![ok_outcome(10, 200, 100), ok_outcome(11, 200, 100)]));

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()))
        .with_media_batch_max_total_bytes(8 * 1024 * 1024);
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("tool should succeed");

    let summary = summary_of(&result.content);
    assert_eq!(summary.returned, 2);
    assert!(summary.failed.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test media_batch`
Expected: FAIL — `no method named 'with_media_batch_max_total_bytes'`.

- [ ] **Step 3: Confirm the server field exists**

`McpServer::media_batch_max_total_bytes` and `with_media_batch_max_total_bytes`
were added in Task 5 so the summary never reported a placeholder. Verify they
are present in `src/mcp/server.rs`; if so this step is a no-op. This task makes
the value *binding* rather than merely reported.

- [ ] **Step 4: Apply the budget in the tool**

`src/mcp/server/impl_media.rs`, in `get_messages_media_batch_impl`, replace the processing loop's body. Before the loop:

```rust
        let mut budget = Base64Budget::new(self.media_batch_max_total_bytes);
```

Replace the `process_image` call and what follows it with:

```rust
            // Encoding runs in request order, so allocation is deterministic no
            // matter which download finished first.
            let Some(allowance) = budget.allowance() else {
                failed.push(MediaBatchFailure {
                    id,
                    reason: "payload_cap_reached".to_string(),
                });
                continue;
            };

            // process_image_with_cap already shrinks the target dimension
            // iteratively until the encoded payload fits — that is the
            // progressive downscaling, no second implementation needed.
            let processed = match process_image_with_cap(&download.bytes, max_dimension, allowance)
            {
                Ok(processed) => processed,
                Err(e) => {
                    // Budget deliberately untouched: a failed image cost nothing,
                    // so later ids keep their full allowance.
                    failed.push(MediaBatchFailure {
                        id,
                        reason: format!("download_failed: {e}"),
                    });
                    continue;
                }
            };
            budget.consume(processed.base64_jpeg.len());
```

The summary's `max_total_bytes` already reads the field (Task 5) — leave it.

Import at the top of the file (or rely on `use super::*` if the parent already re-exports them — check first):

```rust
use crate::mcp::tools::image::process_image_with_cap;
use crate::mcp::tools::media_budget::Base64Budget;
```

`get_message_media_impl` keeps calling plain `process_image`; the single-image tool has no batch budget.

- [ ] **Step 5: Wire the config value**

`src/main.rs`, extend the builder chain that already ends with `.with_response_byte_budget(config.limits.response_byte_budget)`:

```rust
        .with_response_byte_budget(config.limits.response_byte_budget)
        .with_media_batch_max_total_bytes(config.limits.media_batch_max_total_bytes);
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test media_batch`
Expected: PASS, 11 tests.

If `payload_cap_downscales_then_reports_cap_reached` returns 3 images rather than triggering the cap, `create_test_jpeg(1200, 1200)` compresses smaller than assumed. Lower the cap in the test until at least one id is dropped — the assertion of interest is the behaviour, not the specific number.

- [ ] **Step 7: Verify and commit**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: all green.

```bash
git add src/mcp/server.rs src/mcp/server/impl_media.rs src/main.rs src/mcp/tests/media_batch.rs
git commit -m "feat: bound get_messages_media_batch by a total base64 payload cap"
```

---

## Task 7: Charge and refund

**Files:**
- Modify: `src/mcp/server/impl_media.rs`
- Test: `src/mcp/tests/media_batch.rs`

**Interfaces:**
- Consumes: `RateLimiterTrait::refund` (Task 1), `self.media_download_cost` (existing field).
- Produces: nothing new.

- [ ] **Step 1: Write the failing charging tests**

Append to `src/mcp/tests/media_batch.rs`:

```rust
use mockall::predicate::eq;

#[tokio::test]
async fn charges_for_every_requested_id_then_refunds_the_failures() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_download_messages_media().return_once(|_, _, _| {
        Ok(vec![
            ok_outcome(10, 80, 80),
            no_media(11),
            ok_outcome(12, 80, 80),
            not_found(13),
            ok_outcome(14, 80, 80),
        ])
    });

    let mut limiter = MockRateLimiterTrait::new();
    // 5 requested x default cost 3 = 15 acquired up front.
    limiter.expect_acquire().with(eq(15)).times(1).returning(|_| Ok(()));
    // 2 produced nothing x 3 = 6 refunded.
    limiter.expect_refund().with(eq(6)).times(1).return_const(());

    let server = McpServer::new(Arc::new(client), Arc::new(limiter));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11, 12, 13, 14])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("tool should succeed");

    assert_eq!(summary_of(&result.content).returned, 3);
}

#[tokio::test]
async fn a_fully_successful_batch_refunds_nothing() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .return_once(|_, _, _| Ok(vec![ok_outcome(10, 80, 80), ok_outcome(11, 80, 80)]));

    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().with(eq(6)).times(1).returning(|_| Ok(()));
    limiter.expect_refund().with(eq(0)).times(1).return_const(());

    let server = McpServer::new(Arc::new(client), Arc::new(limiter));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11])),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn a_rejected_acquire_performs_no_download() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_download_messages_media().never();

    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().returning(|_| {
        Err(Error::RateLimit {
            retry_after_seconds: 4,
            detail: ": requested 30 tokens, 12.00 available".to_string(),
        })
    });
    limiter.expect_refund().never();

    let server = McpServer::new(Arc::new(client), Arc::new(limiter));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", (1..=10).collect())),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let err = result.expect_err("an unaffordable batch must be refused before any work");
    assert!(
        err.contains("retry after 4 seconds"),
        "the rate-limit error must carry the wait hint: {err}"
    );
}
```

Remove `expect_refund().return_const(())` from `permissive_limiter()`? No — leave it; mockall permits an unmatched-but-declared expectation with no `times` constraint.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test media_batch`
Expected: FAIL — the acquire expectation `with(eq(15))` is unmet because the impl does not acquire at all yet.

- [ ] **Step 3: Add charge and refund**

`src/mcp/server/impl_media.rs`, in `get_messages_media_batch_impl`. Immediately after the `wire_ids` loop and the `max_dimension` clamp, before the download call:

```rust
        // Acquire pessimistically for every requested id BEFORE any network
        // work: charging only for what succeeds would mean the limiter could
        // never refuse a batch, since the downloads would already have happened.
        // One atomic acquire keeps the D5 deficit message accurate.
        let charged = self.media_download_cost * unique.len() as u32;
        self.rate_limiter
            .acquire(charged)
            .await
            .map_err(|e| e.to_string())?;
```

After `returned` is computed and before building the summary:

```rust
        // Ids that produced no image cost nothing — hand their tokens back.
        // The bucket clamps at capacity, so this can never inflate it.
        let refunded = self.media_download_cost * (unique.len() - returned) as u32;
        self.rate_limiter.refund(refunded);
```

`unique.len() >= returned` always holds — `returned` counts a subset of the requested ids — so the subtraction cannot underflow. Add that as a comment.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test media_batch`
Expected: PASS, 14 tests.

- [ ] **Step 5: Add the retry-after regression test**

Append to `src/mcp/tests/media_batch.rs`:

```rust
#[test]
fn rate_limit_errors_carry_a_retry_hint() {
    // Pre-existing behaviour (src/error.rs). Pinned here because batch callers
    // are the ones most likely to hit the limiter and need a precise wait.
    let error = Error::RateLimit {
        retry_after_seconds: 7,
        detail: ": requested 30 tokens, 9.00 available".to_string(),
    };
    assert_eq!(
        error.to_string(),
        "rate limit exceeded: requested 30 tokens, 9.00 available, retry after 7 seconds"
    );
}
```

- [ ] **Step 6: Run tests and commit**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: all green.

```bash
git add src/mcp/server/impl_media.rs src/mcp/tests/media_batch.rs
git commit -m "feat: charge media batches per requested id and refund failures"
```

---

## Task 8: Report media limits in check_mcp_status

**Files:**
- Modify: `src/mcp/tools/types/responses.rs`
- Modify: `src/mcp/server/impl_status.rs:13-33`
- Test: `src/mcp/tests/status.rs`
- Modify: `src/mcp/tools/types/tests/responses_tests.rs:5` — this file constructs a `StatusResponse` by struct literal, so adding a field breaks it. Add a `MediaLimits` value there; do **not** give the new field `#[serde(default)]` to dodge the compile error, since the field must always be present on the wire.

**Interfaces:**
- Consumes: `MAX_MEDIA_BATCH_IDS`, `DEFAULT_MAX_DIMENSION`, `MIN_DIMENSION`, `MAX_DIMENSION` (Task 5); `media_batch_max_total_bytes` (Task 6); `image::MAX_BASE64_LEN`.
- Produces: `StatusResponse::media: MediaLimits`.

- [ ] **Step 1: Write the failing test**

Append to `src/mcp/tests/status.rs` (reuse whatever mock-construction helper that file already defines rather than writing a new one):

```rust
#[tokio::test]
async fn status_reports_media_batch_limits_from_config() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_connected().returning(|| true);
    client.expect_is_premium().returning(|| Some(true));

    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_available_tokens().returning(|| 60.0);
    limiter.expect_capacity().returning(|| 60.0);
    limiter.expect_refill_rate().returning(|| 2.0);

    let server = McpServer::new(Arc::new(client), Arc::new(limiter))
        .with_media_batch_max_total_bytes(1_234_567);
    let json = server
        .check_mcp_status(RequestId(NumberOrString::Number(1)))
        .await
        .expect("status should succeed");
    let status: StatusResponse = serde_json::from_str(&json).expect("valid JSON");

    assert_eq!(status.media.batch_max_ids, 10);
    assert_eq!(
        status.media.max_total_bytes, 1_234_567,
        "the cap must come from config, not a hardcoded literal"
    );
    assert_eq!(status.media.per_image_max_bytes, 1_572_864);
    assert_eq!(status.media.default_max_dimension, 1280);
    assert_eq!(status.media.max_dimension_limit, 2048);
}
```

Match the existing file's imports and the exact `check_mcp_status` call signature — read the neighbouring tests first.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test status`
Expected: FAIL — `no field 'media' on type 'StatusResponse'`.

- [ ] **Step 3: Add the type**

`src/mcp/tools/types/responses.rs`, after `RateLimiterCosts`:

```rust
/// Media retrieval limits, so a caller can plan a run instead of discovering
/// the limits by hitting them (work-order C).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MediaLimits {
    #[schemars(description = "Maximum message_ids per get_messages_media_batch call")]
    pub batch_max_ids: usize,

    #[schemars(description = "Cap on a batch's total base64 payload, in bytes")]
    pub max_total_bytes: u64,

    #[schemars(description = "Cap on one image's base64 payload, in bytes")]
    pub per_image_max_bytes: usize,

    #[schemars(description = "Longest-side pixel limit applied when max_dimension is omitted")]
    pub default_max_dimension: u32,

    #[schemars(description = "Largest max_dimension a request may ask for")]
    pub max_dimension_limit: u32,
}
```

Add the field to `StatusResponse`, after `rate_limiter`:

```rust
    #[schemars(description = "Media retrieval limits: batch size, payload caps, dimension bounds")]
    pub media: MediaLimits,
```

Placing it after `rate_limiter` and before `server_version` is purely cosmetic in JSON; the addition is backward compatible either way.

- [ ] **Step 4: Populate it**

`src/mcp/server/impl_status.rs`, in the `StatusResponse` literal after the `rate_limiter` block:

```rust
            media: MediaLimits {
                batch_max_ids: super::impl_media::MAX_MEDIA_BATCH_IDS,
                max_total_bytes: self.media_batch_max_total_bytes as u64,
                per_image_max_bytes: crate::mcp::tools::image::MAX_BASE64_LEN,
                default_max_dimension: super::impl_media::DEFAULT_MAX_DIMENSION,
                max_dimension_limit: super::impl_media::MAX_DIMENSION,
            },
```

The `impl_media` constants are `pub(super)`, and `impl_status` is a sibling under the same parent, so the path resolves. If it does not, promote the constants to `pub(crate)` rather than duplicating the literals — duplicating them is exactly what this task exists to prevent.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test status`
Expected: PASS.

- [ ] **Step 6: Verify and commit**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: all green.

```bash
git add src/mcp/tools/types/responses.rs src/mcp/server/impl_status.rs src/mcp/tests/status.rs
git commit -m "feat: check_mcp_status reports media batch and payload limits"
```

---

## Task 9: Documentation

**Files:**
- Modify: `README.md`
- Modify: `CHANGELOG.md`
- Modify: `docs/tasklist.md`
- Modify: `docs/memory.md`

- [ ] **Step 1: Add the tool to the README reference**

Find the tool reference table/section (search for `get_message_media`) and add `get_messages_media_batch` alongside it, with a response example showing the interleaved block layout and a `failed` entry. State plainly that the batch is the preferred path for more than one image, and why: one channel resolution and one fetch RPC for the batch versus one of each per call.

- [ ] **Step 2: Write the CHANGELOG entry**

Under `## [Unreleased]`:

```markdown
### Added
- `get_messages_media_batch` returns the images of up to 10 messages from one
  channel in a single call — image block plus metadata block per message, then
  a summary carrying `requested`/`returned`/`failed`/`total_base64_bytes`.
  The batch resolves the channel once and issues one `get_messages_by_id` for
  every id, then downloads with bounded concurrency (4), so N images cost one
  channel resolution and one fetch round trip instead of N of each. For a
  numeric `channel_id` a resolution is a full dialog walk, which is what made
  the per-call path expensive.

  Per-id failures — `not_found`, `no_visual_media`, `payload_cap_reached`,
  `download_failed` — are reported in `failed` and never fail the batch.

- `[limits] media_batch_max_total_bytes` (default 8 MiB) caps a batch's total
  image payload, counted in bytes of base64 as sent to the client. Images are
  downscaled progressively to fit; ids that still do not fit are reported as
  `payload_cap_reached`.

- `check_mcp_status` gains a `media` block (`batch_max_ids`, `max_total_bytes`,
  `per_image_max_bytes`, `default_max_dimension`, `max_dimension_limit`).

### Changed
- `[rate_limiting]` defaults retuned for batch media: `max_tokens` 50 → 60 and
  `media_download_cost` 5 → 3. At the unchanged `refill_rate` of 2.0/sec that
  is 20 images in a burst then one per 1.5 s, up from 10 then one per 2.5 s.
  Batches acquire for every requested id up front and refund the ids that
  produced no image, so admission control stays real while the net charge is
  per image returned. Both values are conservative estimates, not calibrated
  against Telegram's flood thresholds.
```

Do **not** list `retry_after_seconds` as new — it has shipped since the D5 work (`src/error.rs`). Task 7 only pins it with a regression test.

- [ ] **Step 3: Add the tasklist row**

`docs/tasklist.md` — add a row to the phase table following the format of rows 34 and 35 (number, name, status, test count, summary). Use the real post-implementation test count from `cargo test`, not an estimate.

- [ ] **Step 4: Record what was learned**

`docs/memory.md` — record two things:

1. The work order's rate-limit premise (capacity 30, refill 1/sec) did not match `src/config/defaults.rs` (50, 2.0). Work orders quote values that drift; verify against `defaults.rs` before designing around them.
2. `resolve_peer` with a numeric channel id walks the whole dialog list with no cache (`src/telegram/client/resolve.rs`). Any per-message loop over a client method that resolves internally pays that per iteration — this is the reason the batch lives at the client layer rather than the MCP layer.

- [ ] **Step 5: Commit**

```bash
git add README.md CHANGELOG.md docs/tasklist.md docs/memory.md
git commit -m "docs: media throughput — README, changelog, tasklist, memory"
```

---

## Task 10: Live acceptance

**Files:** none — verification only.

Requires an authenticated Telegram session (see the standing note in `docs/memory.md`). If none is available, **stop and report that this task is unrun** rather than marking it done. Do not infer results from the unit tests.

- [ ] **Step 1: Full gate**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all green.

- [ ] **Step 2: Measure the before case**

Against a channel with at least 10 visual posts, time 10 sequential `get_message_media` calls by **numeric** channel id (numeric is the slow resolve path — a username would understate the gain). Record total wall time.

- [ ] **Step 3: Measure the after case**

Time one `get_messages_media_batch` call for the same 10 ids and the same numeric channel id. Record wall time, `returned`, `failed`, and `total_base64_bytes`.

- [ ] **Step 4: Confirm the round-trip claim**

With `RUST_LOG=debug`, confirm the batch logs one dialog walk and one `get_messages_by_id`, not ten of each.

- [ ] **Step 5: Confirm the cap**

Request 10 large photos against a deliberately small `media_batch_max_total_bytes` and confirm the response downscales, then reports `payload_cap_reached`, and that `total_base64_bytes` is under the cap.

- [ ] **Step 6: Record the measurements**

Add the before/after numbers to the CHANGELOG entry, in the table style v0.21.0 used.

```bash
git add CHANGELOG.md docs/memory.md
git commit -m "docs: record live acceptance measurements for media batch"
```

---

## Self-Review

**Spec coverage:**

| Spec section | Task |
|---|---|
| §1 client layer, resolve/fetch once | 3, 4 |
| §1 testability limit stated | 4 (step preamble) |
| §2 request type, validation, dedupe, dimension clamp | 5 |
| §3 payload budget, floor, failed-image invariant | 2, 6 |
| §4 interleaved blocks, summary, reason tokens | 5 |
| §5 refund, acquire-then-refund, config retune | 1, 7 |
| §5 `retry_after_seconds` pre-existing, regression-tested | 7 |
| §6 status media block | 8 |
| Testing plan | 1, 2, 5, 6, 7, 8 |
| Verification (live acceptance) | 10 |
| Documentation | 1, 2, 9 |

No spec requirement is unassigned.

**Type consistency:** `MediaFetchOutcome { message_id: i32, result }` (Task 4) is consumed in Task 5 as `outcome.message_id` widened to `i64` for `MediaBatchFailure { id: i64 }` and `GetMessageMediaResponse { message_id: i64 }` — matches the existing response type. `Base64Budget::{new, allowance, consume}` (Task 2) are the exact names called in Task 6. `with_media_batch_max_total_bytes` (Task 6) is the name used in the Task 6 and Task 8 tests. `MAX_MEDIA_BATCH_IDS` (Task 5) is referenced in Task 8.

**Divergence from the spec, resolved during plan review:** the spec sketches
`MediaFetchOutcome.result` as `Result<MediaDownload, Error>`. That does not
work — there is no `Error::MessageNotFound`; `guard::not_found` returns
`Error::InvalidInput` with the reason only in its message text, so the MCP
layer could produce the `not_found` reason token only by string-matching prose.
The plan introduces `MediaFetchError { NotFound, NoVisualMedia, Failed }`
instead, discriminating where the information is structural. The spec has been
updated to match.

**Verified against source while writing, not assumed:** `require_found`'s
signature and import (`guard.rs:32`), the absence of `Error::MessageNotFound`
(`guard.rs:18`), `Error::NoVisualMedia { media_type: String }`
(`error.rs:38-39`), `check_mcp_status(&self, id: RequestId) -> Result<String,
String>` (`server.rs:202`), and `src/mcp/tests/status.rs`'s existing imports and
mock-setup style.
