# media_type Naming Unification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every MCP tool report a round video as `media_type: "video_note"`, eliminating the `"videonote"` vs `"video_note"` desync between `search_messages` and `transcribe_voice_message`.

**Architecture:** Switch the `MediaType` enum's serde rename rule from `lowercase` to `snake_case` (flips only the single multi-word variant, `VideoNote`), making serde the single source of truth for the wire name. Then retire the hand-rolled string `match` in `transcribe_voice_message` by typing its response field as `MediaType` so it inherits the same serialization.

**Tech Stack:** Rust (nightly, 2024 edition), `serde` / `serde_json`, `schemars` v1, `rmcp` v1.7, `mockall` for trait mocks.

## Global Constraints

- Line length: 100 chars.
- Run `cargo fmt --all` after every code change (layout of match arms etc. is normalized by fmt).
- TDD: write/adjust the failing test before the production change.
- Never `unwrap()` in production code (`?` / `.context(...)`); `expect()`/`unwrap()` allowed in tests.
- Pre-merge gate (all must pass): `cargo fmt --check && cargo clippy -- -D warnings && cargo test`.
- Spec: `docs/superpowers/specs/2026-06-20-media-type-naming-design.md`.

## File Structure

- `src/telegram/types/media.rs` — `MediaType` enum (Task 1). The single source of truth for media wire names.
- `src/mcp/tools/types/responses.rs` — `TranscribeVoiceMessageResponse` DTO (Task 2). `media_type` field retyped `String` → `MediaType`.
- `src/mcp/server.rs` — `transcribe_voice_message_impl` handler (Task 2). Drops the manual string `match`.
- `src/mcp/tests/transcription.rs` — transcribe tool tests + new cross-endpoint regression test (Task 2).
- `README.md`, `CHANGELOG.md` — user-facing docs of the breaking search-output change (Task 1).

---

### Task 1: Switch `MediaType` to snake_case serialization

**Files:**
- Modify: `src/telegram/types/media.rs:8` (the `#[serde(...)]` attribute) and `src/telegram/types/media.rs:106-110` (the unit test)
- Modify (docs): `README.md:460`, `CHANGELOG.md` (`[Unreleased]` section)
- Test: `src/telegram/types/media.rs` inline `#[cfg(test)]` module

**Interfaces:**
- Consumes: nothing (leaf change).
- Produces: `MediaType` now serializes `VideoNote` to the string `"video_note"` (all other variants byte-identical to before). Relied on by Task 2 and by every content-serializing tool (`search_messages`, `get_message_info`, history, `get_message_by_link`).

- [ ] **Step 1: Update the unit test to assert the new wire name (failing test)**

In `src/telegram/types/media.rs`, replace the existing test (lines 106-110):

```rust
    #[test]
    fn media_type_serde_lowercase() {
        let json = serde_json::to_string(&MediaType::VideoNote).unwrap();
        assert_eq!(json, "\"videonote\"");
    }
```

with:

```rust
    #[test]
    fn media_type_serde_snake_case() {
        let json = serde_json::to_string(&MediaType::VideoNote).unwrap();
        assert_eq!(json, "\"video_note\"");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test media_type_serde_snake_case`
Expected: FAIL — `assertion \`left == right\` failed`, left `"\"videonote\""`, right `"\"video_note\""` (the enum still serializes the old way).

- [ ] **Step 3: Change the serde rename rule**

