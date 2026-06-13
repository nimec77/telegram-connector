# Message metadata enrichment — Design

**Date:** 2026-06-13
**Status:** APPROVED
**Source feature request:** `docs/features/2-link-preview.md`

## Problem

The domain `Message` returned by `search_messages` and `get_recent_messages` drops
server-side metadata that grammers already attaches to every message — no extra
network requests are needed to surface it. Four signals are lost:

1. **Forward attribution.** When channel A forwards a post from channel B, the
   result reads as channel A's original content, breaking deduplication and
   source-credibility attribution in news-digest workflows.
2. **Link-preview content.** Many channel posts are just a URL; the substance lives
   in Telegram's server-side webpage preview (title + description), already attached
   to the message as pure text.
3. **Engagement signals.** `views` and `forwards` counts are cheap significance
   signals for ranking stories.
4. **Reply/thread anchoring.** The parent message ID for reply/comment threads.

This is a pure enrichment: no new tools, no schema changes to other tools, no media
download, no fetching of the linked webpage itself, and **zero additional API calls**.

## Decisions resolved during brainstorming

1. **Forwarded channel name/username are omitted, not resolved.** The grammers
   forward header (`MessageFwdHeader`) carries only `from_id` (a TL `Peer` = a bare
   numeric ID) and `from_name` (a display string for hidden senders). The forwarded
   channel's *title* and *username* live in the response's per-message peer map,
   which grammers keeps `pub(crate)` and does not expose; `message.raw` does not
   contain them either. Resolving them would require an extra `resolve` API call,
   which the zero-extra-call constraint forbids. **`channel_name` and
   `channel_username` remain in the schema but are always omitted for now.**
   `channel_id` + the existing `generate_message_link` tool still let the client
   reach the source.

2. **Separate response-DTO layer.** The new fields reach the wire through a
   `MessageResponse` DTO (and a `SearchResponse` wrapper) in
   `src/mcp/tools/types/responses.rs`, mapped from the domain types via `From`
   impls — rather than widening only the domain `Message` and serializing it
   directly. Domain `Message` already derives `Serialize`, so the DTO is primarily a
   layering boundary; this is the accepted cost of the DTO choice.

## Architecture

```
grammers Message ─convert_message()─► domain Message ─From─► MessageResponse ─serde─► JSON
   (telegram layer:                     (entities.rs)        (responses.rs)      (server.rs
    converters.rs)                                                                 tools)
```

- **Domain** — `src/telegram/types/entities.rs`: new `ForwardInfo` and `LinkPreview`
  structs; `Message` gains five optional fields. All derive
  `Debug, Clone, Serialize, Deserialize, JsonSchema`, consistent with `Message` and
  `Channel`.
- **Extraction** — `src/telegram/converters.rs`: two pure helpers plus wiring into
  `convert_message` (the single shared conversion path).
- **DTO** — `src/mcp/tools/types/responses.rs`: `MessageResponse` (mirrors `Message`),
  `SearchResponse` (mirrors `SearchResult`, reusing domain `QueryMetadata`,
  `ForwardInfo`, `LinkPreview` directly), with `From` mapping impls.
- **Tools** — `src/mcp/server.rs`: the result-emitting tools map domain → DTO before
  `serde_json::to_string`.

## Domain types (entities.rs)

```rust
/// Attribution for a forwarded message.
pub struct ForwardInfo {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub channel_id: Option<ChannelId>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub channel_name: Option<ChannelName>,      // always None for now (see decision 1)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub channel_username: Option<Username>,     // always None for now (see decision 1)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sender_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub original_date: Option<DateTime<Utc>>,   // serializes RFC 3339, like `timestamp`
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub original_message_id: Option<MessageId>,
}

/// Telegram's server-side webpage preview.
pub struct LinkPreview {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub site_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,            // truncated to 500 chars
}
```

New `Message` fields (appended; existing fields and JSON names unchanged):

```rust
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub forwarded_from: Option<ForwardInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub link_preview: Option<LinkPreview>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub views: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub forwards: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reply_to_message_id: Option<MessageId>,
```

The newtypes (`ChannelId`, `MessageId`, `ChannelName`, `Username`) are
`#[serde(transparent)]`, so they serialize as bare scalars matching the feature
request's `i64`/`string` wire spec.

## Extraction logic (converters.rs)

Two pure helpers — the testable seam. A grammers `Message` cannot be constructed in
a unit test without a live `Client`, but the raw TL types are public and
hand-constructible, so the helpers take those:

```rust
fn extract_forward_info(header: &tl::types::MessageFwdHeader) -> Option<ForwardInfo>;
fn extract_link_preview(media: &tl::types::MessageMediaWebPage) -> Option<LinkPreview>;
```

`convert_message` wires them in:

