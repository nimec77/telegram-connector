# Converter Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `forwarded_from` enrichment identical across every message-returning MCP tool, add `document_info` / `poll_info` / audio track metadata, and remove the envelope-less conversion path so the defect cannot recur.

**Architecture:** `convert_raw_message` is already the single conversion function; the defect is that `get_message_by_link`, `get_messages_batch`, and `get_channel_stats` fetch through grammers' high-level API, which discards the MTProto response envelope (`chats` + `users`) that forward attribution reads. Migrate those three onto raw TL invocations that keep the envelope — the pattern `raw_pager.rs` already established in Phase 33 — then delete `convert_message`, `EntityLookup::insert_peer`, and gate `EntityLookup::empty` behind `#[cfg(test)]` so no envelope-less entry point exists.

**Tech Stack:** Rust 2024 nightly, `grammers` (Codeberg, rev `9fef0bae`), `rmcp` v3.1, `schemars` v1, `serde`, `mockall`, `tokio`.

**Spec:** `docs/superpowers/specs/2026-08-13-converter-parity-design.md`

## Global Constraints

- **Zero additional network calls.** Every field added here derives from data already in the response. No `resolve_channels`, no `get_entity`, no download during conversion.
- **Backward compatible.** No existing field renamed, retyped, or removed. New fields are `Option`, carry `#[serde(skip_serializing_if = "Option::is_none", default)]`, and are omitted from JSON when absent.
- **Graceful degradation.** A missing entity, attribute, or poll result emits fewer fields — never an error, never a failed batch.
- **Never `unwrap()`** in production code. `expect()` only in tests. Use `?` or `.context("...")`.
- **TDD.** The failing test is written and observed failing before any production code.
- **Line length 100.** Run `cargo fmt --all` after every code change.
- **Pre-commit gate:** `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
- Config tests only: `cargo test config -- --test-threads=1` (env var mutation).
- **Do not** touch Work Order B (search latency) or C (media throughput).

## Verified API facts

> **CORRECTION (2026-08-13, after Task 3).** The poll entries in this block
> were originally transcribed from the WRONG grammers checkout. The cargo cache
> holds two:
> `~/.cargo/git/checkouts/grammers-2861ac880138ee45/fa7692e/` (STALE — do not
> read) and `~/.cargo/git/checkouts/grammers-8937e3b5288aa015/9fef0ba/` (the
> pinned rev, authoritative). Generated bindings for the pinned rev are at
> `target/debug/build/grammers-tl-types/*/out/generated_types.rs`.
>
> The poll shapes below have been corrected. The peer/getMessages/response
> entries were re-verified against the pinned rev and were already correct.
>
> **Treat every TL struct field list here as a starting point, not gospel** —
> flag-gated fields come and go between revs. The compiler is the authority;
> if a field list does not compile, fix it and note the discrepancy in your
> report rather than assuming you are wrong.

These were confirmed against the pinned grammers rev and generated TL.

```rust
// grammers_session::types
pub struct PeerRef { pub id: PeerId, pub auth: PeerAuth }
impl PeerAuth { pub fn from_hash(access_hash: i64) -> Self }
pub enum PeerKind { User, UserSelf, Chat, Channel }
impl PeerId { pub fn kind(&self) -> PeerKind }
PeerId::channel_unchecked(i64) / chat_unchecked(i64) / user_unchecked(i64)
impl From<PeerRef> for tl::enums::InputChannel   // direct, no manual access_hash

// generated TL enum variants
tl::enums::Poll::Poll(tl::types::Poll)
tl::enums::PollResults::Results(Box<tl::types::PollResults>)   // BOXED
tl::enums::PollAnswer::Answer(tl::types::PollAnswer)           // also: ::InputPollAnswer
tl::enums::PollAnswerVoters::Voters(tl::types::PollAnswerVoters)
tl::enums::TextWithEntities::Entities(tl::types::TextWithEntities)

// generated TL struct fields — the pinned rev carries MORE required fields
// than listed here on the poll types; let the compiler enumerate them.
tl::types::Poll { id, closed, public_voters, multiple_choice, quiz,
                  question: enums::TextWithEntities,
                  answers: Vec<enums::PollAnswer>, close_period, close_date,
                  /* + additional flag fields on the pinned rev */ }
tl::types::PollAnswer { text: enums::TextWithEntities, option: Vec<u8>,
                        media, added_by, date }
tl::types::PollAnswerVoters { chosen, correct, option: Vec<u8>,
                              voters: Option<i32>,      // OPTIONAL — own flag bit
                              recent_voters: Option<Vec<enums::Peer>> }
tl::types::PollResults { min, results: Option<Vec<enums::PollAnswerVoters>>,
                         total_voters: Option<i32>, recent_voters, solution,
                         solution_entities }
tl::types::MessageMediaPoll { poll, results, attached_media }
tl::types::TextWithEntities { text: String, entities: Vec<enums::MessageEntity> }

// `PollAnswerVoters.voters` being Option is load-bearing: `results` present
// with an individual count absent is Telegram's partial-disclosure state.
// Emit `voters: None` there — NEVER default it to 0 (that reports a real
// zero-vote result where the truth is "not disclosed").
tl::types::DocumentAttributeFilename { file_name: String }
tl::types::DocumentAttributeAudio { voice, duration, title: Option<String>,
                                    performer: Option<String>, waveform }
tl::types::MessageMediaPoll { poll: enums::Poll, results: enums::PollResults }

// grammers_client::media (all exported from the `media` module)
pub struct Poll { pub raw: tl::types::Poll, pub raw_results: tl::types::PollResults }
impl Poll {
    pub fn from_raw_media(poll: tl::types::MessageMediaPoll) -> Self
    pub fn question(&self) -> &tl::enums::TextWithEntities
    pub fn is_quiz(&self) -> bool
    pub fn closed(&self) -> bool
    pub fn iter_answers(&self) -> impl Iterator<Item = &tl::types::PollAnswer>
    pub fn total_voters(&self) -> Option<i32>
    pub fn iter_voters_summary(&self) -> Option<impl Iterator<Item = &tl::types::PollAnswerVoters>>
}
// NOTE: there is no accessor for `multiple_choice` — read `poll.raw.multiple_choice`.
```

## File Structure

| File | Responsibility | Tasks |
|---|---|---|
| `src/telegram/types/media.rs` | `DocumentInfo`, `PollInfo`, `PollOption` types; `AudioInfo` gains `title`/`performer` | 1, 2, 3 |
| `src/telegram/types/entities.rs` | `Message` gains `document_info` / `poll_info` | 1, 3 |
| `src/telegram/converters/media.rs` | `extract_document_info`, `extract_poll_info`, audio title/performer | 1, 2, 3 |
| `src/telegram/converters/message.rs` | Wire new extractors into `convert_raw_message`; delete `convert_message` | 1, 3, 7 |
| `src/telegram/converters.rs` | Re-exports | 1, 3, 7 |
| `src/mcp/tools/types/responses.rs` | `MessageResponse` gains the two fields + `From<Message>` | 1, 3 |
| `src/telegram/envelope.rs` | Gate `empty` behind `cfg(test)`; delete `insert_peer` | 7 |
| `src/telegram/client/raw_pager.rs` | `fetch_messages_by_id` + its pure helpers | 4 |
| `src/telegram/client/guard.rs` | `require_found` retyped to raw TL | 5 |
| `src/telegram/client/ops_message.rs` | Both tools onto the raw fetch | 5 |
| `src/telegram/client/ops_stats.rs` | Onto `RawHistoryPager` | 6 |
| `src/telegram/client.rs` | Import list follows the deletions | 5, 6, 7 |
| `src/telegram/tests/converters_tests.rs` | Media metadata tests | 1, 2, 3 |
| `src/test_helpers.rs` and 11 other literal sites | `Message` / `AudioInfo` struct literals gain the new fields | 1, 2, 3 |

**Struct-literal blast radius.** `Message` and `MessageResponse` are built with exhaustive struct literals in 12 places (find them with `grep -rn "album: None," src/`). Adding a field breaks all of them at compile time — that is intentional and desirable. Each task that adds a field fixes every site in the same commit.

---

### Task 1: `document_info`

**Files:**
- Modify: `src/telegram/types/media.rs` (add `DocumentInfo` after `AudioKind`, ~line 70)
- Modify: `src/telegram/converters/media.rs` (add `extract_document_info` after `extract_audio_info`, ~line 187)
- Modify: `src/telegram/converters.rs:16-18` (re-export)
- Modify: `src/telegram/types/entities.rs:36` (add field to `Message`)
- Modify: `src/mcp/tools/types/responses.rs:335` and its `From<Message>` impl
- Modify: every `Message` / `MessageResponse` literal site
- Test: `src/telegram/tests/converters_tests.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `pub struct DocumentInfo { file_name: Option<String>, file_size_bytes: u64, mime_type: Option<String> }` and `pub fn extract_document_info(media: &Media) -> Option<DocumentInfo>`, both re-exported from `crate::telegram::converters` / `crate::telegram::types`.

