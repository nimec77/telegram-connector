# Video & Audio Metadata Enrichment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enrich `search_messages` / `get_recent_messages` message responses with optional `video_info` / `audio_info` objects, and add `video_info` to the `get_message_media` metadata block — all derived from grammers document attributes already in hand, with **zero extra API calls**.

**Architecture:** Define `VideoInfo`/`AudioInfo` (+ `VideoKind`/`AudioKind` enums) once in the domain layer (`src/telegram/types/media.rs`), extract them from raw TL document attributes in `src/telegram/converters.rs`, attach them to the domain `Message` in `convert_message`, and reference them directly from the response DTOs — exactly how `link_preview` / `forwarded_from` already work. No parallel DTO structs.

**Tech Stack:** Rust nightly (2024 edition), `grammers-client` (git master) `tl` raw types, `serde`, `schemars` v1 (`#[derive(JsonSchema)]`), `rmcp` v1.7, `mockall` for trait mocks.

## Global Constraints

- Rust **nightly**, edition 2024 (no `rust-toolchain.toml`; nightly implied by edition).
- Line length **100 chars**. Run `cargo fmt --all` after every code change.
- **Never `unwrap()`** in production code (use `?` / `.context(...)`); `expect()` only in tests.
- **TDD:** write the failing test first; no production code without a preceding test.
- Pre-merge gate (all must pass): `cargo fmt --check && cargo clippy -- -D warnings && cargo test`.
- Purely **additive / backward compatible**: existing response fields and JSON names are unchanged; new objects use `#[serde(skip_serializing_if = "Option::is_none", default)]` and are omitted when absent.
- Logging via `tracing`; never log phone numbers, api_hash, passwords, or session tokens.

---

### Task 1: Domain types `VideoInfo` / `AudioInfo` / `VideoKind` / `AudioKind`

**Files:**
- Modify: `src/telegram/types/media.rs` (add types + tests)
- Modify: `src/telegram/types.rs:23` (re-export the new types)

**Interfaces:**
- Produces:
  - `pub struct VideoInfo { duration_seconds: u32, width: u32, height: u32, file_size_bytes: u64, kind: VideoKind, has_thumbnail: bool, mime_type: Option<String> }` (derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema`)
  - `pub enum VideoKind { Video, VideoNote, Animation }` (snake_case serde, `Copy`)
  - `pub struct AudioInfo { duration_seconds: u32, file_size_bytes: u64, kind: AudioKind, mime_type: Option<String> }`
  - `pub enum AudioKind { Audio, Voice }` (snake_case serde, `Copy`)

- [ ] **Step 1: Write the failing tests**

In `src/telegram/types/media.rs`, inside the existing `#[cfg(test)] mod tests { use super::*; ... }` block (after `size_candidate_construction`, before the closing `}` at line 213), add:

```rust
    // =========================================================================
    // VideoInfo / AudioInfo Tests
    // =========================================================================

    #[test]
    fn video_kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&VideoKind::VideoNote).unwrap(),
            "\"video_note\""
        );
        assert_eq!(
            serde_json::to_string(&VideoKind::Animation).unwrap(),
            "\"animation\""
        );
        assert_eq!(serde_json::to_string(&VideoKind::Video).unwrap(), "\"video\"");
    }

    #[test]
    fn audio_kind_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&AudioKind::Voice).unwrap(), "\"voice\"");
        assert_eq!(serde_json::to_string(&AudioKind::Audio).unwrap(), "\"audio\"");
    }

    #[test]
    fn video_info_omits_mime_type_when_none() {
        let info = VideoInfo {
            duration_seconds: 0,
            width: 0,
            height: 0,
            file_size_bytes: 100,
            kind: VideoKind::Animation,
            has_thumbnail: false,
            mime_type: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("mime_type").is_none());
        assert_eq!(json["kind"], "animation");
        assert_eq!(json["duration_seconds"], 0);
    }

    #[test]
    fn audio_info_roundtrips() {
        let info = AudioInfo {
            duration_seconds: 12,
            file_size_bytes: 2048,
            kind: AudioKind::Voice,
            mime_type: Some("audio/ogg".to_string()),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: AudioInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, info);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p telegram-mcp video_kind audio_kind video_info audio_info 2>&1 | head -30`
Expected: FAIL — compile error, `cannot find type VideoKind`/`VideoInfo`/`AudioKind`/`AudioInfo` in this scope.

- [ ] **Step 3: Add the types**

In `src/telegram/types/media.rs`, immediately after the `MediaType` enum (after line 25, before the `MediaFilter` doc comment), insert:

```rust
/// Video-class media metadata, derived entirely from a message's document
/// attributes (no network calls). Present only for video / video_note /
/// animation media.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VideoInfo {
    pub duration_seconds: u32,
    pub width: u32,
    pub height: u32,
    pub file_size_bytes: u64,
    pub kind: VideoKind,
    pub has_thumbnail: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mime_type: Option<String>,
}

/// Closed set of video-class kinds. Dedicated (not reused `MediaType`) so the
/// advertised JSON schema is exactly `video | video_note | animation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VideoKind {
    Video,
    VideoNote,
    Animation,
}

/// Audio-class media metadata (zero-cost, same source as `VideoInfo`).
/// Present only for audio (music) / voice media. Pairs with the transcription
/// tool: duration tells the client whether a voice message is worth a quota call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AudioInfo {
    pub duration_seconds: u32,
    pub file_size_bytes: u64,
    pub kind: AudioKind,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mime_type: Option<String>,
}