- **Forward** — `msg.forward_header()` → `Option<tl::enums::MessageFwdHeader>`; unwrap
  the enum to its inner `tl::types::MessageFwdHeader`; call `extract_forward_info`.
  - `channel_id` ← `header.from_id` when it is `Peer::Channel(PeerChannel)` →
    `ChannelId::new(channel_id).ok()`. `Peer::User`/`Peer::Chat` → `None`.
  - `sender_name` ← `header.from_name`.
  - `original_date` ← `header.date` (i32 unix) → `DateTime::from_timestamp(date, 0)`.
  - `original_message_id` ← `header.channel_post` → `MessageId::new(x as i64).ok()`.
  - `channel_name` / `channel_username` ← always `None` (decision 1).
  - Returns `None` only if the header yields no populated field.
  - **Code comment** notes the raw-TL drop-down: the title/username are not present on
    `from_id`, which is an ID-only `Peer`.
- **Link preview** — `msg.media()` → `Some(Media::WebPage(wp))` → `&wp.raw`
  (`tl::types::MessageMediaWebPage`) → `extract_link_preview`. Inside, match
  `media.webpage` (`tl::enums::WebPage`): only the `Page(tl::types::WebPage)` variant
  carries data; `Empty`/`Pending`/`NotModified` → `None`.
  - `url` ← `webpage.url` (required).
  - `site_name` / `title` ← `webpage.site_name` / `webpage.title`.
  - `description` ← `webpage.description`, truncated to 500 **chars**
    (`s.chars().take(500).collect()` — Unicode scalar values, so Cyrillic text is
    not split mid-codepoint).
  - `media_type` stays `MediaType::None` for a webpage (unchanged existing behavior —
    a webpage preview is not downloadable media).
- **Counts** (high-level accessors, no TL drop-down needed):
  - `views` ← `msg.view_count()` (`Option<i32>`), negatives dropped → `u64`.
  - `forwards` ← `msg.forward_count()` (`Option<i32>`), negatives dropped → `u64`.
  - `reply_to_message_id` ← `msg.reply_to_message_id()` (`Option<i32>`) →
    `MessageId::new(x as i64).ok()`.

**Zero-API-call guarantee holds by construction**: no helper and no new code path
takes a `Client` or performs a request/download.

## DTO layer (responses.rs)

```rust
pub struct MessageResponse {
    // mirrors every Message field, reusing domain newtypes / MediaType /
    // ForwardInfo / LinkPreview, with the same serde attributes.
}
impl From<Message> for MessageResponse { /* field-by-field move */ }

pub struct SearchResponse {
    pub messages: Vec<MessageResponse>,
    pub total_found: u64,
    pub search_time_ms: u64,
    pub query_metadata: QueryMetadata,   // domain type reused directly
}
impl From<SearchResult> for SearchResponse { /* map messages, move the rest */ }
```

Both derive `Debug, Clone, Serialize, Deserialize, JsonSchema`.

## Tool changes (server.rs)

- `search_messages_impl` — serialize `SearchResponse::from(result)` instead of the
  domain `SearchResult`.
- `get_recent_messages_impl` — same.
- `get_message_by_link_impl` — serialize `MessageResponse::from(message)`. This tool
  shares `convert_message`, so its domain `Message` is enriched regardless; routing
  it through the DTO keeps the wire representation consistent. Minor, same path.

## Backward compatibility

- Existing JSON keys and types are unchanged; new fields are appended and all use
  `skip_serializing_if = "Option::is_none"`, so a plain message serializes
  byte-identically to today.
- Existing tests that do `serde_json::from_str::<SearchResult>(&actual)` keep
  passing: existing keys match the DTO output, serde ignores unknown keys by default,
  and plain-message fixtures emit no new keys anyway.

## Testing

- **Extraction unit tests** (`converters.rs`, hand-built TL structs, no `Client` →
  zero-call by construction):
  - forward from a channel → `channel_id`, `original_message_id`, `original_date`
    populated; `channel_name`/`channel_username` `None`.
  - forward from a hidden user → `sender_name` only; `channel_id` `None`.
  - link preview → `url`/`site_name`/`title`/`description`; a 600-char Cyrillic
    description truncates to exactly 500 `chars().count()`.
  - `WebPage::Empty` → `None`.
- **Serialization tests** (`entities.rs` / `responses.rs`):
  - plain message → none of the new keys appear in the JSON.
  - `views`/`forwards` populated vs absent.
  - `MessageResponse` / `SearchResponse` round-trip.
- **test_helpers** (`src/test_helpers.rs` and the inline fixture in `entities.rs`
  tests): default the five new fields to `None`; add
  `create_test_message_with_forward(...)` and
  `create_test_message_with_link_preview(...)` builders.
- Existing mock-based search/history tests already assert the client mock is invoked
  without any download call, covering the zero-extra-cost property at the tool layer.

## Documentation

- **README.md** — extend the `search_messages` and `get_recent_messages` response
  examples with the new optional fields; add a short "Forward attribution & link
  previews" note that states `channel_name`/`channel_username` are omitted by design
  (zero extra API calls).
- **CHANGELOG.md** — `### Added` entry under `[Unreleased]`.

## Non-goals

No media download, no fetching of the linked webpage, no reactions/comments
retrieval, no resolve calls for forwarded channel names, no schema changes to other
tools.
