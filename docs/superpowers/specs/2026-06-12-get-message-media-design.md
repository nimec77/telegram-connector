# get_message_media — Design

**Date:** 2026-06-12
**Status:** APPROVED
**Source feature request:** `docs/features/1-photo-retrieval.md`

## Problem

`search_messages` / `get_recent_messages` only report media *metadata* (`has_media`,
`media_type`). In news channels the photo often IS the content (benchmark tables,
charts, maps, document screenshots), and the MCP client is blind to it. MCP natively
supports image content blocks in tool results, so photos can be returned directly
into the model's vision context.

## Tool contract

`get_message_media` — tool #10 (docs currently say 9 tools; this change updates them).

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `channel_id` | string | yes | channel ID or username (flexible coercion like other tools) |
| `message_id` | integer | yes | flexible coercion (`flexible_i64`) |
| `max_dimension` | integer | no | longest side after downscaling; default 1280, clamped to [64, 2048] |

**Success result** — an rmcp `CallToolResult` with two content blocks:

1. `Content::image(base64_jpeg, "image/jpeg")` — the photo (or video thumbnail),
   re-encoded as JPEG quality 80, longest side ≤ `max_dimension`.
2. `Content::text(json)` — serialized `GetMessageMediaResponse` metadata (see below).

**Behavior by media type** (classified via existing `convert_media_to_type`):

- **Photo** → download the photo itself.
- **Video / Animation / VideoNote** → do NOT download the video; download a
  server-side thumbnail and set `is_thumbnail: true`. Thumbnails go through the same
  size-selection rule as photos (smallest variant whose longest side >= max_dimension,
  else the largest available — video thumbs are small, so the largest usually wins).
  If the document has no usable thumbnail, return
  `Error::DownloadFailed("no downloadable size variant available")`.
- **Anything else** (no media, sticker, document, audio, voice, poll, geo, webpage
  preview, …) → `Error::NoVisualMedia { media_type }`, a structured error, never a panic.

**Hard limits:**

- Refuse to download when the selected photo size exceeds **20 MB** (20_971_520 bytes)
  (`Error::MediaTooLarge { size_bytes, max_bytes }` carrying the actual size).
  The check runs *before* any download, using the byte size grammers reports for the
  size variant actually selected for download — i.e. the bytes that would cross the
  network, which is the protective intent of the limit.
- Cap the returned base64 payload at **1.5 MB** (1_572_864 base64 chars). If the
  encoded image exceeds the cap, downscale further and re-encode (see pipeline below).

## The rmcp return-type deviation (explicit decision)

Project convention says all tools return `Result<String, String>`. That convention is
a *project* choice; rmcp's actual constraint is that the return type implements
`IntoCallToolResult`. Image content cannot be expressed as a JSON string the model can
see, so this tool returns:

```rust
Result<rmcp::model::CallToolResult, String>
```

- `rmcp` 1.7 provides `impl IntoCallToolResult for CallToolResult` and
  `impl<T: IntoCallToolResult, E: IntoCallToolResult> IntoCallToolResult for Result<T, E>`;
  `String` implements `IntoContents`, so the `Err(String)` arm becomes a text content
  block with `is_error: true` — identical client-visible behavior to the other 9 tools.
- `log_tool_outcome` (src/mcp/server.rs) is generalized from
  `&Result<String, String>` to `&Result<T, String>` (generic over the Ok type) so the
  request-id-correlated started/completed/failed logging wrapper pattern is unchanged.

Alternatives rejected:
- *Base64 inside the JSON string response* — the client model cannot see the image;
  defeats the feature's purpose.
- *`Result<CallToolResult, ErrorData>`* — breaks the project's string-error
  convention without benefit.

## Architecture

```
MCP layer (src/mcp/)
  server.rs            get_message_media + get_message_media_impl (tool #10)
  tools/types/requests.rs   GetMessageMediaRequest
  tools/types/responses.rs  GetMessageMediaResponse (metadata text block)
  tools/image.rs       NEW: pure image pipeline (decode → downscale → JPEG → cap loop)
        │ uses MediaDownload (bytes + metadata)
        ▼
Telegram layer (src/telegram/)
  trait_def.rs         + download_message_media() on TelegramClientTrait
  client.rs            impl: resolve peer → fetch message → classify media →
                       select size → 20 MB check → iter_download (in-memory)
  converters.rs        + pure photo-size selection helper
  types/media.rs       + MediaDownload, SizeCandidate
Application layer
  error.rs             + MediaTooLarge, NoVisualMedia, DownloadFailed
  config.rs            + [rate_limiting] media_download_cost (default 5)
                       + [telegram.timeouts] download_secs (default 120)
                       + [observability] max_buffered_payload_bytes (default 256 KiB)
```