/// Closed set of audio-class kinds (`audio | voice`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioKind {
    Audio,
    Voice,
}
```

- [ ] **Step 4: Re-export the new types**

In `src/telegram/types.rs`, change line 23 from:

```rust
pub use media::{MediaDownload, MediaFilter, MediaType, SizeCandidate};
```

to:

```rust
pub use media::{
    AudioInfo, AudioKind, MediaDownload, MediaFilter, MediaType, SizeCandidate, VideoInfo, VideoKind,
};
```

- [ ] **Step 5: Run tests to verify they pass + lint**

Run: `cargo test -p telegram-mcp video_kind audio_kind video_info audio_info 2>&1 | tail -20`
Expected: PASS (4 tests).
Run: `cargo fmt --all && cargo clippy -- -D warnings 2>&1 | tail -5`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/telegram/types/media.rs src/telegram/types.rs
git commit -m "feat: add VideoInfo/AudioInfo domain types"
```

---

### Task 2: Extraction functions `extract_video_info` / `extract_audio_info`

**Files:**
- Modify: `src/telegram/converters.rs` (top import line 3-6; new functions after line 111; tests in the `#[cfg(test)] mod tests` block)

**Interfaces:**
- Consumes: `VideoInfo`, `VideoKind`, `AudioInfo`, `AudioKind` (Task 1); existing `convert_media_to_type`, `MediaType`.
- Produces:
  - `pub fn extract_video_info(media: &Media) -> Option<VideoInfo>`
  - `pub fn extract_audio_info(media: &Media) -> Option<AudioInfo>`

- [ ] **Step 1: Add type imports**

In `src/telegram/converters.rs`, change the import block at lines 3-6 from:

```rust
use crate::telegram::types::{
    Channel, ChannelId, ChannelName, ForwardInfo, LinkPreview, MediaFilter, MediaType, Message,
    MessageId, SizeCandidate, UserId, Username,
};
```

to:

```rust
use crate::telegram::types::{
    AudioInfo, AudioKind, Channel, ChannelId, ChannelName, ForwardInfo, LinkPreview, MediaFilter,
    MediaType, Message, MessageId, SizeCandidate, UserId, Username, VideoInfo, VideoKind,
};
```

- [ ] **Step 2: Write the failing tests**

In `src/telegram/converters.rs`, inside `#[cfg(test)] mod tests { use super::*; ... }`, add these test helpers and tests just before the closing `}` (after `link_preview_empty_webpage_returns_none`, before line 565's `}`). The `tl`, `Media`, `Document` symbols are in scope via `use super::*` (the file's top-level `use grammers_client::{media::{Document, Media, PhotoSize}, tl}`):