In `src/telegram/types/media.rs`, change the attribute on the `MediaType` enum (line 8):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
```

(Only the `rename_all` value changes: `"lowercase"` → `"snake_case"`.)

- [ ] **Step 4: Run the test to verify it passes, then the full suite**

Run: `cargo test media_type_serde_snake_case`
Expected: PASS.

Run: `cargo test`
Expected: PASS — no other test asserts `"videonote"` (only this one did). The existing `transcribe` tests still pass because that handler currently hand-maps the string (changed in Task 2).

- [ ] **Step 5: Update README media-types list**

In `README.md` (line 460), change `videonote` to `video_note` in the Media Types list:

```markdown
**Media Types:** `none`, `photo`, `video`, `document`, `audio`, `voice`, `video_note`, `animation`, `sticker`, `contact`, `location`, `venue`, `poll`, `dice`
```

- [ ] **Step 6: Add a CHANGELOG entry for the breaking change**

In `CHANGELOG.md`, under the existing `## [Unreleased]` heading, add a `### Changed` section:

```markdown
## [Unreleased]

### Changed
- **Breaking:** message `media_type` for round videos is now reported as `"video_note"` (was `"videonote"`) by `search_messages`, `get_recent_messages`, `get_message_info`, and `get_message_by_link`. This aligns the value with `transcribe_voice_message`, the `media_filter` request parameter, and error messages — `MediaType` now serializes as `snake_case`. Clients keying on the literal `"videonote"` must update.
```

- [ ] **Step 7: Format, lint, commit**

Run: `cargo fmt --all && cargo clippy -- -D warnings`
Expected: clean.

```bash
git add src/telegram/types/media.rs README.md CHANGELOG.md
git commit -m "fix: serialize MediaType as snake_case (videonote -> video_note)

Round videos now report media_type \"video_note\" consistently across
search and content tools, matching MediaFilter and the transcribe tool.
VideoNote is the only multi-word variant, so no other wire name changes.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Type the transcribe response `media_type` as `MediaType`

**Files:**
- Modify: `src/mcp/tools/types/responses.rs:60-61` (field type + schemars description)
- Modify: `src/mcp/server.rs:552-564` (drop the string `match`) and `src/mcp/server.rs:576` (tracing arg)
- Modify/Test: `src/mcp/tests/transcription.rs:59`, `:88` (assertions) and a new regression test

**Interfaces:**
- Consumes: `MediaType` snake_case serialization from Task 1; `TranscriptionOutcome { media_type: MediaType, .. }` (existing) and `MediaType::{Voice, VideoNote}` (existing variants).
- Produces: `TranscribeVoiceMessageResponse.media_type: MediaType` (was `String`). Wire output for voice/video_note is unchanged (`"voice"` / `"video_note"`); the advertised JsonSchema becomes a typed enum.

- [ ] **Step 1: Update the transcribe tests to expect the enum + add the regression test (failing)**

In `src/mcp/tests/transcription.rs`, change the assertion at line 59 (in `returns_transcription_text`):

```rust
    assert_eq!(resp.media_type, MediaType::Voice);
```

and the assertion at line 88 (in `returns_partial_flag_and_video_note_type`):

```rust
    assert_eq!(resp.media_type, MediaType::VideoNote);
```

Then append this new regression test to the end of the file (the durable guard against re-drift):

```rust
#[test]
fn media_type_wire_name_is_consistent_across_endpoints() {
    // search_messages serializes MediaType directly; transcribe_voice_message
    // embeds the same MediaType. Both must emit the identical wire string for a
    // round video, or the two endpoints disagree — the bug this guards against.
    let search_name = serde_json::to_string(&MediaType::VideoNote).unwrap();
    assert_eq!(search_name, "\"video_note\"");

    let resp = TranscribeVoiceMessageResponse {
        text: String::new(),
        partial: false,
        duration_seconds: None,
        media_type: MediaType::VideoNote,
    };
    let value = serde_json::to_value(&resp).unwrap();
    assert_eq!(value["media_type"], serde_json::json!("video_note"));
}
```

(`MediaType` and `TranscribeVoiceMessageResponse` are already imported at the top of this file; `serde_json` is already a dependency used here.)

- [ ] **Step 2: Run the tests to verify they fail (compile error)**

Run: `cargo test --lib transcription`
Expected: FAIL to compile — `resp.media_type` is `String`, which has no `PartialEq<MediaType>`, and the regression test constructs the response with `media_type: MediaType::VideoNote` against a `String` field. (Type mismatch is the red state.)

- [ ] **Step 3: Retype the response field**

In `src/mcp/tools/types/responses.rs`, change the `media_type` field of `TranscribeVoiceMessageResponse` (lines 60-61):

```rust
    #[schemars(description = "Media type of the transcribed message (\"voice\" or \"video_note\")")]
    pub media_type: MediaType,
