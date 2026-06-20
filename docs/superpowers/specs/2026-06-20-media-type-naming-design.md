# media_type naming desync — Design

**Date:** 2026-06-20
**Status:** APPROVED
**Reported by:** dev observation (cross-endpoint inconsistency)

## Problem

The same media kind is reported under two different `media_type` strings depending
on which tool produced it:

| Endpoint | How `media_type` is produced | Output for a round video |
|---|---|---|
| `search_messages` (+ `get_message_info`, `get_message_by_link`, history) | serde-serializes `MediaType` directly | `"videonote"` |
| `transcribe_voice_message` | hand-rolled `match` in `src/mcp/server.rs:554` | `"video_note"` |

A client cannot rely on a stable wire name for the same concept.

## Root cause

`MediaType` (`src/telegram/types/media.rs:8`) derives `#[serde(rename_all = "lowercase")]`,
so `VideoNote → "videonote"`. The transcription feature wanted the readable
`"video_note"`, so it bypassed serde with a manual `match` (`server.rs:554-564`,
field typed `String`). That created a **second source of truth** for the name, which
drifted from the serde-derived one.

Supporting facts:

- `VideoNote` is the **only multi-word variant** in `MediaType`. Every other variant
  (`Photo`, `Voice`, `Animation`, …) is a single word, where `lowercase` and
  `snake_case` are byte-identical. Switching the rename rule moves exactly one string.
- `"video_note"` is already the de-facto standard everywhere else: the sibling
  `MediaFilter` enum derives `#[serde(rename_all = "snake_case")]`, the search-filter
  request parser accepts `"video_note"` (`requests.rs:226`), the `NotTranscribable`
  error message says `video_note` (`error.rs:48`), and the transcribe response already
  emits `video_note`.
- The `videonote` outlier is also documented in `README.md:460`.

## Decision

Standardize on **`video_note`** (snake_case) and make the enum's serde derive the
**single source of truth** — no hand-written name literals anywhere.

- **Canonical name:** `video_note`. Confirmed with the user; aligns with `MediaFilter`,
  the request parser, error messages, and the existing transcribe output.
- **Breaking change accepted:** `search_messages` (and the other content-serializing
  tools) change their round-video `media_type` from `"videonote"` to `"video_note"`.
  Any client keying on the old string must update. To be noted in the changelog /
  release notes.
- **No new shared enum.** `MediaType` already is the single source; we only align its
  serialization style. `MediaType` (message-content output) and `MediaFilter`
  (search-filter input) stay separate — different domains, intentionally distinct sets
  of variants.

## Changes

### 1. Core: `MediaType` serialization (single source of truth)

`src/telegram/types/media.rs:8`

```rust
#[serde(rename_all = "lowercase")]   // before
#[serde(rename_all = "snake_case")]  // after
```

Net wire effect: `VideoNote` `"videonote" → "video_note"`; all other variants unchanged.
`MediaType` is output-only (it is never deserialized from client input — `MediaFilter`
is the input type), so changing the accepted/produced string is safe at the request
boundary.

### 2. `transcribe_voice_message`: retire the duplicate (Approach A)

The manual `match` existed only to dodge the old `"videonote"` spelling; with the enum
fixed, the reason is gone.

- `src/mcp/tools/types/responses.rs:61` — `pub media_type: String` → `pub media_type: MediaType`.
- `src/mcp/server.rs:552-564` — delete the string-producing `match`. Keep a guard that
  the transcription outcome is `Voice | VideoNote` (otherwise the existing
  "unexpected media type for transcription" error), then assign the `MediaType` value
  directly. Serde produces the identical `"voice"` / `"video_note"` wire output.
- Update the `#[schemars(description = ...)]` on the field (`responses.rs:60`) to note
  it is a `MediaType` restricted to `voice` / `video_note` in practice.

**Wire output for transcribe is unchanged** (JSON does not distinguish a string field
from a string-valued enum). The advertised JsonSchema becomes *more* precise — a typed
enum rather than a free string. This is the only place a `String` literal duplicated
the enum name; removing it leaves serde as the sole source.

### 3. Tests

- `src/telegram/types/media.rs:106` — rename `media_type_serde_lowercase` →
  `media_type_serde_snake_case`; assert `serde_json::to_string(&MediaType::VideoNote)`
  equals `"\"video_note\""`.
- `src/mcp/tests/transcription.rs:88` — assert `resp.media_type == MediaType::VideoNote`
  (field is now the enum), instead of the `"video_note"` string.
- **New regression test** asserting both endpoints agree on the round-video name — i.e.
  that the `MediaType::VideoNote` serialization used by search and the value returned by
  the transcribe response serialize to the same `"video_note"` string. This locks the
  gap that allowed the drift.

### 4. Docs

- `README.md:460` — `videonote` → `video_note` in the search media-types list.
- `CHANGELOG.md` — record the breaking change to `search_messages` output under the
  appropriate `Changed` heading.

## Out of scope (YAGNI)

- No new umbrella/shared media enum. `MediaType` already centralizes the names.
- No change to `MediaFilter` (already snake_case) beyond confirming alignment.
- No deserialization compatibility shim for the old `"videonote"` string — it is an
  output-only value and the change is intentional.

## Verification

`cargo fmt --check && cargo clippy -- -D warnings && cargo test` must pass. The new
cross-endpoint regression test is the durable guard against re-drift.