```rust
    fn video_doc(
        round_message: bool,
        duration: f64,
        w: i32,
        h: i32,
        size: i64,
        mime: &str,
        with_thumb: bool,
    ) -> Media {
        let thumbs = with_thumb.then(|| {
            vec![tl::enums::PhotoSize::Empty(tl::types::PhotoSizeEmpty {
                r#type: "i".to_string(),
            })]
        });
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
                thumbs,
                video_thumbs: None,
                dc_id: 0,
                attributes: vec![tl::enums::DocumentAttribute::Video(
                    tl::types::DocumentAttributeVideo {
                        round_message,
                        supports_streaming: false,
                        nosound: false,
                        duration,
                        w,
                        h,
                        preload_prefix_size: None,
                        video_start_ts: None,
                        video_codec: None,
                    },
                )],
            })),
            alt_documents: None,
            video_cover: None,
            video_timestamp: None,
            ttl_seconds: None,
        }))
    }

    fn gif_doc(size: i64, with_thumb: bool) -> Media {
        let thumbs = with_thumb.then(|| {
            vec![tl::enums::PhotoSize::Empty(tl::types::PhotoSizeEmpty {
                r#type: "i".to_string(),
            })]
        });
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
                mime_type: "image/gif".to_string(),
                size,
                thumbs,
                video_thumbs: None,
                dc_id: 0,
                attributes: vec![tl::enums::DocumentAttribute::Animated],
            })),
            alt_documents: None,
            video_cover: None,
            video_timestamp: None,
            ttl_seconds: None,
        }))
    }

    fn audio_doc(voice: bool, duration: i32, size: i64, mime: &str) -> Media {
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
                attributes: vec![tl::enums::DocumentAttribute::Audio(
                    tl::types::DocumentAttributeAudio {
                        voice,
                        duration,
                        title: None,
                        performer: None,
                        waveform: None,
                    },
                )],
            })),
            alt_documents: None,
            video_cover: None,
            video_timestamp: None,
            ttl_seconds: None,
        }))
    }

    #[test]
    fn extract_video_info_regular_video() {
        let media = video_doc(false, 30.0, 1920, 1080, 5_000_000, "video/mp4", true);
        let info = extract_video_info(&media).expect("video info present");
        assert_eq!(info.kind, VideoKind::Video);
        assert_eq!(info.duration_seconds, 30);
        assert_eq!(info.width, 1920);
        assert_eq!(info.height, 1080);
        assert_eq!(info.file_size_bytes, 5_000_000);
        assert!(info.has_thumbnail);
        assert_eq!(info.mime_type.as_deref(), Some("video/mp4"));
    }

    #[test]
    fn extract_video_info_round_message_is_video_note() {
        let media = video_doc(true, 5.0, 240, 240, 100_000, "video/mp4", true);
        let info = extract_video_info(&media).expect("video info present");
        assert_eq!(info.kind, VideoKind::VideoNote);
    }

    #[test]
    fn extract_video_info_gif_is_animation_with_zero_dims() {
        let media = gif_doc(20_000, true);
        let info = extract_video_info(&media).expect("video info present");
        assert_eq!(info.kind, VideoKind::Animation);
        assert_eq!(info.duration_seconds, 0);
        assert_eq!(info.width, 0);
        assert_eq!(info.height, 0);
        assert!(info.has_thumbnail);
    }

    #[test]
    fn extract_video_info_without_thumbs_is_false() {
        let media = video_doc(false, 10.0, 640, 480, 1_000, "video/mp4", false);
        let info = extract_video_info(&media).expect("video info present");
        assert!(!info.has_thumbnail);
    }

    #[test]
    fn extract_video_info_none_for_audio() {
        let media = audio_doc(true, 7, 1000, "audio/ogg");
        assert!(extract_video_info(&media).is_none());
    }

    #[test]
    fn extract_audio_info_voice() {
        let media = audio_doc(true, 7, 1000, "audio/ogg");
        let info = extract_audio_info(&media).expect("audio info present");
        assert_eq!(info.kind, AudioKind::Voice);
        assert_eq!(info.duration_seconds, 7);
        assert_eq!(info.file_size_bytes, 1000);
        assert_eq!(info.mime_type.as_deref(), Some("audio/ogg"));
    }

    #[test]
    fn extract_audio_info_music() {
        let media = audio_doc(false, 200, 4_000_000, "audio/mpeg");
        let info = extract_audio_info(&media).expect("audio info present");
        assert_eq!(info.kind, AudioKind::Audio);
    }

    #[test]
    fn extract_audio_info_none_for_video() {
        let media = video_doc(false, 30.0, 1920, 1080, 5_000_000, "video/mp4", true);
        assert!(extract_audio_info(&media).is_none());
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p telegram-mcp extract_video_info extract_audio_info 2>&1 | head -30`
Expected: FAIL — compile error, `cannot find function extract_video_info` / `extract_audio_info`.

- [ ] **Step 4: Implement the extractors**

In `src/telegram/converters.rs`, immediately after `extract_audio_duration` (after line 111, before the `matches_media_filter` doc comment), insert:

```rust
/// Derive `VideoInfo` from a video-class media's document attributes. Returns
/// `None` for non-video media. Reads raw TL attributes because the high-level
/// grammers `Document` API does not expose video duration / pixel dimensions.
/// `image/gif` animations may carry no `Video` attribute, in which case
/// duration/width/height stay `0` (design decision). No network calls.
pub fn extract_video_info(media: &Media) -> Option<VideoInfo> {
    let kind = match convert_media_to_type(media) {
        MediaType::Video => VideoKind::Video,
        MediaType::VideoNote => VideoKind::VideoNote,
        MediaType::Animation => VideoKind::Animation,
        _ => return None,
    };
    let Media::Document(doc) = media else {
        return None;
    };
    let Some(tl::enums::Document::Document(raw)) = doc.raw.document.as_ref() else {
        return None;
    };

    let mut duration_seconds = 0;
    let mut width = 0;
    let mut height = 0;
    for attr in &raw.attributes {
        if let tl::enums::DocumentAttribute::Video(v) = attr {
            duration_seconds = v.duration.max(0.0) as u32;
            width = v.w.max(0) as u32;
            height = v.h.max(0) as u32;
            break;
        }
    }

    Some(VideoInfo {
        duration_seconds,
        width,
        height,
        file_size_bytes: raw.size.max(0) as u64,
        kind,
        has_thumbnail: raw.thumbs.as_ref().is_some_and(|t| !t.is_empty()),
        mime_type: Some(raw.mime_type.clone()),
    })
}

/// Derive `AudioInfo` from an audio-class media's document attributes. Returns
/// `None` for non-audio media. Same zero-cost raw-TL source as
/// [`extract_video_info`].
pub fn extract_audio_info(media: &Media) -> Option<AudioInfo> {
    let kind = match convert_media_to_type(media) {
        MediaType::Audio => AudioKind::Audio,
        MediaType::Voice => AudioKind::Voice,
        _ => return None,
    };
    let Media::Document(doc) = media else {
        return None;
    };
    let Some(tl::enums::Document::Document(raw)) = doc.raw.document.as_ref() else {
        return None;
    };

    let mut duration_seconds = 0;
    for attr in &raw.attributes {
        if let tl::enums::DocumentAttribute::Audio(a) = attr {
            duration_seconds = a.duration.max(0) as u32;
            break;
        }
    }

    Some(AudioInfo {
        duration_seconds,
        file_size_bytes: raw.size.max(0) as u64,
        kind,
        mime_type: Some(raw.mime_type.clone()),
    })
}
```

