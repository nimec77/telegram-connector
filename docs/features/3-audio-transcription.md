Implement a new MCP tool `transcribe_voice_message` in this repository
(telegram-connector: Rust 2024 nightly, rmcp SDK, grammers Telegram client).

## Problem
Voice messages and video notes (round videos) in channels are completely
opaque to the MCP client (Claude) — raw audio bytes would be useless even if
downloaded, since the client cannot listen. Telegram already solves this
server-side: the MTProto method `messages.transcribeAudio` returns a text
transcription (Whisper-based) with NO local ML required. The constraint:
it only works if the authenticated account has **Telegram Premium**, and
Telegram enforces weekly transcription quotas. The tool must use this
server-side API and degrade gracefully — do NOT bundle whisper.cpp or any
local transcription fallback.

## Tool specification
`transcribe_voice_message`
- Parameters:
  - `channel_id` (string, required) — channel ID or username
  - `message_id` (integer, required)
  - `timeout_seconds` (integer, optional, default 30, max 120) — how long to
    wait for Telegram to finish transcribing
- Behavior:
  - Resolve the message; verify it contains a `voice` or `video_note` media
    type. Reject other media types with a structured error (point the caller
    to the message's actual media_type).
  - Invoke `messages.transcribeAudio` via raw TL
    (`tl::functions::messages::TranscribeAudio`) — grammers' high-level API
    does not wrap this; use `client.invoke(...)`.
  - The response (`messages.transcribedAudio`) may arrive with
    `pending = true` and partial text. In that case, listen for
    `UpdateTranscribedAudio` updates matching the returned
    `transcription_id` until `pending = false` or `timeout_seconds`
    elapses. On timeout, return whatever partial text was accumulated with
    `"partial": true` in the response.
- Response (JSON text block):
  - `text` (string) — the transcription
  - `partial` (bool) — true if timed out before completion
  - `duration_seconds` (integer, optional) — audio duration from message
    metadata
  - `media_type` ("voice" | "video_note")

## Error handling (this is the hard part — be exhaustive)
Map Telegram RPC errors to structured tool errors with actionable messages:
- `PREMIUM_ACCOUNT_REQUIRED` → "Transcription requires Telegram Premium on
  the connected account" (distinct error variant, NOT a generic failure)
- `TRANSCRIPTION_FAILED` → transcription unavailable for this audio
- `MSG_VOICE_TOO_LONG` → audio exceeds Telegram's transcription length limit
- `FLOOD_WAIT_X` / quota exhaustion → surface the retry-after seconds,
  consistent with how the existing rate-limit errors are reported
Add the corresponding `thiserror` variants in `src/error.rs`.

## Premium detection (optional but preferred)
At client startup or lazily on first call, check whether the account has
Premium (the `premium` flag on the self user) and cache it. If absent,
fail fast with the Premium error WITHOUT spending an API call on
`transcribeAudio`. Expose the cached flag in `check_mcp_status` output
(e.g. `"premium": true`) so the MCP client can know transcription is
available before trying.

## Integration requirements (follow existing project conventions)
- Request/response types in `src/mcp/tools/types/` with `schemars` schemas.
- Extend `TelegramClientTrait` with a `transcribe_audio(...)` method and
  regenerate `mockall` mocks; the handler must be testable without a live
  connection, including the pending → update → final flow (mock the update
  stream).
- Rate limiter: charge transcription calls more than searches (e.g. 5
  tokens, configurable in `[rate_limiting]`) — Telegram's own weekly quota
  makes these calls precious.
- Wire the tool into `src/mcp/server.rs` alongside existing handlers.

## Quality gates
- `cargo fmt --check && cargo clippy -- -D warnings && cargo test` must pass.
- Unit tests: immediate (non-pending) transcription; pending → completed via
  update; pending → timeout returns partial; non-voice media rejection;
  Premium-absent fast-fail; each RPC error mapping.
- Update README.md (tool reference table + a "Requires Telegram Premium"
  callout) and CHANGELOG.md.

## Non-goals
- No local transcription (no whisper.cpp, no candle, no ONNX) — server-side
  only.
- No raw audio download or base64 audio in responses.
- No translation of the transcription; return Telegram's text as-is.
