# Audit Stage 2: Module Splits & Test Extraction — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring every oversized file from the 2026-08-15 audit under control — one real production split (`raw_pager.rs`), eight test-file extractions/splits, one test prune, and the fixture-duplication cleanup — with zero behavior change.

**Architecture:** No production logic changes. `raw_pager.rs` splits into three sibling modules under `client/` (envelope interpretation / by-id fetch / pagers). All other work moves `#[cfg(test)]` modules into the repo's `#[path]`-included test-file layout, consolidates duplicated fixtures into `src/test_helpers.rs`, and replaces ~60 copy-paste mock-limiter setups and ~42 all-`None` request literals with shared helpers / `..Default::default()`.

**Tech Stack:** Rust nightly (edition 2024 — `env::set_var`/`remove_var` are `unsafe`), tokio, mockall 0.14 (dev-dependency; mocks exist only under `cfg(test)`), rmcp v3.1.

**Spec:** `docs/superpowers/specs/2026-08-15-project-audit.md` (Stage 2 section + the "Also:" fixture note; the `EnvGuard` row in the Stage 2 table)

## Global Constraints

- Pre-merge gate: `cargo fmt --check && cargo clippy -- -D warnings && cargo test` — all green before the PR.
- Branch: `refactor/audit-stage2-splits` off `master`. Conventional commits (`refactor:` for production splits, `test:` for test moves/fixtures, `docs:` for docs).
- Run `cargo fmt --all` after every code change (not just `--check`).
- **Zero behavior change.** Production-code edits are limited to: visibility keywords (`pub(super)`/`pub`), module wiring (`mod` declarations, `#[path]` includes, `use` paths), one added `Default` derive (Task 12), and the comment-text updates named in Tasks 3–4. Nothing else in `src/` production code may change.
- **Test-count invariant.** Baseline: 723 tests pass (tasklist Phase 38). Expected count after each task is stated in its verify step. Only two tasks change the count: Task 7 (−19, mock-test prune → 704) and Task 9 (+1, `EnvGuard` panic test → 705). Final: **705**. Capture the actual baseline in Task 1; if it differs from 723, keep the deltas and shift all absolute numbers.
- Verify count with: `cargo test 2>&1 | grep "test result:"` (compare passed/ignored totals across lines).
- Unused-import arbitration: when a task says "copy the use block and prune", the authority is `cargo clippy -- -D warnings` — copy the stated block, build, delete exactly the imports the compiler flags, add exactly the ones it names as missing.
- Test moves are verbatim: cut the cited test fns (each `#[test]`/`#[tokio::test]` attribute line travels with its fn, including any `#[ignore = "..."]` line above it), paste unmodified, let `cargo fmt --all` fix indentation. Line numbers cited below are from commit `7c6cfc7` and drift as tasks land — **cut by item/test name**, use line numbers only for orientation.
- Explicitly out of scope (audit's own KISS calls): `mcp/server.rs` `ToolInvocation` extraction ("already fine"); `telegram/client/channels.rs` production discovery-vs-subscription split (test extraction only); `mcp/tests/channels.rs` split ("2% over — leave"; Task 10 shrinks it below 500 incidentally).

### The repo's test-include idioms (used throughout; replicate exactly)

`#[path]` on a non-inline `mod` resolves **relative to the directory of the declaring source file** (repo-verified: `src/mcp/server.rs` `#[path = "tests.rs"]` → `src/mcp/tests.rs`; `src/telegram/converters.rs` `#[path = "tests/converters_tests.rs"]` → `src/telegram/tests/converters_tests.rs`).

Form A — leaf module including its own tests (attribute order matters):

```rust
#[cfg(test)]
#[path = "tests/<name>_tests.rs"]
mod tests;
```

Form B — an already-`cfg(test)`-gated aggregator listing test files (no per-mod `#[cfg(test)]`), entries in alphabetical order:

```rust
#[path = "tests/<name>.rs"]
mod <name>;
```

---

### Task 1: Branch + baseline + move raw-TL test helpers to `test_helpers`

**Files:**
- Modify: `src/test_helpers.rs` (append two helpers after `raw_tl_user`, ~line 393)
- Modify: `src/telegram/client/raw_pager.rs` (test module only: delete local helpers `raw_msg` :576-595 and `slice` :597-612, update call sites)

**Interfaces:**
- Consumes: existing `test_helpers::raw_tl_channel`.
- Produces: `pub fn raw_tl_message(...)` and `pub fn raw_tl_messages_slice(...)` in `crate::test_helpers` — verbatim bodies of `raw_msg`/`slice`, used by Tasks 3–4's extracted test files.

- [ ] **Step 1: Create the branch and record the baseline**

```bash
git checkout -b refactor/audit-stage2-splits master
cargo test 2>&1 | grep "test result:"
```

Expected: all green, 723 passed total (plus 5 ignored). Record the actual numbers — they are the invariant for every later task.

- [ ] **Step 2: Move the two helpers**

Cut `fn raw_msg(...)` (raw_pager.rs:576-595) and `fn slice(...)` (:597-612) out of raw_pager's `mod tests`. Paste both at the end of `src/test_helpers.rs` (before its `#[cfg(test)] mod tests` at :395), verbatim except:
- rename `raw_msg` → `raw_tl_message`, `slice` → `raw_tl_messages_slice` (matches the file's `raw_tl_*` family);
- change `fn` → `pub fn` on both;
- add a `///` doc line each, e.g. `/// Raw-TL message for pager/envelope tests.` and `/// Raw-TL messages.ChannelMessages slice wrapping the given messages.`;
- `raw_tl_messages_slice` keeps its internal `raw_tl_channel(...)` call (now same-module);
- keep signatures and bodies byte-identical otherwise. If the bodies referenced imports from raw_pager's test module (`tl`, `DateTime`), `test_helpers.rs` already imports `grammers_client::tl` and `chrono::DateTime` (:6-11) — clippy arbitrates any remainder.

- [ ] **Step 3: Rewire raw_pager's tests**

In raw_pager's `mod tests`: add `use crate::test_helpers::{raw_tl_message, raw_tl_messages_slice};` to the test-module use block (:570-574) and rename every `raw_msg(` → `raw_tl_message(` and `slice(` → `raw_tl_messages_slice(` call (call sites in tests at :614-633, 635-644, 646-662, 664-679, 681-724, 726-758, 884-896, 898-912, 926-962).

- [ ] **Step 4: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
```

Expected: green, count unchanged (723).

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "test: move raw-TL message/slice fixtures to test_helpers"
```

---

### Task 2: Extract `raw_page.rs` (envelope interpretation) from `raw_pager.rs`

**Files:**
- Create: `src/telegram/client/raw_page.rs`
- Modify: `src/telegram/client/raw_pager.rs` (remove moved items, import them back)
- Modify: `src/telegram/client.rs:33-46` (add `mod raw_page;`)

**Interfaces:**
- Consumes: `crate::telegram::envelope::EntityLookup` (unchanged).
- Produces (all `pub(super)`, i.e. visible throughout `telegram::client`): `struct RawPage` (all fields `pub(super)` too), `fn raw_peer_id`, `fn unpack_page`, `fn input_peer_for_message`, `fn fill_buffer`, `fn chat_peer_for_message`. `fn channel_access_hash` stays private to `raw_page` (its only caller, `input_peer_for_message`, moves with it).

- [ ] **Step 1: Create `src/telegram/client/raw_page.rs`**

Header:

```rust
//! Raw-TL response-envelope interpretation shared by the pagers and the
//! by-id fetch: page unpacking, peer/id extraction, and buffer fill.
```

Then copy raw_pager.rs's top-level use block (:10-18) and move these items verbatim (cut from raw_pager.rs), in this order: `RawPage` (:23-30), `raw_peer_id` (:41-48), `unpack_page` (:50-77, its fn-local `use tl::enums::messages::Messages;` travels inside it), `channel_access_hash` (:149-163), `input_peer_for_message` (:165-204), `fill_buffer` (:206-215), `chat_peer_for_message` (:520-566, its fn-local `use grammers_client::peer::{Peer, User};` travels inside it).

Visibility edits in the new file: `struct RawPage` → `pub(super) struct RawPage` and every field gets `pub(super)`; `raw_peer_id`, `unpack_page`, `input_peer_for_message`, `fill_buffer`, `chat_peer_for_message` each get `pub(super) fn`. `channel_access_hash` stays `fn`. Prune the copied use block via clippy.

- [ ] **Step 2: Rewire `raw_pager.rs`**

Add at the top of raw_pager.rs's use block:

```rust
use super::raw_page::{RawPage, chat_peer_for_message, fill_buffer, input_peer_for_message, unpack_page};
```

(raw_pager's remaining code — `advance_history_offsets`, `advance_search_offsets`, the three pagers — consumes exactly these five; `raw_peer_id` is no longer used here. Prune now-unused imports, e.g. `PeerId`/`PeerKind` if flagged.) The inline test module still compiles unchanged: its `use super::*` picks these names up through raw_pager's imports, and `RawPage { .. }` literals in tests work because the fields are `pub(super)`.

- [ ] **Step 3: Declare the module**

In `src/telegram/client.rs`, insert into the alphabetical mod list (:33-46), before `mod raw_pager;`:

```rust
mod raw_page;
```

- [ ] **Step 4: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
```

Expected: green, 723. `wc -l src/telegram/client/raw_page.rs` ≈ 190; raw_pager.rs ≈ 820 (tests still inline).

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "refactor: extract raw-TL envelope interpretation into client/raw_page"
```

---

### Task 3: Extract `raw_fetch.rs` (by-id fetch) + its tests

**Files:**
- Create: `src/telegram/client/raw_fetch.rs`
- Create: `src/telegram/client/tests/raw_fetch_tests.rs` (new `tests/` directory)
- Modify: `src/telegram/client/raw_pager.rs` (remove moved items + their 5 tests; make `PAGE_LIMIT` `pub(super)`)
- Modify: `src/telegram/client.rs` (add `mod raw_fetch;`)
- Modify: `src/telegram/client/ops_message.rs:6` (import path)
- Modify: `src/telegram/trait_def.rs:57,64` and `src/telegram/client/guard.rs:44` (comment text only)

**Interfaces:**
- Consumes: `super::raw_page::{RawPage, raw_peer_id, unpack_page}`, `super::raw_pager::PAGE_LIMIT`.
- Produces: `pub(super) enum GetMessagesRequest`, `pub(super) async fn fetch_messages_by_id` (same signatures as today — callers in `ops_message.rs` change only the module path). `get_messages_request` and `index_messages` stay private to `raw_fetch`.

- [ ] **Step 1: Create `src/telegram/client/raw_fetch.rs`**

Header:

```rust
//! Raw-TL by-id message fetch (`getMessages`) preserving the response
//! envelope, so converters can enrich from the same call's chats/users.
```

Use block (prune via clippy):

```rust
use super::raw_page::{RawPage, raw_peer_id, unpack_page};
use super::raw_pager::PAGE_LIMIT;
use crate::telegram::envelope::EntityLookup;
use grammers_client::Client;
use grammers_client::tl;
use grammers_mtsender::InvocationError;
use grammers_session::types::{PeerId, PeerKind, PeerRef};
use std::collections::HashMap;
use std::sync::Arc;
```

Move verbatim from raw_pager.rs: `GetMessagesRequest` (:217-224), `get_messages_request` (:226-239), `index_messages` (:241-254), `fetch_messages_by_id` (:256-277). No visibility changes (`GetMessagesRequest` and `fetch_messages_by_id` are already `pub(super)`).

In raw_pager.rs, change `const PAGE_LIMIT: i32 = 100;` to `pub(super) const PAGE_LIMIT: i32 = 100;` (raw_fetch and the extracted test files reach it cross-module).

- [ ] **Step 2: Move the 5 raw_fetch tests out of raw_pager's test module**

Create `src/telegram/client/tests/raw_fetch_tests.rs`:

```rust
//! Tests for the raw-TL by-id fetch (`raw_fetch`).

use super::*;
use crate::test_helpers::raw_tl_message;
use grammers_session::types::{PeerAuth, PeerId, PeerKind, PeerRef};
```

(prune via clippy), then move verbatim from raw_pager's test module: helpers `channel_ref` (:850-855) and `chat_ref` (:857-862), and tests `channel_peer_routes_to_channels_get_messages` (:864-872), `non_channel_peer_routes_to_messages_get_messages` (:874-882), `index_messages_keys_by_id_regardless_of_response_order` (:884-896), `index_messages_drops_a_message_from_a_different_peer` (:898-912), `index_messages_keeps_empty_placeholders_for_the_caller_to_classify` (:914-924).

At the bottom of `raw_fetch.rs` (Form A):

```rust
#[cfg(test)]
#[path = "tests/raw_fetch_tests.rs"]
mod tests;
```

- [ ] **Step 3: Wire and repoint**

- `src/telegram/client.rs`: insert `mod raw_fetch;` in the alphabetical list (after `mod ops_transcribe;`, before `mod raw_page;`).
- `src/telegram/client/ops_message.rs:6`: `use super::raw_pager::fetch_messages_by_id;` → `use super::raw_fetch::fetch_messages_by_id;`.
- Comment text (no code): in `src/telegram/trait_def.rs:57` and `:64` and `src/telegram/client/guard.rs:44`, replace `raw_pager::fetch_messages_by_id` with `raw_fetch::fetch_messages_by_id`.

- [ ] **Step 4: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
```

Expected: green, 723 (5 tests moved, none lost — check `cargo test raw_fetch 2>&1 | grep "test result:"` shows 5 passed). `wc -l src/telegram/client/raw_fetch.rs` ≈ 85.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "refactor: extract raw-TL by-id fetch into client/raw_fetch"
```

---

### Task 4: Move the remaining raw_pager/raw_page tests to `client/tests/`

**Files:**
- Create: `src/telegram/client/tests/raw_page_tests.rs`
- Create: `src/telegram/client/tests/raw_pager_tests.rs`
- Modify: `src/telegram/client/raw_pager.rs` (delete the inline test module, add Form A include)
- Modify: `src/telegram/client/raw_page.rs` (add Form A include)
- Modify: `src/mcp/tests/parity.rs:6` (comment text only)

**Interfaces:**
- Consumes: `test_helpers::{raw_tl_message, raw_tl_messages_slice, raw_tl_channel, raw_tl_community}`; `raw_pager::PAGE_LIMIT` (`pub(super)` since Task 3).
- Produces: nothing new — pure relocation.

- [ ] **Step 1: Create `src/telegram/client/tests/raw_page_tests.rs`**

```rust
//! Tests for raw-TL envelope interpretation (`raw_page`).

use super::*;
use crate::telegram::client::raw_pager::PAGE_LIMIT;
use crate::test_helpers::{raw_tl_channel, raw_tl_community, raw_tl_message, raw_tl_messages_slice};
```

(prune via clippy; `EntityLookup` arrives via `use super::*` since raw_page.rs imports it), then move verbatim these 6 tests from raw_pager's inline test module: `unpack_slice_computes_last_chunk_and_keeps_envelope` (:614-633), `unpack_messages_variant_is_always_last_chunk` (:635-644), `global_offset_peer_resolves_access_hash_from_envelope` (:664-679), `global_offset_peer_resolves_community_and_forbidden_chats` (:681-724), `fetch_decode_builds_an_entity_map_from_the_response_envelope` (:926-962), `unpack_page_treats_not_modified_as_an_empty_final_page` (:964-975).

At the bottom of `raw_page.rs`:

```rust
#[cfg(test)]
#[path = "tests/raw_page_tests.rs"]
mod tests;
```

- [ ] **Step 2: Create `src/telegram/client/tests/raw_pager_tests.rs`**

```rust
//! Tests for the raw-TL pagers: offset advancement and search-window math.

use super::*;
use crate::test_helpers::{raw_tl_message, raw_tl_messages_slice};
use chrono::DateTime;
```

(prune via clippy), then move verbatim the remaining 8 tests: `history_offsets_advance_from_last_message` (:646-662), `search_offsets_advance_from_last_message` (:726-758), `window_bounds_maps_both_ends_widened_by_a_second` (:760-769), `window_bounds_open_upper_end_is_unbounded_sentinel` (:771-779), `window_bounds_clamps_pre_epoch_lower_end_to_unbounded` (:781-789), `window_bounds_clamps_beyond_i32_range` (:791-798), `window_bounds_saturates_the_upper_end_at_i32_max` (:800-813), `apply_window_lands_both_bounds_on_the_tl_request` (:815-848).

Delete the now-empty inline `#[cfg(test)] mod tests { ... }` from raw_pager.rs entirely and replace it with:

```rust
#[cfg(test)]
#[path = "tests/raw_pager_tests.rs"]
mod tests;
```

- [ ] **Step 3: Fix the stale cross-reference comment**

`src/mcp/tests/parity.rs:6`: the doc comment says envelope decode is covered by `raw_pager`'s test — change the module name to `raw_page` (comment text only).

- [ ] **Step 4: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
wc -l src/telegram/client/raw_pager.rs src/telegram/client/raw_page.rs src/telegram/client/raw_fetch.rs
```

Expected: green, 723; raw_pager.rs ≈ 330 lines, raw_page.rs ≈ 195, raw_fetch.rs ≈ 90 — all under 500. `cargo test client::raw 2>&1 | grep "test result:"` should account for all 19 relocated tests.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "test: move raw_pager/raw_page tests to client/tests/"
```

---

### Task 5: Extract `converters/message.rs` tests

**Files:**
- Create: `src/telegram/tests/message_tests.rs`
- Modify: `src/telegram/converters/message.rs` (delete inline test module :321-843, add include)

- [ ] **Step 1: Move the module contents**

Cut the entire inline module (`#[cfg(test)]` at :321 through the closing `}` at :843). Create `src/telegram/tests/message_tests.rs` containing:

```rust
//! Tests for raw-TL → domain message conversion (forward enrichment,
//! links, reactions, timestamps).
```

followed by the module's **contents** (the use block at :323-330 and everything after it), with the `mod tests {` wrapper and its closing brace removed — one indent level out; `cargo fmt --all` normalizes.

Replace the deleted module in `message.rs` with (note the `../` — message.rs lives in `src/telegram/converters/`, and the file goes to the shared `src/telegram/tests/` directory like its sibling converters tests):

```rust
#[cfg(test)]
#[path = "../tests/message_tests.rs"]
mod tests;
```

`use super::*` semantics are unchanged: the module is still a child of `converters::message`, so its tests keep access to the file's private items.

- [ ] **Step 2: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
wc -l src/telegram/converters/message.rs src/telegram/tests/message_tests.rs
```

Expected: green, 723; message.rs ≈ 325 lines. message_tests.rs ≈ 525 — knowingly a hair over 500; the audit accepted this (production cohesion mattered, the test module was the overshoot).

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "test: extract converters/message tests to telegram/tests/message_tests"
```

---

### Task 6: Extract `client/channels.rs` tests

**Files:**
- Create: `src/telegram/client/tests/channels_tests.rs`
- Modify: `src/telegram/client/channels.rs` (delete inline test module :310-518, add include)

- [ ] **Step 1: Move the module contents**

Cut the inline module (`#[cfg(test)]` at :310 through :518 — the use block :312-315, helpers `inert_client`/`raw_channel`/`raw_small_group`, and all 9 tests). Create `src/telegram/client/tests/channels_tests.rs` with:

```rust
//! Tests for channel discovery/subscription helpers (`channels`).
```

followed by the module contents unwrapped (same procedure as Task 5). Replace in `channels.rs` with:

```rust
#[cfg(test)]
#[path = "tests/channels_tests.rs"]
mod tests;
```

- [ ] **Step 2: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
wc -l src/telegram/client/channels.rs
```

Expected: green, 723; channels.rs ≈ 315 lines.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "test: extract client/channels tests to client/tests/"
```

---

### Task 7: Prune `client_tests.rs` mock-only tests

The audit found these tests assert on the mockall mock itself — no production code is under test. Spec said 21; the verified count is **19** (plus the two local fixture fns and the file-level use block that only they consume).

**Files:**
- Modify: `src/telegram/tests/client_tests.rs` (710 → ~45 lines)

- [ ] **Step 1: Delete**

Delete lines 3-667: the use block (:3-8), `create_test_channel` (:10-24), `create_test_message` (:26-54), both banner comments, and all 19 `mock_*` tests (`mock_is_connected_returns_true` :60 through `mock_download_message_media_returns_media_download` :640-667).

Keep: the `username_to_resolve` comment preamble (:669-672) and `mod username_to_resolve_tests` (:673-710) — it is self-contained (own `use crate::telegram::client::username_to_resolve;`).

Update the file doc comment (line 1) to:

```rust
//! Tests for `TelegramClient` helper fns (`username_to_resolve`).
```

- [ ] **Step 2: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
```

Expected: green, **704 passed** (−19). `cargo test username_to_resolve 2>&1 | grep "test result:"` shows 6 passed.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "test: prune mock-only client tests (asserted on the mock, not production code)"
```

---### Task 8: Split `telegram/tests/converters_tests.rs` three ways

**Files:**
- Create: `src/telegram/tests/converters_thumb_forward_tests.rs`
- Create: `src/telegram/tests/converters_av_tests.rs`
- Create: `src/telegram/tests/converters_doc_poll_tests.rs`
- Delete: `src/telegram/tests/converters_tests.rs`
- Modify: `src/telegram/converters.rs:22-24` (one include → three)

**Interfaces:**
- Produces: `pub(super) fn video_doc` / `pub(super) fn audio_doc` inside `converters_av_tests.rs` (`pub(super)` = the `converters` module), consumed by two tests in the doc/poll file.

- [ ] **Step 1: Create the three files**

Each starts with a `//!` doc line and a copy of the original file-level use block (:1-7), pruned via clippy:

```rust
use super::media::matches_media_filter_raw;
use super::message::{extract_forward_info, extract_link_preview};
use super::*;
use crate::telegram::envelope::EntityLookup;
use crate::telegram::types::{AudioKind, MediaFilter, SizeCandidate, VideoKind};
use grammers_client::media::{Document, Media, Poll};
use grammers_client::tl;
```

Distribute verbatim by name (helpers listed with the tests that own them):

**`converters_thumb_forward_tests.rs`** — `//! Converter tests: thumbnail size selection, raw media filter, forward extraction, link previews.` Helpers `candidate` (:9-16), `raw_message_with_media` (:74-131), `fwd_header` (:154-170), `webpage_media` (:172-206); tests `selects_smallest_candidate_that_satisfies_max_dimension`, `falls_back_to_largest_when_none_satisfies`, `empty_candidates_returns_none`, `longest_side_is_what_counts`, `tie_on_longest_side_picks_first_candidate`, `single_candidate_below_threshold_is_returned_via_fallback`, `raw_filter_matches_photo_media`, `raw_filter_url_matches_text_without_media`, `forward_from_channel_without_envelope_extracts_ids_only`, `forward_from_hidden_user_has_name_only`, `forward_with_zero_date_has_no_original_date`, `link_preview_extracted_and_description_truncated_to_500_chars`, `link_preview_empty_webpage_returns_none` (13 tests, lines 9-271).

**`converters_av_tests.rs`** — `//! Converter tests: audio and video metadata extraction.` Helpers `video_doc` (:273-322), `gif_doc` (:324-353), `audio_doc` (:355-394) — mark `video_doc` and `audio_doc` as `pub(super) fn` (the doc/poll file imports them); `gif_doc` stays private. Tests `extract_video_info_regular_video`, `extract_video_info_round_message_is_video_note`, `extract_video_info_gif_is_animation_with_zero_dims`, `extract_video_info_without_thumbs_is_false`, `extract_video_info_none_for_audio`, `extract_audio_info_voice`, `extract_audio_info_music`, `extract_audio_info_none_for_video`, `audio_info_carries_title_and_performer`, `audio_info_without_id3_metadata_omits_title_and_performer`, `audio_info_omits_absent_title_from_json` (11 tests, lines 396-501).

**`converters_doc_poll_tests.rs`** — `//! Converter tests: document and poll metadata extraction.` Add `use super::av_tests::{audio_doc, video_doc};` to its use block (two tests build av media to assert `extract_document_info` returns `None`). Helpers `plain_doc` (:503-536), `poll_media` (:582-659); tests `document_info_reads_filename_size_and_mime`, `document_info_without_filename_attribute_omits_the_name`, `document_info_is_none_for_video_media`, `document_info_is_none_for_audio_media`, `document_info_absent_from_json_when_media_is_not_a_document`, `poll_info_reads_question_options_and_per_option_voters`, `poll_info_without_results_keeps_options_and_omits_voters`, `poll_info_matches_voters_to_options_by_key_not_position`, `poll_info_flags_a_quiz_and_multiple_choice`, `poll_info_is_none_for_non_poll_media`, `poll_option_omits_absent_voters_from_json`, `poll_info_omits_voters_for_an_option_whose_count_is_undisclosed` (12 tests, lines 503-847).

- [ ] **Step 2: Rewire and delete**

In `src/telegram/converters.rs`, replace the include at :22-24 with:

```rust
#[cfg(test)]
#[path = "tests/converters_av_tests.rs"]
mod av_tests;
#[cfg(test)]
#[path = "tests/converters_doc_poll_tests.rs"]
mod doc_poll_tests;
#[cfg(test)]
#[path = "tests/converters_thumb_forward_tests.rs"]
mod thumb_forward_tests;
```

Delete `src/telegram/tests/converters_tests.rs` (`git rm`).

- [ ] **Step 3: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
```

Expected: green, 704 (13+11+12 = 36 tests relocated, none lost). All three new files < 400 lines.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "test: split converters tests into thumb-forward/av/doc-poll files"
```

---

### Task 9: `EnvGuard` + four-way split of `config/tests.rs`

Two commits: the drop-guard first (it fixes the stage-1 review finding that `remove_var` cleanup runs only on the success path), then the mechanical split.

**Files:**
- Modify: `src/config/tests.rs` (add `EnvGuard`; then shrink to an aggregator)
- Create: `src/config/tests/env_tests.rs`, `src/config/tests/load_tests.rs`, `src/config/tests/validation_tests.rs`, `src/config/tests/defaults_tests.rs`
- `src/config.rs:434-436` (the `#[path = "config/tests.rs"]` include) is **unchanged**.

**Interfaces:**
- Consumes: existing `ENV_LOCK`/`env_lock()` (:6-15), `create_test_config` (:130-170).
- Produces: `struct EnvGuard` with `fn new() -> Self`, `fn set(&mut self, key: &'static str, value: impl AsRef<std::ffi::OsStr>)`, `fn remove(&mut self, key: &'static str)` — private to the test aggregator, visible to the four leaf modules as `super::EnvGuard`.

- [ ] **Step 1: Write the failing panic-restore test**

Append to `src/config/tests.rs`:

```rust
#[test]
fn env_guard_restores_env_on_panic() {
    let result = std::panic::catch_unwind(|| {
        let mut env_guard = EnvGuard::new();
        env_guard.set("ENV_GUARD_PANIC_PROBE", "leaked?");
        panic!("assertion-failure stand-in");
    });
    assert!(result.is_err());
    let _env_guard = EnvGuard::new(); // re-serialize before probing
    assert!(env::var_os("ENV_GUARD_PANIC_PROBE").is_none());
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test env_guard_restores_env_on_panic`
Expected: FAIL to compile — `EnvGuard` not found.

- [ ] **Step 3: Implement `EnvGuard`**

Add below `env_lock()` (after :15), plus `use std::ffi::OsString;` in the use block:

```rust
/// RAII guard for tests that mutate process environment variables.
///
/// Construction takes `ENV_LOCK`, serializing all env-mutating tests.
/// `set`/`remove` record a variable's prior value the first time they touch
/// it, and `Drop` restores every touched variable — so a failing assertion
/// cannot leak env state into subsequent tests (before this guard, cleanup
/// ran only on the success path).
struct EnvGuard {
    saved: Vec<(&'static str, Option<OsString>)>,
    _lock: MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn new() -> Self {
        Self { saved: Vec::new(), _lock: env_lock() }
    }

    fn set(&mut self, key: &'static str, value: impl AsRef<std::ffi::OsStr>) {
        self.save(key);
        unsafe { env::set_var(key, value) };
    }

    fn remove(&mut self, key: &'static str) {
        self.save(key);
        unsafe { env::remove_var(key) };
    }

    fn save(&mut self, key: &'static str) {
        if !self.saved.iter().any(|(k, _)| *k == key) {
            self.saved.push((key, env::var_os(key)));
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, old) in self.saved.iter().rev() {
            match old {
                Some(value) => unsafe { env::set_var(key, value) },
                None => unsafe { env::remove_var(key) },
            }
        }
    }
}
```

Note `EnvGuard::new()` takes `ENV_LOCK` — never construct two guards in one scope (self-deadlock). One guard handles any number of variables.

- [ ] **Step 4: Convert the 11 env-mutating tests**

In each, replace `let _env = env_lock();` with `let mut env_guard = EnvGuard::new();`, replace every `unsafe { env::set_var(K, V); }` with `env_guard.set(K, V);`, delete every trailing `unsafe { env::remove_var(K); }` cleanup block, and in `test_resolve_path_default` replace the up-front `unsafe { env::remove_var(...) }` with `env_guard.remove("TELEGRAM_MCP_CONFIG");`. The 11 tests (all current `env_lock()` call sites): `test_expand_env_vars_single_variable` (:24), `test_expand_env_vars_multiple_variables` (:37), `test_expand_env_vars_no_recursive_expansion` (:63), `test_expand_env_vars_numeric_unquoting` (:88), `test_load_valid_config` (:226), `test_load_config_with_env_vars` (:272), `test_load_missing_config` (:334), `test_load_invalid_toml` (:348), `test_resolve_path_from_env` (:368), `test_resolve_path_default` (:383), `test_load_config_with_file_logging_options` (:591). Worked example — before (:22-33):

```rust
#[test]
fn test_expand_env_vars_single_variable() {
    let _env = env_lock();
    unsafe {
        env::set_var("TEST_VAR", "test_value");
    }
    let result = expand_env_vars("prefix_${TEST_VAR}_suffix").unwrap();
    assert_eq!(result, "prefix_test_value_suffix");
    unsafe {
        env::remove_var("TEST_VAR");
    }
}
```

after:

```rust
#[test]
fn test_expand_env_vars_single_variable() {
    let mut env_guard = EnvGuard::new();
    env_guard.set("TEST_VAR", "test_value");
    let result = expand_env_vars("prefix_${TEST_VAR}_suffix").unwrap();
    assert_eq!(result, "prefix_test_value_suffix");
}
```

File-writing tests keep their `fs::write`/`fs::remove_file` lines as-is (temp-file cleanup on panic is out of scope; the files live in `env::temp_dir()`).

- [ ] **Step 5: Verify and commit the guard**

Run: `cargo test config 2>&1 | grep "test result:"` then the full gate.
Expected: green, **705 passed** (+1 for the panic test).

```bash
git add -A && git commit -m "test: EnvGuard drop-guard restores env vars on panic in config tests"
```

- [ ] **Step 6: Split into aggregator + four leaves**

Shrink `src/config/tests.rs` to: the doc comment, use block (`use super::*; use std::env; use std::ffi::OsString; use std::sync::{Mutex, MutexGuard, PoisonError};` — drop `use std::fs;`, it moves to load_tests), `ENV_LOCK` + `env_lock()`, `EnvGuard`, `create_test_config`, and four Form B declarations:

```rust
#[path = "tests/defaults_tests.rs"]
mod defaults_tests;
#[path = "tests/env_tests.rs"]
mod env_tests;
#[path = "tests/load_tests.rs"]
mod load_tests;
#[path = "tests/validation_tests.rs"]
mod validation_tests;
```

Each leaf begins with a `//!` doc line plus:

```rust
use crate::config::*;
```

then only what it needs from: `use super::EnvGuard;`, `use super::create_test_config;`, `use std::env;`, `use std::fs;` (clippy arbitrates). Distribute all 67 tests verbatim by name (5 `#[ignore = "for CI/CD passing tests"]` attributes travel with their tests):

**`env_tests.rs`** (12) — `//! Env-var expansion and config-path resolution.`: `test_expand_env_vars_no_variables`, `test_expand_env_vars_single_variable`, `test_expand_env_vars_multiple_variables`, `test_expand_env_vars_missing_variable_returns_error`, `test_expand_env_vars_no_recursive_expansion`, `test_expand_env_vars_incomplete_syntax`, `test_expand_env_vars_numeric_unquoting`, `test_expand_env_vars_skips_toml_comment_lines`, `test_expand_env_vars_skips_inline_comment_after_hash`, `test_resolve_path_from_env`, `test_resolve_path_default`, `env_guard_restores_env_on_panic`.

**`load_tests.rs`** (5) — `//! File-based Config::load tests — every env-mutating loader in one auditable place.`: `test_load_valid_config`, `test_load_config_with_env_vars`, `test_load_missing_config`, `test_load_invalid_toml`, `test_load_config_with_file_logging_options`.

**`validation_tests.rs`** (22) — `//! Validation rules, credential predicates, and secret redaction.`: `test_has_auth_credentials_all_present`, `test_has_auth_credentials_missing_api_hash`, `test_has_auth_credentials_missing_phone`, `test_has_auth_credentials_empty_api_hash`, `test_validate_for_setup_missing_auth_credentials`, `test_validate_for_setup_valid_credentials`, `test_auth_credentials_getter`, `test_secret_does_not_expose_in_debug`, `test_secret_expose_returns_actual_value`, `test_timeout_config_validate_rejects_zero_resolve`, `test_timeout_config_validate_rejects_zero_history`, `test_timeout_config_validate_rejects_zero_search`, `test_timeout_config_validate_accepts_defaults`, `test_download_secs_zero_fails_validation`, `limits_config_rejects_zero_budget`, `search_config_rejects_zero_deadline`, `search_config_rejects_deadline_over_one_hour`, `zero_media_batch_payload_cap_is_rejected`, `below_floor_media_batch_payload_cap_is_rejected`, `a_media_cost_above_the_bucket_capacity_is_rejected`, `a_transcription_cost_above_the_bucket_capacity_is_rejected`, `costs_equal_to_capacity_are_accepted`.

**`defaults_tests.rs`** (28) — `//! Defaults and in-memory TOML table parsing (no env, no files).`: `test_logging_config_defaults_file_enabled`, `test_logging_config_defaults_max_log_days`, `test_logging_config_defaults_log_path`, `test_default_logging_config_has_file_fields`, `test_default_timeout_config_values`, `test_telegram_config_default_timeouts_when_section_absent`, `test_telegram_config_default_max_download_bytes`, `test_transcription_config_defaults_when_section_absent`, `test_observability_defaults_when_table_absent`, `test_observability_partial_table_fills_defaults`, `test_media_download_cost_default`, `test_default_rate_limit_has_transcription_cost`, `test_download_secs_default`, `test_max_buffered_payload_bytes_default`, `limits_config_defaults_when_absent`, `search_deadline_defaults_to_twenty_seconds`, `retuned_media_rate_limit_defaults`, `media_batch_payload_cap_defaults_to_eight_mib`, `test_telegram_config_timeout_partial_override`, `test_telegram_config_timeout_full_override`, `test_telegram_config_max_download_bytes_override`, `test_transcription_config_override`, `test_observability_table_parsed`, `test_media_download_cost_from_toml`, `test_download_secs_from_toml`, `test_max_buffered_payload_bytes_from_toml`, `limits_config_parses_response_byte_budget`, `search_config_accepts_explicit_deadline`.

Keep the section banner comments with their sections where they still make sense; drop any left stranded.

- [ ] **Step 7: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
cargo test config:: 2>&1 | grep "test result:"
```

Expected: green, 705 total; 62 passed + 5 ignored across the config leaves; `wc -l` — aggregator ≈ 110, every leaf < 400.

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "test: split config tests into env/load/validation/defaults files"
```

---

### Task 10: Fixture consolidation in `test_helpers` (MCP tests)

**Files:**
- Modify: `src/test_helpers.rs` (two new helpers)
- Modify: `src/mcp/tests/history.rs` (delete local `create_test_message` :17-44)
- Modify: `src/mcp/tests/channels.rs` (delete local `create_test_channel` :14-28)
- Modify: `src/mcp/tests/discovery.rs` (delete local `create_test_channel` :14-28)
- Modify: `src/mcp/tests/parity.rs` (delete local `permissive_limiter` :35-39)
- Modify: `src/mcp/tests/media_batch.rs` (delete local `permissive_limiter` :67-73)

**Interfaces:**
- Produces in `crate::test_helpers`:
  - `pub fn create_test_channel_named(id: i64, name: &str, is_subscribed: bool) -> Channel` — consolidates the two byte-identical-but-for-`is_subscribed` locals (existing `create_test_channel(id, username)` has different semantics — name derived from username, `last_message_date: Some(now)` — so call sites cannot simply switch to it).
  - `#[cfg(test)] pub fn permissive_limiter() -> crate::rate_limiter::MockRateLimiterTrait` — **must** be `#[cfg(test)]`-gated: `test_helpers` is an ungated `pub mod` and `MockRateLimiterTrait` only exists under `cfg(test)` (`#[cfg_attr(test, mockall::automock)]`, mockall is a dev-dependency).

- [ ] **Step 1: Add the two helpers to `src/test_helpers.rs`**

After `create_test_channel_detailed` (:206):

```rust
/// Create a test channel with an explicit display name and subscription
/// state (fixed username "testchannel").
pub fn create_test_channel_named(id: i64, name: &str, is_subscribed: bool) -> Channel {
    Channel {
        id: ChannelId::new(id).expect("Test channel ID must be positive"),
        name: ChannelName::new(name).expect("Valid channel name"),
        username: Some(Username::new("testchannel").expect("Valid username")),
        chat_type: ChatType::Channel,
        description: Some("Test channel".to_string()),
        member_count: Some(1000),
        is_verified: false,
        is_public: true,
        is_subscribed,
        last_message_date: None,
    }
}
```

Near the end of the file (before its `#[cfg(test)] mod tests`):

```rust
/// A rate limiter that accepts every acquire and swallows refunds — for
/// tests where limiting is not the subject.
#[cfg(test)]
pub fn permissive_limiter() -> crate::rate_limiter::MockRateLimiterTrait {
    let mut limiter = crate::rate_limiter::MockRateLimiterTrait::new();
    limiter.expect_acquire().returning(|_| Ok(()));
    limiter.expect_refund().return_const(());
    limiter
}
```

- [ ] **Step 2: Adopt in the five files**

- `history.rs`: delete local `create_test_message` (:17-44); change :11 to `use crate::test_helpers::{create_test_message, create_test_search_result};`; prune newly unused names from the :7-10 type import via clippy.
- `channels.rs`: delete local `create_test_channel` (:14-28) and its doc comment; add `use crate::test_helpers::create_test_channel_named;`; rewrite every `create_test_channel(ID, NAME)` call → `create_test_channel_named(ID, NAME, true)` (includes the one inside the local `n_channels` helper, which stays).
- `discovery.rs`: same, with `false` as the third argument; delete the "(mirrors channels.rs's local helper)" doc comment along with the helper.
- `parity.rs`: delete local `permissive_limiter` (:35-39); add `use crate::test_helpers::permissive_limiter;` (the shared version also stubs `expect_refund` — an expectation without `.times()` imposes no call requirement, so parity's behavior is unchanged).
- `media_batch.rs`: delete local `permissive_limiter` (:67-73); add `use crate::test_helpers::permissive_limiter;` (bodies are identical).

- [ ] **Step 3: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
wc -l src/mcp/tests/channels.rs
```

Expected: green, 705; `mcp/tests/channels.rs` now < 500 (the audit's "2% over — leave" row resolves itself).

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "test: consolidate duplicated channel/message/limiter fixtures into test_helpers"
```

---

### Task 11: Permissive-limiter sweep across `src/mcp/tests/`

Replace every inline permissive limiter with the shared helper. **Apply only where the limiter gets no other expectation and is never asserted on afterwards** — skip any site with `.times(...)`, `.withf(...)`, `.with(...)`, `.return_once(...)`, an `Err` return, or later `limiter.` method calls in the same test.

**Files (candidate-site counts from the audit mapping — re-verify each with `grep -n "expect_acquire" <file>`):**
- Modify: `src/mcp/tests/search.rs` (16), `history.rs` (14), `links.rs` (8), `media.rs` (8), `message_by_link.rs` (4), `discovery.rs` (3), `batch.rs` (2), `channels.rs` (2), `multi_channel.rs` (1), `resolve.rs` (1), `stats.rs` (1)

- [ ] **Step 1: Rewrite each qualifying site**

Before:

```rust
let mut limiter = MockRateLimiterTrait::new();
limiter.expect_acquire().returning(|_| Ok(()));
```

After (add `use crate::test_helpers::permissive_limiter;` to the file's use block once):

```rust
let limiter = permissive_limiter();
```

If the inline setup also has a permissive `limiter.expect_refund().return_const(());` line, delete that too (the shared helper covers it). Where the file's `MockRateLimiterTrait` import becomes unused (every limiter in the file is now shared), remove the import — clippy flags it.

- [ ] **Step 2: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
grep -rn "returning(|_| Ok(()))" src/mcp/tests/ | wc -l
```

Expected: green, 705; the grep count drops to ~0 (bespoke non-permissive limiters may legitimately remain).

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "test: adopt shared permissive_limiter across mcp tests"
```

---

### Task 12: All-`None` request-literal cleanup (search + history)

**Files:**
- Modify: `src/mcp/tools/types/requests.rs:175` (add `Default` to one derive)
- Modify: `src/mcp/tests/search.rs` (23 `SearchRequest` literals; delete local `search_request` helper)
- Modify: `src/mcp/tests/history.rs` (20 `GetRecentMessagesRequest` literals)

- [ ] **Step 1: Derive `Default` for `GetRecentMessagesRequest`**

`requests.rs:175`: `#[derive(Debug, Clone, Deserialize, JsonSchema)]` → `#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]`. All 12 fields are `Option`, so the derived default is the all-`None` literal these tests spell out. (`SearchRequest` at :95 already derives `Default` — one search test relies on it today.) No serde/schema impact: `Default` participates in neither `Deserialize` nor `JsonSchema` derives; the schema-integrity tests confirm.

- [ ] **Step 2: Rewrite the literals**

In both files, every request literal keeps only its non-`None` fields plus `..Default::default()`:

```rust
let request = SearchRequest {
    query: "test".to_string(),
    ..Default::default()
};
```

```rust
let request = GetRecentMessagesRequest {
    channel_id: Some("123".to_string()),
    ..Default::default()
};
```

Exception: in `history.rs`'s `get_recent_messages_missing_channel_id_fails`, keep `channel_id: None` explicit — the absent channel is the point of the test:

```rust
let request = GetRecentMessagesRequest {
    channel_id: None,
    ..Default::default()
};
```

Delete `search.rs`'s local `search_request` helper (:1138-1155) and inline the two-field literal at its 2 call sites (:1178, :1208).

- [ ] **Step 3: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
```

Expected: green, 705. `search.rs` shrinks to roughly 990 lines, `history.rs` to roughly 760.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "test: collapse all-None request literals via Default"
```

---

### Task 13: Split `mcp/tests/search.rs` three ways

**Files:**
- Create: `src/mcp/tests/search_core.rs`, `src/mcp/tests/search_dates.rs`, `src/mcp/tests/search_shaping.rs`
- Delete: `src/mcp/tests/search.rs`
- Modify: `src/mcp/tests.rs` (replace the `search` entry with three, alphabetical)

- [ ] **Step 1: Create the three files**

Each starts with a `//!` doc line and a copy of search.rs's use block (:3-15 as amended by Tasks 11-12), pruned via clippy. Distribute the 24 tests verbatim by name:

**`search_core.rs`** (8) — `//! search_messages: core behavior (queries, filters, limits, rate limiting).`: `search_messages_returns_results`, `search_messages_empty_query_fails`, `search_messages_rate_limited`, `search_messages_with_channel_filter`, `search_messages_applies_limits`, `search_allows_empty_query_with_media_filter`, `search_passes_media_filter_to_params`, `search_accepts_username_channel_id`.

**`search_dates.rs`** (8) — `//! search_messages: from_date/to_date validation and windowing.`: `search_passes_date_range_to_client`, `search_rejects_invalid_from_date`, `search_rejects_inverted_range`, `search_accepts_equal_from_and_to_date`, `search_rejects_to_date_older_than_hours_back_window`, `search_accepts_to_date_inside_hours_back_window`, `search_rejects_blank_from_date`, `search_accepts_padded_from_date`.

**`search_shaping.rs`** (8) — `//! search_messages: response shaping — serialization, cursors, compact format, degradation flags.`: `search_messages_serializes_enrichment_fields`, `search_messages_serializes_enriched_forward_without_resolve_calls`, `search_response_reports_window_and_returned`, `search_messages_rejects_cursors_without_channel`, `search_messages_rejects_compact_without_channel`, `search_messages_shapes_response_end_to_end_for_single_channel`, `timed_out_search_returns_partial_results_not_an_error`, `healthy_search_omits_the_degradation_flags`.

- [ ] **Step 2: Rewire**

In `src/mcp/tests.rs`, replace:

```rust
#[path = "tests/search.rs"]
mod search;
```

with (keeping the list alphabetical):

```rust
#[path = "tests/search_core.rs"]
mod search_core;
#[path = "tests/search_dates.rs"]
mod search_dates;
#[path = "tests/search_shaping.rs"]
mod search_shaping;
```

Delete `src/mcp/tests/search.rs` (`git rm`).

- [ ] **Step 3: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
cargo test search 2>&1 | grep "test result:"
```

Expected: green, 705; all three files < 450 lines.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "test: split search tests into core/dates/shaping files"
```

---

### Task 14: Split `mcp/tests/history.rs` three ways

**Files:**
- Create: `src/mcp/tests/history_core.rs`, `src/mcp/tests/history_dates.rs`, `src/mcp/tests/history_paging.rs`
- Delete: `src/mcp/tests/history.rs`
- Modify: `src/mcp/tests.rs` (replace the `history` entry with three, alphabetical)

- [ ] **Step 1: Create the three files**

Same procedure as Task 13 (shared use block from history.rs :3-15 as amended by Tasks 10-12, pruned per file). Distribute the 20 tests verbatim by name:

**`history_core.rs`** (8) — `//! get_recent_messages: core behavior (identifiers, filters, limits, album collapsing).`: `get_recent_messages_returns_results`, `get_recent_messages_missing_channel_id_fails`, `get_recent_messages_with_media_filter`, `get_recent_messages_applies_limits`, `get_recent_messages_with_username_passes_identifier_without_pre_resolving`, `get_recent_messages_rate_limited`, `collapse_albums_flag_reaches_params`, `collapse_albums_defaults_to_true`.

**`history_dates.rs`** (6) — `//! get_recent_messages: from_date/to_date validation and windowing.`: `get_recent_messages_passes_date_range_to_client`, `get_recent_messages_accepts_equal_from_and_to_date`, `get_recent_messages_rejects_inverted_range`, `get_recent_messages_rejects_to_date_older_than_hours_back_window`, `get_recent_messages_accepts_to_date_inside_hours_back_window`, `get_recent_messages_rejects_blank_to_date`.

**`history_paging.rs`** (6) — `//! get_recent_messages: cursors, byte budget, and response shaping.`: `get_recent_messages_emits_next_cursor_when_limit_truncates`, `get_recent_messages_passes_cursor_params_to_client`, `get_recent_messages_rejects_inverted_cursor_range`, `get_recent_messages_truncates_long_text`, `get_recent_messages_compact_hoists_channel_header`, `get_recent_messages_oversized_page_stays_under_budget`. (The two shaping tests ride here rather than a fourth file — the budget test already straddles both concerns.)

- [ ] **Step 2: Rewire**

In `src/mcp/tests.rs`, replace the `history` pair with `history_core` / `history_dates` / `history_paging` entries (alphabetical — they sort between `discovery` and `last_responses`). `git rm src/mcp/tests/history.rs`.

- [ ] **Step 3: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
```

Expected: green, 705; all three files < 350 lines.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "test: split history tests into core/dates/paging files"
```

---

### Task 15: Split `mcp/tests/media_batch.rs` into core/budget + fixtures

**Files:**
- Create: `src/mcp/tests/media_batch_fixtures.rs`, `src/mcp/tests/media_batch_core.rs`, `src/mcp/tests/media_batch_budget.rs`
- Delete: `src/mcp/tests/media_batch.rs`
- Modify: `src/mcp/tests.rs` (replace the `media_batch` entry with three, alphabetical)

**Interfaces:**
- Produces: `media_batch_fixtures` with `pub(super)` fns `photo_download`, `ok_outcome`, `err_outcome`, `no_media`, `not_found`, `request`, `summary_of` (`pub(super)` = the `mcp::tests` aggregator module — visible to both sibling test files).

- [ ] **Step 1: Create `media_batch_fixtures.rs`**

`//! Shared fixtures for the media-batch test files.` — move the seven local helpers verbatim from media_batch.rs (:15-80, minus `permissive_limiter`, gone in Task 10), each `fn` → `pub(super) fn`. Use block: copy media_batch.rs's (:3-13), prune via clippy (it needs `MediaDownload`, `MediaFetchError`, `MediaFetchOutcome`, `MediaType`, `GetMessagesMediaBatchRequest`, `MediaBatchSummary`, `ContentBlock`, `create_test_jpeg`).

- [ ] **Step 2: Create the two test files**

Both use blocks: copy media_batch.rs's (:3-13) plus `use super::media_batch_fixtures::{...};` naming what each file calls; prune via clippy. Distribute the 18 tests verbatim by name:

**`media_batch_core.rs`** (8) — `//! get_messages_media_batch: batch mechanics, ordering, validation.`: `mixed_batch_returns_images_and_reports_failures`, `metadata_blocks_are_adjacent_to_their_images_in_request_order`, `channel_level_failure_fails_the_call`, `empty_message_ids_is_rejected_without_a_network_call`, `more_than_ten_ids_is_rejected`, `duplicate_ids_are_deduped_preserving_first_seen_order`, `max_dimension_is_clamped_to_the_supported_range`, `batch_of_one_matches_the_single_tool_metadata` (its fn-scoped `use crate::mcp::tools::GetMessageMediaRequest;` travels inside it).

**`media_batch_budget.rs`** (10) — `//! get_messages_media_batch: payload cap and rate-limit charging/refunds.`: `channel_level_failure_refunds_all_but_one_token`, `payload_cap_downscales_then_reports_cap_reached`, `cap_reached_ids_are_reported_in_request_order`, `payload_cap_drops_refund_their_cost`, `a_generous_cap_returns_every_image`, `charges_for_every_requested_id_then_refunds_the_failures`, `a_fully_successful_batch_refunds_nothing`, `a_rejected_acquire_performs_no_download`, `rate_limit_errors_carry_a_retry_hint` (the file's only sync `#[test]`), `an_enormous_media_cost_refunds_without_overflowing`. Move media_batch.rs's mid-file `use mockall::predicate::eq;` (:506) into this file's top use block.

- [ ] **Step 3: Rewire**

In `src/mcp/tests.rs`, replace the `media_batch` pair with `media_batch_budget` / `media_batch_core` / `media_batch_fixtures` entries (alphabetical). `git rm src/mcp/tests/media_batch.rs`.

- [ ] **Step 4: Verify**

```bash
cargo fmt --all && cargo clippy -- -D warnings && cargo test
cargo test media_batch 2>&1 | grep "test result:"
```

Expected: green, 705; core ≈ 270 lines, budget ≈ 320, fixtures ≈ 80.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "test: split media_batch tests into core/budget with shared fixtures"
```

---

### Task 16: Docs, tracking, final gate

**Files:**
- Modify: `.claude/rules/ast-index.md` (the ">500 lines" list under "Mandatory read rules")
- Modify: `CLAUDE.md` (Test Organization bullet)
- Modify: `docs/tasklist.md` (Phase 39 row + progress counter)
- Modify: `docs/memory.md` (journal entry)

- [ ] **Step 1: Refresh the >500-line file list**

```bash
find src -name '*.rs' | xargs wc -l | sort -rn | awk '$1 > 500'
```

Expected survivors: `src/mcp/server.rs` (~649, macro-bound — stays by design) and `src/telegram/tests/message_tests.rs` (~525, accepted overshoot). Update `.claude/rules/ast-index.md`'s "Mandatory read rules" item 1 to name exactly the files the sweep reports (dropping `raw_pager.rs`, `config/tests.rs`, and the split `mcp/tests/` files).

- [ ] **Step 2: Update CLAUDE.md's Test Organization bullet**

Replace:

```markdown
- Env-mutating config tests self-serialize via `ENV_LOCK` (`src/config/tests.rs`); plain `cargo test` is safe
```

with:

```markdown
- Env-mutating config tests self-serialize through the `EnvGuard` drop-guard (`ENV_LOCK` in `src/config/tests.rs`), which also restores variables on panic; plain `cargo test` is safe
```

- [ ] **Step 3: Record Phase 39 in `docs/tasklist.md` and add a `docs/memory.md` entry**

Tasklist: append a Phase 39 row — audit stage 2: `raw_pager` split into `raw_page`/`raw_fetch`/pagers; test extraction for `converters/message`, `client/channels`, converters/search/history/media_batch/config test splits; 19 mock-only client tests pruned; `EnvGuard`; fixture consolidation (`create_test_channel_named`, `permissive_limiter`, `raw_tl_message`/`raw_tl_messages_slice`, `..Default::default()` literals). Tests 723 → 705 (−19 pruned, +1 EnvGuard). Update the progress counter to 39/39. Memory.md: brief entry noting the `#[path]` resolution rule (relative to the declaring file's directory) and the `../tests/` idiom introduced for `converters/message.rs`, plus the EnvGuard no-nesting constraint.

- [ ] **Step 4: Final gate and push**

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A && git commit -m "docs: record audit stage 2 (module splits & test extraction)"
git push -u origin refactor/audit-stage2-splits
```

Expected: gate green, 705 passed. Then follow superpowers:finishing-a-development-branch (PR + review per repo workflow).

---

## Deviations from the audit table (deliberate, with reasons)

- **`client_tests.rs` mock tests: 19, not 21** — re-verified count at head; the prune list in Task 7 is exhaustive.
- **`mcp/server.rs` `ToolInvocation` extraction: skipped** — the audit marked the file "already fine"; the extraction was optional and buys ~50 lines.
- **search enrichment-serialization tests live in `search_shaping.rs`** — they assert response JSON, and moving them keeps `search_core.rs` under 500.
- **history's shaping tests live in `history_paging.rs`** — the audit named three files; the budget test already straddles paging and shaping.
- **config gets exactly the four named leaves** — in-memory TOML-override tests go to `defaults_tests.rs` (rule: "no env, no files"), keeping `load_tests.rs` as the audit intended: every env-mutating loader in one auditable place.
