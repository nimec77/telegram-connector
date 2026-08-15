# Audit Stage 3 — Duplication / KISS Refactors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the duplication the 2026-08-15 audit found (MCP list-tool prologue/fan-out, telegram cursor/album-admit blocks, serde copy-paste pairs, hand-inlined empty guards) and split the two oversized units (`ops_search::search_messages_impl`, `impl_media` batch loop), with zero behavior change except two documented message/log unifications.

**Architecture:** Pure extract-function/extract-struct refactors. New shared helpers land next to their existing homes (`helpers.rs`, `shaping.rs`, `fanout.rs`, `albums.rs`, `client.rs`, `responses.rs`); call sites shrink to one-liners. No new modules except one `#[path]`-included test file. All 16 tools stay in `server.rs` (rmcp macro constraint).

**Tech Stack:** Rust nightly (2024 edition), serde/serde_json, futures `StreamExt::buffered`, mockall-backed test suite (~705 lib tests).

**Spec:** `docs/superpowers/specs/2026-08-15-project-audit.md` (Stage 3 section + the two dedup-flavored hygiene items). Findings were re-verified at the cited lines on 2026-08-15 at v0.22.1 master.

## Global Constraints

- Branch: `refactor/audit-stage3-dedup` off `master` (already created). Execute in an isolated worktree per superpowers:using-git-worktrees.
- Pre-merge gate after EVERY task: `cargo fmt --check && cargo clippy -- -D warnings && cargo test` (run `cargo fmt --all` first, then the gate).
- Behavior-preserving except two accepted deltas: (1) the 7 client-layer empty-guard messages unify from "Channel reference cannot be empty" to "Channel identifier cannot be empty" (Task 9; no test pins the old text — verified); (2) the global-search debug log's `duration_ms` measures from `search_global`'s entry instead of the outer impl's entry (Task 8; debug-level diagnostic only).
- Advertised tool JSON schemas must not change — `schema_integrity` tests pin this; `deserialize_with` is invisible to schemars.
- Never `unwrap()` in production code; 100-char lines; `tracing` for logs.
- Error message strings copied in this plan are exact — do not rephrase them; MCP tests pin several.
- TDD: new helpers get failing tests first (compile failure = red). Move-only refactor tasks (5, 8, 10) are covered by the existing suite; run the named suites before and after.
- Commit style: `refactor:` / `test:` conventional commits, one commit per task.

---

### Task 1: MCP cursor-bounds + max_text_length dedup

The ~34-line cursor parse/cross-validate block and the 6-line max_text_length block are byte-identical in `search_messages_impl` (impl_search.rs:64–91) and `get_recent_messages_impl` (impl_search.rs:270–297).

**Files:**
- Modify: `src/mcp/tools/helpers.rs` (add `parse_cursor_bounds` + tests in the existing inline `mod tests`)
- Modify: `src/mcp/tools/shaping.rs` (add `resolve_max_text_length` + a new small inline `#[cfg(test)] mod tests`)
- Modify: `src/mcp/server/impl_search.rs` (both impls call the helpers)

**Interfaces:**
- Produces: `pub(crate) fn parse_cursor_bounds(before_id: Option<i64>, after_id: Option<i64>) -> Result<(Option<MessageId>, Option<MessageId>), String>` in `crate::mcp::tools::helpers`
- Produces: `pub(crate) fn resolve_max_text_length(requested: Option<u32>) -> Result<u32, String>` in `crate::mcp::tools::shaping`

- [ ] **Step 1: Write the failing tests**

In `src/mcp/tools/helpers.rs`, append to the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn parse_cursor_bounds_passes_valid_pair() {
    let (before, after) = parse_cursor_bounds(Some(10), Some(5)).expect("valid");
    assert_eq!(before.map(|b| b.get()), Some(10));
    assert_eq!(after.map(|a| a.get()), Some(5));
}

#[test]
fn parse_cursor_bounds_none_stays_none() {
    let (before, after) = parse_cursor_bounds(None, None).expect("valid");
    assert!(before.is_none() && after.is_none());
}

#[test]
fn parse_cursor_bounds_rejects_crossed_pair_naming_both_ids() {
    let err = parse_cursor_bounds(Some(5), Some(10)).unwrap_err();
    assert!(err.contains("before_id (5)"), "got: {err}");
    assert!(err.contains("after_id (10)"), "got: {err}");
}

#[test]
fn parse_cursor_bounds_rejects_equal_pair() {
    assert!(parse_cursor_bounds(Some(7), Some(7)).is_err());
}

#[test]
fn parse_cursor_bounds_prefixes_the_failing_field() {
    let err = parse_cursor_bounds(Some(-1), None).unwrap_err();
    assert!(err.starts_with("before_id:"), "got: {err}");
    let err = parse_cursor_bounds(None, Some(-1)).unwrap_err();
    assert!(err.starts_with("after_id:"), "got: {err}");
}
```

In `src/mcp/tools/shaping.rs`, add at the end of the file (it has no test module today):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_max_text_length_defaults_when_omitted() {
        assert_eq!(resolve_max_text_length(None), Ok(DEFAULT_MAX_TEXT_LENGTH));
    }

    #[test]
    fn resolve_max_text_length_passes_explicit_value() {
        assert_eq!(resolve_max_text_length(Some(64)), Ok(64));
    }

    #[test]
    fn resolve_max_text_length_rejects_zero() {
        let err = resolve_max_text_length(Some(0)).unwrap_err();
        assert!(err.contains("greater than 0"), "got: {err}");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test parse_cursor_bounds resolve_max_text_length 2>&1 | tail -20`
Expected: compile error — `parse_cursor_bounds` / `resolve_max_text_length` not found.

- [ ] **Step 3: Implement the helpers**

In `src/mcp/tools/helpers.rs`, after `parse_message_id`:

```rust
/// Parse and cross-validate the optional cursor bounds (A8). Both ids parse
/// via `parse_message_id`; when both are present, `before_id` must exceed
/// `after_id` — the page covers `after_id < id < before_id`.
pub(crate) fn parse_cursor_bounds(
    before_id: Option<i64>,
    after_id: Option<i64>,
) -> Result<(Option<MessageId>, Option<MessageId>), String> {
    let before = before_id
        .map(parse_message_id)
        .transpose()
        .map_err(|e| format!("before_id: {}", e))?;
    let after = after_id
        .map(parse_message_id)
        .transpose()
        .map_err(|e| format!("after_id: {}", e))?;
    if let (Some(b), Some(a)) = (before, after)
        && b.get() <= a.get()
    {
        return Err(format!(
            "before_id ({}) must be greater than after_id ({}): the page covers after_id \
             < id < before_id",
            b.get(),
            a.get()
        ));
    }
    Ok((before, after))
}
```