```

(`MediaType` is already imported at the top of `responses.rs`.)

- [ ] **Step 4: Drop the hand-rolled string match in the handler**

In `src/mcp/server.rs`, replace the block at lines 552-564:

```rust
        // The feature contract specifies "voice" / "video_note"; MediaType's
        // lowercase serialization would emit "videonote", so map explicitly.
        let media_type = match outcome.media_type {
            crate::telegram::types::MediaType::Voice => "voice",
            crate::telegram::types::MediaType::VideoNote => "video_note",
            other => {
                return Err(format!(
                    "unexpected media type for transcription: {:?}",
                    other
                ));
            }
        }
        .to_string();
```

with:

```rust
        // Only voice and video_note are transcribable. Pass the MediaType through
        // unchanged so its serde derive (snake_case) is the single source of the
        // "voice" / "video_note" wire names — shared with search_messages.
        let media_type = match outcome.media_type {
            mt @ (crate::telegram::types::MediaType::Voice
            | crate::telegram::types::MediaType::VideoNote) => mt,
            other => {
                return Err(format!(
                    "unexpected media type for transcription: {:?}",
                    other
                ));
            }
        };
```

- [ ] **Step 5: Fix the tracing argument (MediaType has no Display)**

In `src/mcp/server.rs` at line 576, change the `media_type` tracing field from `Display` (`%`) to `Debug` (`?`), since `MediaType` does not implement `Display`:

```rust
            media_type = ?response.media_type,
```

- [ ] **Step 6: Run the tests to verify they pass, then the full suite**

Run: `cargo test --lib transcription`
Expected: PASS — `returns_transcription_text`, `returns_partial_flag_and_video_note_type`, and `media_type_wire_name_is_consistent_across_endpoints` all green.

Run: `cargo test`
Expected: PASS.

- [ ] **Step 7: Format, lint, commit**

Run: `cargo fmt --all && cargo clippy -- -D warnings`
Expected: clean.

```bash
git add src/mcp/tools/types/responses.rs src/mcp/server.rs src/mcp/tests/transcription.rs
git commit -m "refactor: type transcribe response media_type as MediaType

Remove the hand-rolled voice/video_note string match; the MediaType serde
derive is now the single source of the wire name. Adds a cross-endpoint
regression test locking search and transcribe to the same string.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- Spec §"Changes" item 1 (MediaType snake_case) → Task 1 Steps 1-4. ✓
- Spec §"Changes" item 2 (transcribe Approach A) → Task 2 Steps 1, 3-5. ✓
- Spec §"Changes" item 3 (tests: rename lowercase test, enum assertion, regression test) → Task 1 Step 1 + Task 2 Step 1. ✓
- Spec §"Changes" item 4 (docs: README + CHANGELOG breaking note) → Task 1 Steps 5-6. ✓
- Spec §"Verification" (fmt/clippy/test gate) → both tasks Step 7. ✓

**Placeholder scan:** No TBD/TODO/"handle edge cases"; every code step shows full code. ✓

**Type consistency:** `MediaType` (already imported in `responses.rs` and `transcription.rs`); response field `media_type: MediaType`; handler binds `mt @ (Voice | VideoNote)`; tracing uses `?` (Debug) because `MediaType` has no `Display`; regression test uses `serde_json::to_value` + `serde_json::json!`. Field name `media_type` consistent across DTO, handler, and tests. ✓
