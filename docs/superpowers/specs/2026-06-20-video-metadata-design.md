# Design: Video & Audio Metadata Enrichment

**Date:** 2026-06-20
**Feature spec:** `docs/features/4-video-metadata.md`
**Status:** APPROVED

## Goal

Let the MCP client (Claude) describe a video honestly — length, shape, kind —
and decide whether fetching its thumbnail is worthwhile, **without ever
downloading the full video**. Enrich `search_messages` / `get_recent_messages`
message responses with an optional `video_info` object (and the analogous
`audio_info` for audio-class media), and surface `video_info` in the existing
`get_message_media` tool's metadata block.

All new data is derived from grammers document attributes already present on the
in-hand `Message` — **zero extra API calls**.

## Scope decisions (resolved during brainstorming)

1. **Missing dimensions default to 0.** Video-class media that lacks a
   `DocumentAttributeVideo` (rare `image/gif` animations) reports
   `duration_seconds`/`width`/`height` as `0`. They stay required `u32` per the
   spec; `file_size_bytes`/`mime_type`/`kind`/`has_thumbnail` remain accurate.
2. **Define once, reuse.** `VideoInfo`/`AudioInfo` live in
   `src/telegram/types/media.rs` with `serde` + `schemars` derives, are added to
   the domain `Message`, and are referenced directly from `MessageResponse` /
   `GetMessageMediaResponse` — exactly how `link_preview` / `forwarded_from` work
   today. No parallel DTO structs.

## Current state (verified)

- `get_message_media` (Tool 10) **already exists** and already returns the
  server-side **thumbnail** for `video` / `animation` / `video_note` with
  `is_thumbnail: true` (`src/telegram/client.rs:744-751`; test
  `video_thumbnail_sets_is_thumbnail` in `src/mcp/tests/media.rs`). Part B is
  therefore the "verify + enrich" branch, not a new tool.
- Existing enrichment pattern (`ForwardInfo`, `LinkPreview`) defines a type once
  in the domain layer and reuses it directly as the response DTO field — there is
  **no** separate DTO struct.
- Relevant grammers TL fields (from `grammers-tl-types/tl/api.tl`):
  - `documentAttributeVideo { round_message: flag, duration: double, w: int, h: int, ... }`
  - `documentAttributeAudio { voice: flag, duration: int, ... }`
  - `document { mime_type: string, size: long, thumbs: flags.0?Vector<PhotoSize>, attributes: Vector<DocumentAttribute> }`
- `extract_audio_duration` (`converters.rs:96`) already reads raw TL attributes —
  the new extractors follow the same access path.
- Tests can construct a `Media::Document` from raw TL via
  `Document::from_raw_media(tl::types::MessageMediaDocument { .. })`, so unit
  tests exercise extraction with **zero** mockall download/request expectations.

## 1. New domain types — `src/telegram/types/media.rs`

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioKind {
    Audio,
    Voice,
}
```

Dedicated `VideoKind`/`AudioKind` enums (not reused `MediaType`) keep the
advertised JSON schema to exactly the closed sets the spec names
(`video|video_note|animation`, `audio|voice`).

## 2. Extraction — `src/telegram/converters.rs` (zero API calls)

```rust
pub fn extract_video_info(media: &Media) -> Option<VideoInfo> {
    let kind = match convert_media_to_type(media) {
        MediaType::Video => VideoKind::Video,
        MediaType::VideoNote => VideoKind::VideoNote,
        MediaType::Animation => VideoKind::Animation,
        _ => return None,
    };
    let Media::Document(doc) = media else { return None };
    let Some(tl::enums::Document::Document(raw)) = doc.raw.document.as_ref() else {
        return None;
    };

    // Read raw TL attributes: the high-level grammers Document API does not
    // expose video duration / pixel dimensions. image/gif animations may carry
    // no Video attribute at all, in which case these stay 0 (design decision).
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

pub fn extract_audio_info(media: &Media) -> Option<AudioInfo> {
    let kind = match convert_media_to_type(media) {
        MediaType::Audio => AudioKind::Audio,
        MediaType::Voice => AudioKind::Voice,
        _ => return None,
    };
    let Media::Document(doc) = media else { return None };
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

`kind` is derived from `convert_media_to_type` so it can never drift from
`detect_document_type`'s classification.

## 3. Wiring into messages

- **`convert_message`** computes both from the `media` it already holds:
  ```rust
  let video_info = media.as_ref().and_then(extract_video_info);
  let audio_info = media.as_ref().and_then(extract_audio_info);
  ```
  and adds them to the returned `Message`.
- **Domain `Message`** (`src/telegram/types/entities.rs`) gains:
  ```rust
  #[serde(skip_serializing_if = "Option::is_none", default)]
  pub video_info: Option<VideoInfo>,
  #[serde(skip_serializing_if = "Option::is_none", default)]
  pub audio_info: Option<AudioInfo>,
  ```
- **`MessageResponse`** (`src/mcp/tools/types/responses.rs`) gains the same two
  fields; `From<Message>` maps them through. `search_messages` and
  `get_recent_messages` pick them up automatically.
- Test fixtures updated: `create_test_message()` in `entities.rs` tests and in
  `src/test_helpers.rs` default both to `None`.

## 4. Part B — `get_message_media` enrichment

The tool already downloads video-class thumbnails. Remaining work is only adding
`video_info` to its metadata block:

- **`MediaDownload`** (`media.rs`) gains `pub video_info: Option<VideoInfo>`.
- **`client.rs`** download path sets `video_info: extract_video_info(&media)`
  (the path already has `media` in hand; `None` for photos).
- **`GetMessageMediaResponse`** (`responses.rs`) gains
  `#[serde(skip_serializing_if = "Option::is_none", default)] pub video_info: Option<VideoInfo>`.
- **`get_message_media_impl`** passes `download.video_info` into the response.
- Update all three `MediaDownload` constructors (`client.rs`,
  `src/telegram/tests/client_tests.rs`, `media.rs` test) for the new field.
- **No rate-limiter change.** The tool already charges the configurable
  `media_download_cost`; the spec's "default 2 tokens" clause applies only to the
  new-minimal-tool branch, which does not apply here.

## 5. Tests

**`converters.rs` unit tests** (build `Media::Document` from raw TL, no network,
zero mockall expectations):
- regular video → `kind: video`, correct dims/duration/`file_size_bytes`,
  `has_thumbnail: true`, `mime_type` set
- video note (`round_message: true`) → `kind: video_note`
- animation (`Animated` attribute / `image/gif`) → `kind: animation`
- video without `thumbs` → `has_thumbnail: false`
- voice (`voice: true`) → `AudioInfo { kind: voice }`
- music → `AudioInfo { kind: audio }`

**`entities.rs`**: plain text message → both `video_info` and `audio_info` absent
from serialized JSON.

**`src/mcp/tests/media.rs`**: `get_message_media` metadata block carries
`video_info` for a video message.

## 6. Documentation

- **`README.md`**: response examples gain `video_info` / `audio_info`; note that
  `get_message_media` metadata now includes `video_info`. No new tool-reference
  entry (no new tool created).
- **`CHANGELOG.md`**: entry under `[Unreleased]`.

## Backward compatibility

Purely additive. Existing response fields and JSON names are unchanged; the new
objects are omitted (`skip_serializing_if`) whenever the message has no
video/audio-class media.

## Non-goals (unchanged from feature spec)

- No full video download under any parameter combination.
- No frame extraction, no ffmpeg, no video transcription.
- No streaming or partial-range downloads of the video document itself.