In `src/mcp/tools/shaping.rs`, next to `DEFAULT_MAX_TEXT_LENGTH`:

```rust
/// Resolve the effective `max_text_length`: default when omitted, rejecting 0.
pub(crate) fn resolve_max_text_length(requested: Option<u32>) -> Result<u32, String> {
    let max_text_length = requested.unwrap_or(DEFAULT_MAX_TEXT_LENGTH);
    if max_text_length == 0 {
        return Err("max_text_length must be greater than 0".to_string());
    }
    Ok(max_text_length)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test parse_cursor_bounds resolve_max_text_length`
Expected: 8 PASS.

- [ ] **Step 5: Rewire both impls**

In `src/mcp/server/impl_search.rs`:
- Change line 7 to `use crate::mcp::tools::helpers::{parse_cursor_bounds, wire_message_id};`
- In `search_messages_impl`, replace the block from `// Parse and cross-validate the cursor bounds (A8).` (line 64) through the `if max_text_length == 0 { ... }` check (line 91) with:

```rust
        let (before_id, after_id) = parse_cursor_bounds(request.before_id, request.after_id)?;
        let max_text_length = shaping::resolve_max_text_length(request.max_text_length)?;
```

- In `get_recent_messages_impl`, replace the identical block (lines 270–297) with the same two lines. Keep the `let format = request.format.unwrap_or_default();` line that follows.

- [ ] **Step 6: Full gate**

Run: `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all pass — `src/mcp/tests/search_core.rs` cursor tests pin the exact error strings and must stay green.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "refactor: extract shared cursor-bounds and max_text_length parsing (audit S3.1)"
```

---

### Task 2: `fanout::run` scaffold

The 25-line buffered-stream fan-out scaffold is duplicated at impl_search.rs:134–158 and :342–368; only the per-channel fetch differs.

**Files:**
- Modify: `src/mcp/tools/fanout.rs` (add `run` + test)
- Modify: `src/mcp/server/impl_search.rs` (both fan-out branches call it)
- Modify: `src/mcp/server.rs` (drop `use futures::StreamExt;` if it becomes unused)

**Interfaces:**
- Consumes: `ChannelFetchOutcome` (existing, same file)
- Produces: `pub(crate) async fn run<F, Fut>(list: Vec<String>, fetch: F) -> Vec<ChannelFetchOutcome> where F: Fn(String) -> Fut, Fut: Future<Output = Result<SearchResult, String>>` in `crate::mcp::tools::fanout`

- [ ] **Step 1: Write the failing test**

In `src/mcp/tools/fanout.rs` tests module (reuses the existing `result_with` fixture):

```rust
#[tokio::test]
async fn run_pairs_each_channel_with_its_outcome_in_list_order() {
    let outcomes = run(vec!["a".into(), "b".into()], |reference| async move {
        if reference == "a" {
            Ok(result_with(&[(1, 5)], 1, false))
        } else {
            Err(format!("boom {reference}"))
        }
    })
    .await;
    assert_eq!(outcomes.len(), 2);
    assert_eq!(outcomes[0].channel, "a");
    assert!(outcomes[0].result.is_ok());
    assert_eq!(outcomes[1].channel, "b");
    assert_eq!(outcomes[1].result.as_ref().unwrap_err(), "boom b");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test run_pairs_each_channel 2>&1 | tail -5`
Expected: compile error — `run` not found in this scope.

- [ ] **Step 3: Implement `run`**

In `src/mcp/tools/fanout.rs`, add imports `use futures::StreamExt;` and `use std::future::Future;`, then below `ChannelFetchOutcome`:

```rust
/// Fetch every channel in `list` through `fetch` with bounded concurrency
/// (`FANOUT_CONCURRENCY`), pairing each reference with its outcome. Outcome
/// order follows `list` (buffered preserves order).
pub(crate) async fn run<F, Fut>(list: Vec<String>, fetch: F) -> Vec<ChannelFetchOutcome>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<SearchResult, String>>,
{
    futures::stream::iter(list.into_iter().map(|reference| {
        let result = fetch(reference.clone());
        async move {
            ChannelFetchOutcome {
                channel: reference,
                result: result.await,
            }
        }
    }))
    .buffered(FANOUT_CONCURRENCY)
    .collect()
    .await
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test run_pairs_each_channel`
Expected: PASS.

- [ ] **Step 5: Rewire both fan-out branches**

In `search_messages_impl`, replace the `let outcomes = futures::stream::iter(...)...await;` block (currently lines 134–158) with:

```rust
            let outcomes = fanout::run(list, |reference| {
                let base = params_template.clone(); // SearchParams minus channel_id
                async move {
                    match self.search_channel_id(&reference).await {
                        Ok(channel_id) => {
                            let params = SearchParams {
                                channel_id: Some(channel_id),
                                ..base
                            };
                            self.telegram_client
                                .search_messages(&params)
                                .await
                                .map_err(|e| e.to_string())
                        }
                        Err(e) => Err(e),
                    }
                }
            })
            .await;
```

In `get_recent_messages_impl`, replace its twin block (currently lines 342–368) with:

```rust
            let outcomes = fanout::run(list, |reference| {
                let base = params_template.clone(); // HistoryParams minus target
                async move {
                    match history_target(&reference) {
                        Ok((channel_id, channel_identifier)) => {
                            let params = HistoryParams {
                                channel_id,
                                channel_identifier,
                                ..base
                            };
                            self.telegram_client
                                .get_recent_messages(&params)
                                .await
                                .map_err(|e| e.to_string())
                        }
                        Err(e) => Err(e),
                    }
                }
            })
            .await;
```

(The `Arc::clone(&self.telegram_client)` in the old history block is dropped — the closure borrows `self` exactly like the search block always did.)

- [ ] **Step 6: Full gate**

Run: `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: pass. If clippy flags `futures::StreamExt` as unused in `src/mcp/server.rs`, delete that import line (fan-out was its only consumer). `src/mcp/tests/multi_channel.rs` pins fan-out behavior and must stay green.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "refactor: extract fanout::run scaffold shared by both list tools (audit S3.1)"
```

---

### Task 3: shared `unique_channel_count`

The distinct-channel HashSet recompute is duplicated at `shaping.rs:106–113` and `fanout.rs:84–87`.

**Files:**
- Modify: `src/mcp/tools/types/responses.rs` (add fn next to `MessageResponse`)
- Test: `src/mcp/tools/types/tests/responses_tests.rs`
- Modify: `src/mcp/tools/shaping.rs` (`fit_to_budget`), `src/mcp/tools/fanout.rs` (`merge_results`)

