# Audit Stage 1: Correctness Fixes + Dead Code — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the three correctness findings from the 2026-08-15 audit (env-var test race, phone-redaction panic, silent i64→i32 message-id truncation) and delete the dead code the audit surfaced.

**Architecture:** No structural change. A test-module mutex makes plain `cargo test` safe; `redact_phone` becomes char-aware and auth uses it; a shared `wire_message_id` helper replaces three unchecked `as i32` casts; four dead items are deleted.

**Tech Stack:** Rust nightly (edition 2024 — `env::set_var` is `unsafe`), tokio, mockall, rmcp v3.1.

**Spec:** `docs/superpowers/specs/2026-08-15-project-audit.md` (Stage 1 section)

## Global Constraints

- Pre-merge gate: `cargo fmt --check && cargo clippy -- -D warnings && cargo test` — all green before the PR.
- Never `unwrap()` in production code; line length 100; run `cargo fmt --all` after every code change.
- Conventional commits (`fix:`, `chore:`, `docs:`); branch `fix/audit-stage1-correctness`.
- TDD for behavior changes (Tasks 1–3). Deletions (Task 4) are guarded by the existing suite + clippy.

---

### Task 1: Env-var lock in config tests (kill the parallel-run race)

**Files:**
- Modify: `src/config/tests.rs` (add lock at top; guard line in every env-mutating test)
- Modify: `justfile:17-19` (delete `test-config` recipe)
- Modify: `CLAUDE.md:12` and `CLAUDE.md:50` (drop the serial mandate)
- Modify: `README.md:1618`, `docs/workflow.md:78` (same)

**Interfaces:**
- Consumes: nothing.
- Produces: `fn env_lock() -> MutexGuard<'static, ()>` (private to the test module).

- [ ] **Step 1: Demonstrate the race (best-effort — it is probabilistic)**

Run: `for i in $(seq 1 10); do cargo test config 2>&1 | tail -1; done`
Expected: at least one non-green run (env vars race across threads). Record the outcome either way; a lucky all-green streak does not invalidate the fix — the race is real by inspection (e.g. `test_expand_env_vars_numeric_unquoting` and `test_load_config_with_env_vars` both set `TEST_PHONE` to different values).

- [ ] **Step 2: Add the lock to `src/config/tests.rs` (top of file, after existing imports)**

```rust
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Serializes tests that mutate process environment variables. The test
/// harness runs tests on parallel threads within one process and the
/// environment is process-global, so every test that calls
/// `env::set_var`/`env::remove_var` must hold this lock for its whole body.
/// A poisoned lock is safe to reuse: each test restores the vars it set.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}
```

- [ ] **Step 3: Guard every env-mutating test**

Enumerate with `grep -n "set_var\|remove_var" src/config/tests.rs`; add `let _env = env_lock();` as the FIRST line of each test that appears. As of the audit that is exactly these 11 tests: `test_expand_env_vars_single_variable` (:12), `test_expand_env_vars_multiple_variables` (:24), `test_expand_env_vars_no_recursive_expansion` (:49), `test_expand_env_vars_numeric_unquoting` (:73), `test_load_valid_config` (:210), `test_load_config_with_env_vars` (:255), `test_load_missing_config` (:316), `test_load_invalid_toml` (:329), `test_resolve_path_from_env` (:348), `test_resolve_path_default` (:362), `test_load_config_with_file_logging_options` (set_var at :588).

- [ ] **Step 4: Verify parallel stability**

Run: `for i in $(seq 1 10); do cargo test config 2>&1 | tail -1; done`
Expected: 10/10 green on the default parallel harness (no `--test-threads=1`).

- [ ] **Step 5: Retire the serial mandate in live docs**

- `justfile`: delete the `test-config` recipe (lines 17-19).
- `CLAUDE.md:12`: replace the serial line with a note that config tests self-serialize via an internal `ENV_LOCK`.
- `CLAUDE.md:50` ("Test Organization"): same replacement.
- `README.md:1618` and `docs/workflow.md:78`: replace `cargo test config -- --test-threads=1` with `cargo test config`.
- Historical docs (`docs/memory.md`, `docs/tasklist.md` phase notes, `docs/phase-20-plan.md`, `docs/refactoring/`) are journal entries — leave them.

- [ ] **Step 6: Full gate + commit**