- [ ] **Step 1: Write the failing tests**

Append to `src/telegram/tests/converters_tests.rs`. The fixture mirrors the existing `audio_doc` / `video_doc` helpers in the same file.

```rust
fn plain_doc(file_name: Option<&str>, size: i64, mime: &str) -> Media {
    let attributes = file_name
        .map(|name| {
            vec![tl::enums::DocumentAttribute::Filename(
                tl::types::DocumentAttributeFilename {
                    file_name: name.to_string(),
                },
            )]
        })
        .unwrap_or_default();
    Media::Document(Document::from_raw_media(tl::types::MessageMediaDocument {
        nopremium: false,
        spoiler: false,
        video: false,
        round: false,
        voice: false,
        document: Some(tl::enums::Document::Document(tl::types::Document {
            id: 1,
            access_hash: 0,
            file_reference: Vec::new(),
            date: 0,
            mime_type: mime.to_string(),
            size,
            thumbs: None,
            video_thumbs: None,
            dc_id: 0,
            attributes,
        })),
        alt_documents: None,
        video_cover: None,
        video_timestamp: None,
        ttl_seconds: None,
    }))
}

#[test]
fn document_info_reads_filename_size_and_mime() {
    let media = plain_doc(Some("Как мы строим RAG.pdf"), 2_411_008, "application/pdf");

    let info = extract_document_info(&media).expect("document info present");

    assert_eq!(info.file_name.as_deref(), Some("Как мы строим RAG.pdf"));
    assert_eq!(info.file_size_bytes, 2_411_008);
    assert_eq!(info.mime_type.as_deref(), Some("application/pdf"));
}

#[test]
fn document_info_without_filename_attribute_omits_the_name() {
    let media = plain_doc(None, 512, "application/zip");

    let info = extract_document_info(&media).expect("document info present");

    assert_eq!(info.file_name, None);
    assert_eq!(info.file_size_bytes, 512);
}

#[test]
fn document_info_is_none_for_video_media() {
    let media = video_doc(false, 30.0, 1920, 1080, 5_000_000, "video/mp4", true);

    assert!(extract_document_info(&media).is_none());
}

#[test]
fn document_info_is_none_for_audio_media() {
    let media = audio_doc(false, 184, 7_340_032, "audio/mpeg");

    assert!(extract_document_info(&media).is_none());
}

#[test]
fn document_info_absent_from_json_when_media_is_not_a_document() {
    let msg = crate::test_helpers::create_test_message(1, "текст", 100);

    let json = serde_json::to_value(&msg).expect("serializes");

    assert!(json.get("document_info").is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib document_info`
Expected: FAIL — `cannot find function extract_document_info in this scope`.

- [ ] **Step 3: Add the `DocumentInfo` type**

In `src/telegram/types/media.rs`, after the `AudioKind` enum:

```rust
/// Generic-document metadata, derived entirely from the document attributes
/// already on the message (no network calls). Present only when `media_type`
/// is `document` — video / audio / voice / animation media carry their own
/// `video_info` / `audio_info` instead, so nothing is emitted twice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DocumentInfo {
    /// From `DocumentAttributeFilename`; often the only description a
    /// document post carries.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub file_name: Option<String>,
    pub file_size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mime_type: Option<String>,
}
```

- [ ] **Step 4: Add the extractor**

In `src/telegram/converters/media.rs`, after `extract_audio_info`. Add `DocumentInfo` to the `use crate::telegram::types::{...}` list at the top of the file.

```rust
/// Derive `DocumentInfo` from a generic document's attributes. Returns `None`
/// for every other media class, including the document-backed ones (video,
/// audio, voice, animation, sticker) that already have a dedicated info
/// object. Same zero-cost raw-TL source as [`extract_video_info`].
pub fn extract_document_info(media: &Media) -> Option<DocumentInfo> {
    if convert_media_to_type(media) != MediaType::Document {
        return None;
    }
    let Media::Document(doc) = media else {
        return None;
    };
    let Some(tl::enums::Document::Document(raw)) = doc.raw.document.as_ref() else {
        return None;
    };

    let file_name = raw.attributes.iter().find_map(|attr| match attr {
        tl::enums::DocumentAttribute::Filename(f) => Some(f.file_name.clone()),
        _ => None,
    });

    Some(DocumentInfo {
        file_name,
        file_size_bytes: raw.size.max(0) as u64,
        mime_type: Some(raw.mime_type.clone()),
    })
}
```

- [ ] **Step 5: Re-export**

In `src/telegram/converters.rs`, add `extract_document_info` to the `pub use media::{...}` list (keep alphabetical order — it goes before `extract_video_info`).

`src/telegram/types.rs:27-30` re-exports names individually, so add `DocumentInfo` there too:

```rust
pub use media::{
    AudioInfo, AudioKind, DocumentInfo, MediaDownload, MediaFilter, MediaType, SizeCandidate,
    VideoInfo, VideoKind,
};
```

- [ ] **Step 6: Add the field to the domain type and the wire type**

`src/telegram/types/entities.rs` — add to `Message` immediately after `audio_info`, and add `DocumentInfo` to the `use super::media::{...}` import:

```rust
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub document_info: Option<DocumentInfo>,
```

`src/mcp/tools/types/responses.rs` — the same field on `MessageResponse` after `audio_info`, plus `document_info: m.document_info,` in the `From<Message>` impl.

- [ ] **Step 7: Populate it in the converter**

`src/telegram/converters/message.rs` — add the import to the existing `use super::media::{...}` line, then beside the existing extractor calls (~line 271):

```rust
    let document_info = media.as_ref().and_then(extract_document_info);
```

and add `document_info,` to the `Message { ... }` literal after `audio_info`.

- [ ] **Step 8: Fix every struct literal the new field broke**

Run `cargo build 2>&1 | grep "missing field"` to list them, then add `document_info: None,` to each. Expect ~12 sites across `src/test_helpers.rs`, `src/mcp/tools/shaping.rs`, `src/mcp/tools/types/tests/responses_tests.rs`, `src/mcp/tests/history.rs`, `src/mcp/tests/search.rs`, `src/telegram/tests/client_tests.rs`, `src/telegram/converters/message.rs`, `src/telegram/types/entities.rs`.

- [ ] **Step 9: Run the tests**

Run: `cargo test --lib document_info`
Expected: PASS, 5 tests.

- [ ] **Step 10: Full gate and commit**

```bash
cargo fmt --all
cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "feat: document_info metadata for generic-document media

Zero-cost: file_name / file_size_bytes / mime_type read from the document
attributes already on the message. Emitted only when media_type is
\"document\" — video and audio media keep their own info objects rather
than carrying a duplicate."
```

---

### Task 2: `audio_info` gains `title` / `performer`

**Files:**
- Modify: `src/telegram/types/media.rs:56-62` (`AudioInfo`)
- Modify: `src/telegram/converters/media.rs:160-187` (`extract_audio_info`)
- Modify: `src/telegram/tests/converters_tests.rs:355` (`audio_doc` fixture gains two params)
- Modify: `src/telegram/types/media.rs:320` and `src/telegram/types/entities.rs:368` (`AudioInfo` literals)

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces: `AudioInfo` with two additional public fields, `title: Option<String>` and `performer: Option<String>`.

- [ ] **Step 1: Write the failing tests**

First widen the existing fixture in `src/telegram/tests/converters_tests.rs:355` — change the signature and the two attribute fields:

```rust
fn audio_doc(
    voice: bool,
    duration: i32,
    size: i64,
    mime: &str,
    title: Option<&str>,
    performer: Option<&str>,
) -> Media {
```

and inside the `DocumentAttributeAudio` literal replace the hardcoded `title: None, performer: None,` with:

```rust
                    title: title.map(|s| s.to_string()),
                    performer: performer.map(|s| s.to_string()),
```

Update the existing `audio_doc(...)` call sites in this file to pass `None, None`. Then append:

```rust
#[test]
fn audio_info_carries_title_and_performer() {
    let media = audio_doc(
        false,
        184,
        7_340_032,
        "audio/mpeg",
        Some("Ноктюрн"),
        Some("Шопен"),
    );

    let info = extract_audio_info(&media).expect("audio info present");

    assert_eq!(info.title.as_deref(), Some("Ноктюрн"));
    assert_eq!(info.performer.as_deref(), Some("Шопен"));
    assert_eq!(info.duration_seconds, 184);
}

#[test]
fn audio_info_without_id3_metadata_omits_title_and_performer() {
    let media = audio_doc(true, 12, 4096, "audio/ogg", None, None);

    let info = extract_audio_info(&media).expect("audio info present");

    assert_eq!(info.title, None);
    assert_eq!(info.performer, None);
    assert_eq!(info.kind, AudioKind::Voice);
}

#[test]
fn audio_info_omits_absent_title_from_json() {
    let media = audio_doc(true, 12, 4096, "audio/ogg", None, None);
    let info = extract_audio_info(&media).expect("audio info present");

    let json = serde_json::to_value(&info).expect("serializes");

    assert!(json.get("title").is_none());
    assert!(json.get("performer").is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib audio_info`
Expected: FAIL — `no field 'title' on type 'AudioInfo'`.

- [ ] **Step 3: Add the fields**

`src/telegram/types/media.rs`, appended to `AudioInfo`:

```rust
    /// Track title from `DocumentAttributeAudio`; absent when Telegram
    /// carries no ID3 metadata (the common case for voice messages).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    /// Track performer from `DocumentAttributeAudio`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub performer: Option<String>,
```

- [ ] **Step 4: Populate them**

`src/telegram/converters/media.rs` — rewrite the attribute walk in `extract_audio_info` so it captures all three values in the one pass it already makes:

```rust
    let mut duration_seconds = 0;
    let mut title = None;
    let mut performer = None;
    for attr in &raw.attributes {
        if let tl::enums::DocumentAttribute::Audio(a) = attr {
            duration_seconds = a.duration.max(0) as u32;
            title = a.title.clone();
            performer = a.performer.clone();
            break;
        }
    }

    Some(AudioInfo {
        duration_seconds,
        file_size_bytes: raw.size.max(0) as u64,
        kind,
        mime_type: Some(raw.mime_type.clone()),
        title,
        performer,
    })
```

- [ ] **Step 5: Fix the other `AudioInfo` literals**

Run `cargo build 2>&1 | grep "missing field"`. Add `title: None, performer: None,` to the literals at `src/telegram/types/media.rs:320` and `src/telegram/types/entities.rs:368`.

- [ ] **Step 6: Run the tests**

Run: `cargo test --lib audio_info`
Expected: PASS.

- [ ] **Step 7: Full gate and commit**

```bash
cargo fmt --all
cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "feat: audio_info carries track title and performer

Read from the same DocumentAttributeAudio attribute already walked for
duration. Both optional and omitted from JSON when Telegram supplies no
ID3 metadata, so voice messages are unchanged on the wire."
```

---

### Task 3: `poll_info`

**Files:**
- Modify: `src/telegram/types/media.rs` (add `PollInfo`, `PollOption`)
- Modify: `src/telegram/converters/media.rs` (add `extract_poll_info`)
- Modify: `src/telegram/converters.rs` (re-export)
- Modify: `src/telegram/types/entities.rs` (`Message.poll_info`)
- Modify: `src/mcp/tools/types/responses.rs` (`MessageResponse.poll_info` + `From<Message>`)
- Modify: `src/telegram/converters/message.rs` (populate)
- Test: `src/telegram/tests/converters_tests.rs`

**Interfaces:**
- Consumes: nothing from Tasks 1-2.
- Produces: `pub fn extract_poll_info(media: &Media) -> Option<PollInfo>`; `PollInfo { question: String, options: Vec<PollOption>, total_voters: Option<u64>, closed: bool, multiple_choice: bool, quiz: bool }`; `PollOption { text: String, voters: Option<u64> }`.

- [ ] **Step 1: Write the failing tests**

Append to `src/telegram/tests/converters_tests.rs`. Add `Poll` to the `use grammers_client::media::{...}` import at the top of the file.

```rust
fn poll_media(
    question: &str,
    answers: &[&str],
    voters: Option<&[i32]>,
    total_voters: Option<i32>,
    closed: bool,
    multiple_choice: bool,
    quiz: bool,
) -> Media {
    let text = |s: &str| {
        tl::enums::TextWithEntities::Entities(tl::types::TextWithEntities {
            text: s.to_string(),
            entities: Vec::new(),
        })
    };
    // The `option` bytes are the key linking an answer to its vote count.
    let raw_answers = answers
        .iter()
        .enumerate()
        .map(|(i, a)| {
            tl::enums::PollAnswer::Answer(tl::types::PollAnswer {
                text: text(a),
                option: vec![i as u8],
            })
        })
        .collect();
    let results = voters.map(|counts| {
        counts
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                tl::enums::PollAnswerVoters::Voters(tl::types::PollAnswerVoters {
                    chosen: false,
                    correct: false,
                    option: vec![i as u8],
                    voters: v,
                })
            })
            .collect()
    });
    Media::Poll(Poll::from_raw_media(tl::types::MessageMediaPoll {
        poll: tl::enums::Poll::Poll(tl::types::Poll {
            id: 1,
            closed,
            public_voters: false,
            multiple_choice,
            quiz,
            question: text(question),
            answers: raw_answers,
            close_period: None,
            close_date: None,
        }),
        results: tl::enums::PollResults::Results(tl::types::PollResults {
            min: false,
            results,
            total_voters,
            recent_voters: None,
            solution: None,
            solution_entities: None,
        }),
    }))
}

#[test]
fn poll_info_reads_question_options_and_per_option_voters() {
    let media = poll_media(
        "Какой стек выбрать?",
        &["Rust", "Go"],
        Some(&[287, 125]),
        Some(412),
        true,
        false,
        false,
    );

    let info = extract_poll_info(&media).expect("poll info present");

    assert_eq!(info.question, "Какой стек выбрать?");
    assert_eq!(info.options.len(), 2);
    assert_eq!(info.options[0].text, "Rust");
    assert_eq!(info.options[0].voters, Some(287));
    assert_eq!(info.options[1].text, "Go");
    assert_eq!(info.options[1].voters, Some(125));
    assert_eq!(info.total_voters, Some(412));
    assert!(info.closed);
    assert!(!info.multiple_choice);
    assert!(!info.quiz);
}

#[test]
fn poll_info_without_results_keeps_options_and_omits_voters() {
    let media = poll_media(
        "Придёте на митап?",
        &["Да", "Нет"],
        None,
        None,
        false,
        false,
        false,
    );

    let info = extract_poll_info(&media).expect("poll info present");

    assert_eq!(info.options.len(), 2);
    assert_eq!(info.options[0].voters, None);
    assert_eq!(info.options[1].voters, None);
    assert_eq!(info.total_voters, None);
    assert!(!info.closed);
}

#[test]
fn poll_info_matches_voters_to_options_by_key_not_position() {
    // Results arrive in a different order than the answers: option b'\x01'
    // (Go) first. Matching by the `option` bytes must still be correct.
    let mut media = poll_media(
        "Какой стек выбрать?",
        &["Rust", "Go"],
        Some(&[287, 125]),
        Some(412),
        false,
        false,
        false,
    );
    if let Media::Poll(ref mut poll) = media
        && let Some(results) = poll.raw_results.results.as_mut()
    {
        results.reverse();
    }

    let info = extract_poll_info(&media).expect("poll info present");

    assert_eq!(info.options[0].text, "Rust");
    assert_eq!(info.options[0].voters, Some(287));
    assert_eq!(info.options[1].text, "Go");
    assert_eq!(info.options[1].voters, Some(125));
}

#[test]
fn poll_info_flags_a_quiz_and_multiple_choice() {
    let media = poll_media("2+2?", &["4", "5"], None, None, false, true, true);

    let info = extract_poll_info(&media).expect("poll info present");

    assert!(info.quiz);
    assert!(info.multiple_choice);
}

#[test]
fn poll_info_is_none_for_non_poll_media() {
    let media = plain_doc(Some("slides.pdf"), 1024, "application/pdf");

    assert!(extract_poll_info(&media).is_none());
}

#[test]
fn poll_option_omits_absent_voters_from_json() {
    let media = poll_media("Придёте?", &["Да"], None, None, false, false, false);
    let info = extract_poll_info(&media).expect("poll info present");

    let json = serde_json::to_value(&info).expect("serializes");

    assert!(json["options"][0].get("voters").is_none());
    assert!(json.get("total_voters").is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib poll_info`
Expected: FAIL — `cannot find function extract_poll_info in this scope`.