**Interfaces:**
- Produces: `pub(crate) fn unique_channel_count(messages: &[MessageResponse]) -> u32` in `crate::mcp::tools::types::responses`

- [ ] **Step 1: Write the failing test**

In `src/mcp/tools/types/tests/responses_tests.rs`:

```rust
#[test]
fn unique_channel_count_dedupes_and_skips_missing_ids() {
    use crate::test_helpers::create_test_message;

    let mut hoisted = MessageResponse::from(create_test_message(4, "d", 300));
    hoisted.channel_id = None; // compact hoisting cleared the per-message field

    let messages = vec![
        MessageResponse::from(create_test_message(1, "a", 100)),
        MessageResponse::from(create_test_message(2, "b", 100)),
        MessageResponse::from(create_test_message(3, "c", 200)),
        hoisted,
    ];
    assert_eq!(unique_channel_count(&messages), 2);
    assert_eq!(unique_channel_count(&[]), 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test unique_channel_count 2>&1 | tail -5` — expected: compile error, fn not found.

- [ ] **Step 3: Implement**

In `src/mcp/tools/types/responses.rs`:

```rust
/// Distinct channel ids across a shaped message list. Multi-channel counts
/// recompute from the messages because compact hoisting may have cleared
/// per-message channel fields (`channel_id: None` entries are skipped).
pub(crate) fn unique_channel_count(messages: &[MessageResponse]) -> u32 {
    messages
        .iter()
        .filter_map(|m| m.channel_id.as_ref().map(|c| c.get()))
        .collect::<std::collections::HashSet<i64>>()
        .len() as u32
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test unique_channel_count` — expected: PASS.

- [ ] **Step 5: Rewire the two call sites**

`src/mcp/tools/fanout.rs` `merge_results` — replace the `let unique: HashSet<i64> = ...` block (lines 84–87) and the `channels_in_results: unique.len() as u32` field with:

```rust
    let channels_in_results = unique_channel_count(&messages);
```

(compute just before building `SearchResponse`; use `channels_in_results` in the field). Import: `use crate::mcp::tools::types::responses::unique_channel_count;` (or extend the existing `responses::` import list).

`src/mcp/tools/shaping.rs` `fit_to_budget` — replace lines 106–113 with:

```rust
        let count = unique_channel_count(&resp.messages);
        if count > 0 {
            resp.query_metadata.channels_in_results = count;
        }
```

- [ ] **Step 6: Full gate** — `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test` (fanout merge tests + `mcp/tests/search_shaping.rs` budget tests pin both call sites).

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "refactor: share unique_channel_count between shaping and fanout (audit S3.6)"
```

---

### Task 4: serde_helpers generics

Two copy-paste pairs: `deserialize_optional_media_filter`/`deserialize_optional_response_format` (~60 lines) and `flexible_opt_u32`/`flexible_opt_i64` (~50 lines). One generic each. `flexible_i64`, `flexible_string`, `flexible_opt_string`, `flexible_opt_bool` stay as-is (not pairs — scope per spec).

**Files:**
- Modify: `src/mcp/tools/types/serde_helpers.rs`
- Modify: `src/mcp/tools/types/tests/serde_helpers_tests.rs` (attribute strings + 2 new tests)
- Modify: `src/mcp/tools/types/requests.rs` (import list line 3 + ~25 attribute strings)

**Interfaces:**
- Produces: `pub fn flexible_opt_enum<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error> where T: DeserializeOwned` — replaces both `deserialize_optional_*` fns
- Produces: `pub fn flexible_opt_int<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error> where T: Deserialize<'de> + FromStr` — replaces `flexible_opt_u32`/`flexible_opt_i64`
- serde's `deserialize_with = "flexible_opt_int"` infers `T` from the field type — no turbofish needed in attributes.

- [ ] **Step 1: Write the failing tests**

In `src/mcp/tools/types/tests/serde_helpers_tests.rs`:

```rust
#[derive(Deserialize)]
struct OptIntBothT {
    #[serde(default, deserialize_with = "flexible_opt_int")]
    small: Option<u32>,
    #[serde(default, deserialize_with = "flexible_opt_int")]
    wide: Option<i64>,
}

#[test]
fn flexible_opt_int_serves_both_widths() {
    let t: OptIntBothT = serde_json::from_str(r#"{"small": "10", "wide": -5}"#).unwrap();
    assert_eq!(t.small, Some(10));
    assert_eq!(t.wide, Some(-5));
    assert!(serde_json::from_str::<OptIntBothT>(r#"{"small": -1}"#).is_err());
}

#[derive(Deserialize)]
struct OptEnumBothT {
    #[serde(default, deserialize_with = "flexible_opt_enum")]
    format: Option<ResponseFormat>,
    #[serde(default, deserialize_with = "flexible_opt_enum")]
    filter: Option<MediaFilter>,
}

#[test]
fn flexible_opt_enum_serves_both_enums() {
    let t: OptEnumBothT =
        serde_json::from_str(r#"{"format": "compact", "filter": "photo"}"#).unwrap();
    assert_eq!(t.format, Some(ResponseFormat::Compact));
    assert_eq!(t.filter, Some(MediaFilter::Photo));
    let empty: OptEnumBothT = serde_json::from_str(r#"{"format": "", "filter": ""}"#).unwrap();
    assert!(empty.format.is_none() && empty.filter.is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test flexible_opt_int_serves flexible_opt_enum_serves 2>&1 | tail -5` — compile error expected.

- [ ] **Step 3: Implement the generics**

In `src/mcp/tools/types/serde_helpers.rs`, replace the four fns (`deserialize_optional_media_filter`, `deserialize_optional_response_format`, `flexible_opt_u32`, `flexible_opt_i64`) with:

```rust
/// Deserialize an optional string-encoded enum, treating empty strings and
/// JSON `null` as `None`. Handles MCP clients that send `"field": ""`
/// instead of omitting the field. Non-empty values parse with `T`'s own
/// `Deserialize`, so `T`'s error text is preserved.
pub fn flexible_opt_enum<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) if s.is_empty() => Ok(None),
        Some(value) => serde_json::from_value(value)
            .map(Some)
            .map_err(Error::custom),
    }
}

/// Deserialize `Option<T>` for an integer `T`, accepting either a JSON number
/// or a numeric string. The string form is trimmed before parsing. An
/// empty/whitespace string or a JSON `null` becomes `None`. Floats,
/// negatives (for unsigned `T`), out-of-range, and non-numeric values error.
pub fn flexible_opt_int<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + std::str::FromStr,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr<T> {
        Num(T),
        Str(String),
    }

    match Option::<NumOrStr<T>>::deserialize(deserializer)? {
        None => Ok(None),
        Some(NumOrStr::Num(n)) => Ok(Some(n)),
        Some(NumOrStr::Str(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed
                .parse::<T>()
                .map(Some)
                .map_err(|_| Error::custom(format!("expected an integer, got '{}'", s)))
        }
    }
}
```