### New trait method (the mock seam)

```rust
/// Download the visual media (photo, or thumbnail for video-like media) of a message.
///
/// `max_dimension` is a hint for server-side size selection: the smallest available
/// photo size whose longest side >= max_dimension is downloaded (largest size if none
/// qualifies). Exact downscaling to max_dimension happens later, in the MCP layer.
async fn download_message_media(
    &self,
    channel_ref: &str,
    message_id: i32,
    max_dimension: u32,
) -> Result<MediaDownload, Error>;
```

`MediaDownload` (src/telegram/types/media.rs) — domain type, no grammers leakage:

```rust
pub struct MediaDownload {
    pub bytes: Vec<u8>,            // raw downloaded JPEG bytes
    pub media_type: MediaType,     // Photo | Video | Animation | VideoNote
    pub is_thumbnail: bool,        // true for video-like media
    pub caption: Option<String>,   // msg.text(), None if empty
    pub width: Option<u32>,        // dimensions of the downloaded size, if known
    pub height: Option<u32>,
    pub source_size_bytes: u64,    // byte size of the downloaded size variant
}
```

`mockall` regenerates `MockTelegramClientTrait` automatically; tool tests feed it a
`MediaDownload` containing a real tiny in-memory JPEG so the full pipeline runs
without a live Telegram connection.

### Client implementation (src/telegram/client.rs)

Reuses the exact resolve/fetch shape of `get_message_by_id` (numeric ID → dialog walk;
username → `resolve_username`; then `get_messages_by_id`), each grammers call wrapped
in `with_timeout`. New work after the message is fetched:

1. `msg.media()` → classify with `convert_media_to_type`.
2. Photo: `photo.thumbs()` → build `Vec<SizeCandidate>` → pure selector picks the
   smallest size with `max(w, h) >= max_dimension`, else the largest available.
   Video-like: `document.thumbs()` → same selection rule via the pure selector
   (video thumbs are small, so the largest is usually chosen).
3. 20 MB pre-download check on the selected candidate's size.
4. Download in-memory with `client.iter_download(&photo_size)`, accumulating chunks,
   wrapped in `with_timeout("download_media", timeouts.download_secs, …)`. The running
   byte count is also checked against 20 MB during accumulation (defense in depth —
   reported sizes are untrusted input).
5. Return `MediaDownload`. No disk I/O anywhere — stream, encode, return, drop.

`SizeCandidate { width, height, size_bytes, photo_type }` is extracted from grammers
`PhotoSize` variants (`Size`, `Cached`, `Progressive` carry dimensions; `Stripped`,
`Path`, `Empty` are skipped — they are tiny inline previews or vectors, not photo
content). The selector is a pure function in `converters.rs`, unit-testable without
grammers values.

### Image pipeline (src/mcp/tools/image.rs — pure, no I/O)

```rust
pub struct ProcessedImage {
    pub base64_jpeg: String,
    pub width: u32,
    pub height: u32,
    pub encoded_size_bytes: usize, // JPEG bytes before base64 expansion
}

pub fn process_image(bytes: &[u8], max_dimension: u32) -> Result<ProcessedImage, Error>;
```

1. Decode with the `image` crate (JPEG — Telegram photos and thumbs are always JPEG).
   Decode failure → `Error::DownloadFailed`.
2. If longest side > `max_dimension`, downscale preserving aspect ratio
   (`image::imageops::resize`, Lanczos3 — quality matters for charts/text in photos).
3. Encode JPEG quality 80, base64-encode (`base64` crate, standard alphabet).
4. If base64 length > 1.5 MB cap: multiply the current dimension by
   `min(0.9, sqrt(cap / actual_len))` — never more than 0.9, so every iteration
   shrinks by at least 10% and the loop provably converges — and re-encode, up to
   5 iterations. If still over the cap (practically impossible), `Error::DownloadFailed`
   with an explanatory message.

### Tool handler (src/mcp/server.rs)

`get_message_media_impl`:

1. Clamp `max_dimension` to [64, 2048], default 1280.
2. `rate_limiter.acquire(self.media_download_cost)` — cost is a `McpServer` field
   (default 5), set in `main.rs` via builder method
   `.with_media_download_cost(config.rate_limiting.media_download_cost)`, same pattern
   as `with_observability`.
