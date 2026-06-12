Implement a new MCP tool `get_message_media` in this repository (telegram-connector:
Rust 2024 nightly, rmcp SDK, grammers Telegram client).

## Problem
The existing `search_messages` / `get_recent_messages` tools only report media
*metadata* (`has_media`, `media_type`). The MCP client (Claude) is blind to the
actual content of photos, even though in news channels the photo often IS the
content (benchmark tables, charts, maps, document screenshots). MCP natively
supports image content blocks in tool results, so photos can be returned
directly into the model's vision context — no OCR pipeline needed.

## Tool specification
`get_message_media`
- Parameters:
  - `channel_id` (string, required) — channel ID or username
  - `message_id` (integer, required)
  - `max_dimension` (integer, optional, default 1280, max 2048) — longest side
    after downscaling
- Behavior:
  - Resolve the message via the existing grammers client wrapper.
  - If the message has a **photo**: download it with grammers `download_media`,
    downscale with the `image` crate so the longest side <= `max_dimension`,
    re-encode as JPEG (quality ~80), return as an MCP **image content block**
    (base64 + mime type), plus a JSON text block with metadata (original size,
    returned size, media_type, caption if present).
  - If the message has a **video / animation / video_note**: do NOT download
    the video. Download only its server-side **thumbnail** and return it the
    same way, with `"is_thumbnail": true` in the metadata.
  - If the message has no visual media: return a structured error, not a panic.
- Hard limits:
  - Refuse downloads where the source photo exceeds 20 MB (return an error
    with the actual size).
  - Cap the returned base64 payload at ~1.5 MB after re-encoding; downscale
    further if needed to fit.

## Integration requirements (follow existing project conventions)
- Add request/response types under `src/mcp/tools/types/` (requests.rs /
  responses.rs) with `schemars` JSON schemas, consistent with existing tools.
- Extend `TelegramClientTrait` (src/telegram/trait_def.rs) with the new
  download method(s) and regenerate `mockall` mocks; the tool handler must be
  unit-testable against the mock without a live Telegram connection.
- Media download is heavier than a search call: charge it more against the
  token-bucket rate limiter (e.g. 5 tokens per download vs 1 for searches);
  make the cost configurable in `[rate_limiting]`.
- Type conversions go in `src/telegram/converters.rs`; new media handling
  types in `src/telegram/types/media.rs`.
- Use the existing error types (`thiserror`) — add variants as needed
  (MediaTooLarge, NoVisualMedia, DownloadFailed).
- Wire the tool into `src/mcp/server.rs` alongside the existing handlers
  (this becomes tool #9).

## Quality gates
- `cargo fmt --check && cargo clippy -- -D warnings && cargo test` must pass.
- Add unit tests: photo path, video-thumbnail path, no-media error path,
  oversize rejection, downscaling correctness (dimensions and payload cap).
- Update README.md (MCP Tools Reference section, same table format as other
  tools) and CHANGELOG.md.

## Non-goals
- No full video download, no audio handling, no OCR, no transcription.
- No disk caching of downloaded media — stream, encode, return, drop.