`use serde_json` is already reachable (the old enum fns used `serde_json::from_value`).

- [ ] **Step 4: Update all references**

- `src/mcp/tools/types/requests.rs` line 3 import list: replace `deserialize_optional_media_filter`, `deserialize_optional_response_format`, `flexible_opt_u32`, `flexible_opt_i64` with `flexible_opt_enum`, `flexible_opt_int`.
- In `requests.rs` attributes: every `"flexible_opt_u32"` and `"flexible_opt_i64"` → `"flexible_opt_int"`; every `"deserialize_optional_media_filter"` and `"deserialize_optional_response_format"` → `"flexible_opt_enum"`.
- In `serde_helpers_tests.rs`: same attribute renames (including the fully-qualified `crate::mcp::tools::types::serde_helpers::flexible_opt_i64` probe at ~line 132 → `...::flexible_opt_int`).

```bash
sed -i '' -e 's/flexible_opt_u32/flexible_opt_int/g; s/flexible_opt_i64/flexible_opt_int/g; s/deserialize_optional_media_filter/flexible_opt_enum/g; s/deserialize_optional_response_format/flexible_opt_enum/g' \
  src/mcp/tools/types/requests.rs src/mcp/tools/types/tests/serde_helpers_tests.rs
```

Then dedupe any doubled names the sed produced in the two `use` lists (e.g. `flexible_opt_int, flexible_opt_int`).

- [ ] **Step 5: Full gate**

Run: `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all existing serde tests pass with only attribute-string changes (behavior identical); `schema_integrity` stays green (schemas ignore `deserialize_with`).

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "refactor: collapse serde_helpers copy-paste pairs into flexible_opt_int/flexible_opt_enum (audit S3.4)"
```

---

### Task 5: `impl_media` batch loop → per-outcome function

The ~90-line loop body in `get_messages_media_batch_impl` (impl_media.rs:129–219) becomes a free async fn; the loop becomes a 10-line match. Pure refactor — covered by `src/mcp/tests/media.rs` + `src/mcp/tests/media_batch_budget.rs`.

**Files:**
- Modify: `src/mcp/server/impl_media.rs`

**Interfaces:**
- Consumes: `MediaFetchOutcome` (`src/telegram/types/media.rs:209`, fields `message_id: i32`, `result: Result<MediaDownload, MediaFetchError>`), `Base64Budget`, `process_image_with_cap`, `media_metadata`, `failure_reason`, `post_download_failure_reason` (all existing)
- Produces (file-private): `async fn process_media_outcome(channel_id: &str, max_dimension: u32, budget: &mut Base64Budget, outcome: MediaFetchOutcome) -> Result<(String, String), String>` — `Ok((base64_jpeg, metadata_json))` or `Err(machine-readable reason)`

- [ ] **Step 1: Baseline** — Run: `cargo test --lib media` — record pass count.

- [ ] **Step 2: Extract the function**

Add below `media_metadata` (import `MediaFetchOutcome` alongside the existing `MediaDownload` import):

```rust
/// Process one batch outcome end to end: download-failure mapping, budget
/// allowance, decode/shrink/encode on a blocking thread, metadata
/// serialization. Returns the content pair for a success, or the
/// machine-readable failure reason. The budget is consumed only after every
/// fallible step has succeeded, so a failed id never charges the cap and
/// later ids keep their full allowance.
async fn process_media_outcome(
    channel_id: &str,
    max_dimension: u32,
    budget: &mut Base64Budget,
    outcome: MediaFetchOutcome,
) -> Result<(String, String), String> {
    let id = i64::from(outcome.message_id);
    let mut download = outcome.result.map_err(|e| failure_reason(&e))?;

    let Some(allowance) = budget.allowance() else {
        return Err("payload_cap_reached".to_string());
    };

    // process_image_with_cap already shrinks the target dimension iteratively
    // until the encoded payload fits — that is the progressive downscaling.
    // Encode on a blocking thread: a Lanczos3 resize plus JPEG encode is
    // hundreds of milliseconds of pure CPU. A panicked/cancelled task reports
    // the id rather than failing the batch.
    let bytes = std::mem::take(&mut download.bytes);
    let processed = tokio::task::spawn_blocking(move || {
        process_image_with_cap(&bytes, max_dimension, allowance)
    })
    .await
    .map_err(|join_error| format!("internal_error: {join_error}"))?
    .map_err(|e| post_download_failure_reason(&e))?;

    // Serialize before consuming the budget, so a (today unreachable)
    // serialization failure lands this id in `failed` with the budget
    // untouched instead of leaking the allowance.
    let metadata = media_metadata(channel_id.to_string(), id, download, &processed);
    let metadata_json = json_response(&metadata).map_err(|e| format!("internal_error: {e}"))?;

    budget.consume(processed.base64_jpeg.len());
    Ok((processed.base64_jpeg, metadata_json))
}
```

Replace the loop body (the whole `for outcome in outcomes { ... }` block) with:

```rust
        for outcome in outcomes {
            let id = i64::from(outcome.message_id);
            // Encoding runs in request order, so budget allocation is
            // deterministic no matter which download finished first.
            match process_media_outcome(&request.channel_id, max_dimension, &mut budget, outcome)
                .await
            {
                Ok((base64_jpeg, metadata_json)) => {
                    total_base64_bytes += base64_jpeg.len();
                    content.push(ContentBlock::image(base64_jpeg, "image/jpeg"));
                    content.push(ContentBlock::text(metadata_json));
                    returned += 1;
                }
                Err(reason) => failed.push(MediaBatchFailure { id, reason }),
            }
        }
```

- [ ] **Step 3: Full gate**

Run: `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: same pass count as Step 1 baseline; `media_batch_budget` tests pin allowance/refund/failure-token behavior exactly.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "refactor: extract per-outcome fn from media batch loop (audit S3.6)"
```

---

### Task 6: telegram `cursor_wire_bounds`

The 18-line cursor→i32 conversion block is duplicated at `ops_search.rs:35–52` and `ops_history.rs:84–101`.

**Files:**
- Modify: `src/telegram/client.rs` (add fn + declare test module)
- Create: `src/telegram/client/tests/helpers_tests.rs`
- Modify: `src/telegram/client/ops_search.rs`, `src/telegram/client/ops_history.rs`

**Interfaces:**
- Produces: `fn cursor_wire_bounds(before_id: Option<MessageId>, after_id: Option<MessageId>) -> Result<(Option<i32>, Option<i32>), Error>` — private in `client.rs`; submodules reach it via their existing `use super::*;` (private parent items are visible to child modules).

