# Media Batch Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all ten findings from the media-throughput whole-branch review — one user-visible contract defect, one arithmetic asymmetry, one latency opportunity, two module-layering inversions, and five duplication/clarity hazards — and ship them as 0.22.1.

**Architecture:** The contract defect is fixed at its cause, not its symptom: `process_image_with_cap` currently returns one error variant for two distinct failure modes, so the call site physically cannot label them correctly. A new typed `Error::PayloadCapExceeded` splits them, and a small classifier maps errors to the machine-readable reason tokens. The layering work moves three constants to their owning modules, which deletes both upward dependencies and six hand-copied numbers. Everything else is local cleanup.

**Tech Stack:** Rust nightly (edition 2024), `thiserror`, `tokio`, `mockall`, `image`, `rmcp` v3.1.

**Spec:** `docs/superpowers/specs/2026-08-14-media-batch-review-fixes-design.md`

## Global Constraints

- Line length 100 chars. Run `cargo fmt --all` after every code change.
- **Never `unwrap()`** in production code; `expect()` only in tests.
- TDD: the failing test comes first, always. No production code without a preceding test.
- Config tests mutate env vars and MUST run serial: `cargo test config -- --test-threads=1`.
- Pre-merge gate (all must pass): `cargo fmt --check && cargo clippy -- -D warnings && cargo test`.
- Baseline test count at branch point: **709 passing, 0 failed**.
- Conventional-commit messages (`feat:`, `fix:`, `refactor:`, `docs:`, `chore:`).
- Work on branch `fix/media-batch-review`, cut from `master` at `61471cf` or later.

---

### Task 0: Create the branch

**Files:** none (git only)

- [ ] **Step 1: Confirm a clean tree and cut the branch**

```bash
git status --short          # must print nothing
git checkout -b fix/media-batch-review
cargo test 2>&1 | grep "^test result:"   # baseline: 709 passed total
```

---

### Task 1: `Error::PayloadCapExceeded` — split the two failure modes

**Files:**
- Modify: `src/error.rs` (add variant after `DownloadFailed`, around line 44)
- Modify: `src/mcp/tools/image.rs:90-92` (the shrink-loop exhaustion return)
- Test: `src/mcp/tools/image.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces: `Error::PayloadCapExceeded { limit: usize }`. Task 2 matches on this variant.

**Why:** `process_image_with_cap` returns `Error::DownloadFailed` both when an image fails to decode *and* when the shrink loop cannot fit the cap. The batch tool must report those differently (`download_failed` vs `payload_cap_reached`), which is impossible while they share a variant.

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `src/mcp/tools/image.rs`:

```rust
#[test]
fn cap_exhaustion_returns_payload_cap_exceeded_not_download_failed() {
    // A 100-byte cap is unreachable: even a heavily downscaled JPEG carries
    // several hundred bytes of headers, so the shrink loop provably exhausts.
    // The passthrough branch is skipped too (its base64 length exceeds 100).
    let jpeg = crate::test_helpers::create_test_jpeg(64, 64);
    let err = process_image_with_cap(&jpeg, 64, 100).expect_err("cap is unreachable");
    assert!(
        matches!(err, Error::PayloadCapExceeded { limit: 100 }),
        "cap exhaustion must be its own variant, got: {err:?}"
    );
}