Run: `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all green.

```bash
git add src/config/tests.rs justfile CLAUDE.md README.md docs/workflow.md
git commit -m "fix: serialize env-mutating config tests with an internal lock"
```

---

### Task 2: Char-aware `redact_phone`, used by `interactive_auth`

**Files:**
- Modify: `src/logging.rs:85-98` (`redact_phone`)
- Modify: `src/telegram/auth.rs:18-22` (use the helper) and `:59-76` (delete placeholder test module)
- Test: `src/logging_tests.rs` (new regression test beside the existing `redact_phone` tests)

**Interfaces:**
- Consumes: existing `pub fn redact_phone(phone: &str) -> String` (`logging.rs:85`).
- Produces: same signature, now safe for any UTF-8 input.

- [ ] **Step 1: Write the failing test**

In `src/logging_tests.rs`, next to the existing `redact_phone` tests:

```rust
#[test]
fn redact_phone_multibyte_input_does_not_panic() {
    // 8 chars but 24 bytes: byte index 4 is not a char boundary, so the old
    // byte-slicing implementation panicked here.
    assert_eq!(redact_phone("€€€€€€€€"), "€€€€***€€€");
}
```

- [ ] **Step 2: Run it — expect a panic-failure**

Run: `cargo test redact_phone_multibyte -- --nocapture`
Expected: FAIL with `byte index 4 is not a char boundary`.

- [ ] **Step 3: Make `redact_phone` char-aware**

Replace the body at `logging.rs:85-98`:

```rust
pub fn redact_phone(phone: &str) -> String {
    let chars: Vec<char> = phone.chars().collect();
    if chars.len() <= 6 {
        return "[REDACTED]".to_string();
    }

    let start: String = chars[..4].iter().collect();
    let end: String = chars[chars.len() - 3..].iter().collect();
    format!("{start}***{end}")
}
```

(Behavior for ASCII input is unchanged — the existing tests must stay green.)

- [ ] **Step 4: Run the logging tests**

Run: `cargo test redact`
Expected: PASS, including all pre-existing redaction tests.

- [ ] **Step 5: Route auth logging through the helper**

In `src/telegram/auth.rs`: add `use crate::logging::redact_phone;` and replace lines 18-22 with:

```rust
    // Request login code
    tracing::info!("Requesting login code for phone: {}", redact_phone(phone));
```

Delete the placeholder test module (`auth.rs:59-76` — `test_auth_module_compiles` tests nothing). Preserve its manual-testing checklist by moving it into `interactive_auth`'s doc comment as a `# Manual testing` section (5 numbered steps, verbatim).

- [ ] **Step 6: Full gate + commit**

Run: `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all green (test count drops by 1 from the deleted placeholder, rises by 1 from the new test).

```bash
git add src/logging.rs src/logging_tests.rs src/telegram/auth.rs
git commit -m "fix: char-aware phone redaction; use it in interactive auth logging"
```

---

### Task 3: `wire_message_id` — reject out-of-range ids instead of wrapping

**Files:**
- Modify: `src/mcp/tools/helpers.rs` (new fn after `dedupe_and_validate_ids`; refactor that fn to use it; unit tests in its test module)
- Modify: `src/mcp/tools.rs` (re-export alongside the existing helper re-exports if module privacy requires it)
- Modify: `src/mcp/server/impl_media.rs:24/38` and `:254/279`, `src/mcp/server/impl_search.rs:454/465`
- Test: `src/mcp/tests/media.rs`, `src/mcp/tests/transcription.rs`, `src/mcp/tests/message_by_link.rs`

**Interfaces:**
- Consumes: `MessageId::as_i32(&self) -> Option<i32>` (`ids.rs:59`); `parse_message_id(id: i64) -> Result<MessageId, String>` (`helpers.rs:47`); `MessageId` is `Copy`.
- Produces: `pub(crate) fn wire_message_id(id: MessageId) -> Result<i32, String>` with error text `message_id {id} exceeds Telegram's message id range` (identical wording to the existing batch-path error at `helpers.rs:75`).

- [ ] **Step 1: Write the three failing tool tests**

