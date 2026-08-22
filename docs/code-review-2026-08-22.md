# Code Review Findings — 2026-08-22

Full review of telegram-connector v0.22.5 (~27k LOC): server/tools layer, telegram client,
config, observability, rate limiter, link parsing.

**Verification basis:** `cargo fmt --check` ✅ · `cargo clippy --all-targets -- -D warnings` ✅ ·
manual line-by-line review with grep cross-checks.

**Verdict:** no critical bugs. The codebase is disciplined (poison-recovery locks, hard
timeouts on all MTProto calls, thorough untrusted-input validation, secrets behind
`SecretString`, zero `unwrap()` in production paths). The real problems are silent
configuration dead-zones and dead public surface.

---

## High severity

### H1. Dead `[search]` config keys advertised to users

- **Where:** `src/config.rs:151-164`, `src/config/defaults.rs:36-46`, `README.md:77-79`
- **Description:** `search.default_hours_back`, `search.max_results_default`, and
  `search.max_results_limit` are deserialized and stored but read by **no runtime code**
  (verified by grep; only config defaults and tests touch them). Actual behavior comes from
  compiled-in constants in `src/telegram/types/params.rs` (`SearchParams::DEFAULT_HOURS_BACK = 48`,
  `MAX_LIMIT = 100`, …), applied via request-level clamps in `src/mcp/server/impl_search.rs:54-62`.
  Only `[search] deadline_seconds` is consumed (`src/telegram/client/lifecycle.rs`).
  A user who sets `default_hours_back = 200` gets silently no effect — and the README documents
  these keys as working configuration.
- **Fix:** delete the three fields, their default fns (`default_hours_back()`,
  `default_max_results_default()`, `default_max_results_limit()` at `defaults.rs:36-46`), their
  entries in `default_search_config()` (`defaults.rs:148-150`), the associated tests, and the
  README lines. The constants in `params.rs` are already the single source of truth.
  (Wiring them through instead would be a behavior change needing a separate decision.)

### H2. `refill_rate ≤ 0` is not validated — permanent lockout with nonsensical retry hint

- **Where:** `src/rate_limiter.rs:44`; missing from `RateLimitConfig::validate()` at `src/config.rs:205-223`
- **Description:**
  ```rust
  let retry_after = (tokens_needed / self.refill_rate).ceil() as u64;
  ```
  With `refill_rate = 0.0` the division yields `inf`; the cast saturates to `u64::MAX`, so every
  rejected call reports *"retry after 18446744073709551615 seconds"* while the bucket never
  refills — every call fails forever. A negative value is worse: `TokenBucket::refill()` actively
  drains tokens over time. `validate()` currently checks only that per-call costs don't exceed
  `max_tokens`.
- **Fix:** add to `RateLimitConfig::validate()`:
  ```rust
  if self.refill_rate <= 0.0 {
      anyhow::bail!("rate_limiting.refill_rate must be > 0");
  }
  ```

---

## Medium severity

### M1. Dead error variants: `Error::Config`, `Error::Network`, `Error::Mcp`

- **Where:** `src/error.rs:20-27`
- **Description:** constructed nowhere outside their own Display tests (`error.rs:111/117/123`;
  confirmed across the tree). All real errors route through the other variants or `anyhow`
  at the app boundary. Each dead variant drags a Display test asserting behavior of code
  nothing calls.
- **Fix:** delete the three variants and their three tests (`test_config_error_display`,
  `test_network_error_display`, `test_mcp_error_display`). Git history keeps them if needed later.

### M2. Unmetered Telegram RPC in single-channel search path

- **Where:** `src/mcp/server/impl_search.rs:157-172`
- **Description:** the single/global path resolves a username (`resolve_channel_identity` → a real
  Telegram RPC) **before** `acquire(1)`, so resolution work bypasses the rate limiter entirely.
  The fan-out path does the same resolve *inside* its post-acquire fetch closure
  (`impl_search.rs:117-134`), and `get_messages_media_batch_impl` explicitly documents the
  opposite policy ("work happened and should not be free"). Inconsistent admission control:
  repeated failing searches can hammer username-resolution RPCs at zero token cost.
- **Fix:** move `acquire(1)` before the `search_channel_id` call (matching fan-out ordering),
  or document why resolution is deliberately free.

### M3. Fan-out charges for failed channels — no refund, unlike batch tools

- **Where:** `src/mcp/server/impl_search.rs:112-115`, `fanout::merge_results`
- **Description:** `acquire(list.len())` is atomic and final: if 15 of 20 channels fail to resolve
  or fetch, all 20 tokens stay spent (failures become `channel_errors`). This contrasts with
  `get_messages_media_batch_impl` (`src/mcp/server/impl_media.rs:127,159-162`) which refunds
  pessimistically-acquired tokens for ids that produced nothing, per its documented philosophy.
  Same inconsistency in `get_recent_messages_impl` fan-out (`impl_search.rs:288-291`).
- **Fix:** either refund `failed_count` after merge (needs a count returned from
  `merge_results`), or document the asymmetry as intended (partial results from other channels
  still justify full charge).

### M4. Dead builder API on domain params

- **Where:** `src/telegram/types/params.rs:44-70` (`SearchParams::new`, `impl Default`),
  `params.rs:115` (`HistoryParams::new`), `params.rs:136-158` (four `HistoryParams` builders)