- [ ] **Step 3: Add the types**

`src/telegram/types/media.rs`, after `DocumentInfo`:

```rust
/// Poll / quiz content and results, read from the poll media already on the
/// message (no network calls). Results are whatever Telegram delivered — no
/// separate call is made to fetch them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PollInfo {
    pub question: String,
    pub options: Vec<PollOption>,
    /// Absent when the poll has no disclosed results yet.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub total_voters: Option<u64>,
    pub closed: bool,
    pub multiple_choice: bool,
    /// A graded quiz rather than an opinion poll.
    pub quiz: bool,
}

/// One poll answer with its vote count when Telegram has disclosed results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PollOption {
    pub text: String,
    /// Absent when results are undisclosed — an unvoted poll degrades to
    /// text-only options rather than to a separate response shape.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub voters: Option<u64>,
}
```

- [ ] **Step 4: Add the extractor**

`src/telegram/converters/media.rs`, after `extract_document_info`. Add `PollInfo, PollOption` to the file's `use crate::telegram::types::{...}` list and `use std::collections::HashMap;` at the top.

```rust
/// Derive `PollInfo` from poll media. Returns `None` for every other media
/// class. Answers are matched to their vote counts by the `option` bytes key
/// that both `PollAnswer` and `PollAnswerVoters` carry — never by position,
/// which Telegram does not guarantee. Undisclosed results degrade to
/// text-only options; nothing is fabricated and no call is made.
pub fn extract_poll_info(media: &Media) -> Option<PollInfo> {
    let Media::Poll(poll) = media else {
        return None;
    };

    let voters_by_option: HashMap<&[u8], u64> = poll
        .iter_voters_summary()
        .map(|voters| {
            voters
                .map(|v| (v.option.as_slice(), u64::try_from(v.voters).unwrap_or(0)))
                .collect()
        })
        .unwrap_or_default();

    let options = poll
        .iter_answers()
        .map(|answer| {
            let tl::enums::TextWithEntities::Entities(text) = &answer.text;
            PollOption {
                text: text.text.clone(),
                voters: voters_by_option.get(answer.option.as_slice()).copied(),
            }
        })
        .collect();

    let tl::enums::TextWithEntities::Entities(question) = poll.question();

    Some(PollInfo {
        question: question.text.clone(),
        options,
        total_voters: poll.total_voters().and_then(|v| u64::try_from(v).ok()),
        closed: poll.closed(),
        // No accessor for this one in the pinned rev; `raw` is public and the
        // repo already reads document attributes the same way.
        multiple_choice: poll.raw.multiple_choice,
        quiz: poll.is_quiz(),
    })
}
```

- [ ] **Step 5: Re-export, add the field, populate it**

- `src/telegram/converters.rs`: add `extract_poll_info` to the `pub use media::{...}` list.
- `src/telegram/types.rs`: add `PollInfo, PollOption` to the `pub use media::{...}` list alongside `DocumentInfo`.
- `src/telegram/types/entities.rs`: add to `Message` after `document_info`, and to the `use super::media::{...}` import:

```rust
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub poll_info: Option<PollInfo>,
```

- `src/mcp/tools/types/responses.rs`: same field on `MessageResponse` after `document_info`, plus `poll_info: m.poll_info,` in `From<Message>`.
- `src/telegram/converters/message.rs`: add the import, then beside the other extractors:

```rust
    let poll_info = media.as_ref().and_then(extract_poll_info);
```

and `poll_info,` in the `Message { ... }` literal.

- [ ] **Step 6: Fix every broken struct literal**

Run `cargo build 2>&1 | grep "missing field"` and add `poll_info: None,` to each site.

- [ ] **Step 7: Run the tests**

Run: `cargo test --lib poll_info`
Expected: PASS, 6 tests.

- [ ] **Step 8: Full gate and commit**

```bash
cargo fmt --all
cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "feat: poll_info with question, options, and per-option voters

Read from Media::Poll's public raw/raw_results — zero extra calls. Answers
are matched to vote counts by the option bytes key rather than by position.
Deviates from the work order's array-of-strings shape: total_voters without
a per-option breakdown does not tell a caller what the poll concluded, and
an undisclosed-results poll degrades to text-only options rather than to a
second response shape."
```

---

### Task 4: Envelope-preserving `getMessages`

