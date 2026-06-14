# transcribe_voice_message — Design

**Date:** 2026-06-14
**Status:** APPROVED
**Source feature request:** `docs/features/3-audio-transcription.md`

## Problem

Voice messages and video notes (round videos) are opaque to the MCP client — raw
audio bytes are useless to a model that cannot listen. Telegram solves this
server-side: `messages.transcribeAudio` returns a Whisper-based text transcription
with **no** local ML. Constraints: it requires **Telegram Premium** on the connected
account, and Telegram enforces weekly transcription quotas. This tool uses the
server-side API only and degrades gracefully — no bundled `whisper.cpp`, `candle`, or
ONNX fallback, no raw audio download, no translation.

## Key decisions (brainstorming)

- **Waiting mechanism: poll by re-invoking `transcribeAudio`.** When the first call
  returns `pending = true`, the tool re-invokes `transcribeAudio` on an interval until
  `pending = false` or the timeout elapses. Re-calls return Telegram's server-cached
  transcription (same `transcription_id`, updated `text`/`pending`) and do **not**
  re-charge the weekly quota. Chosen over listening to `UpdateTranscribedAudio`
  because this connector has **no** update loop today and grammers' update stream is a
  single-consumer receiver (`UpdateStream`, `updateTranscribedAudio` only via
  `next_raw()`) — wiring a global update consumer + dispatcher into a pull-based
  connector is disproportionate architecture for one tool. Trade-off: slight latency
  granularity from the poll interval; deviates from the feature request's literal
  "listen for updates" wording.
- **Premium detection: eager at startup, lazy fallback.** `run_mcp_server` warms a
  cached premium flag via one `get_me()` before serving, so `check_mcp_status` reports
  it accurately from the first request. If the eager warm-up fails (e.g. transient),
  the flag stays unknown and is resolved lazily on first use.

## Tool contract

`transcribe_voice_message` — tool #11 (docs currently say 10 tools; this change
updates them).

| Parameter | Type | Required | Notes |
|---|---|---|---|
| `channel_id` | string | yes | channel ID or username (flexible coercion, `flexible_string`) |
| `message_id` | integer | yes | flexible coercion (`flexible_i64`) |
| `timeout_seconds` | integer | no | how long to wait for completion; default **30**, clamped to **[1, 120]** (`flexible_opt_u32`) |

**Success result** — JSON text block, serialized `TranscribeVoiceMessageResponse`:

| Field | Type | Notes |
|---|---|---|
| `text` | string | the transcription (possibly partial) |
| `partial` | bool | `true` if the timeout elapsed before Telegram finished |
| `duration_seconds` | integer? | audio duration from message metadata; omitted if unavailable |
| `media_type` | string | `"voice"` or `"video_note"` (serialized `MediaType`) |

**Behavior:**

1. Parse `message_id`; clamp `timeout_seconds` to `[1, 120]` (default 30).
2. **Premium fast-fail** — query the cached premium flag (`is_premium()`):
   - `Some(false)` → return `PremiumRequired` **without** spending a `transcribeAudio`
     call.
   - `Some(true)` / `None` (unknown) → proceed (an unknown flag falls through to the
     RPC error path, which still maps `PREMIUM_ACCOUNT_REQUIRED` correctly).
3. **Rate-limit** — `acquire(transcription_cost)` (default **5** tokens; searches cost 1).
4. Resolve + transcribe (see Telegram layer).
5. Serialize the response.

## Architecture

### Telegram layer (`src/telegram/`)

Add to `TelegramClientTrait` (regenerate `mockall` mock):

```rust
/// Transcribe a voice / video-note message's audio via messages.transcribeAudio.
/// Resolves the peer once, validates media type, then polls until the
/// transcription completes or `timeout_secs` elapses.
async fn transcribe_audio(
    &self,
    channel_ref: &str,
    message_id: i32,
    timeout_secs: u32,
) -> Result<TranscriptionOutcome, Error>;

/// Cached Telegram Premium flag for the connected account.
/// Returns the cached value; if unknown, performs one get_me() and caches it.
/// Returns None only when premium status could not be determined.
async fn is_premium(&self) -> Option<bool>;
```