#[test]
fn a_corrupt_image_is_still_a_download_failure_not_a_cap_failure() {
    let err = process_image_with_cap(b"not an image", 1280, 1_572_864)
        .expect_err("undecodable bytes must fail");
    assert!(
        matches!(err, Error::DownloadFailed(_)),
        "a decode failure must not be reported as a cap failure, got: {err:?}"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib image:: 2>&1 | tail -20`
Expected: FAIL — `no variant named PayloadCapExceeded found for enum Error` (compile error).

- [ ] **Step 3: Add the variant**

In `src/error.rs`, immediately after the `DownloadFailed` variant:

```rust
    /// An image downloaded successfully but could not be shrunk under the
    /// caller's base64 payload cap. Distinct from `DownloadFailed` because
    /// the MCP batch tool reports the two with different machine-readable
    /// reason tokens, and a retry helps only one of them.
    #[error("image could not be reduced below the {limit}-byte payload cap")]
    PayloadCapExceeded { limit: usize },
```

The `Display` text is byte-identical to the string the shrink loop formats today, so no user-facing message changes — only the type does.

- [ ] **Step 4: Return the new variant from the shrink loop**

In `src/mcp/tools/image.rs`, replace the trailing `Err(...)` of `process_image_with_cap`:

```rust
    Err(Error::PayloadCapExceeded {
        limit: max_base64_len,
    })
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib image:: 2>&1 | grep "^test result:"`
Expected: PASS, and the pre-existing image tests still pass.

- [ ] **Step 6: Verify nothing else matched on the old shape**

Run: `cargo clippy -- -D warnings 2>&1 | tail -5`
Expected: clean. (If any caller string-matched the old message, it surfaces here.)

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add src/error.rs src/mcp/tools/image.rs
git commit -m "fix: give payload-cap exhaustion its own error variant

process_image_with_cap returned Error::DownloadFailed both for undecodable
bytes and for an image that could not be shrunk under the cap, making the
two indistinguishable at the call site."
```

---

### Task 2: Classify post-download failures correctly (fixes findings 1 and 10)

**Files:**
- Modify: `src/mcp/server/impl_media.rs:171-203` (the two copy-pasted failure arms)
- Modify: `src/mcp/server/impl_media.rs` (add classifier beside `failure_reason`, ~line 352)
- Test: `src/mcp/tests/media_batch.rs`

**Interfaces:**
- Consumes: `Error::PayloadCapExceeded { limit }` from Task 1.
- Produces: `fn post_download_failure_reason(error: &Error) -> String`, used only inside `impl_media.rs`.

**Why:** finding 1 — a cap-driven drop is reported as `download_failed`, so a client retries something that can never succeed. Finding 10 — the two failure arms are copy-pasted bodies carrying the same mislabel twice.

- [ ] **Step 1: Write the failing test**

`post_download_failure_reason` is private to `impl_media.rs`, so test it where it lives rather
than widening its visibility. Add a `#[cfg(test)] mod tests` block at the bottom of
`src/mcp/server/impl_media.rs` (or extend one if present):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_exhaustion_maps_to_the_payload_cap_token() {
        let reason = post_download_failure_reason(&Error::PayloadCapExceeded { limit: 32_768 });
        assert_eq!(
            reason, "payload_cap_reached",
            "an image that downloaded fine but could not be shrunk is a cap drop, \
             not a download failure"
        );
    }

    #[test]
    fn a_real_failure_still_maps_to_the_download_failed_token() {
        let reason = post_download_failure_reason(&Error::DownloadFailed("boom".to_string()));
        assert_eq!(reason, "download_failed: media download failed: boom");
    }
}
```

The second assertion embeds `Error::DownloadFailed`'s own `#[error("media download failed: {0}")]`
prefix — that is the real `Display` output, not a typo.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib post_download 2>&1 | tail -20`
Expected: FAIL — `cannot find function post_download_failure_reason`.

- [ ] **Step 3: Add the classifier**

In `src/mcp/server/impl_media.rs`, directly below the existing `failure_reason`:

```rust
/// Map a failure that happened *after* a successful download to a stable,
/// machine-readable reason token.
///
/// Unlike `failure_reason`, this matches a catch-all: `Error` is the crate-wide
/// enum with sixteen variants, only one of which is meaningful to a caller
/// here. Enumerating the rest would be noise, and `download_failed` with the
/// error's text attached is the honest default for all of them.
fn post_download_failure_reason(error: &Error) -> String {
    match error {
        Error::PayloadCapExceeded { .. } => "payload_cap_reached".to_string(),
        other => format!("download_failed: {other}"),
    }
}
```

- [ ] **Step 4: Rewrite the two failure arms**

Replace the `process_image_with_cap` arm (currently `impl_media.rs:174-182`):

```rust
                Err(e) => {
                    // Budget deliberately untouched: nothing was emitted, so
                    // later ids keep their full allowance.
                    failed.push(MediaBatchFailure {
                        id,
                        reason: post_download_failure_reason(&e),
                    });
                    continue;
                }
```

And the `json_response` arm (currently `:194-203`):

```rust
                Err(e) => {
                    // Neither a download failure nor a cap drop: serializing the
                    // metadata failed. Unreachable today (the response type has
                    // no map keys or floats, the only things that make
                    // serde_json::to_string fail) but not a compile-time
                    // guarantee, so it gets an honest token of its own.
                    // Budget deliberately untouched, same reasoning as above.
                    failed.push(MediaBatchFailure {
                        id,
                        reason: format!("internal_error: {e}"),
                    });
                    continue;
                }
```

- [ ] **Step 5: Run the full media suite**

Run: `cargo test --lib media 2>&1 | grep "^test result:"`
Expected: PASS. Existing batch tests are unaffected — no currently-tested path produces
`PayloadCapExceeded`.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add src/mcp/server/impl_media.rs src/mcp/tests/media_batch.rs
git commit -m "fix: report cap-driven drops as payload_cap_reached

An image that downloaded successfully but could not be shrunk under its
remaining allowance was reported as download_failed, inviting a retry that
could never succeed. Also collapses the two copy-pasted failure arms and
gives the (unreachable) serialization failure its own internal_error token."
```

---

### Task 3: Count successes explicitly (finding 9)

**Files:**
- Modify: `src/mcp/server/impl_media.rs:212` and the two `content.push` calls (~`:208-209`)
- Test: covered by existing `src/mcp/tests/media_batch.rs` tests

**Interfaces:** none crossing tasks.

**Why:** `let returned = content.len() / 2;` derives the success count from the content vector's
shape. A future third block per success silently corrupts both the summary and the refund
arithmetic, with no signal at the change site.

- [ ] **Step 1: Confirm the existing tests cover the count**

Run: `cargo test --lib mixed_batch_returns_images_and_reports_failures 2>&1 | grep "^test result:"`
Expected: PASS. This test asserts `returned` in the summary, so it is the regression net for
this change.

- [ ] **Step 2: Introduce the counter**

Beside the other accumulators near `impl_media.rs:141`:

```rust
        let mut returned = 0usize;
```

At the push site, replace the two bare pushes with:

```rust
            content.push(ContentBlock::image(processed.base64_jpeg, "image/jpeg"));
            content.push(ContentBlock::text(metadata_json));
            returned += 1;
```

And delete the derived binding:

```rust
        let returned = content.len() / 2;   // <- remove this line
```

- [ ] **Step 3: Run the media suite**

Run: `cargo test --lib media 2>&1 | grep "^test result:"`
Expected: PASS, identical counts to Step 1.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add src/mcp/server/impl_media.rs
git commit -m "refactor: count batch successes explicitly instead of halving content.len()"
```

---

### Task 4: Refund with saturating arithmetic (finding 2, part 1)

**Files:**
- Modify: `src/mcp/server/impl_media.rs:218`
- Test: `src/mcp/tests/media_batch.rs`

**Interfaces:** none crossing tasks.

**Why:** the charge at `:113` uses `saturating_mul`; the refund uses unchecked `*`. Two spellings
of the same arithmetic in one function is how the asymmetry survived review once already.

- [ ] **Step 1: Write the failing test**

Add to `src/mcp/tests/media_batch.rs`:

```rust
#[tokio::test]
async fn an_enormous_media_cost_refunds_without_overflowing() {
    // Every id fails, so the refund multiplies the cost by the full request
    // size. With an unchecked `*` this panics in debug builds.
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .return_once(|_, _, _| Ok(vec![not_found(10), not_found(11), not_found(12)]));

    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().times(1).returning(|_| Ok(()));
    limiter.expect_refund().times(1).return_const(());

    let server = McpServer::new(Arc::new(client), Arc::new(limiter))
        .with_media_download_cost(u32::MAX / 2);

    let result = server
        .get_messages_media_batch(
            Parameters(request("chan", vec![10, 11, 12])),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    assert!(result.is_ok(), "a huge configured cost must not panic the call");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib an_enormous_media_cost 2>&1 | tail -20`
Expected: FAIL — `attempt to multiply with overflow` panic in the refund line.

- [ ] **Step 3: Make the refund saturating**

In `src/mcp/server/impl_media.rs`, replace the refund computation:

```rust
        let refunded = self
            .media_download_cost
            .saturating_mul((unique.len() - returned) as u32);
        self.rate_limiter.refund(refunded);
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib an_enormous_media_cost 2>&1 | grep "^test result:"`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add src/mcp/server/impl_media.rs src/mcp/tests/media_batch.rs
git commit -m "fix: saturate the media batch refund like the matching charge"
```

---

### Task 5: Validate rate-limit costs at startup (finding 2, part 2)

**Files:**
- Modify: `src/config.rs` (add `impl RateLimitConfig { fn validate }` after the struct, ~line 200; wire into `Config::load`'s validation chain at ~line 344)
- Test: `src/config/tests.rs`

**Interfaces:**
- Produces: `RateLimitConfig::validate(&self) -> anyhow::Result<()>`, called from `Config::load`.

**Why:** `RateLimitConfig` is the only config section with no `validate()`. A cost above
`max_tokens` is not merely overflow-prone — it *guarantees* every call of that kind fails, because
the bucket can never hold enough tokens. Catching that at startup beats discovering it per call.

The bound is expressed against `max_tokens` alone, entirely inside `config`, so it introduces no
dependency on `MAX_MEDIA_BATCH_IDS` (which lives in the MCP layer — reaching for it here would
recreate the inversion Task 8 removes).

- [ ] **Step 1: Write the failing test**

Add to `src/config/tests.rs`:

```rust
#[test]
fn a_media_cost_above_the_bucket_capacity_is_rejected() {
    let config = RateLimitConfig {
        max_tokens: 10,
        refill_rate: 2.0,
        media_download_cost: 11,
        transcription_cost: 5,
    };
    let err = config.validate().expect_err("an unsatisfiable cost must be rejected");
    assert!(
        err.to_string().contains("media_download_cost"),
        "the error must name the offending key, got: {err}"
    );
}

#[test]
fn a_transcription_cost_above_the_bucket_capacity_is_rejected() {
    let config = RateLimitConfig {
        max_tokens: 10,
        refill_rate: 2.0,
        media_download_cost: 3,
        transcription_cost: 11,
    };
    let err = config.validate().expect_err("an unsatisfiable cost must be rejected");
    assert!(err.to_string().contains("transcription_cost"), "got: {err}");
}

#[test]
fn costs_equal_to_capacity_are_accepted() {
    // Exactly-capacity is satisfiable from a full bucket, so it is legal.
    let config = RateLimitConfig {
        max_tokens: 10,
        refill_rate: 2.0,
        media_download_cost: 10,
        transcription_cost: 10,
    };
    assert!(config.validate().is_ok());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test config -- --test-threads=1 2>&1 | tail -20`
Expected: FAIL — `no method named validate found for struct RateLimitConfig`.

- [ ] **Step 3: Implement the validation**

In `src/config.rs`, after the `RateLimitConfig` struct:

```rust
impl RateLimitConfig {
    /// Reject a per-call cost the bucket can never satisfy. A cost above
    /// `max_tokens` means every call of that kind fails on a full bucket —
    /// a configuration that is not merely tight but unsatisfiable. It also
    /// keeps the batch charge/refund arithmetic far from `u32` overflow.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.media_download_cost > self.max_tokens {
            anyhow::bail!(
                "rate_limiting.media_download_cost ({}) exceeds max_tokens ({}), \
                 so every media call would fail even on a full bucket",
                self.media_download_cost,
                self.max_tokens
            );
        }
        if self.transcription_cost > self.max_tokens {
            anyhow::bail!(
                "rate_limiting.transcription_cost ({}) exceeds max_tokens ({}), \
                 so every transcription call would fail even on a full bucket",
                self.transcription_cost,
                self.max_tokens
            );
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Wire it into the validation chain**

In `Config::load`, alongside the existing three (`telegram.timeouts`, `limits`, `search`):

```rust
        config
            .rate_limiting
            .validate()
            .context("invalid rate_limiting configuration")?;
```

- [ ] **Step 5: Run the config suite**

Run: `cargo test config -- --test-threads=1 2>&1 | grep "^test result:"`
Expected: PASS. Watch for pre-existing fixtures that set a cost above their `max_tokens` — if one
exists, it was already broken and its fixture needs correcting.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add src/config.rs src/config/tests.rs
git commit -m "feat: reject rate-limit costs the bucket can never satisfy"
```

---

### Task 6: Move image encoding off the async worker (finding 3)

**Files:**
- Modify: `src/mcp/server/impl_media.rs` (batch loop ~`:171`, single-message path ~`:40`)
- Test: covered by the existing media suite (this change is behavior-neutral by construction)

**Interfaces:** none crossing tasks.

**Why:** a ten-image batch runs up to ten sequential Lanczos3 resize + JPEG encode passes inline on
a tokio worker. The branch parallelized the I/O half of the pipeline and left the CPU half serial.

**Constraint — do not "improve" on this:** encodes run in request order *deliberately*, so budget
allocation is deterministic no matter which download finished first. Parallelizing the encodes
would let the payload cap allocate against whichever image finished first — a real, nondeterministic
behavior change. Keep the loop sequential; only move each encode to a blocking thread.

- [ ] **Step 1: Record the current green state**

Run: `cargo test --lib media 2>&1 | grep "^test result:"`
Expected: PASS. These same tests passing unchanged after the edit is the acceptance criterion.

- [ ] **Step 2: Offload the batch encode**

In the batch loop, change the binding to `let mut download = ...` and replace the
`process_image_with_cap` call:

```rust
            // Encode on a blocking thread: a Lanczos3 resize plus JPEG encode is
            // hundreds of milliseconds of pure CPU, and ten of them back to back
            // would pin a tokio worker for the whole batch. The loop stays
            // sequential and in request order so budget allocation remains
            // deterministic — only the CPU leaves the async worker.
            let bytes = std::mem::take(&mut download.bytes);
            let encode = tokio::task::spawn_blocking(move || {
                process_image_with_cap(&bytes, max_dimension, allowance)
            })
            .await;

            let processed = match encode {
                Ok(Ok(processed)) => processed,
                Ok(Err(e)) => {
                    // Budget deliberately untouched: nothing was emitted, so
                    // later ids keep their full allowance.
                    failed.push(MediaBatchFailure {
                        id,
                        reason: post_download_failure_reason(&e),
                    });
                    continue;
                }
                Err(join_error) => {
                    // The blocking task panicked or was cancelled. Report the id
                    // rather than failing the batch, so the other ids' work and
                    // their token charges are not thrown away.
                    failed.push(MediaBatchFailure {
                        id,
                        reason: format!("internal_error: {join_error}"),
                    });
                    continue;
                }
            };
```

`media_metadata` reads only `media_type`, `is_thumbnail`, `caption`, the dimension/size fields and
`video_info` — never `bytes` — so `std::mem::take` is safe here.

- [ ] **Step 3: Offload the single-message encode**

The single-message path has the same inline encode (this is where the pattern was copied from).
It currently reads, at `impl_media.rs:42`:

```rust
        let processed = process_image(&download.bytes, max_dimension).map_err(|e| e.to_string())?;
```

Note it calls `process_image`, **not** `process_image_with_cap` — keep that. Change the binding
above it to `let mut download = ...` and replace the line with:

```rust
        let bytes = std::mem::take(&mut download.bytes);
        let processed = tokio::task::spawn_blocking(move || process_image(&bytes, max_dimension))
            .await
            .map_err(|e| format!("image encode task failed: {e}"))?
            .map_err(|e| e.to_string())?;
```

Here a join error propagates as a normal `Err(String)` — there is no batch of other ids to protect.
Add `process_image` to the `use crate::mcp::tools::image::{...}` list at the top of the file if it
is not already imported.

- [ ] **Step 4: Run the full suite**

Run: `cargo test 2>&1 | grep "^test result:"`
Expected: PASS, same counts as Step 1. Any change in results means the offload altered behavior —
stop and investigate rather than adjusting tests.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add src/mcp/server/impl_media.rs
git commit -m "perf: encode images on a blocking thread instead of the async worker

Keeps the loop sequential and in request order so payload-budget allocation
stays deterministic; only the CPU work leaves the tokio worker."
```

---

### Task 7: Give the telegram layer its own download concurrency (finding 4)

**Files:**
- Modify: `src/telegram/client/ops_media.rs:113` and its constant block near the top
- Test: none new (compiler-enforced; the import simply disappears)

**Interfaces:**
- Produces: `pub(crate) const MEDIA_DOWNLOAD_CONCURRENCY: usize` in `ops_media.rs`.

**Why:** `ops_media.rs` reaches up to `crate::mcp::tools::fanout::FANOUT_CONCURRENCY` — the only
`crate::mcp` reference anywhere under `src/telegram/`. Media downloads (multi-hundred-KB binary
transfers) and search fan-out (small JSON RPCs) have different flood characteristics and no reason
to share a knob; retuning one silently retunes the other.

- [ ] **Step 1: Confirm the inversion exists**

Run: `grep -rn "crate::mcp" src/telegram/`
Expected: exactly one hit, at `ops_media.rs:113`. That count going to zero is this task's
acceptance check.

- [ ] **Step 2: Add the telegram-owned constant**

Near the top of `src/telegram/client/ops_media.rs`:

```rust
/// Concurrent media downloads in flight within one batch call.
///
/// Deliberately owned by this layer rather than shared with the MCP fan-out
/// constant it currently equals: these are multi-hundred-KB binary transfers,
/// not small JSON round trips, and the two should be tunable apart.
pub(crate) const MEDIA_DOWNLOAD_CONCURRENCY: usize = 4;
```

- [ ] **Step 3: Use it**

```rust
            .buffered(MEDIA_DOWNLOAD_CONCURRENCY)
```

- [ ] **Step 4: Verify the edge is gone and tests pass**

Run: `grep -rn "crate::mcp" src/telegram/ ; cargo test --lib telegram 2>&1 | grep "^test result:"`
Expected: no grep hits; tests PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add src/telegram/client/ops_media.rs
git commit -m "refactor: give media downloads a telegram-owned concurrency constant

Removes the only crate::mcp reference under src/telegram/."
```

---

### Task 8: State the grammers length contract honestly (finding 8)

**Files:**
- Modify: `src/telegram/client/ops_media.rs:88-92`
- Test: covered by the existing telegram client suite

**Interfaces:** none crossing tasks.

**Why:** `.chain(std::iter::repeat_with(|| None))` can never be pulled from — the pinned grammers
rev returns exactly `message_ids.len()` slots. It is dead code that misleads the next reader into
believing the guarantee is unreliable, while providing no actual safety net (nothing tests it).

A bare `.zip()` alone would be worse than the padding if the guarantee ever broke: trailing ids
would vanish from *both* `content` and `failed`, so `returned + failed != requested` and the
caller would never learn those ids were dropped. The assertion states the contract and catches a
regression in dev and test builds without shipping dead production code.

- [ ] **Step 1: Replace the padding with an asserted zip**

```rust
        // grammers returns exactly one slot per requested id, in request order
        // (pinned rev 9fef0ba, client/messages.rs:1145 collects
        // `message_ids.iter().map(|id| map.remove(id))`), so the lengths match
        // by construction. A None slot is a deleted or inaccessible message.
        debug_assert_eq!(
            messages.len(),
            message_ids.len(),
            "grammers must return one slot per requested id"
        );
        let slots: Vec<(i32, Option<_>)> =
            message_ids.iter().copied().zip(messages).collect();
```

- [ ] **Step 2: Run the telegram suite**

Run: `cargo test --lib telegram 2>&1 | grep "^test result:"`
Expected: PASS. Mock-based tests that build slot vectors must still line up; if any test supplies
a short vector, the `debug_assert` fires and that test's fixture was lying about grammers'
behavior — fix the fixture, not the assertion.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add src/telegram/client/ops_media.rs
git commit -m "refactor: assert grammers' slot-count contract instead of padding for it"
```

---

### Task 9: Move the payload floor into config (finding 5)

**Files:**
- Modify: `src/config.rs:1` (drop the import), and add the constant beside `MAX_SEARCH_DEADLINE_SECONDS` (~line 148)
- Modify: `src/mcp/tools/media_budget.rs:7-12` (import instead of define)
- Test: covered by existing `media_budget` and config tests

**Interfaces:**
- Produces: `pub(crate) const MIN_IMAGE_BASE64_BYTES: usize` in `crate::config`.

**Why:** `config.rs:1` imports `MIN_IMAGE_BASE64_BYTES` from `crate::mcp::tools::media_budget`,
making the application-layer config module depend on the MCP server layer above it. `config.rs`
already houses exactly this kind of value — `MAX_SEARCH_DEADLINE_SECONDS` is a validation-bound
constant owned by config — so the constant is simply in the wrong file.

- [ ] **Step 1: Move the definition**

Delete the `pub(crate) const MIN_IMAGE_BASE64_BYTES` definition from `media_budget.rs` and add it
to `src/config.rs`, next to `MAX_SEARCH_DEADLINE_SECONDS`:

```rust
/// Floor below which an image would be downscaled past usefulness. Once a
/// batch's remaining budget drops under this, `Base64Budget::allowance`
/// returns `None` rather than emitting an unreadable thumbnail — which is why
/// `limits.media_batch_max_total_bytes` is validated against it here.
pub(crate) const MIN_IMAGE_BASE64_BYTES: usize = 32_768;
```

- [ ] **Step 2: Fix both sides' imports**

In `src/config.rs`, delete line 1 (`use crate::mcp::tools::media_budget::MIN_IMAGE_BASE64_BYTES;`).

In `src/mcp/tools/media_budget.rs`, add beside the existing `image::MAX_BASE64_LEN` import:

```rust
use crate::config::MIN_IMAGE_BASE64_BYTES;
```

The `media_budget` tests reference the constant via `use super::*`, so they keep compiling
unchanged.

- [ ] **Step 3: Verify the edge is gone and tests pass**

Run: `grep -n "crate::mcp" src/config.rs ; cargo test --lib 2>&1 | grep "^test result:"`
Expected: no grep hits; tests PASS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add src/config.rs src/mcp/tools/media_budget.rs
git commit -m "refactor: move the image payload floor into config

config.rs validated a limit using a constant owned by the MCP layer above it."
```

---

### Task 10: Make `server.rs` use the real config defaults (finding 7 + the regression test)

**Files:**
- Modify: `src/config.rs:6` (`mod defaults;` → `pub(crate) mod defaults;`)
- Modify: `src/mcp/server.rs:40-48` (delete both `DEFAULT_*` constants) and `:67-87` (`McpServer::new`)
- Test: `src/mcp/tests/server_core.rs`

**Interfaces:**
- Consumes: `crate::config::defaults::{default_response_byte_budget, default_media_batch_max_total_bytes, default_media_download_cost, default_transcription_cost, default_transcription_default_timeout, default_transcription_max_timeout}`. (Verified names — note the timeout functions have **no** `_seconds` suffix, unlike the config keys they back.)

**Why:** `server.rs` hand-copies two `DEFAULT_*` constants and inlines four more numbers
(`media_download_cost: 3`, `transcription_cost: 5`, and the two transcription timeouts) purely
because `config.rs` declares `mod defaults;` privately — even though every function inside is
already `pub(crate)`. Change a shipped default and any construction path that skips the matching
`with_*` builder silently keeps the old value, with no compiler or test signal.

- [ ] **Step 1: Write the failing test**

Add to `src/mcp/tests/server_core.rs`:

```rust
#[test]
fn server_defaults_match_the_shipped_config_defaults() {
    // The bug this guards: changing a default in config/defaults.rs while
    // server.rs keeps a hand-copied number desyncs every construction path
    // that does not call the matching with_* builder.
    use crate::config::defaults::*;

    let server = McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    );

    assert_eq!(server.media_download_cost(), default_media_download_cost());
    assert_eq!(server.transcription_cost(), default_transcription_cost());
    assert_eq!(
        server.response_byte_budget() as u64,
        default_response_byte_budget()
    );
    assert_eq!(
        server.media_batch_max_total_bytes() as u64,
        default_media_batch_max_total_bytes()
    );
}
```

The fields are private. Add `#[cfg(test)]` accessors on `McpServer` returning each field — four
one-line methods — rather than widening the fields themselves:

```rust
    #[cfg(test)]
    pub(crate) fn media_download_cost(&self) -> u32 {
        self.media_download_cost
    }
```

…and the same shape for `transcription_cost`, `response_byte_budget` (returns `usize`), and
`media_batch_max_total_bytes` (returns `usize`).

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib server_defaults_match 2>&1 | tail -20`
Expected: FAIL — `module defaults is private`.

- [ ] **Step 3: Open the defaults module**

In `src/config.rs`:

```rust
pub(crate) mod defaults;
```

(`use defaults::*;` below it is unaffected.)

- [ ] **Step 4: Delete the duplicates and call the real functions**

Delete both constants from `src/mcp/server.rs` (`DEFAULT_RESPONSE_BYTE_BUDGET` and
`DEFAULT_MEDIA_BATCH_MAX_TOTAL_BYTES`, including the comment noting the module was unreachable),
then in `McpServer::new`:

```rust
            media_download_cost: default_media_download_cost(),
            transcription_cost: default_transcription_cost(),
            transcription_default_timeout_secs: default_transcription_default_timeout(),
            transcription_max_timeout_secs: default_transcription_max_timeout(),
            response_byte_budget: default_response_byte_budget() as usize,
            media_batch_max_total_bytes: default_media_batch_max_total_bytes() as usize,
```

with this import added at the top of `src/mcp/server.rs`:

```rust
use crate::config::defaults::{
    default_media_batch_max_total_bytes, default_media_download_cost,
    default_response_byte_budget, default_transcription_cost,
    default_transcription_default_timeout, default_transcription_max_timeout,
};
```

The two byte-budget functions return `u64` while the struct fields are `usize`, hence the casts;
the four cost/timeout functions return `u32` and need none.

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test --lib server_defaults_match 2>&1 | grep "^test result:"`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add src/config.rs src/mcp/server.rs src/mcp/tests/server_core.rs
git commit -m "refactor: build McpServer defaults from config::defaults

Six hand-copied numbers in server.rs existed only because mod defaults was
private; a test now fails if the two ever desync again."
```

---

### Task 11: Extract the shared id validation (finding 6)

**Files:**
- Modify: `src/mcp/tools/helpers.rs` (add the helper near `parse_message_id`)
- Modify: `src/mcp/server/impl_media.rs:78-102`, `src/mcp/server/impl_message_batch.rs:20-44`
- Test: `src/mcp/tools/helpers.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `pub fn dedupe_and_validate_ids(ids: &[i64], cap: usize) -> Result<(Vec<i64>, Vec<i32>), String>`
  returning `(unique_ids_in_first_seen_order, wire_ids)`.

**Why:** the dedupe + cap-check + `parse_message_id` loop in the two batch tools is byte-identical
apart from the cap constant, comment text included. Both error strings are already parameterized by
that constant, so one helper reproduces both messages exactly — no test churn, no behavior change.

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `src/mcp/tools/helpers.rs`:

```rust
#[test]
fn dedupe_preserves_first_seen_order() {
    let (unique, wire) = dedupe_and_validate_ids(&[3, 1, 3, 2, 1], 10).expect("valid ids");
    assert_eq!(unique, vec![3, 1, 2]);
    assert_eq!(wire, vec![3, 1, 2]);
}

#[test]
fn over_cap_is_rejected_with_the_cap_in_the_message() {
    let err = dedupe_and_validate_ids(&[1, 2, 3], 2).expect_err("over cap");
    assert_eq!(err, "message_ids accepts at most 2 ids per call, got 3");
}

#[test]
fn dedupe_happens_before_the_cap_check() {
    // Three ids but two distinct: under a cap of 2 this must be accepted.
    let (unique, _) = dedupe_and_validate_ids(&[7, 7, 8], 2).expect("duplicates do not count");
    assert_eq!(unique, vec![7, 8]);
}

#[test]
fn an_id_beyond_the_i32_range_is_rejected() {
    let err = dedupe_and_validate_ids(&[i64::from(i32::MAX) + 1], 10).expect_err("out of range");
    assert!(err.contains("exceeds Telegram's message id range"), "got: {err}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib dedupe 2>&1 | tail -20`
Expected: FAIL — `cannot find function dedupe_and_validate_ids`.

- [ ] **Step 3: Implement the helper**

In `src/mcp/tools/helpers.rs`:

```rust
/// Dedupe message ids (silently, preserving first-seen order), enforce a
/// per-call cap, and validate each id's sign and i32 range.
///
/// Shared by `get_messages_batch` and `get_messages_media_batch`, which differ
/// only in their cap. Returns the deduped ids alongside their wire (`i32`)
/// forms, in the same order. Dedupe runs before the cap check, so a caller
/// repeating an id is not penalised for it.
pub fn dedupe_and_validate_ids(ids: &[i64], cap: usize) -> Result<(Vec<i64>, Vec<i32>), String> {
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<i64> = ids.iter().copied().filter(|id| seen.insert(*id)).collect();

    if unique.len() > cap {
        return Err(format!(
            "message_ids accepts at most {cap} ids per call, got {}",
            unique.len()
        ));
    }

    let mut wire_ids = Vec::with_capacity(unique.len());
    for id in &unique {
        let parsed = parse_message_id(*id)?;
        wire_ids.push(
            parsed
                .as_i32()
                .ok_or_else(|| format!("message_id {} exceeds Telegram's message id range", id))?,
        );
    }

    Ok((unique, wire_ids))
}
```

- [ ] **Step 4: Call it from both tools**

In `src/mcp/server/impl_media.rs`, replace lines 78-102 with:

```rust
        let (unique, wire_ids) =
            dedupe_and_validate_ids(&request.message_ids, MAX_MEDIA_BATCH_IDS)?;
```

In `src/mcp/server/impl_message_batch.rs`, replace lines 20-44 with:

```rust
        let (unique, wire_ids) = dedupe_and_validate_ids(&request.message_ids, MAX_BATCH_IDS)?;
```

Add the import to each file if `use super::*` does not already bring it in. If either tool does not
use one of the two returned bindings, prefix it with `_` rather than dropping it.

- [ ] **Step 5: Run both tools' suites**

Run: `cargo test --lib -- batch 2>&1 | grep "^test result:"`
Expected: PASS with no test edits. Both tools' cap-rejection tests assert on the exact strings the
helper now produces — that they still pass is the proof the extraction is behavior-preserving.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add src/mcp/tools/helpers.rs src/mcp/server/impl_media.rs src/mcp/server/impl_message_batch.rs
git commit -m "refactor: share id dedupe/validation between the two batch tools"
```

---

### Task 12: Documentation and 0.22.1 release

**Files:**
- Modify: `README.md` (§16 `get_messages_media_batch` failure reasons)
- Modify: `src/mcp/server.rs` (the `#[tool(description = ...)]` string for `get_messages_media_batch`)
- Modify: `config.example.toml` (`[rate_limiting]` validation note)
- Modify: `CHANGELOG.md`, `Cargo.toml`, `Cargo.lock`
- Modify: `docs/memory.md`, `docs/tasklist.md`

- [ ] **Step 1: Update the failure-reason vocabulary in README §16**

Document that `payload_cap_reached` now covers both budget exhaustion *and* an image that cannot be
shrunk to fit, and add the new token:

```markdown
| `payload_cap_reached` | The batch's total base64 budget was exhausted before this id, **or** the image downloaded but could not be shrunk under its remaining allowance. Raise `[limits] media_batch_max_total_bytes` or request fewer ids — retrying this id alone will not help. |
| `internal_error: <detail>` | The image was produced but its metadata could not be serialized. Not reachable in normal operation; report it if seen. |
```

- [ ] **Step 2: Update the tool description string**

In `src/mcp/server.rs`, find the `#[tool(description = "...")]` for `get_messages_media_batch` and
extend the failure-reason list with `internal_error`, matching the README wording. Keep it one line
(rustfmt cannot wrap string literals).

- [ ] **Step 3: Note the new config validation**

In `config.example.toml`, under `[rate_limiting]`:

```toml
# Costs are validated at startup: a cost above max_tokens is rejected, since
# such a call could never be served even from a full bucket.
```

- [ ] **Step 4: Bump the version**

In `Cargo.toml`: `version = "0.22.1"`. Then run `cargo build` to refresh `Cargo.lock`.

- [ ] **Step 5: Write the CHANGELOG entry**

```markdown
## [0.22.1] - 2026-08-14

### Fixed
- `get_messages_media_batch` now reports an image that downloaded successfully
  but could not be shrunk under its remaining payload allowance as
  `payload_cap_reached`, not `download_failed`. The cause was one error variant
  (`Error::DownloadFailed`) serving two distinct failure modes; cap exhaustion
  now has its own `Error::PayloadCapExceeded`. A client branching on the token
  was being told to retry something that could never succeed.
- The batch refund now uses saturating multiplication, matching the charge it
  reverses. With a large configured `media_download_cost` the old unchecked
  multiply could panic in debug builds or wrap in release.
- Rate-limit costs are validated at startup: a `media_download_cost` or
  `transcription_cost` above `max_tokens` is rejected, since such a call could
  never be served even from a full bucket.

### Changed
- Image encoding runs on a blocking thread instead of the async worker. The
  batch loop stays sequential and in request order, so payload-budget
  allocation is unchanged and deterministic.
- A metadata-serialization failure inside a batch is reported as
  `internal_error: <detail>` rather than being mislabelled a download failure.
  Not reachable in normal operation.

### Internal
- Removed both module-layering inversions: `src/telegram/` no longer reaches up
  into `crate::mcp` for its download concurrency, and `config.rs` no longer
  imports a validation bound from the MCP layer.
- `McpServer::new` builds its defaults from `config::defaults` instead of six
  hand-copied numbers; a test now fails if the two desync.
- Shared id dedupe/validation between the two batch tools; explicit success
  counter in place of `content.len() / 2`; grammers' slot-count contract is
  asserted rather than padded for.
```

- [ ] **Step 6: Record the durable lessons in `docs/memory.md`**

Add a dated section. The two worth keeping:

- **One error variant serving two failure modes makes a caller-facing contract unimplementable.**
  The review found the symptom at the call site (`download_failed` where the docs promised
  `payload_cap_reached`) but the cause was in the callee's type: `process_image_with_cap` returned
  `Error::DownloadFailed` for both cap exhaustion and decode failure, so no amount of care at the
  call site could have labelled them apart. When a reason token looks wrong, check whether the
  callee can even express the distinction before fixing the caller.
- **A constant's home determines the dependency direction.** Three separate findings — a
  `telegram → mcp` import, a `config → mcp` import, and six hand-copied default values — were all
  one misplacement each. The `defaults` case is the sharpest: every function in the module was
  already `pub(crate)`, and only the module declaration's privacy forced the duplication. Before
  copying a value across a module boundary, check whether the boundary is the thing that's wrong.

- [ ] **Step 7: Add the tasklist row**

Append a Phase 37 row to `docs/tasklist.md` summarizing the ten fixes, the new test count, and
pointing at this plan and its spec.

- [ ] **Step 8: Run the full gate**

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
cargo test config -- --test-threads=1
```

Expected: all green. Record the final test count — it should be 709 plus roughly a dozen new tests.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "docs: media batch review fixes — README, changelog, memory, tasklist

chore: release v0.22.1"
```

---

## Verification Checklist

Before merging `fix/media-batch-review`:

- [ ] `cargo fmt --check` clean
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo test` green, count recorded in the tasklist row
- [ ] `cargo test config -- --test-threads=1` green
- [ ] `grep -rn "crate::mcp" src/telegram/` returns nothing
- [ ] `grep -n "crate::mcp" src/config.rs` returns nothing
- [ ] `grep -n "DEFAULT_RESPONSE_BYTE_BUDGET\|DEFAULT_MEDIA_BATCH_MAX_TOTAL_BYTES" src/mcp/server.rs` returns nothing
- [ ] Code review requested (`superpowers:requesting-code-review`) covering the whole branch
- [ ] Optional live probe with `scripts/mcp_probe.py`: one `get_messages_media_batch` call against a
      real session, confirming Task 6 changed nothing observable

## Notes for the executor

**On testing the cap-reason mapping end to end.** There is deliberately no integration test driving
a real image through the batch tool into `payload_cap_reached`. The floor
(`MIN_IMAGE_BASE64_BYTES`, 32 KiB) guarantees any allowance handed to `process_image_with_cap` is at
least 32 KiB, and the shrink loop's ratio is aggressive enough (`sqrt` of the byte overshoot) that
whether a given fixture exhausts five iterations depends on the JPEG encoder's exact output size.
A test built that way could pass for the wrong reason today and flake on an `image` crate bump.
Task 1 proves the variant, Task 2 proves the mapping, and their composition is a single `match` arm.
If you want end-to-end coverage anyway, raise it in review rather than inventing a fixture —
do not weaken the floor to make a test constructible.

**On the `internal_error` token.** This adds a fifth reason to a documented vocabulary for a path
that cannot currently be reached, which is itself a small contract change. The spec flags it as a
judgment call: the alternative is leaving it labelled `download_failed`, perpetuating exactly the
mislabel Task 2 exists to fix. If review prefers the smaller contract, drop the token in Tasks 2, 6
and 12 and keep `download_failed` for that one arm — nothing else in the plan depends on it.