- [ ] **Step 1: Write the failing tests**

Create `src/telegram/client/tests/helpers_tests.rs`:

```rust
//! Tests for client-wide shared helpers (cursor wire bounds; from Task 9 on,
//! also the empty-identifier guard).

use super::*;
use crate::telegram::types::MessageId;

fn mid(id: i64) -> MessageId {
    MessageId::new(id).expect("positive test id")
}

#[test]
fn cursor_wire_bounds_passes_in_range_ids() {
    let (before, after) = cursor_wire_bounds(Some(mid(10)), Some(mid(5))).expect("in range");
    assert_eq!(before, Some(10));
    assert_eq!(after, Some(5));
}

#[test]
fn cursor_wire_bounds_none_stays_none() {
    let (before, after) = cursor_wire_bounds(None, None).expect("ok");
    assert!(before.is_none() && after.is_none());
}

#[test]
fn cursor_wire_bounds_rejects_beyond_i32_naming_the_field() {
    let big = i64::from(i32::MAX) + 1;
    let err = cursor_wire_bounds(Some(mid(big)), None).unwrap_err();
    assert!(err.to_string().contains("before_id"), "got: {err}");
    let err = cursor_wire_bounds(None, Some(mid(big))).unwrap_err();
    assert!(err.to_string().contains("after_id"), "got: {err}");
}
```

In `src/telegram/client.rs` (which lives in `src/telegram/`, so `#[path]` is relative to that dir), declare next to the existing module declarations:

```rust
#[cfg(test)]
#[path = "client/tests/helpers_tests.rs"]
mod helpers_tests;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test cursor_wire_bounds 2>&1 | tail -5` — compile error expected.

- [ ] **Step 3: Implement**

In `src/telegram/client.rs`, near `peer_to_ref`:

```rust
/// Convert the optional cursor bounds to Telegram's `i32` wire ids (A8).
/// MTProto message ids are `i32`; an out-of-range cursor cannot be sent, so
/// it is rejected with the field name in the error.
fn cursor_wire_bounds(
    before_id: Option<MessageId>,
    after_id: Option<MessageId>,
) -> Result<(Option<i32>, Option<i32>), Error> {
    fn wire(field: &str, id: Option<MessageId>) -> Result<Option<i32>, Error> {
        match id {
            Some(id) => id
                .as_i32()
                .ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "{field} {} exceeds Telegram's message id range",
                        id.get()
                    ))
                })
                .map(Some),
            None => Ok(None),
        }
    }
    Ok((wire("before_id", before_id)?, wire("after_id", after_id)?))
}
```

(If `MessageId` is not already in `client.rs` scope, add it to the existing `crate::telegram::types` import.)

- [ ] **Step 4: Run tests to verify they pass** — `cargo test cursor_wire_bounds` → 3 PASS.

- [ ] **Step 5: Rewire both ops files**

In `ops_search.rs` replace lines 33–52 (the comment + both `match` blocks) and in `ops_history.rs` lines 82–101 with:

```rust
        // Convert cursor bounds once, outside the timeout closures, so `?` maps
        // through the existing error path (A8).
        let (before_offset, after_bound) = cursor_wire_bounds(params.before_id, params.after_id)?;
```

- [ ] **Step 6: Full gate** — `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "refactor: share cursor_wire_bounds across search and history ops (audit S3.2)"
```

---

### Task 7: `PageAccumulator` next to `PostCounter`

The ~20-line album-admit/limit/`has_more` block is triplicated (`ops_search.rs` channel loop + global loop, `ops_history.rs` loop), and each caller separately re-applies `collapse_albums`.

**Files:**
- Modify: `src/telegram/albums.rs` (struct + tests in the existing inline tests module)
- Modify: `src/telegram/client/ops_search.rs`, `src/telegram/client/ops_history.rs`

**Interfaces:**
- Produces in `crate::telegram::albums`:
  - `pub(crate) struct PageAccumulator`
  - `pub(crate) fn new(collapse_albums: bool, limit: usize) -> Self`
  - `pub(crate) fn push(&mut self, message: Message) -> bool` — `false` latches `has_more`; caller breaks
  - `pub(crate) fn has_more(&self) -> bool`
  - `pub(crate) fn into_messages(self) -> Vec<Message>` — applies `collapse_albums` when enabled
- Task 8 consumes `PageAccumulator` in its method signatures.

- [ ] **Step 1: Write the failing tests**

In `src/telegram/albums.rs` tests module (reuses the existing `album_member` fixture):

```rust
#[test]
fn accumulator_admits_album_siblings_beyond_limit_and_latches_has_more() {
    let mut page = PageAccumulator::new(true, 1);
    assert!(page.push(album_member(1, 7, "caption")));
    assert!(page.push(album_member(2, 7, "")), "sibling passes beyond limit");
    assert!(!page.push(create_test_message(3, "next post", 100)));
    assert!(page.has_more());
    assert_eq!(page.into_messages().len(), 1, "album collapsed to one post");
}

#[test]
fn accumulator_without_collapse_refuses_at_limit() {
    let mut page = PageAccumulator::new(false, 2);
    assert!(page.push(create_test_message(1, "a", 100)));
    assert!(page.push(create_test_message(2, "b", 100)));
    assert!(!page.push(create_test_message(3, "c", 100)));
    assert!(page.has_more());
    assert_eq!(page.into_messages().len(), 2);
}

#[test]
fn accumulator_reports_no_has_more_when_nothing_refused() {
    let mut page = PageAccumulator::new(true, 5);
    assert!(page.push(create_test_message(1, "a", 100)));
    assert!(!page.has_more());
}
```

- [ ] **Step 2: Run tests to verify they fail** — `cargo test accumulator_ 2>&1 | tail -5` → compile error.

- [ ] **Step 3: Implement**

In `src/telegram/albums.rs`, below `PostCounter`:

```rust
/// Accumulates one result page inside a fetch loop, owning the post-level
/// limit decision the three fetch loops used to hand-inline (audit S3.2):
/// with `collapse_albums`, a message that would START a post beyond `limit`
/// is refused (trailing siblings of admitted albums pass — A2); without it,
/// the limit-th overflow message is refused instead of pushed blind, proving
/// a qualifying message exists beyond the page (A8). A refusal latches
/// `has_more`.
#[derive(Debug)]
pub(crate) struct PageAccumulator {
    messages: Vec<Message>,
    counter: PostCounter,
    collapse: bool,
    limit: usize,
    has_more: bool,
}

impl PageAccumulator {
    pub(crate) fn new(collapse_albums: bool, limit: usize) -> Self {
        Self {
            messages: Vec::new(),
            counter: PostCounter::default(),
            collapse: collapse_albums,
            limit,
            has_more: false,
        }
    }

    /// Admit `message` into the page. Returns `false` when the page is full —
    /// the caller stops fetching.
    pub(crate) fn push(&mut self, message: Message) -> bool {
        let admitted = if self.collapse {
            self.counter.admit(album_key(&message), self.limit)
        } else {
            self.messages.len() < self.limit
        };
        if !admitted {
            self.has_more = true;
            return false;
        }
        self.messages.push(message);
        true
    }

    pub(crate) fn has_more(&self) -> bool {
        self.has_more
    }

    /// Finish the page: collapse album siblings when enabled.
    pub(crate) fn into_messages(self) -> Vec<Message> {
        if self.collapse {
            collapse_albums(self.messages)
        } else {
            self.messages
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass** — `cargo test accumulator_` → 3 PASS.

- [ ] **Step 5: Rewire the three fetch loops**

`src/telegram/client/ops_history.rs`:
- Import: `use crate::telegram::albums::PageAccumulator;` (drop `PostCounter, album_key, collapse_albums` from the import — no longer used here).
- Inside the timeout closure: replace `let mut messages = Vec::new(); let mut has_more = false; let mut counter = PostCounter::default();` with `let mut page = PageAccumulator::new(params.collapse_albums, params.limit as usize);` (keep the budget line).
- Replace the whole `if let Some(converted) = convert_raw_message(...) { if params.collapse_albums { ... } else { ... } }` block with:

```rust
                    if let Some(converted) = convert_raw_message(&raw_msg, &peer, &entities)
                        && !page.push(converted)
                    {
                        break;
                    }
```

- Closure returns `Ok((page, budget))`; the destructuring becomes `let (page, budget) = with_timeout(...).await?;` followed by:

```rust
        let has_more = page.has_more();
        let messages = page.into_messages();
```

- Delete the old `let messages = if params.collapse_albums { collapse_albums(messages) } else { messages };` block.

`src/telegram/client/ops_search.rs` — same transformation twice:
- Import change identical to ops_history.
- Channel path: closure builds `let mut page = PageAccumulator::new(params.collapse_albums, params.limit as usize);` (replacing `messages`/`has_more`/`counter`), the admit block becomes `if let Some(converted) = convert_raw_message(&raw_msg, peer, &entities) && !page.push(converted) { break; }`, and the closure returns `Ok((page, channels_scanned, budget))`. The `.map(...)` after `with_timeout` becomes `.map(|(page, channels_scanned, budget)| (page, Some(channels_scanned), budget))?`.
- Global path: same replacement; the converted-message block becomes:

```rust
                        if let Some(peer) = chat_peer.as_ref()
                            && let Some(converted) = convert_raw_message(&raw_msg, peer, &entities)
                            && !page.push(converted)
                        {
                            break;
                        }
```

  and the closure returns `Ok((page, budget))`; the outer arm becomes `(page, None, budget)`.
- After the branch, replace the collapse block with:

```rust
        let has_more = page.has_more();
        let mut messages = page.into_messages();

        // Sort by timestamp (newest first)
        messages.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
```

  (the binding `(page, channels_scanned, budget)` replaces the old 4-tuple; `has_more` now comes from the accumulator).
- The global-path page-fetch debug log reads `kept = messages.len()` from the closure-local vec, which no longer exists. Add an accessor to `PageAccumulator` (include it in Step 3):

```rust
    /// Messages admitted so far (pre-collapse) — used by progress logging.
    pub(crate) fn len(&self) -> usize {
        self.messages.len()
    }
```

  and change the log field to `kept = page.len()`.

- [ ] **Step 6: Full gate** — `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`. Album/limit behavior is pinned by `albums.rs` tests and `mcp/tests/search_core.rs`.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "refactor: PageAccumulator owns the triplicated admit/limit/has_more block (audit S3.2)"
```

---

### Task 8: split `ops_search::search_messages_impl` into channel vs global paths

After Tasks 6–7 the impl is still one ~250-line function with two `with_timeout` bodies inline. Move each body into its own private method; the impl keeps validation, dispatch, and the shared epilogue. Move-only — the two method bodies are the existing `with_timeout(...)` blocks relocated verbatim.

**Files:**
- Modify: `src/telegram/client/ops_search.rs`

**Interfaces:**
- Consumes: `PageAccumulator` (Task 7), `cursor_wire_bounds` (Task 6), `SearchBudget`, `RawChannelSearchPager`, `RawGlobalSearchPager` (existing)
- Produces (private methods on `TelegramClient`):
  - `async fn search_in_channel(&self, params: &SearchParams, channel_id: ChannelId, cutoff_time: DateTime<Utc>, before_offset: Option<i32>, after_bound: Option<i32>) -> Result<(PageAccumulator, u32, SearchBudget), Error>`
  - `async fn search_global(&self, params: &SearchParams, cutoff_time: DateTime<Utc>) -> Result<(PageAccumulator, SearchBudget), Error>`

- [ ] **Step 1: Baseline** — `cargo test --lib search` — record pass count.

- [ ] **Step 2: Extract `search_in_channel`**

New method containing the channel-path `with_timeout("search_messages_channel", ...)` block exactly as it stands after Task 7 (dialog walk, raw pager, budget, accumulator). The method starts at the `with_timeout` call and returns its result directly:

```rust
    /// Channel-scoped search: walk dialogs to the target channel, then page
    /// the raw messages.Search pager under the search timeout. Returns the
    /// accumulated page, the channels-scanned count, and the budget counters.
    async fn search_in_channel(
        &self,
        params: &SearchParams,
        channel_id: ChannelId,
        cutoff_time: DateTime<Utc>,
        before_offset: Option<i32>,
        after_bound: Option<i32>,
    ) -> Result<(PageAccumulator, u32, SearchBudget), Error> {
        with_timeout(
            "search_messages_channel",
            self.timeouts.search_secs,
            async {
                // ...existing closure body moved verbatim (uses channel_id
                // directly instead of dereferencing the Option)...
            },
        )
        .await
    }
```

Add `use chrono::{DateTime, Utc};` (or rely on what `use super::*` already provides — compiler will say).

- [ ] **Step 3: Extract `search_global`**

Same move for the global `with_timeout("search_all_messages", ...)` block including its `debug_span!` and `mtproto_nanos` bookkeeping. The one deliberate delta: the block's final debug log used the outer `start_time`; give the method its own `let start_time = Instant::now();` at entry (debug-level diagnostic; noted in Global Constraints):