`src/mcp/tests/media.rs` (uses the file's existing `request` helper at :34):

```rust
#[tokio::test]
async fn rejects_message_id_beyond_wire_range() {
    // No expectations: neither the rate limiter nor the client may be
    // called for an id that cannot exist on the wire.
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", i64::from(i32::MAX) + 1, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let err = result.expect_err("out-of-range id must be rejected");
    assert!(err.contains("exceeds Telegram's message id range"), "got: {err}");
}
```

`src/mcp/tests/transcription.rs` (uses the file's `request` and `server` helpers):

```rust
#[tokio::test]
async fn rejects_message_id_beyond_wire_range() {
    let server = server(MockTelegramClientTrait::new(), MockRateLimiterTrait::new());
    let result = server
        .transcribe_voice_message(
            Parameters(request("news", i64::from(i32::MAX) + 1, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;
    let err = result.expect_err("out-of-range id must be rejected");
    assert!(err.contains("exceeds Telegram's message id range"), "got: {err}");
}
```

`src/mcp/tests/message_by_link.rs` (2147483648 = `i32::MAX + 1`):

```rust
#[tokio::test]
async fn rejects_link_with_message_id_beyond_wire_range() {
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetMessageByLinkRequest {
        link: "https://t.me/swodki/2147483648".to_string(),
    };
    let result = server
        .get_message_by_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;
    let err = result.expect_err("out-of-range id must be rejected");
    assert!(err.contains("exceeds Telegram's message id range"), "got: {err}");
}
```

(If `GetMessageByLinkRequest` at `requests.rs:250` has fields beyond `link`, fill them with `None`/defaults.)

- [ ] **Step 2: Run them — expect mockall unexpected-call panics**

Run: `cargo test rejects_message_id_beyond_wire_range rejects_link_with_message_id`
Expected: 3 FAIL — today the wrapped id flows on and hits an un-expected mock call (`acquire` / `is_premium`), which panics.

- [ ] **Step 3: Add the helper + unit tests**

In `src/mcp/tools/helpers.rs`, directly after `dedupe_and_validate_ids`:

```rust
/// A validated [`MessageId`] as Telegram's wire (`i32`) form, or a
/// caller-facing error naming the id when it exceeds the wire range.
pub(crate) fn wire_message_id(id: MessageId) -> Result<i32, String> {
    id.as_i32()
        .ok_or_else(|| format!("message_id {} exceeds Telegram's message id range", id.get()))
}
```

Refactor `dedupe_and_validate_ids` (`helpers.rs:69-77`) to use it:

```rust
    for id in &unique {
        let parsed = parse_message_id(*id)?;
        wire_ids.push(wire_message_id(parsed)?);
    }
```

Unit tests in the file's existing test module:

```rust
#[test]
fn wire_message_id_rejects_beyond_i32() {
    let id = parse_message_id(i64::from(i32::MAX) + 1).expect("positive id parses");
    let err = wire_message_id(id).expect_err("must reject");
    assert!(err.contains("exceeds Telegram's message id range"));
}

#[test]
fn wire_message_id_passes_in_range() {
    let id = parse_message_id(42).expect("valid id");
    assert_eq!(wire_message_id(id), Ok(42));
}
```

- [ ] **Step 4: Convert at the three call sites (each BEFORE any rate-limiter/client call)**

Each `impl_*.rs` file imports the helper itself (do not widen the `server.rs` prelude): `use crate::mcp::tools::helpers::wire_message_id;` — or via a `tools.rs` re-export if `mod helpers` is private.

- `impl_media.rs` `get_message_media_impl`: after `:24` add `let wire_id = wire_message_id(message_id)?;`; at `:38` pass `wire_id` instead of `message_id.get() as i32`.
- `impl_media.rs` `transcribe_voice_message_impl`: after the parse at `:254` add the same line; at `:279` pass `wire_id`.
- `impl_search.rs` `get_message_by_link_impl`: after the `channel_identifier` match ending `:454` add the same line; at `:465` pass `wire_id`.

- [ ] **Step 5: Run the new tests + neighbors**

Run: `cargo test rejects_message_id_beyond_wire_range rejects_link_with_message_id wire_message_id && cargo test media && cargo test batch`
Expected: all PASS (batch tests confirm the `dedupe_and_validate_ids` refactor is behavior-identical).

- [ ] **Step 6: Full gate + commit**

Run: `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all green.

```bash
git add src/mcp/tools/helpers.rs src/mcp/tools.rs src/mcp/server/impl_media.rs \
  src/mcp/server/impl_search.rs src/mcp/tests/media.rs src/mcp/tests/transcription.rs \
  src/mcp/tests/message_by_link.rs
git commit -m "fix: reject message ids beyond Telegram's i32 wire range instead of wrapping"
```

---

### Task 4: Delete dead code

**Files:**
- Modify: `src/telegram/client/auth.rs` (delete `sign_in`, `:20-39`)
- Modify: `src/mcp/tools/helpers.rs` (delete `parse_optional_channel_id` `:82-96` incl. doc comment, and its 3 tests `:249-267`)
- Modify: `src/mcp/tools.rs:18` (drop `parse_optional_channel_id` from the re-export list)
- Modify: `src/telegram/converters/media.rs` (delete `matches_media_filter` `:317-322` incl. doc comment; keep `matches_media_filter_raw` and `media_matches_filter`)
- Modify: `src/telegram/converters.rs:17` (drop `matches_media_filter` from the re-export list)
- Modify: `src/telegram/albums.rs:50` (replace `#[allow(dead_code)]` on `overflowed` with `#[cfg(test)]`)

**Interfaces:**
- Consumes: audit verification that each item has zero production callers (spec, Stage 1 §4).
- Produces: nothing — deletions only. `TelegramClient::request_login_code` and `check_password` stay (both used by `interactive_auth`).

- [ ] **Step 1: Re-verify zero callers, then delete all four items**

Run first (each must return only the definition/re-export/test lines listed above):
`grep -rn "sign_in\|parse_optional_channel_id\|matches_media_filter\b\|overflowed" src --include="*.rs"`
Note: `interactive_auth`'s `client.client().sign_in(...)` (`auth.rs:32`) is the raw grammers call, not the deleted wrapper — it stays. Then apply the deletions/edits listed under **Files**.

- [ ] **Step 2: Let the compiler sweep the residue**

Run: `cargo clippy -- -D warnings`
Expected: clean after removing any now-unused imports the deletions expose (e.g. `grammers_client::message::Message` in `converters/media.rs` if only `matches_media_filter` used it).

- [ ] **Step 3: Full test suite**

Run: `cargo test`
Expected: all green; count drops by exactly the 3 deleted `parse_optional_channel_id` tests.

- [ ] **Step 4: Commit**

```bash
git add src/telegram/client/auth.rs src/mcp/tools/helpers.rs src/mcp/tools.rs \
  src/telegram/converters/media.rs src/telegram/converters.rs src/telegram/albums.rs
git commit -m "chore: remove dead code surfaced by the 2026-08-15 audit"
```

---

### Task 5: Tracking docs, gate, review, PR

**Files:**
- Modify: `docs/tasklist.md` (new phase row: "38 | Audit stage 1: correctness + dead code")
- Modify: `docs/memory.md` (short journal entry: audit ran, stage 1 shipped, stages 2-4 specced)
- Create: (already exists on this branch) spec + this plan — committed here if not yet committed

**Interfaces:**
- Consumes: Tasks 1-4 all committed and green.
- Produces: a PR ready for review.

- [ ] **Step 1: Add the tasklist row and memory entry**

`docs/tasklist.md` progress table, after row 37: phase 38, status ✅, note pointing at the spec and this plan. `docs/memory.md`: dated entry with the three fixes + deletions, and the parallel-`cargo test` behavior change.

- [ ] **Step 2: Full pre-merge gate, fresh**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all green. Also run `cargo test config` 3× more (parallel) as a final race check.

- [ ] **Step 3: Commit docs**

```bash
git add docs/tasklist.md docs/memory.md docs/superpowers/specs/2026-08-15-project-audit.md \
  docs/superpowers/plans/2026-08-15-audit-stage1-correctness.md
git commit -m "docs: record 2026-08-15 audit spec and stage-1 plan; tasklist phase 38"
```

- [ ] **Step 4: Request code review (superpowers:requesting-code-review), then open the PR**

```bash
git push -u origin fix/audit-stage1-correctness
gh pr create --title "fix: audit stage 1 — correctness fixes + dead code" \
  --body "$(cat <<'EOF'
Closes stage 1 of the 2026-08-15 project audit (docs/superpowers/specs/2026-08-15-project-audit.md):

- Config tests self-serialize env-var mutation via ENV_LOCK — plain `cargo test` is now race-free; the `test-config` serial recipe is retired.
- `redact_phone` is char-aware (no panic on multibyte/short input) and `interactive_auth` now uses it instead of hand-slicing the phone number.
- New `wire_message_id` helper rejects message ids beyond Telegram's i32 wire range at all three single-message tool sites (previously wrapped silently); batch path refactored onto the same helper.
- Dead code removed: `TelegramClient::sign_in`, `parse_optional_channel_id`, `matches_media_filter`, placeholder auth test; `PostCounter::overflowed` gated `#[cfg(test)]`.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Self-review (done at planning time)

- **Spec coverage:** Stage 1 items 1-4 map to Tasks 1-4; Task 5 is workflow overhead. Hygiene-backlog items are deliberately out of scope (spec keeps them).
- **Placeholders:** none — every code step carries the actual code; the two "adjust if reality differs" notes (`GetMessageByLinkRequest` extra fields, `helpers` module privacy) name the exact fallback.
- **Type consistency:** `wire_message_id(id: MessageId) -> Result<i32, String>` used identically in Tasks 3's helper, tests, and call sites; `MessageId` is `Copy` so pass-by-value then later `message_id.get()` reuse in `impl_media.rs:49` stays valid.