- [ ] **Step 5: Run tests to verify they pass + lint**

Run: `cargo test -p telegram-mcp extract_video_info extract_audio_info 2>&1 | tail -20`
Expected: PASS (8 tests).
Run: `cargo fmt --all && cargo clippy -- -D warnings 2>&1 | tail -5`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/telegram/converters.rs
git commit -m "feat: extract video/audio metadata from document attributes"
```

---

### Task 3: Wire `video_info` / `audio_info` into the domain `Message`

**Files:**
- Modify: `src/telegram/types/entities.rs` (struct + import + `create_test_message` test fixture + new test)
- Modify: `src/telegram/converters.rs:359` (populate in `convert_message`)
- Modify: `src/test_helpers.rs:25` (`create_test_message` fixture)
- Modify: `src/mcp/tools/types/responses.rs:337,373` (two test `Message` literals)
- Modify: `src/mcp/tests/history.rs:17` (test fixture)
- Modify: `src/mcp/tests/search.rs:21,240,347` (three test literals)
- Modify: `src/telegram/tests/client_tests.rs:27` (test fixture)

**Interfaces:**
- Consumes: `VideoInfo`/`AudioInfo` (Task 1), `extract_video_info`/`extract_audio_info` (Task 2).
- Produces: domain `Message` gains `pub video_info: Option<VideoInfo>` and `pub audio_info: Option<AudioInfo>` (both `#[serde(skip_serializing_if = "Option::is_none", default)]`). `convert_message` populates them.

> **Why this task touches so many files:** adding two fields to `Message` breaks every `Message { ... }` struct literal (the compiler requires all fields). All literals below already end with `reply_to_message_id: None,` followed by the closing `}` — the universal fixture edit is to insert two lines after `reply_to_message_id: None,`. Only `convert_message` (production) uses the computed values instead of `None`.

- [ ] **Step 1: Write the failing test**

In `src/telegram/types/entities.rs`, inside `#[cfg(test)] mod tests`, add a new test after `message_includes_new_fields_when_present` (after line 200, before `channel_serialization`):

```rust
    #[test]
    fn message_includes_video_and_audio_info_when_present() {
        use super::super::media::{AudioInfo, AudioKind, VideoInfo, VideoKind};

        let mut msg = create_test_message();
        msg.has_media = true;
        msg.media_type = MediaType::Video;
        msg.video_info = Some(VideoInfo {
            duration_seconds: 42,
            width: 1280,
            height: 720,
            file_size_bytes: 9_000_000,
            kind: VideoKind::Video,
            has_thumbnail: true,
            mime_type: Some("video/mp4".to_string()),
        });
        msg.audio_info = Some(AudioInfo {
            duration_seconds: 8,
            file_size_bytes: 4096,
            kind: AudioKind::Voice,
            mime_type: None,
        });

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["video_info"]["kind"], "video");
        assert_eq!(json["video_info"]["duration_seconds"], 42);
        assert_eq!(json["video_info"]["has_thumbnail"], true);
        assert_eq!(json["audio_info"]["kind"], "voice");
        assert!(json["audio_info"].get("mime_type").is_none());
    }
```

Also extend `message_omits_new_fields_when_absent` (lines 161-169) by adding two assertions before its closing `}`:

```rust
        assert!(json.get("video_info").is_none());
        assert!(json.get("audio_info").is_none());
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p telegram-mcp message_includes_video_and_audio_info_when_present 2>&1 | head -30`
Expected: FAIL — compile error, `no field video_info on type Message` (struct + `create_test_message` fixture don't have the field yet).

- [ ] **Step 3: Add the fields to the `Message` struct**

In `src/telegram/types/entities.rs`, change the import at line 4 from:

```rust
use super::media::MediaType;
```

to:

```rust
use super::media::{AudioInfo, MediaType, VideoInfo};
```

Then add two fields to the `Message` struct, immediately after `reply_to_message_id` (after line 32, before the closing `}` at line 33):

```rust
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub video_info: Option<VideoInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub audio_info: Option<AudioInfo>,
```

- [ ] **Step 4: Update every `Message { ... }` literal to compile**

Insert the following two lines immediately after `reply_to_message_id: None,` in each of these test fixtures / literals:

```rust
            video_info: None,
            audio_info: None,
```

(match the surrounding indentation at each site)

Sites:
- `src/telegram/types/entities.rs:117` (in the `create_test_message` test fixture)
- `src/test_helpers.rs:40`
- `src/mcp/tools/types/responses.rs:352` (in `message_response_maps_and_omits_absent_fields`)
- `src/mcp/tools/types/responses.rs:388` (in `search_response_maps_from_search_result`)
- `src/mcp/tests/history.rs:32`
- `src/mcp/tests/search.rs:36`
- `src/mcp/tests/search.rs` (the literal starting at line 240 — after its `reply_to_message_id: None,`)
- `src/mcp/tests/search.rs` (the literal starting at line 347 — after its `reply_to_message_id: None,`)
- `src/telegram/tests/client_tests.rs:42`

> Line numbers for the second/third search.rs literals will shift after earlier inserts. To be safe, after editing, run `grep -rn "reply_to_message_id: None," src/ | grep -v video_info` is **not** reliable; instead rely on the compiler: `cargo build --tests` will name every remaining `Message` literal missing the fields. Fix each until it builds.

- [ ] **Step 5: Populate the fields in `convert_message` (production)**

In `src/telegram/converters.rs`, in `convert_message`, after the `link_preview` block (after line 347, before the `forwarded_from` block) add:

```rust
    // Zero-cost media metadata derived from the in-hand document attributes.
    let video_info = media.as_ref().and_then(extract_video_info);
    let audio_info = media.as_ref().and_then(extract_audio_info);
```

Then in the `Some(Message { ... })` literal (lines 359-375), add after `reply_to_message_id,` (line 374):

```rust
        video_info,
        audio_info,
```

- [ ] **Step 6: Run tests + build to verify everything passes**

Run: `cargo build --tests 2>&1 | tail -20`
Expected: builds clean (no missing-field errors).
Run: `cargo test -p telegram-mcp message_includes_video_and_audio_info_when_present message_omits_new_fields_when_absent 2>&1 | tail -20`
Expected: PASS.
Run: `cargo fmt --all && cargo clippy -- -D warnings 2>&1 | tail -5`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src/telegram/types/entities.rs src/telegram/converters.rs src/test_helpers.rs \
  src/mcp/tools/types/responses.rs src/mcp/tests/history.rs src/mcp/tests/search.rs \
  src/telegram/tests/client_tests.rs
git commit -m "feat: attach video_info/audio_info to domain Message in convert_message"
```

---

### Task 4: Map `video_info` / `audio_info` through `MessageResponse`

**Files:**
- Modify: `src/mcp/tools/types/responses.rs` (import line 3-6; `MessageResponse` struct ~line 210; `From<Message>` impl ~line 230; new test)

**Interfaces:**
- Consumes: domain `Message.video_info` / `Message.audio_info` (Task 3), `VideoInfo`/`AudioInfo` (Task 1).
- Produces: `MessageResponse` gains `pub video_info: Option<VideoInfo>` / `pub audio_info: Option<AudioInfo>` mapped through `From<Message>`. `search_messages` / `get_recent_messages` / `get_message_by_link` pick them up automatically.

- [ ] **Step 1: Write the failing test**

In `src/mcp/tools/types/responses.rs`, inside `#[cfg(test)] mod tests`, add after `message_response_maps_and_omits_absent_fields` (after line 363):

```rust
    #[test]
    fn message_response_maps_video_info() {
        use crate::telegram::types::{
            ChannelId, ChannelName, MediaType, Message, MessageId, Username, VideoInfo, VideoKind,
        };

        let msg = Message {
            id: MessageId::new(1).unwrap(),
            channel_id: ChannelId::new(100).unwrap(),
            channel_name: ChannelName::new("Test").unwrap(),
            channel_username: Username::new("testchan").unwrap(),
            text: String::new(),
            timestamp: chrono::Utc::now(),
            sender_id: None,
            sender_name: None,
            has_media: true,
            media_type: MediaType::Video,
            forwarded_from: None,
            link_preview: None,
            views: None,
            forwards: None,
            reply_to_message_id: None,
            video_info: Some(VideoInfo {
                duration_seconds: 30,
                width: 1920,
                height: 1080,
                file_size_bytes: 5_000_000,
                kind: VideoKind::Video,
                has_thumbnail: true,
                mime_type: Some("video/mp4".to_string()),
            }),
            audio_info: None,
        };

        let dto = MessageResponse::from(msg);
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["video_info"]["kind"], "video");
        assert_eq!(json["video_info"]["width"], 1920);
        assert!(json.get("audio_info").is_none());
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p telegram-mcp message_response_maps_video_info 2>&1 | head -30`
Expected: FAIL — compile error: `Message` has fields `video_info`/`audio_info` not present in `MessageResponse` construction inside `From<Message>` (the literal in the test sets them, but `MessageResponse` has no such field yet → also the `From` impl won't map them).

- [ ] **Step 3: Add fields + import + mapping**

In `src/mcp/tools/types/responses.rs`, change the import block at lines 3-6 from:

```rust
use crate::telegram::types::{
    Channel, ChannelId, ChannelName, ForwardInfo, LinkPreview, MediaType, Message, MessageId,
    QueryMetadata, SearchResult, UserId, Username,
};
```

to:

```rust
use crate::telegram::types::{
    AudioInfo, Channel, ChannelId, ChannelName, ForwardInfo, LinkPreview, MediaType, Message,
    MessageId, QueryMetadata, SearchResult, UserId, Username, VideoInfo,
};
```

Add two fields to `MessageResponse`, after `reply_to_message_id` (after line 210, before the closing `}` at line 211):

```rust
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub video_info: Option<VideoInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub audio_info: Option<AudioInfo>,
```

In the `From<Message> for MessageResponse` impl, after `reply_to_message_id: m.reply_to_message_id,` (line 230, before the closing `}`):

```rust
            video_info: m.video_info,
            audio_info: m.audio_info,
```

- [ ] **Step 4: Run the test to verify it passes + lint**

Run: `cargo test -p telegram-mcp message_response_maps_video_info 2>&1 | tail -20`
Expected: PASS.
Run: `cargo fmt --all && cargo clippy -- -D warnings 2>&1 | tail -5`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/mcp/tools/types/responses.rs
git commit -m "feat: surface video_info/audio_info in MessageResponse"
```

---

### Task 5: Part B — add `video_info` to `get_message_media` metadata

**Files:**
- Modify: `src/telegram/types/media.rs` (`MediaDownload` struct + `media_download_construction` test)
- Modify: `src/telegram/client.rs` (converters import line 5-8; `MediaDownload` literal line 824)
- Modify: `src/mcp/tools/types/responses.rs` (`GetMessageMediaResponse` struct ~line 182; `get_message_media_response_serializes` test literal ~line 322)
- Modify: `src/mcp/server.rs:488` (`get_message_media_impl` metadata construction)
- Modify: `src/mcp/tests/media.rs` (three `MediaDownload` literals: helper line 20, line 95, line 278; new test)
- Modify: `src/telegram/tests/client_tests.rs:561` (`MediaDownload` literal)

**Interfaces:**
- Consumes: `VideoInfo` (Task 1), `extract_video_info` (Task 2).
- Produces: `MediaDownload` gains `pub video_info: Option<VideoInfo>`; `GetMessageMediaResponse` gains `pub video_info: Option<VideoInfo>` (`skip_serializing_if`); `get_message_media_impl` passes `download.video_info` through.

> Adding a field to `MediaDownload` breaks all six construction sites — update them all in this task. The production site (`client.rs:824`) sets `video_info: extract_video_info(&media)`; every test site sets `video_info: None` except the new assertion test.

- [ ] **Step 1: Write the failing test**

In `src/mcp/tests/media.rs`, add after `video_thumbnail_sets_is_thumbnail` (after line 125):

```rust
#[tokio::test]
async fn video_metadata_included_in_response() {
    use crate::telegram::types::{VideoInfo, VideoKind};

    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_download_message_media()
        .return_once(|_, _, _| {
            let bytes = create_test_jpeg(320, 180);
            let source_size_bytes = bytes.len() as u64;
            Ok(MediaDownload {
                bytes,
                media_type: MediaType::Video,
                is_thumbnail: true,
                caption: None,
                width: Some(320),
                height: Some(180),
                source_size_bytes,
                video_info: Some(VideoInfo {
                    duration_seconds: 30,
                    width: 1920,
                    height: 1080,
                    file_size_bytes: 5_000_000,
                    kind: VideoKind::Video,
                    has_thumbnail: true,
                    mime_type: Some("video/mp4".to_string()),
                }),
            })
        });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 7, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let call_result = result.expect("tool should succeed");
    let RawContent::Text(text) = &call_result.content[1].raw else {
        panic!("second content block must be text");
    };
    let metadata: GetMessageMediaResponse = serde_json::from_str(&text.text).unwrap();
    let vi = metadata.video_info.expect("video_info present in metadata");
    assert_eq!(vi.kind, VideoKind::Video);
    assert_eq!(vi.duration_seconds, 30);
    assert_eq!(vi.width, 1920);
    assert!(vi.has_thumbnail);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p telegram-mcp video_metadata_included_in_response 2>&1 | head -30`
Expected: FAIL — compile error: `MediaDownload` has no field `video_info` / `GetMessageMediaResponse` has no field `video_info`.

- [ ] **Step 3: Add `video_info` to `MediaDownload`**

In `src/telegram/types/media.rs`, add a field to `MediaDownload`, after `source_size_bytes: u64,` (after line 78, before the closing `}` at line 79):

```rust
    /// Zero-cost video metadata for video-class media (`None` for photos).
    pub video_info: Option<VideoInfo>,
}
```

(i.e. insert the field; `VideoInfo` is defined in this same file, no import needed.)

Then update the unit-test literal `media_download_construction` (lines 190-198): add after `source_size_bytes: 2,`:

```rust
            video_info: None,
```

- [ ] **Step 4: Set `video_info` in the production download path**

In `src/telegram/client.rs`, change the converters import (lines 5-8) to add `extract_video_info`:

```rust
use crate::telegram::converters::{
    convert_media_filter, convert_media_to_type, convert_message, convert_peer_to_channel,
    extract_audio_duration, extract_video_info, matches_media_filter, select_size_candidate,
    size_candidates,
};
```

Then in `download_message_media`, in the returned `MediaDownload` literal (lines 824-832), add after `source_size_bytes: selected.size_bytes,`:

```rust
            video_info: extract_video_info(&media),
```

(`media` is in scope — it was bound at line 735 and is not moved.)

- [ ] **Step 5: Add `video_info` to `GetMessageMediaResponse` and wire it through**

In `src/mcp/tools/types/responses.rs`, add a field to `GetMessageMediaResponse`, after `mime_type` (after line 181, before the closing `}` at line 182):

```rust
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[schemars(description = "Zero-cost video metadata (duration, dimensions, kind); video media only")]
    pub video_info: Option<VideoInfo>,