Production `TelegramClient::transcribe_audio`:

- `resolve_peer(channel_ref)` **once** → `PeerRef` → `InputPeer` (reused for every
  poll; avoids repeating the expensive numeric-ID dialog walk on each iteration).
- `get_messages_by_id` once → grammers message; `convert_media_to_type` →
  `ensure_transcribable(media_type)`:
  - `Voice` / `VideoNote` → OK.
  - anything else → `Error::NotTranscribable { media_type }` naming the actual type.
- Extract `duration_seconds` best-effort from the document's audio/video attribute
  (`None` if unavailable).
- `invoke(tl::functions::messages::TranscribeAudio { peer, msg_id })` once →
  `tl::enums::messages::TranscribedAudio::Audio(t)` → initial
  `TranscriptionState { transcription_id: t.transcription_id, text: t.text, pending: t.pending }`.
- Hand off to the orchestrator (below). Build `TranscriptionOutcome` from the final
  state + `media_type` + `duration_seconds`.
- All `invoke` failures pass through `map_transcribe_rpc_error`.

`is_premium`: `TelegramClient` gains a `tokio::sync::RwLock<Option<bool>>` premium
cache. `is_premium()` returns the cached value; on `None`, calls `get_me()` →
`user.is_premium()`, caches, returns it; on `get_me()` failure leaves it `None` and
returns `None`. `run_mcp_server` calls `is_premium()` once eagerly (warm-up) before
wrapping the client in `Arc`.

### Poll orchestrator (pure, testable seam)

A free generic async function, independent of grammers, in the telegram module:

```rust
async fn poll_until_complete<F, Fut>(
    initial: TranscriptionState,
    timeout: Duration,
    interval: Duration,
    mut poll: F,
) -> (TranscriptionState, bool /* partial */)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<TranscriptionState, Error>>,
```

- If `initial` is not pending, return `(initial, false)` immediately.
- Otherwise loop: `sleep(interval)`, `poll()`; on the latest non-pending state return
  `(state, false)`; when the deadline passes, return the last accumulated state with
  `partial = true`. A `poll()` error before any success propagates; an error after a
  prior partial returns the partial.
- Uses `tokio::time` (`Instant`/`sleep`). Production `poll` re-invokes
  `TranscribeAudio` against the cached `InputPeer`. Poll interval constant ≈ **2s**.

### RPC error mapping (`src/telegram/`)

`map_transcribe_rpc_error(InvocationError) -> Error`, matching `InvocationError::Rpc(RpcError { name, value, .. })`:

| Telegram RPC | Mapped `Error` |
|---|---|
| `PREMIUM_ACCOUNT_REQUIRED` | `PremiumRequired` |
| `TRANSCRIPTION_FAILED` | `TranscriptionFailed(name)` |
| `MSG_VOICE_TOO_LONG` | `VoiceTooLong` |
| `FLOOD_WAIT_X` (and quota exhaustion) | `RateLimit { retry_after_seconds: value }` — reuses the existing variant for reporting consistency |
| anything else | `TelegramApi(..)` |

### Error variants (`src/error.rs`)

New `thiserror` variants:

- `PremiumRequired` → `"transcription requires Telegram Premium on the connected account"`
- `TranscriptionFailed(String)` → `"transcription failed: {0}"`
- `VoiceTooLong` → `"audio exceeds Telegram's transcription length limit"`
- `NotTranscribable { media_type: String }` → `"message is not transcribable (media type: {media_type}); only voice and video_note are supported"`

`FLOOD_WAIT` reuses the existing `RateLimit { retry_after_seconds }`.

### Domain types (`src/telegram/types/`)

```rust
pub struct TranscriptionState {
    pub transcription_id: i64,
    pub text: String,
    pub pending: bool,
}

pub struct TranscriptionOutcome {
    pub text: String,
    pub partial: bool,
    pub media_type: MediaType,
    pub duration_seconds: Option<u32>,
}
```

