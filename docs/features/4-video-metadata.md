Enrich video-related message metadata in this repository (telegram-connector:
Rust 2024 nightly, rmcp SDK, grammers Telegram client). Full video download
is explicitly OUT of scope — videos are too large to transfer and the MCP
client (Claude) cannot watch them anyway. The goal is to let the client
describe a video honestly (length, shape, kind) and decide whether fetching
its thumbnail is worthwhile.

## Part A — video metadata in message responses (primary deliverable)
Extend the `Message` response schema used by `search_messages` and
`get_recent_messages` with an optional `video_info` object (omit when the
message has no video-class media, via `skip_serializing_if`):

- `duration_seconds` (u32)
- `width` (u32), `height` (u32)
- `file_size_bytes` (u64)
- `kind` ("video" | "video_note" | "animation") — round video notes and
  GIF-class animations are distinct from regular videos
- `has_thumbnail` (bool) — whether Telegram stores a server-side thumbnail
  for this media
- `mime_type` (string, optional)

All of this lives on the grammers media document attributes
(`DocumentAttributeVideo`, document size, mime_type) — zero extra API
calls. Extraction goes in `src/telegram/converters.rs`; if the high-level
grammers API doesn't expose an attribute, read it from the raw TL document
attributes and note that in a code comment.

Also populate the analogous data for audio-class media while in there:
an optional `audio_info` object with `duration_seconds`, `file_size_bytes`,
`kind` ("audio" | "voice"), `mime_type`. Same source, same zero-cost rule.
(This pairs with the transcription tool: duration tells the client whether
a voice message is worth a transcription-quota call.)

## Part B — thumbnail retrieval (conditional)
- IF the tool `get_message_media` already exists in src/mcp (from a prior
  task): verify it handles `video`, `video_note`, and `animation` by
  returning the server-side thumbnail as an MCP image content block with
  `"is_thumbnail": true` metadata. Add the `video_info` object from Part A
  to its metadata text block. Fix gaps if any; do not duplicate logic.
- IF it does not exist: implement a minimal tool `get_video_thumbnail`
  (params: `channel_id` string required, `message_id` integer required)
  that downloads ONLY the thumbnail (grammers `download_media` on the thumb
  size, never the full document), returns it as an MCP image content block
  (JPEG, base64) plus a JSON metadata block containing `video_info`.
  Thumbnails are small (typically <50 KB) — no downscaling pipeline needed,
  but reject and error if a thumb unexpectedly exceeds 1 MB.

## Integration requirements (follow existing project conventions)
- Response DTOs in `src/mcp/tools/types/responses.rs` with `schemars`
  schemas; domain types in `src/telegram/types/media.rs`.
- Part A must not add client calls: tests assert conversion works from a
  single Message object (mockall: zero download/request expectations).
- Part B (if the minimal tool is built): extend `TelegramClientTrait` with
  a thumbnail download method, regenerate mocks, charge the rate limiter
  a configurable cost (default 2 tokens — lighter than full photo
  download), wire into `src/mcp/server.rs`.
- Existing response fields and JSON names stay unchanged (backward
  compatible enrichment only).

## Quality gates
- `cargo fmt --check && cargo clippy -- -D warnings && cargo test` must pass.
- Unit tests: regular video, video_note, animation (kind mapping for each);
  video without thumbnail (`has_thumbnail: false`); voice vs audio kind
  mapping in `audio_info`; plain text message — both objects absent from
  serialized JSON; (Part B) thumbnail returned for each video kind,
  no-thumbnail error path.
- Update README.md (response examples + tool reference if the new tool is
  created) and CHANGELOG.md.

## Non-goals
- No full video download under any parameter combination.
- No frame extraction, no ffmpeg, no video transcription.
- No streaming or partial-range downloads of the video document itself.