```

(`VideoInfo` is already imported into this file by Task 4.)

Update the `get_message_media_response_serializes` test literal (lines 310-323): add after `mime_type: "image/jpeg".to_string(),`:

```rust
            video_info: None,
```

In `src/mcp/server.rs`, in `get_message_media_impl`, in the `GetMessageMediaResponse { ... }` literal (lines 488-501), add after `mime_type: "image/jpeg".to_string(),`:

```rust
            video_info: download.video_info,
```

- [ ] **Step 6: Update remaining `MediaDownload` test literals to compile**

Add `video_info: None,` after `source_size_bytes` in each remaining literal:
- `src/mcp/tests/media.rs:27` (`photo_download` helper — after `source_size_bytes,`)
- `src/mcp/tests/media.rs:102` (`video_thumbnail_sets_is_thumbnail` — after `source_size_bytes,`)
- `src/mcp/tests/media.rs:285` (`corrupt_image_bytes_return_decode_error` — after `source_size_bytes: 32,`)
- `src/telegram/tests/client_tests.rs:568` (after `source_size_bytes: 3,`)

> Rely on `cargo build --tests` to name any literal still missing the field.

- [ ] **Step 7: Run tests + build to verify everything passes**

Run: `cargo build --tests 2>&1 | tail -20`
Expected: builds clean.
Run: `cargo test -p telegram-mcp video_metadata_included_in_response 2>&1 | tail -20`
Expected: PASS.
Run: `cargo test 2>&1 | tail -15`
Expected: full suite passes.
Run: `cargo fmt --all && cargo clippy -- -D warnings 2>&1 | tail -5`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add src/telegram/types/media.rs src/telegram/client.rs src/mcp/tools/types/responses.rs \
  src/mcp/server.rs src/mcp/tests/media.rs src/telegram/tests/client_tests.rs
git commit -m "feat: include video_info in get_message_media metadata"
```