```rust
    /// Global search via the raw messages.SearchGlobal pager with server-side
    /// window bounds. Cursor rejection happens in the dispatcher — this path
    /// has no per-channel offset to ride.
    async fn search_global(
        &self,
        params: &SearchParams,
        cutoff_time: DateTime<Utc>,
    ) -> Result<(PageAccumulator, SearchBudget), Error> {
        let start_time = Instant::now();
        let span = tracing::debug_span!(
            "search_global",
            query = %params.query,
            media_filter = ?params.media_filter,
            window_from = %cutoff_time,
        );
        with_timeout(
            "search_all_messages",
            self.timeouts.search_secs,
            async move {
                // ...existing closure body moved verbatim...
            }
            .instrument(span),
        )
        .await
    }
```

- [ ] **Step 4: Rewrite the dispatcher**

`search_messages_impl` becomes validation + dispatch + shared epilogue:

```rust
        let (page, channels_scanned, budget) = if let Some(channel_id) = params.channel_id {
            let (page, scanned, budget) = self
                .search_in_channel(params, channel_id, cutoff_time, before_offset, after_bound)
                .await?;
            (page, Some(scanned), budget)
        } else {
            // Cursors are single-channel only (decision 2): global search has no
            // per-channel offset_id to ride, and no way to bound it client-side
            // without scanning every channel's history.
            if params.before_id.is_some() || params.after_id.is_some() {
                return Err(Error::InvalidInput(
                    "before_id/after_id require channel_id: cursor pagination is per-channel"
                        .to_string(),
                ));
            }
            let (page, budget) = self.search_global(params, cutoff_time).await?;
            (page, None, budget)
        };
```

Epilogue (accumulator finish, sort, `channels_in_results` count, `tracing::info!`, `SearchResult` construction) stays exactly as after Task 7.

- [ ] **Step 5: Full gate** — `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`; pass count matches Step 1.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "refactor: split search_messages_impl into channel and global paths (audit S3.3)"
```

---

### Task 9: promote `validate_channel_identifier` to a client-wide guard

The empty-reference guard is hand-inlined 7 times (`resolve.rs:91`, `ops_message.rs:15,68`, `ops_media.rs:22,65`, `ops_transcribe.rs:19`, `ops_stats.rs:16`) with wording drift from the shared fn in `channels.rs:239`. Verified: no test pins "Channel reference cannot be empty" (0 test hits), so unifying to "Channel identifier cannot be empty" is safe; `channels_tests.rs` pins the surviving wording.

**Files:**
- Modify: `src/telegram/client.rs` (fn moves here), `src/telegram/client/channels.rs` (fn removed; call sites unchanged — they resolve via `use super::*`)
- Modify: `src/telegram/client/{resolve.rs, ops_message.rs, ops_media.rs, ops_transcribe.rs, ops_stats.rs}`
- Test: `src/telegram/client/tests/helpers_tests.rs`

**Interfaces:**
- Produces: `fn validate_channel_identifier(identifier: &str) -> Result<(), Error>` — private in `client.rs`, reachable by every submodule via `use super::*`. Same signature as today; only its home moves.

- [ ] **Step 1: Write the failing tests**

In `src/telegram/client/tests/helpers_tests.rs`:

```rust
#[test]
fn empty_channel_identifier_is_rejected() {
    let err = validate_channel_identifier("").expect_err("empty must be rejected");
    assert!(
        err.to_string().contains("Channel identifier cannot be empty"),
        "got: {err}"
    );
}

#[test]
fn non_empty_channel_identifier_passes() {
    assert!(validate_channel_identifier("durov").is_ok());
}
```

- [ ] **Step 2: Run tests to verify they fail** — `cargo test channel_identifier 2>&1 | tail -5` → compile error (`validate_channel_identifier` is private to `channels.rs`, not visible from `client.rs`'s test child).

- [ ] **Step 3: Move the fn**

Cut `validate_channel_identifier` (with its doc comment) from `channels.rs:235–246` and paste into `client.rs` next to `cursor_wire_bounds`, updating the doc comment's second paragraph to:

```rust
/// Reject an empty channel identifier before it reaches peer resolution.
///
/// Shared client-wide so every entry point reports the same error for the
/// same caller mistake.
```

`channels.rs`'s two call sites (`:53`, `:75`) keep working unchanged — the name now resolves through the parent module.

- [ ] **Step 4: Run tests to verify they pass** — `cargo test channel_identifier` → 2 PASS (plus `channels_tests.rs` stays green).

- [ ] **Step 5: Replace the 7 inline guards**

In each of `resolve.rs:91`, `ops_message.rs:15`, `ops_message.rs:68`, `ops_media.rs:22`, `ops_media.rs:65`, `ops_transcribe.rs:19`, `ops_stats.rs:16`, replace:

```rust
        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }
```

with:

```rust
        validate_channel_identifier(channel_ref)?;
```

Then verify no old wording survives: `grep -rn "Channel reference cannot be empty" src` → 0 hits.

- [ ] **Step 6: Full gate** — `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "refactor: promote validate_channel_identifier to client-wide guard (audit S3.5)"
```

---

### Task 10: replace the `use super::*` prelude in the nine `impl_*.rs` submodules

`server.rs:1–42` doubles as an implicit prelude for nine submodules via `use super::*;`. Each file should import its own deps so reading one file tells you what it uses, and pruning server.rs imports stops breaking siblings. Mechanical, compiler-driven, zero behavior change.

**Files:**
- Modify: `src/mcp/server/impl_channels.rs`, `impl_discovery.rs`, `impl_links.rs`, `impl_media.rs`, `impl_message_batch.rs`, `impl_resolve.rs`, `impl_search.rs`, `impl_stats.rs`, `impl_status.rs`
- Modify: `src/mcp/server.rs` (prune imports it no longer uses itself)

**Interfaces:**
- Consumes: everything already public/crate-visible; no new items. Cross-submodule `pub(super)` constants (e.g. `impl_media::MAX_MEDIA_BATCH_IDS`) import as `use super::impl_media::MAX_MEDIA_BATCH_IDS;`.

- [ ] **Step 1: Convert one file completely (worked example: `impl_search.rs`)**

Replace `use super::*;` with (post-Task-1/2 state; adjust to what the compiler confirms):

```rust
use super::McpServer;
use crate::link::{ChannelRef, parse_telegram_link};
use crate::mcp::tools::helpers::{parse_cursor_bounds, wire_message_id};
use crate::mcp::tools::{
    GetMessageByLinkRequest, GetRecentMessagesRequest, MessageResponse, ResponseFormat,
    SearchRequest, SearchResponse, fanout, json_response, parse_channel_id, parse_optional_utc,
    shaping, validate_date_window,
};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::{ChannelId, HistoryParams, SearchParams};
```

Run `cargo check 2>&1 | head -40` and add exactly the imports rustc names (using the same source paths `server.rs` used for that item), until the file compiles. Private-field access (`self.rate_limiter`, `self.response_byte_budget`) needs no import — the submodule is inside the defining module tree.

- [ ] **Step 2: Repeat for the other eight files**

Same procedure per file: delete `use super::*;`, add `use super::McpServer;`, run `cargo check`, add the named imports. `impl_media.rs` keeps its existing three explicit `use crate::mcp::tools::...` lines. Do them one at a time so each error batch belongs to one file.

- [ ] **Step 3: Prune `server.rs`**

Run: `cargo clippy -- -D warnings` — remove every import in `server.rs:1–42` now flagged unused (only items server.rs itself references survive, e.g. the rmcp items, `ToolRouter`, config defaults, `Arc`, `Duration`/`Instant`).

- [ ] **Step 4: Full gate** — `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test` (all ~705 tests; this task must not change any behavior).

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "refactor: give each server submodule explicit imports instead of the super::* prelude (audit S3.6)"
```