**Files:**
- Modify: `src/telegram/client/raw_pager.rs` (add `fetch_messages_by_id` + two pure helpers, after `fill_buffer` ~line 173; tests go in the file's existing `mod tests` at line 422)

**Interfaces:**
- Consumes: existing private helpers in this module — `unpack_page(res, limit) -> RawPage`, `raw_peer_id(raw) -> Option<&tl::enums::Peer>`.
- Produces:
  - `pub(super) enum GetMessagesRequest { Channel(tl::functions::channels::GetMessages), Plain(tl::functions::messages::GetMessages) }`
  - `fn get_messages_request(peer: PeerRef, ids: &[i32]) -> GetMessagesRequest`
  - `fn index_messages(messages: Vec<tl::enums::Message>, peer: PeerRef) -> HashMap<i32, tl::enums::Message>`
  - `pub(super) async fn fetch_messages_by_id(client: &Client, peer: PeerRef, ids: &[i32]) -> Result<(HashMap<i32, tl::enums::Message>, Arc<EntityLookup>), InvocationError>`

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests` block in `src/telegram/client/raw_pager.rs` (it already does `use super::*`, so the private helpers are in scope). Add to that module's imports: `use grammers_session::types::{PeerAuth, PeerId};` and `use std::collections::HashMap;` — `PeerRef` is already imported at file level.

**Reuse the module's existing fixture.** `mod tests` already defines `fn raw_msg(id: i32, date: i32, channel_id: i64) -> tl::enums::Message`, a Service-variant message in the given channel. Use it; do not add a second one.

```rust
fn channel_ref(id: i64) -> PeerRef {
    PeerRef {
        id: PeerId::channel_unchecked(id),
        auth: PeerAuth::from_hash(0),
    }
}

fn chat_ref(id: i64) -> PeerRef {
    PeerRef {
        id: PeerId::chat_unchecked(id),
        auth: PeerAuth::default(),
    }
}

#[test]
fn channel_peer_routes_to_channels_get_messages() {
    let request = get_messages_request(channel_ref(1144180066), &[610121, 610122]);

    match request {
        GetMessagesRequest::Channel(r) => assert_eq!(r.id.len(), 2),
        GetMessagesRequest::Plain(_) => panic!("channel peer must use channels.GetMessages"),
    }
}

#[test]
fn non_channel_peer_routes_to_messages_get_messages() {
    let request = get_messages_request(chat_ref(521440428), &[7]);

    match request {
        GetMessagesRequest::Plain(r) => assert_eq!(r.id.len(), 1),
        GetMessagesRequest::Channel(_) => panic!("chat peer must use messages.GetMessages"),
    }
}

#[test]
fn index_messages_keys_by_id_regardless_of_response_order() {
    let messages = vec![
        raw_msg(610122, 1_700_000_100, 1144180066),
        raw_msg(610121, 1_700_000_000, 1144180066),
    ];

    let indexed = index_messages(messages, channel_ref(1144180066));

    assert_eq!(indexed.len(), 2);
    assert_eq!(indexed[&610121].id(), 610121);
    assert_eq!(indexed[&610122].id(), 610122);
}

#[test]
fn index_messages_drops_a_message_from_a_different_peer() {
    // messages.GetMessages resolves bare ids across every dialog, so a
    // response can name a chat we did not ask about.
    let messages = vec![
        raw_msg(610121, 1_700_000_000, 1144180066),
        raw_msg(610122, 1_700_000_100, 999_999),
    ];

    let indexed = index_messages(messages, channel_ref(1144180066));

    assert_eq!(indexed.len(), 1);
    assert!(indexed.contains_key(&610121));
    assert!(!indexed.contains_key(&610122));
}

#[test]
fn index_messages_keeps_empty_placeholders_for_the_caller_to_classify() {
    let messages = vec![tl::enums::Message::Empty(tl::types::MessageEmpty {
        id: 609784,
        peer_id: None,
    })];

    let indexed = index_messages(messages, channel_ref(1144180066));

    assert!(indexed.contains_key(&609784));
}

#[test]
fn fetch_decode_builds_an_entity_map_from_the_response_envelope() {
    // THE load-bearing test for work order A. The bug was that
    // getMessages responses had their chats/users discarded, leaving
    // forwards ids-only. This asserts the decode keeps them, so a forward
    // source the account does not subscribe to is still attributable.
    let res = tl::enums::messages::Messages::ChannelMessages(
        tl::types::messages::ChannelMessages {
            inexact: false,
            pts: 1,
            count: 1,
            offset_id_offset: None,
            messages: vec![raw_msg(298716, 1_700_000_000, 1912881684)],
            topics: vec![],
            // The forward SOURCE — a channel we never asked about, present
            // only because the envelope names every entity its messages
            // reference.
            chats: vec![tl::enums::Chat::Channel(raw_tl_channel(
                1783384254,
                "Pavel Zloi",
                Some("evilfreelancer"),
            ))],
            users: vec![],
        },
    );

    let page = unpack_page(res, PAGE_LIMIT);
    let entities = EntityLookup::from_envelope(&page.chats, &page.users);

    let source = tl::enums::Peer::Channel(tl::types::PeerChannel {
        channel_id: 1783384254,
    });
    let info = entities
        .get(&source)
        .expect("envelope must name the forward source");
    assert_eq!(info.display_name.as_deref(), Some("Pavel Zloi"));
    assert_eq!(info.username.as_deref(), Some("evilfreelancer"));
}

#[test]
fn unpack_page_treats_not_modified_as_an_empty_final_page() {
    let page = unpack_page(
        tl::enums::messages::Messages::NotModified(tl::types::messages::MessagesNotModified {
            count: 0,
        }),
        PAGE_LIMIT,
    );

    assert!(page.messages.is_empty());
    assert!(page.last_chunk);
}
```

`EntityLookup` and its `get` are `pub(crate)`, so the test module reaches them; add `use crate::telegram::envelope::EntityLookup;` if `use super::*` does not already bring it in.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib raw_pager`
Expected: FAIL — `cannot find function get_messages_request in this scope`. Note that `fetch_decode_builds_an_entity_map_from_the_response_envelope` may compile and pass immediately, since it exercises `unpack_page` + `from_envelope`, which already exist. That is expected: it is the characterization test proving the decode path this task builds on preserves the envelope. If it FAILS, stop — the premise of work order A is wrong.

- [ ] **Step 3: Implement the pure helpers**

In `src/telegram/client/raw_pager.rs`, after `fill_buffer`. Add `use grammers_session::types::{PeerId, PeerKind};` and `use std::collections::HashMap;` to the file's imports (`PeerRef` is already imported).

```rust
/// Which RPC a peer's messages must be fetched with. Channel-namespace peers
/// require `channels.GetMessages`; `messages.GetMessages` resolves bare ids
/// across the account's dialogs and would return the wrong chat's message.
/// Mirrors grammers `client/messages.rs::get_messages_by_id` in the pinned rev.
pub(super) enum GetMessagesRequest {
    Channel(tl::functions::channels::GetMessages),
    Plain(tl::functions::messages::GetMessages),
}

fn get_messages_request(peer: PeerRef, ids: &[i32]) -> GetMessagesRequest {
    let id = ids
        .iter()
        .map(|&id| tl::enums::InputMessage::Id(tl::types::InputMessageId { id }))
        .collect();
    if peer.id.kind() == PeerKind::Channel {
        GetMessagesRequest::Channel(tl::functions::channels::GetMessages {
            channel: peer.into(),
            id,
        })
    } else {
        GetMessagesRequest::Plain(tl::functions::messages::GetMessages { id })
    }
}

/// Key a response's messages by id, dropping any that belong to a different
/// peer (grammers applies the same guard). `MessageEmpty` placeholders are
/// kept: the caller distinguishes "deleted" from "wrong peer", and both map
/// to missing anyway (work-order B1 guard).
fn index_messages(
    messages: Vec<tl::enums::Message>,
    peer: PeerRef,
) -> HashMap<i32, tl::enums::Message> {
    messages
        .into_iter()
        .filter(|raw| {
            raw_peer_id(raw).is_none_or(|p| PeerId::from(p.clone()) == peer.id)
        })
        .map(|raw| (raw.id(), raw))
        .collect()
}
```

- [ ] **Step 4: Implement the async fetch**

Immediately after the helpers:

```rust
/// Raw `getMessages` preserving the response envelope (get_message_by_link /
/// get_messages_batch path).
///
/// Same request and same RPC count as grammers' `get_messages_by_id`, but it
/// keeps the `chats`+`users` arrays that forward attribution reads instead of
/// collapsing them into a crate-private `PeerMap` (see `telegram/envelope.rs`).
/// Zero additional network calls.
pub(super) async fn fetch_messages_by_id(
    client: &Client,
    peer: PeerRef,
    ids: &[i32],
) -> Result<(HashMap<i32, tl::enums::Message>, Arc<EntityLookup>), InvocationError> {
    let response = match get_messages_request(peer, ids) {
        GetMessagesRequest::Channel(request) => client.invoke(&request).await?,
        GetMessagesRequest::Plain(request) => client.invoke(&request).await?,
    };
    // `limit` only drives the pager's last-chunk rule, which getMessages has
    // no use for; PAGE_LIMIT keeps the single decode path.
    let page = unpack_page(response, PAGE_LIMIT);
    let entities = Arc::new(EntityLookup::from_envelope(&page.chats, &page.users));
    Ok((index_messages(page.messages, peer), entities))
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test --lib raw_pager`
Expected: PASS — 6 new tests plus the module's existing ones.

- [ ] **Step 6: Full gate and commit**

```bash
cargo fmt --all
cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "feat: envelope-preserving raw getMessages fetch

Mirrors grammers get_messages_by_id request-for-request — same routing on
peer kind, same by-id keying, same wrong-peer filter — but keeps the
response envelope so forward attribution has the entity map it needs. Not
yet wired to a caller."
```

---

### Task 5: `get_message_by_link` and `get_messages_batch` onto the raw fetch

**Files:**
- Modify: `src/telegram/client/guard.rs:19-37` (`require_found` retyped)
- Modify: `src/telegram/client/ops_message.rs` (both `*_impl` methods)
- Modify: `src/telegram/client.rs:10-15` (imports)
- Test: `src/telegram/converters/message.rs` `mod tests` (conversion-level assertion)

**Interfaces:**
- Consumes: `fetch_messages_by_id` from Task 4.
- Produces: no new public API. `require_found` changes to
  `fn require_found(fetched: Option<tl::enums::Message>, channel_ref: &str, message_id: i32) -> Result<tl::enums::Message, Error>`.

- [ ] **Step 1: Write the failing test**

The end-to-end proof lands in Task 8 (it needs a mocked client). Here, assert the conversion contract that the migration depends on: a forward enriches when a real envelope is supplied. Append inside `mod tests` in `src/telegram/converters/message.rs`:

```rust
#[test]
fn batch_style_conversion_enriches_forward_from_a_shared_envelope() {
    // One envelope shared by every message in a getMessages response — the
    // shape fetch_messages_by_id returns. Both messages must attribute.
    let peer = public_channel_peer(1144180066, "swodki");
    let entities = EntityLookup::from_envelope(
        &[tl::enums::Chat::Channel(raw_tl_channel(
            1783384254,
            "Pavel Zloi",
            Some("evilfreelancer"),
        ))],
        &[],
    );

    for id in [610121, 610122] {
        let raw = raw_forwarded_message(id, fwd_header(channel_fwd_peer(1783384254), None, None));
        let msg = convert_raw_message(&raw, &peer, &entities).expect("converts");
        let fwd = msg.forwarded_from.expect("forward attribution present");
        assert_eq!(
            fwd.channel_name.as_ref().map(|n| n.as_str()),
            Some("Pavel Zloi")
        );
        assert_eq!(
            fwd.channel_username.as_ref().map(|u| u.as_str()),
            Some("evilfreelancer")
        );
    }
}
```

- [ ] **Step 2: Run it to confirm it passes for the right reason**

Run: `cargo test --lib batch_style_conversion`
Expected: PASS. This is a characterization test — it pins the behavior the migration must preserve. If it fails, stop: `convert_raw_message` is not what this plan assumes.

- [ ] **Step 3: Retype `require_found`**

`src/telegram/client/guard.rs` — the whole function, doc comment kept:

```rust
pub(super) fn require_found(
    fetched: Option<tl::enums::Message>,
    channel_ref: &str,
    message_id: i32,
) -> Result<tl::enums::Message, Error> {
    match fetched {
        Some(raw) if !is_empty_variant(&raw) => Ok(raw),
        _ => {
            tracing::warn!(
                channel_ref = %channel_ref,
                message_id,
                "Message not found or deleted"
            );
            Err(Error::InvalidInput(format!(
                "Message {message_id} not found or deleted in channel {channel_ref}"
            )))
        }
    }
}
```

Its existing test `require_found_maps_absent_slot_to_not_found_error` passes `None` and still compiles unchanged.

- [ ] **Step 4: Migrate `get_message_by_id_impl`**

`src/telegram/client/ops_message.rs` — add `use super::raw_pager::fetch_messages_by_id;` at the top, then replace the fetch-and-convert block (currently lines 29-61) with:

```rust
        // Raw getMessages instead of grammers' get_messages_by_id: same RPC,
        // but it keeps the response envelope so a forward from a channel we
        // do not subscribe to is still attributed (zero extra calls).
        let (mut by_id, entities) =
            with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
                fetch_messages_by_id(&self.client, peer_ref, &[message_id])
                    .await
                    .map_err(|e| {
                        tracing::error!(
                            channel_ref = %channel_ref,
                            message_id,
                            error = %e,
                            "Failed to get message by ID"
                        );
                        Error::TelegramApi(format!("Failed to get message: {}", e))
                    })
            })
            .await?;

        // Deleted ids come back as a MessageEmpty placeholder, not as an
        // absent entry (work-order B1).
        let raw = require_found(by_id.remove(&message_id), channel_ref, message_id)?;

        convert_raw_message(&raw, &peer, &entities).ok_or_else(|| {
            tracing::error!(
                channel_ref = %channel_ref,
                message_id,
                "Failed to convert message to domain type"
            );
            Error::TelegramApi("Failed to convert message".to_string())
        })
```

- [ ] **Step 5: Migrate `get_messages_batch_impl`**

Replace the fetch (currently lines 83-97) and the classification loop (lines 105-125) with:

```rust
        let (mut by_id, entities) =
            with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
                fetch_messages_by_id(&self.client, peer_ref, message_ids)
                    .await
                    .map_err(|e| {
                        tracing::error!(
                            channel_ref = %channel_ref,
                            count = message_ids.len(),
                            error = %e,
                            "Failed to get messages batch"
                        );
                        Error::TelegramApi(format!("Failed to get messages: {}", e))
                    })
            })
            .await?;

        // Single pass so every requested id lands in exactly one of
        // `messages` / `missing_ids` — never silently in neither. An absent
        // entry and a MessageEmpty both mean the id does not exist in this
        // channel (work-order B1 guard); a present, non-empty message that
        // still fails domain conversion is logged and reported as missing
        // rather than dropped.
        let mut messages = Vec::with_capacity(message_ids.len());
        let mut missing_ids = Vec::with_capacity(message_ids.len());
        for &message_id in message_ids {
            match by_id.remove(&message_id) {
                Some(raw) if !is_empty_variant(&raw) => {
                    match convert_raw_message(&raw, &peer, &entities) {
                        Some(converted) => messages.push(converted),
                        None => {
                            tracing::warn!(
                                channel_ref = %channel_ref,
                                message_id,
                                "Failed to convert message in batch; reporting as missing"
                            );
                            missing_ids.push(i64::from(message_id));
                        }
                    }
                }
                _ => missing_ids.push(i64::from(message_id)),
            }
        }
```

- [ ] **Step 6: Update `src/telegram/client.rs` imports**

`convert_raw_message` is already imported there; `convert_message` still is (Task 7 removes it). No change needed yet unless the compiler reports an unused import — if it does, leave `convert_message` in place, since `ops_stats.rs` still uses it until Task 6.

- [ ] **Step 7: Run the tests**

Run: `cargo test`
Expected: PASS. Existing `get_messages_batch` / `get_message_by_link` tests must be green without modification — the semantics are unchanged.

- [ ] **Step 8: Full gate and commit**

```bash
cargo fmt --all
cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "fix: get_message_by_link and get_messages_batch attribute forwards

Both fetched through grammers' high-level get_messages_by_id, which drops
the response envelope, so forwarded_from degraded to ids-only on exactly
the path documented for re-fetching truncated text. Same RPC count; the
missing-id and MessageEmpty semantics are unchanged."
```

---

### Task 6: `get_channel_stats` onto `RawHistoryPager`

**Files:**
- Modify: `src/telegram/client/ops_stats.rs:32-62`

**Interfaces:**
- Consumes: `RawHistoryPager` (existing), `timestamp_from_raw` (existing).
- Produces: nothing new. This retires the last `convert_message` caller so Task 7 can delete it.

- [ ] **Step 1: Confirm the existing stats tests pass and pin the behavior**

Run: `cargo test --lib stats`
Expected: PASS. Note the count — it must not change after this task. Stats aggregates and does not return messages, so this migration must be behavior-neutral.

- [ ] **Step 2: Migrate the sweep**

`src/telegram/client/ops_stats.rs` — add `use super::raw_pager::RawHistoryPager;` at the top, then replace the `with_timeout("iter_messages", ...)` block body (lines 33-61) with:

```rust
            with_timeout("iter_messages", self.timeouts.history_secs, async {
                let mut messages = Vec::new();
                let mut scanned = 0u32;
                let mut oldest: Option<chrono::DateTime<chrono::Utc>> = None;
                let mut complete = true;
                // Raw GetHistory pager instead of grammers' iter_messages:
                // same request, but it keeps the response envelope, which is
                // what lets the envelope-less converter be deleted entirely.
                let mut pager = RawHistoryPager::new(&self.client, peer_ref);
                while let Some((raw_msg, entities)) = pager
                    .next()
                    .await
                    .map_err(|e| Error::TelegramApi(format!("Failed to iterate messages: {}", e)))?
                {
                    if timestamp_from_raw(&raw_msg).is_none_or(|t| t < cutoff) {
                        break; // reached the window edge: sweep is complete
                    }
                    if scanned >= ChannelStats::MAX_MESSAGES_SCANNED {
                        complete = false; // cap hit with in-window messages left
                        break;
                    }
                    scanned += 1;
                    if let Some(t) = timestamp_from_raw(&raw_msg) {
                        oldest =
                            Some(oldest.map_or(t, |o: chrono::DateTime<chrono::Utc>| o.min(t)));
                    }
                    if let Some(converted) = convert_raw_message(&raw_msg, &peer, &entities) {
                        messages.push(converted);
                    }
                }
                Ok((messages, scanned, oldest, complete))
            })
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --lib stats`
Expected: PASS, same count as Step 1. The window cutoff, `MAX_MESSAGES_SCANNED` cap, `complete` flag, and oldest-timestamp tracking are all unchanged.

- [ ] **Step 4: Full gate and commit**

```bash
cargo fmt --all
cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "refactor: get_channel_stats sweeps via RawHistoryPager

Behavior-neutral — stats aggregates rather than returning messages. This
retires the last envelope-less conversion caller so the next commit can
delete that path outright."
```

---

### Task 7: Structural guard — delete the envelope-less path

**Files:**
- Modify: `src/telegram/converters/message.rs:313-332` (delete `convert_message`)
- Modify: `src/telegram/converters.rs:20` (drop the re-export)
- Modify: `src/telegram/envelope.rs:44-57, 116-143` (gate `empty`, delete `insert_peer`, document the invariant)
- Modify: `src/telegram/client.rs:10-15` (drop `convert_message` from imports)

**Interfaces:**
- Consumes: Tasks 5 and 6 (no callers left).
- Produces: `convert_raw_message` is the sole conversion entry point; `EntityLookup::from_envelope` is its sole production constructor.

- [ ] **Step 1: Prove there are no callers left**

```bash
ast-index update
ast-index callers "convert_message"
ast-index usages "insert_peer"
```

Expected: `convert_message` shows only the `converters.rs` re-export line; `insert_peer` shows none. If any production caller remains, stop — Task 5 or 6 is incomplete.

- [ ] **Step 2: Delete `convert_message`**

Remove the whole function and its doc comment from `src/telegram/converters/message.rs` (lines 313-332), and remove `pub use message::convert_message;` from `src/telegram/converters.rs`. Remove `convert_message,` from the import list in `src/telegram/client.rs`.

Also delete the now-unused imports it pulled in: check whether `use grammers_client::peer::Peer` style imports in `message.rs` are still needed and let `cargo clippy -D warnings` decide.

- [ ] **Step 3: Delete `insert_peer` and gate `empty`**

In `src/telegram/envelope.rs`, delete the entire `insert_peer` method (lines 116-143), and change `empty` to:

```rust
    /// Test-only: an entity map with no entries, for asserting the
    /// envelope-miss degradation path.
    ///
    /// Deliberately NOT available to production code. Conversion requires an
    /// `EntityLookup`, and `from_envelope` is the only way to build one
    /// outside tests — so a fetch path physically cannot convert without a
    /// real response envelope. This is the structural guarantee that replaced
    /// `convert_message`, which existed solely to satisfy the converter's
    /// signature without an envelope and silently degraded every forward it
    /// touched (work order A).
    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self::default()
    }
```

Extend the module doc at the top of `envelope.rs` with a line recording the invariant:

```rust
//! `from_envelope` is the only production constructor: conversion cannot be
//! reached without a real response envelope, so forward attribution cannot
//! silently degrade on a code path that forgets to supply one.
```

- [ ] **Step 4: Run the full suite**

Run: `cargo test`
Expected: PASS. Test-only `EntityLookup::empty()` call sites in `converters_tests.rs` and `message.rs` still compile, because they are inside `#[cfg(test)]` modules.

- [ ] **Step 5: Verify the guard actually holds**

Confirm the type-level claim by attempting the bug. Temporarily add to `src/telegram/client/ops_message.rs`:

```rust
let entities = EntityLookup::empty();
```

Run: `cargo build`
Expected: FAIL — `cannot find function 'empty' in this scope` (it is `#[cfg(test)]`). **Delete the temporary line** and re-run `cargo build` to confirm it is green. If the build succeeded with the line present, the gate is wrong — fix it before continuing.

- [ ] **Step 6: Full gate and commit**

```bash
cargo fmt --all
cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "refactor: delete the envelope-less conversion path

convert_message and EntityLookup::insert_peer existed only to satisfy the
converter's signature without a response envelope, which is precisely how
forwarded_from degraded on get_message_by_link and get_messages_batch.
With every fetch path now raw, they have no callers; EntityLookup::empty
becomes cfg(test)-only.

Conversion now requires an EntityLookup that only from_envelope can build
outside tests, so a tool added later inherits enrichment structurally
rather than by remembering to ask for it."
```

---

### Task 8: Response-shaping parity and zero-call invariant

**What this task does and does not prove.** These tests mock
`TelegramClientTrait`, which sits *above* the layer the bug lived in — every
tool is handed the same already-enriched domain `Message`, so identical
serialization is guaranteed by the mock, not by the fix. This task is
therefore a net for a *different* regression: a DTO mapping or
`format: "compact"` change that drops `forwarded_from` on one tool's
response but not another's. That is worth catching and currently untested.

The load-bearing proof of work order A lives in Task 4
(`fetch_decode_builds_an_entity_map_from_the_response_envelope`) and Task 7
Step 5 (the compile-fail guard check). Do not weaken those on the theory
that this task covers them — it does not.

**Files:**
- Create: `src/mcp/tests/parity.rs`
- Modify: `src/mcp/tests.rs` (declare the module)

**Interfaces:**
- Consumes: everything from Tasks 1-7.
- Produces: the regression net.

- [ ] **Step 1: Write the failing parity test**

Create `src/mcp/tests/parity.rs`. One enriched-forward fixture is returned by each tool's mock, and the serialized `forwarded_from` must be identical across all four.

Fixture facts (verified): `create_test_message_with_enriched_forward(id, text, channel_id, forwarded_channel_id)` sets `channel_name` to `"Военкор"`, `channel_username` to `"voenkor_ru"`, and `post_author` to `"И. Петров"`.

```rust
//! Response-shaping parity: given the same domain `Message`, every
//! message-returning tool must serialize the same `forwarded_from`.
//!
//! Scope note — these tests mock `TelegramClientTrait`, which is ABOVE the
//! conversion layer, so they cannot prove that fetching enriches. That is
//! covered by `raw_pager`'s envelope-decode test and by the type-level
//! guard in `envelope.rs`. What this file catches is a DTO or compact-format
//! change that drops `forwarded_from` on one tool's response shape only.

use crate::mcp::server::McpServer;
use crate::mcp::tools::types::requests::GetMessagesBatchRequest;
use crate::mcp::tools::{GetMessageByLinkRequest, GetRecentMessagesRequest, SearchRequest};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{Message, MessageBatch, QueryMetadata, SearchResult};
use crate::test_helpers::create_test_message_with_enriched_forward;
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

const CHANNEL_ID: i64 = 1144180066;
const MESSAGE_ID: i64 = 298716;
const FORWARDED_FROM_ID: i64 = 1783384254;

fn fixture() -> Message {
    create_test_message_with_enriched_forward(
        MESSAGE_ID,
        "переслано",
        CHANNEL_ID,
        FORWARDED_FROM_ID,
    )
}

fn search_result(messages: Vec<Message>) -> SearchResult {
    let returned = messages.len() as u64;
    SearchResult {
        messages,
        returned,
        has_more: false,
        search_time_ms: 1,
        query_metadata: QueryMetadata {
            query: String::new(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(48),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
        },
    }
}

fn permissive_limiter() -> MockRateLimiterTrait {
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().returning(|_| Ok(()));
    limiter
}

/// The `forwarded_from` object as it appears on the wire. Handles both
/// response shapes: a `messages` array, and `get_message_by_link`'s bare
/// serialized message.
fn forward_json(response: &str) -> serde_json::Value {
    let parsed: serde_json::Value = serde_json::from_str(response).expect("valid JSON");
    let message = parsed["messages"]
        .as_array()
        .and_then(|m| m.first())
        .cloned()
        .unwrap_or_else(|| parsed.clone());
    message["forwarded_from"].clone()
}

async fn via_get_recent_messages() -> String {
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_get_recent_messages()
        .returning(|_| Ok(search_result(vec![fixture()])));

    let server = McpServer::new(Arc::new(telegram), Arc::new(permissive_limiter()));
    server
        .get_recent_messages(
            Parameters(GetRecentMessagesRequest {
                channel_id: Some(CHANNEL_ID.to_string()),
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
            }),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("get_recent_messages ok")
}

async fn via_search_messages() -> String {
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_search_messages()
        .returning(|_| Ok(search_result(vec![fixture()])));

    let server = McpServer::new(Arc::new(telegram), Arc::new(permissive_limiter()));
    // Global search (no channel_id) so no resolve_channel_identity is needed.
    server
        .search_messages(
            Parameters(SearchRequest {
                query: "переслано".to_string(),
                channel_id: None,
                channel_ids: None,
                hours_back: None,
                limit: None,
                media_filter: None,
                ..Default::default()
            }),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("search_messages ok")
}

async fn via_get_message_by_link() -> String {
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_get_message_by_id()
        .return_once(|_, _| Ok(fixture()));

    let server = McpServer::new(Arc::new(telegram), Arc::new(permissive_limiter()));
    server
        .get_message_by_link(
            Parameters(GetMessageByLinkRequest {
                link: format!("https://t.me/testchannel/{MESSAGE_ID}"),
            }),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("get_message_by_link ok")
}

async fn via_get_messages_batch() -> String {
    let mut telegram = MockTelegramClientTrait::new();
    telegram.expect_get_messages_batch().returning(|_, _| {
        Ok(MessageBatch {
            messages: vec![fixture()],
            missing_ids: Vec::new(),
        })
    });

    let server = McpServer::new(Arc::new(telegram), Arc::new(permissive_limiter()));
    server
        .get_messages_batch_impl(GetMessagesBatchRequest {
            channel_id: CHANNEL_ID.to_string(),
            message_ids: vec![MESSAGE_ID],
            max_text_length: None,
        })
        .await
        .expect("get_messages_batch ok")
}

#[tokio::test]
async fn forwarded_from_is_identical_across_every_message_returning_tool() {
    let expected = forward_json(&via_get_recent_messages().await);

    // Guard the guard: a null here would make every comparison below vacuous.
    assert_eq!(
        expected["channel_name"], "Военкор",
        "fixture must carry forward attribution, else this test proves nothing"
    );

    assert_eq!(
        forward_json(&via_search_messages().await),
        expected,
        "search_messages diverged"
    );
    assert_eq!(
        forward_json(&via_get_message_by_link().await),
        expected,
        "get_message_by_link diverged"
    );
    assert_eq!(
        forward_json(&via_get_messages_batch().await),
        expected,
        "get_messages_batch diverged"
    );
}
```

`SearchRequest` derives `Default` (`src/mcp/tools/types/requests.rs:95`), so `..Default::default()` covers the remaining fields.

- [ ] **Step 2: Declare the module**

Add to `src/mcp/tests.rs`, keeping the list alphabetical (between `multi_channel` and `resolve`):

```rust
#[path = "tests/parity.rs"]
mod parity;
```

- [ ] **Step 3: Run it**

Run: `cargo test --lib forwarded_from_is_identical`
Expected: PASS. If a route diverges, that route was missed in Task 5 or 6.

- [ ] **Step 4: Write the zero-call invariant test**

Append to `src/mcp/tests/parity.rs`. Trait method names verified against `src/telegram/trait_def.rs`: the resolve and download methods are `resolve_channels` and `download_message_media`.

```rust
#[tokio::test]
async fn converting_a_full_batch_issues_no_resolve_or_download_calls() {
    let mut telegram = MockTelegramClientTrait::new();

    // The batch fetch is the ONLY call permitted. A resolve or a download
    // during conversion would be a zero-extra-call violation — mockall fails
    // the test on any invocation of these.
    telegram.expect_resolve_channels().never();
    telegram.expect_download_message_media().never();

    let messages: Vec<Message> = (0..100)
        .map(|i| {
            create_test_message_with_enriched_forward(
                MESSAGE_ID + i,
                "переслано",
                CHANNEL_ID,
                FORWARDED_FROM_ID,
            )
        })
        .collect();
    telegram
        .expect_get_messages_batch()
        .times(1)
        .return_once(move |_, _| {
            Ok(MessageBatch {
                messages,
                missing_ids: Vec::new(),
            })
        });

    let server = McpServer::new(Arc::new(telegram), Arc::new(permissive_limiter()));
    let out = server
        .get_messages_batch_impl(GetMessagesBatchRequest {
            channel_id: CHANNEL_ID.to_string(),
            message_ids: (0..100).map(|i| MESSAGE_ID + i).collect(),
            max_text_length: None,
        })
        .await
        .expect("batch ok");

    let json: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(json["messages"][0]["forwarded_from"]["channel_name"], "Военкор");

    // Id accounting at the MCP layer: every requested id is accounted for as
    // returned, missing, or budget-omitted — never silently dropped.
    let returned = json["messages"].as_array().map_or(0, |a| a.len());
    let missing = json["missing"].as_array().map_or(0, |a| a.len());
    let omitted = json["omitted_ids"].as_array().map_or(0, |a| a.len());
    assert_eq!(
        returned + missing + omitted,
        100,
        "every requested id must be accounted for exactly once"
    );
}
```

**Note on the layer below.** The same conservation property inside
`get_messages_batch_impl`'s loop (`ops_message.rs`) cannot be reached from
here — these tests mock `TelegramClientTrait`, and that loop lives in the real
`TelegramClient`, which needs a live grammers client. That loop is instead
guaranteed *structurally*: it iterates `message_ids` (not response slots),
every match arm pushes to exactly one vec, and the fallback arm is an
irrefutable `_`. Do not fabricate a mocked test that appears to cover it —
mocking at the wrong layer would assert nothing while looking like coverage.

Note: the response is subject to `[limits] response_byte_budget` (40 000), so 100 messages may be truncated with the remainder reported in `omitted_ids`. That is expected and does not affect this assertion — it reads the first message only.

- [ ] **Step 5: Run the full suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 6: Full gate and commit**

```bash
cargo fmt --all
cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "test: forwarded_from parity across every message-returning tool

Regression net behind the type-level guard: one fixture through
get_recent_messages, search_messages, get_message_by_link, and
get_messages_batch must serialize an identical forwarded_from. Plus a
100-message batch asserting no resolve or download call fires during
conversion."
```

---

### Task 9: Documentation and release notes

**Files:**
- Modify: `README.md` (response examples, tool reference)
- Modify: `CHANGELOG.md`
- Modify: `docs/tasklist.md` (progress table + phase section)
- Modify: `docs/memory.md`

**Interfaces:**
- Consumes: Tasks 1-8 complete and green.
- Produces: nothing code-facing.

- [ ] **Step 1: Update README response examples**

Find the message-shaped JSON examples (`grep -n "audio_info\|video_info\|forwarded_from" README.md`) and add `document_info` and `poll_info` examples plus the extended `audio_info`:

```json
"document_info": {
  "file_name": "Как мы строим RAG.pdf",
  "file_size_bytes": 2411008,
  "mime_type": "application/pdf"
},
"poll_info": {
  "question": "Какой стек выбрать?",
  "options": [
    {"text": "Rust", "voters": 287},
    {"text": "Go", "voters": 125}
  ],
  "total_voters": 412,
  "closed": true,
  "multiple_choice": false,
  "quiz": false
}
```

State that `document_info` appears only for `media_type: "document"` and that `poll_info.options[].voters` is omitted when Telegram has not disclosed results.

- [ ] **Step 2: Update CHANGELOG.md**

Add an `## [Unreleased]` section (or extend the existing one) following the file's established format:

```markdown
### Fixed
- `forwarded_from` now carries `channel_name` / `channel_username` on
  `get_message_by_link` and `get_messages_batch`, matching
  `get_recent_messages` / `search_messages`. Both tools fetched through
  grammers' high-level `get_messages_by_id`, which discards the MTProto
  response envelope that attribution reads — so re-fetching a forward for
  its full text silently dropped attribution the caller already had.

### Added
- `document_info` (`file_name`, `file_size_bytes`, `mime_type`) on messages
  whose `media_type` is `document`.
- `poll_info` (`question`, `options` with per-option `voters`,
  `total_voters`, `closed`, `multiple_choice`, `quiz`) on poll messages.
- `audio_info` gains `title` and `performer`.

### Changed
- `get_channel_stats` sweeps via the raw history pager (behavior-neutral).
- Internal: `convert_message` and `EntityLookup::insert_peer` removed;
  `EntityLookup::empty` is `#[cfg(test)]`-only. Conversion now requires a
  real response envelope by construction.
```

All additions are zero extra network calls and are omitted from JSON when absent.

- [ ] **Step 3: Update docs/tasklist.md**

Add a Phase 34 row to the progress table, matching the existing row style, and bump **Overall Progress** to `34/34 phases complete`:

```
| 34 | Converter parity (work order A) | ✅ Complete | <count> | `forwarded_from` enrichment reaches `get_message_by_link` / `get_messages_batch` via a raw envelope-preserving `getMessages` (`raw_pager::fetch_messages_by_id`); `get_channel_stats` moved to `RawHistoryPager`; `convert_message` + `EntityLookup::insert_peer` deleted and `EntityLookup::empty` gated `#[cfg(test)]`, making envelope-less conversion unrepresentable. Adds `document_info`, `poll_info` (per-option voters), and `audio_info.title`/`performer` — all zero-call. |
```

Fill `<count>` from the actual `cargo test` total.

- [ ] **Step 4: Update docs/memory.md**

Record the two durable lessons, following the file's existing entry format:

- **A shared function is not a shared path.** Phase 33 built one converter and still shipped a path-dependent bug, because enrichment quality depended on an argument (`EntityLookup`) that two callers could not supply. When enrichment depends on data the fetch layer may discard, auditing "does everyone call the shared function" proves nothing — audit what everyone *passes*.
- **Delete the weak overload rather than testing for it.** `convert_message` existed only to make an envelope-less call compile. Removing it, instead of adding a test that enumerates tools, is what makes the bug unrepresentable — an enumeration has the same failure mode as the original defect.

- [ ] **Step 5: Final gate**

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

Expected: all green. Record the test count.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "docs: converter parity — README, changelog, tasklist, memory"
```

- [ ] **Step 7: Manual acceptance (requires a live authenticated session)**

These cannot run offline — they need the deployed MCP server against real Telegram. Report results rather than assuming them.

1. Fetch channel `1912881684` message `298716` via `get_recent_messages`, `get_message_by_link`, **and** `get_messages_batch`. All three must return `"channel_name":"Pavel Zloi"`.
2. Fetch channel `2246801752` message `198` and confirm `document_info.file_name` is populated.

If either fails, do not close the work order — report which route diverged and what it returned.

---

## Notes for the implementer

- **`ast-index` first.** `ast-index search "name"`, `ast-index outline <file>` before reading any file over 500 lines (`src/mcp/server.rs`, `src/telegram/client/raw_pager.rs`, `src/telegram/converters/message.rs` all qualify). `Grep` only for string literals and attribute text.
- **Tasks 1-3 are independent of 4-7.** They can be reordered or parallelized across worktrees. Tasks 4→5→6→7 are a strict chain: 7 cannot compile until 5 and 6 land.
- **Do not "fix" the work order's diagnosis in code.** It claims tools bypass the shared converter; they do not. The spec records why. If a route looks like it needs its own converter, re-read the spec's root-cause section before writing one.
- **The peer-match filter in Task 4 is not defensive padding.** `messages.GetMessages` (the non-channel branch) resolves bare ids across every dialog the account has. Without the filter, a batch could return a message from an unrelated chat under a requested id.