---

### Task 6: Documentation (README + CHANGELOG)

**Files:**
- Modify: `README.md` (search response example ~line 410-447; the enrichment note ~line 449-458; the `get_message_media` metadata example ~line 552-568)
- Modify: `CHANGELOG.md` (`[Unreleased]` section, line 7)

**Interfaces:** None (docs only). No test cycle; the deliverable is reviewed prose. Verify with `cargo fmt --check && cargo clippy -- -D warnings && cargo test` (unchanged from prior task) so docs land on a green tree.

- [ ] **Step 1: Add `video_info` / `audio_info` to the search response example**

In `README.md`, in the `search_messages` JSON response example, add a `video_info` block to the example message. After `"reply_to_message_id": 41` (line 436), change it to add the new objects (insert a comma after `41`):

```json
      "reply_to_message_id": 41,
      "video_info": {
        "duration_seconds": 95,
        "width": 1280,
        "height": 720,
        "file_size_bytes": 12485760,
        "kind": "video",
        "has_thumbnail": true,
        "mime_type": "video/mp4"
      },
      "audio_info": {
        "duration_seconds": 8,
        "file_size_bytes": 41200,
        "kind": "voice",
        "mime_type": "audio/ogg"
      }
```

- [ ] **Step 2: Extend the enrichment explanatory note**

In `README.md`, at the end of the "Forward attribution & link previews" paragraph (after line 458, "...so existing consumers are unaffected."), add a new paragraph:

```markdown

**Video & audio metadata:** Messages with video-class media carry an optional
`video_info` object — `duration_seconds`, `width`, `height`, `file_size_bytes`,
`kind` (`video` | `video_note` | `animation`), `has_thumbnail`, and `mime_type` —
and audio-class media carry an optional `audio_info` object (`duration_seconds`,
`file_size_bytes`, `kind` (`audio` | `voice`), `mime_type`). Both are derived from
the message's document attributes with **no extra API calls** (the full video is
never downloaded), so the client can judge a clip's length and shape — and whether
fetching its thumbnail via `get_message_media`, or transcribing a voice message, is
worthwhile — before spending a request. Rare GIF-class animations without a video
attribute report `duration_seconds`/`width`/`height` as `0`. Both objects are
omitted when the message has no video/audio media.
```

- [ ] **Step 3: Add `video_info` to the `get_message_media` metadata example**

In `README.md`, in the `### 10. get_message_media` metadata example (lines 553-568), change `"mime_type": "image/jpeg"` (line 566) to add a trailing `video_info` (for a video/thumbnail message):

```json
  "mime_type": "image/jpeg",
  "video_info": {
    "duration_seconds": 95,
    "width": 1280,
    "height": 720,
    "file_size_bytes": 12485760,
    "kind": "video",
    "has_thumbnail": true,
    "mime_type": "video/mp4"
  }
```

And update the "What it returns" bullet for videos (line 534) to mention `video_info`:

```markdown
- **Videos, animations, video notes:** only the server-side thumbnail is available; it is returned as an image block with `is_thumbnail: true` and a `video_info` object (duration, dimensions, kind) in the metadata.
```

- [ ] **Step 4: Add the CHANGELOG entry**

In `CHANGELOG.md`, under `## [Unreleased]` (line 7), add:

```markdown

### Added
- `search_messages`, `get_recent_messages`, and `get_message_by_link` now enrich messages with optional, zero-extra-API-call media metadata: `video_info` for video-class media (`duration_seconds`, `width`, `height`, `file_size_bytes`, `kind` — `video`/`video_note`/`animation` —, `has_thumbnail`, `mime_type`) and `audio_info` for audio-class media (`duration_seconds`, `file_size_bytes`, `kind` — `audio`/`voice` —, `mime_type`). Both are derived from the message's document attributes — the full video/audio is never downloaded — and are omitted when absent, so existing consumers are unaffected. `get_message_media` now also includes `video_info` in its metadata block.
```

- [ ] **Step 5: Verify the tree is green**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test 2>&1 | tail -15`
Expected: all pass (docs changes don't affect compilation; this confirms nothing regressed).

- [ ] **Step 6: Commit**

```bash
git add README.md CHANGELOG.md
git commit -m "docs: document video_info/audio_info enrichment"
```

---

## Self-Review

**Spec coverage** (against `docs/superpowers/specs/2026-06-20-video-metadata-design.md` and `docs/features/4-video-metadata.md`):

| Spec requirement | Task |
|---|---|
| `VideoInfo` (duration/width/height/file_size_bytes/kind/has_thumbnail/mime_type) | Task 1 |
| `AudioInfo` (duration/file_size_bytes/kind/mime_type) | Task 1 |
| Dedicated `VideoKind` / `AudioKind` closed enums | Task 1 |
| Extraction in `converters.rs`, zero API calls, raw-TL read with code comment | Task 2 |
| Missing dimensions default to 0 (gif animation) | Task 2 (`extract_video_info_gif_is_animation_with_zero_dims`) |
| `kind` derived from `convert_media_to_type` (no drift) | Task 2 |
| Domain `Message` gains both fields; `convert_message` populates | Task 3 |
| `MessageResponse` gains both fields, `From<Message>` maps them | Task 4 |
| `search_messages` / `get_recent_messages` pick up automatically | Task 4 (via `MessageResponse`/`SearchResponse`) |
| Test fixtures default both to `None` | Task 3 |
| Part B: `MediaDownload`, `client.rs`, `GetMessageMediaResponse`, `get_message_media_impl` | Task 5 |
| Update all `MediaDownload` constructors | Task 5 (6 sites) |
| No rate-limiter change | (no task — intentionally unchanged) |
| Tests: regular video, video_note, animation, no-thumbnail, voice, music, plain-text-absent, media metadata carries video_info | Tasks 2, 3, 5 |
| README + CHANGELOG | Task 6 |
| Backward compatible (skip_serializing_if) | Tasks 1, 3, 4, 5 |

**Non-goals honored:** no full video download (Part B still only downloads the thumbnail, unchanged); no ffmpeg/frame extraction/transcription added.

**Type consistency:** `VideoInfo`/`AudioInfo`/`VideoKind`/`AudioKind` field/variant names are identical across Tasks 1, 2, 3, 4, 5. `extract_video_info(&Media) -> Option<VideoInfo>` and `extract_audio_info(&Media) -> Option<AudioInfo>` signatures used identically in Tasks 2, 3, 5. Every `MediaDownload`/`Message`/`MessageResponse`/`GetMessageMediaResponse` literal site is enumerated.

**No placeholders:** every code step contains complete code; fixture-update steps give the exact two-line insertion plus the exact file:line list, with `cargo build --tests` as the backstop for line drift after inserts.