---

### Task 11: rate limiter — poison recovery + `available_tokens` dedup (hygiene batch)

`rate_limiter.rs` holds the codebase's only production `unwrap()`s (4 × `lock().unwrap()` at :81, :114, :124, :129) and a verbatim-duplicated `available_tokens` body (:80–85 vs :128–132).

**Files:**
- Modify: `src/rate_limiter.rs`
- Test: `src/rate_limiter/tests.rs` (existing `test_config(max_tokens, refill_rate)` fixture)

**Interfaces:**
- Produces (private): `fn bucket(&self) -> std::sync::MutexGuard<'_, TokenBucket>` on `RateLimiter`
- The trait impl's `available_tokens` delegates to the inherent method — one body total.

- [ ] **Step 1: Write the failing test**

In `src/rate_limiter/tests.rs`:

```rust
#[test]
fn a_poisoned_bucket_lock_recovers_instead_of_panicking() {
    let limiter = Arc::new(RateLimiter::new(&test_config(5, 1.0)));
    let poisoner = Arc::clone(&limiter);
    let _ = std::thread::spawn(move || {
        #[allow(clippy::unwrap_used)]
        let _guard = poisoner.bucket.lock().unwrap();
        panic!("poison the bucket lock");
    })
    .join();
    assert_eq!(limiter.available_tokens(), 5.0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test poisoned_bucket_lock` — expected: FAIL — the test thread's panic poisons the mutex and `available_tokens` panics on `lock().unwrap()`.

- [ ] **Step 3: Implement**

In `impl RateLimiter`:

```rust
    /// Lock the bucket, recovering from a poisoned lock: the bucket state is
    /// a pair of plain numbers that is valid after any interleaving, so a
    /// panic in another holder cannot leave it inconsistent.
    fn bucket(&self) -> std::sync::MutexGuard<'_, TokenBucket> {
        self.bucket
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
```

Rewrite the inherent `available_tokens` to use `self.bucket()`; rewrite the trait impl as:

```rust
    async fn acquire(&self, tokens: u32) -> Result<(), Error> {
        let mut bucket = self.bucket();
        bucket
            .try_acquire(tokens)
            .map_err(|(available, retry_after_seconds)| Error::RateLimit {
                retry_after_seconds,
                detail: format!(": requested {tokens} tokens, {available:.2} available"),
            })
    }

    fn refund(&self, tokens: u32) {
        self.bucket().refund(tokens);
    }

    fn available_tokens(&self) -> f64 {
        RateLimiter::available_tokens(self)
    }
```

- [ ] **Step 4: Run tests to verify they pass** — `cargo test --lib rate_limiter` → all pass including the new poison test.

- [ ] **Step 5: Full gate + commit**

```bash
cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A && git commit -m "fix: recover from poisoned rate-limiter lock; dedupe available_tokens (audit hygiene)"
```

---

### Task 12: docs, final gate, review, PR

**Files:**
- Modify: `CLAUDE.md` (coercion-helper list), `docs/memory.md` (current state + open items)

- [ ] **Step 1: Update docs**

- `CLAUDE.md`, "Flexible scalar coercion" paragraph: replace the helper list with `flexible_opt_int`, `flexible_i64`, `flexible_string`, `flexible_opt_string`, `flexible_opt_bool`, `flexible_opt_enum`.
- `docs/memory.md`: in **Current state**, extend the audit line to "Stages 1–3 shipped"; in **Open items**, change "Audit stages 3–4 … not started" to Stage 4 only. Add nothing else unless a durable lesson emerged during execution.
- Note (no file change now): the remaining hygiene backlog (config panics, logging init, `process::exit`, base64 underflow, trait docs, tokio features, `cli.rs`, doc numbering, `redact_phone` threshold) stays in the spec for Stage 4 batching; the `impl_message_batch`/`impl_media` 6-line precheck duplication is deliberately skipped ("barely worth it" per spec).

- [ ] **Step 2: Final full gate**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: clean, ~705+ tests (baseline 705 + ~16 new).

- [ ] **Step 3: Commit docs**

```bash
git add -A && git commit -m "docs: record audit stage 3 completion; update coercion helper names"
```

- [ ] **Step 4: Review + PR**

Use superpowers:requesting-code-review, then superpowers:finishing-a-development-branch. PR: `refactor/audit-stage3-dedup` → `master`, title "refactor: audit stage 3 — duplication/KISS refactors", body summarizing the six spec items + hygiene batch and naming the two accepted behavior deltas (guard wording unification; global-search debug `duration_ms` scope). After merge: delete this plan file (delete-on-merge) and mark Stage 3 ✅ in the spec.

---

## Self-Review (done at plan time)

- **Spec coverage:** S3.1 → Tasks 1–2; S3.2 → Tasks 6–7; S3.3 → Task 8; S3.4 → Task 4; S3.5 → Task 9; S3.6 (three "smaller" items) → Tasks 3, 5, 10; hygiene rate-limiter items → Task 11. Remaining hygiene items explicitly deferred to Stage 4 (Task 12 note).
- **Verified against source at plan time:** all cited line numbers, the 7 guard sites (no test pins the old wording), `MessageId::new` returning `Result`, `MediaFetchOutcome` field names, `test_config` fixture shape, serde attribute counts, `result_with`/`album_member`/`create_test_message` fixture availability.
- **Type consistency:** `PageAccumulator` produced in Task 7 is consumed by Task 8's signatures; `cursor_wire_bounds` (Task 6) feeds Task 8's dispatcher; Task 10's worked example assumes Tasks 1–2 landed (imports `parse_cursor_bounds`, `fanout`, no `StreamExt`/`Arc`).