### MCP types (`src/mcp/tools/types/`)

- `requests.rs`: `TranscribeVoiceMessageRequest { channel_id, message_id, timeout_seconds: Option<u32> }`
  with the existing `flexible_*` `deserialize_with` helpers and `schemars` descriptions.
- `responses.rs`: `TranscribeVoiceMessageResponse { text, partial, duration_seconds: Option<u32>, media_type: MediaType }`.
  Add `premium: Option<bool>` to `StatusResponse` (populated from `is_premium()`).

### Config (`src/config.rs`)

Add `transcription_cost: u32` to `RateLimitConfig` with `#[serde(default = "default_transcription_cost")]`, default **5**.

### Server wiring (`src/mcp/server.rs`)

- Tool #11: `#[tool] transcribe_voice_message` logging wrapper (request-id-correlated
  started/completed/failed) → private `transcribe_voice_message_impl`.
- `McpServer` gains a `transcription_cost` field + `with_transcription_cost(u32)`
  builder, mirroring `media_download_cost` / `with_media_download_cost`.
- `check_mcp_status_impl` populates `StatusResponse.premium` from `is_premium()`.
- `main.rs::run_mcp_server` adds `.with_transcription_cost(config.rate_limiting.transcription_cost)`
  and warms premium before serving.

## Test plan (all without a live connection)

- **Orchestrator** (`poll_until_complete`, `tokio::time::pause()` + scripted poll
  closure):
  - immediate non-pending → returns immediately, `partial = false`;
  - pending → completed after N polls → `partial = false`, final text;
  - pending forever → timeout → `partial = true` with last accumulated text.
- **RPC mapping** (`map_transcribe_rpc_error`, synthetic `RpcError` per name): each row
  of the mapping table → expected `Error` variant; `FLOOD_WAIT` carries the seconds.
- **`ensure_transcribable`** (pure over `MediaType`): `Voice`/`VideoNote` pass; `Photo`,
  `Document`, `None`, … rejected with `NotTranscribable` naming the type.
- **Handler** (`MockTelegramClientTrait` for `transcribe_audio` + `is_premium`,
  `MockRateLimiterTrait`):
  - premium-absent fast-fail: `is_premium → Some(false)`, assert `transcribe_audio`
    **not** called and error mentions Premium;
  - non-voice rejection: `transcribe_audio → Err(NotTranscribable{..})`, assert error
    string names the actual media type;
  - complete: `transcribe_audio → Ok(outcome{partial:false})` → JSON shape;
  - partial: `transcribe_audio → Ok(outcome{partial:true})` → `partial:true` in JSON;
  - each RPC-mapped error (`PremiumRequired`, `VoiceTooLong`, `TranscriptionFailed`,
    `RateLimit`) → expected error string.
- **`check_mcp_status`**: `premium` field surfaces the cached flag.

## Docs

- `README.md`: add `transcribe_voice_message` to the tool reference table; add a
  "Requires Telegram Premium" callout; bump the tool count (10 → 11).
- `CHANGELOG.md`: new entry under the appropriate version section.
- `docs/tasklist.md`: add the phase row on completion.

## Non-goals

- No local transcription (no `whisper.cpp`, `candle`, ONNX).
- No raw audio download or base64 audio in responses.
- No translation — return Telegram's text as-is.
- No live `UpdateTranscribedAudio` stream consumption (polling instead, per decision).

## Risks / verification

- **Re-invoke quota behavior:** the design assumes re-calling `transcribeAudio` for an
  in-progress transcription returns the server-cached result without re-charging the
  weekly quota. Confirm during implementation via the response's `trial_remains_*`
  fields (they should not decrement on cache hits). If this proves false, revisit the
  poll interval (fewer, wider-spaced polls) — the orchestrator's interval is the single
  knob.
- **`get_messages_by_id` for the media check** shares the existing `resolve_secs` /
  `history_secs` timeout budgets; the transcription wait is bounded separately by
  `timeout_seconds`.