- **Description:** production builds both structs exclusively via struct literals
  (`impl_search.rs:79-90, 255-266, 341-345`). The builders' clamping logic duplicates clamps the
  MCP layer already applies — and worse, they are not a correct-by-construction seam: a caller
  skipping them gets no clamp. Only `params.rs`'s own tests use them.
- **Fix:** delete the four builder methods and unused constructors; convert remaining tests to
  struct literals.

### M5. Test-only / dead public helpers

- **Where & evidence** (zero non-test callers verified):
  - `logging::redact_hash` — `src/logging.rs:126` (sibling `redact_phone` *is* used, `auth.rs:29`)
  - `Message::is_recent` / `Message::is_text_only` — `src/telegram/types/entities.rs:59,65`
  - `ResponseBuffer::is_empty` — `src/mcp/observability/buffer.rs:75` (production reads use `len()`)
  - `Config::load()` — `src/config.rs:335-337` (production always uses `load_from(Some(path))`, `main.rs:19`)
  - `test_helpers::create_test_message_with_time` — `src/test_helpers.rs:55`: zero callers anywhere,
    not even self-tests. Also `create_test_message_with_media/_with_sender/_with_forward` are
    referenced only by their own `_works` self-tests.
- **Fix:** delete `create_test_message_with_time` outright; inline `redact_hash` into tests;
  delete or mark `#[cfg(test)]` the rest where the lib's external-consumer surface allows
  (this lib exists solely to serve its own binary).

---

## Low severity

### L1. Near-duplicate fan-out blocks in impl_search.rs

- **Where:** `src/mcp/server/impl_search.rs:101-153` and `279-330`
- **Description:** two ~50-line blocks with identical shape (scope guard → `acquire(n)` →
  `fanout::run` → `merge_results` → `shape_response(CompactScope::Multi)` → `json_response`),
  differing only in target resolution and client method. Changes must be mirrored by hand.
- **Fix:** extract one helper taking resolve + fetch closures (~60 duplicated lines removed).

### L2. Triplicated pager skeleton

- **Where:** `src/telegram/client/raw_pager.rs:135-154, 214-231, 300-342`
- **Description:** `RawHistoryPager::next`, `RawChannelSearchPager::next`, `RawGlobalSearchPager::next`
  are three copies of the same buffer-drain algorithm, differing only in offset advance.
  Mitigating factor: each copy carries load-bearing comments on measurement semantics.
- **Fix (optional):** private helper taking page + advance closure. Defensible as-is.

### L3. Triplicated "all-digits ⇒ numeric id" dispatch

- **Where:** `src/mcp/server/impl_search.rs:211-221` (`search_channel_id`),
  `impl_search.rs:428-434` (`history_target`), `src/telegram/client/resolve.rs:108`
- **Description:** three independent copies of the same identifier-kind dispatch.
- **Fix:** unify into one function in `src/mcp/tools/helpers.rs`.

### L4. Payload serialization cost paid even when buffering disabled

- **Where:** `src/mcp/observability/transport.rs:63-65`
- **Description:** every server response is fully re-serialized via `serde_json::to_string` to get
  exact size and recovery copy — before `ResponseBuffer::push` checks `capacity == 0`. With
  buffering disabled (`[observability] response_buffer_size = 0`), each response (media ones are
  ~1.5 MB) still pays an extra full serialization.
- **Fix:** check capacity before serializing, or make size estimation cheap enough to skip the
  copy when the buffer won't store it.

---

## Verified clean

- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`: zero findings.
- No `unwrap()`/`expect()` in production paths (only startup signal-handler installs in
  `main.rs:160,166`, acceptable) and no `#[allow(dead_code)]` anywhere.
- Observability layer: bounded ring buffer, poison-recovery locks, slow-write detection.
- Rate limiter: refund correctly clamped at capacity (cannot inflate bucket); sync `Mutex`
  critical sections are short — no await-held locks.
- Timeout budgets applied to grammers network paths; search deadline latches `timed_out`
  correctly and treats 0 as disabled.
- Input validation at the MCP boundary: i64→i32 wire-range checks, cursor ordering
  (`before_id > after_id`), dedupe-before-cap, empty-window rejection, blank-entry rejection,
  RFC 3339 parsing that refuses to treat blanks as absent.
- Secrets (`api_hash`, `phone_number`) behind `SecretString`; logs record message IDs, never text.
- `parse_telegram_link` rejects non-t.me hosts (including `https://evil.com/t.me/...` and
  userinfo tricks like `t.me@evil.com`).
- All Cargo.toml dependencies verified used; every tokio feature maps to a documented call site.
- `TelegramClientTrait` / `RateLimiterTrait` DI seams justified (mockall); every trait method has
  ≥1 production call site.

---

## Recommended fix order

1. **H2** — one-line config validation, closes a permanent-lockout foot-gun.
2. **M2/M3** — decide rate-limit accounting policy for resolves/failures and apply consistently.
3. **H1 + M1 + M4 + M5** — one "dead surface removal" pass (config fields, error variants,
   builders, test-only helpers, README lines).
4. **L1/L3** — dedupe during the next touch of those files.
5. **L2/L4** — optional polish.