3. `telegram_client.download_message_media(…)`.
4. `process_image(…)`.
5. Build `CallToolResult::success(vec![Content::image(…), Content::text(metadata_json)])`.

`GetMessageMediaResponse` (responses.rs, the text block):

```rust
pub struct GetMessageMediaResponse {
    pub channel_id: String,
    pub message_id: i64,
    pub media_type: MediaType,
    pub is_thumbnail: bool,
    pub caption: Option<String>,
    pub original_width: Option<u32>,    // dimensions of the downloaded source
    pub original_height: Option<u32>,
    pub original_size_bytes: u64,
    pub returned_width: u32,            // dimensions after processing
    pub returned_height: u32,
    pub returned_size_bytes: usize,     // encoded JPEG bytes (pre-base64)
    pub mime_type: String,              // always "image/jpeg"
}
```

## Error handling

New `thiserror` variants in `src/error.rs`:

```rust
#[error("media too large: {size_bytes} bytes exceeds limit of {max_bytes} bytes")]
MediaTooLarge { size_bytes: u64, max_bytes: u64 },

#[error("message has no visual media (media type: {media_type})")]
NoVisualMedia { media_type: String },

#[error("media download failed: {0}")]
DownloadFailed(String),
```

All tool-level failures surface as `Err(String)` via `e.to_string()`, exactly like the
other tools. No panics; no `unwrap()` in production code.

## Configuration

```toml
[rate_limiting]
media_download_cost = 5        # tokens per get_message_media call (searches cost 1)

[telegram.timeouts]
download_secs = 120            # wall-clock budget for the media download call

[observability]
max_buffered_payload_bytes = 262144   # see below
```

All new fields have `#[serde(default)]` defaults; existing configs keep working.

**Observability interaction (targeted improvement):** `InstrumentedTransport` stores
every response payload in the `ResponseBuffer` ring (default 10 entries) for
`get_last_responses`. A ~1.5 MB base64 response would pin megabytes of RAM and be
replayed as text. New rule: payloads larger than `max_buffered_payload_bytes`
(default 256 KiB) are recorded with accurate `size_bytes`/`request_id`/`tool_name`
but a stub payload (`{"omitted":"payload exceeded max_buffered_payload_bytes"}`), so
`get_last_responses` reports the entry without re-emitting the image.

## Dependencies

- `image = { version = "0.25", default-features = false, features = ["jpeg"] }` —
  decode/resize/encode; JPEG-only keeps compile time and attack surface down.
- `base64 = "0.22"` — payload encoding.

## Testing (TDD — tests first, per task)

| Area | Where | Cases |
|---|---|---|
| Image pipeline | `src/mcp/tools/image.rs` inline tests | downscaling correctness (dimensions, aspect ratio), no upscale of small images, payload-cap loop converges, invalid bytes → error; test images generated in-memory with the `image` crate |
| Size selector | `converters.rs` inline tests | picks smallest ≥ max_dimension, falls back to largest, skips dimension-less variants, empty → None |
| Tool handler | `src/mcp/tests/media.rs` (new) | photo path returns image+text blocks, video path sets `is_thumbnail`, no-media → NoVisualMedia error string, MediaTooLarge propagation, rate-limiter charged `media_download_cost` (mock expects `acquire(5)`), max_dimension clamping |
| Errors | `src/error.rs` inline tests | Display format of the 3 new variants (existing convention) |
| Config | `src/config/tests.rs` | new defaults + TOML parsing (serial, `--test-threads=1`) |
| Requests | requests/serde tests | flexible coercion for the 3 params, default max_dimension |
| Observability | `src/mcp/tests/last_responses.rs` + observability tests | oversized payload stored as stub, size_bytes accurate |

Quality gate (must pass before merge): `cargo fmt --check && cargo clippy -- -D warnings && cargo test`.

## Documentation updates

- README.md — MCP Tools Reference table row for `get_message_media` (same format).
- CHANGELOG.md — `[Unreleased]` → Added.
- CLAUDE.md — "9 tools" → 10 (and tool-count mentions in `src/mcp/tools.rs` header,
  `.claude/rules/ast-index.md`, project-conventions skill say 7/8/9 — align them).
- `docs/memory.md` — record the `Result<CallToolResult, String>` deviation decision.

## Non-goals

- No full video download, no audio handling, no OCR, no transcription.
- No disk caching of downloaded media.
- No sticker/webpage-preview/document-image support — `NoVisualMedia` for all of them.
- No retry logic beyond what grammers provides; a failed download is a tool error.
