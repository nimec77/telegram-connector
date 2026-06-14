# transcribe_voice_message Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an MCP tool `transcribe_voice_message` that transcribes voice messages and video notes via Telegram's server-side `messages.transcribeAudio` (Premium-gated, no local ML).

**Architecture:** A handler in `server.rs` does Premium fast-fail → rate-limit → `TelegramClientTrait::transcribe_audio`. The production client resolves the peer once, validates media type, invokes `TranscribeAudio`, then polls by re-invoking until `pending=false` or timeout. The poll loop, RPC-error mapping, and media-type check are pure functions tested in isolation; the handler is tested with `mockall`. A Premium flag is detected eagerly at startup and cached.

**Tech Stack:** Rust 2024 nightly, `rmcp` (tool macros), `grammers-client` (raw TL `invoke`), `mockall`, `tokio` (paused-time tests), `thiserror`, `schemars`.

**Conventions:**
- TDD: write the failing test, see it fail, implement, see it pass, commit.
- After every code change run `cargo fmt --all`.
- All commits end with the trailer:
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`
- Per-task gate: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`.
- Config tests run serial: `cargo test config -- --test-threads=1`.

**Reference facts (verified against the grammers checkout):**
- `tl::functions::messages::TranscribeAudio { peer: tl::enums::InputPeer, msg_id: i32 }`, `RemoteCall::Return = tl::enums::messages::TranscribedAudio`.
- `tl::enums::messages::TranscribedAudio::Audio(tl::types::messages::TranscribedAudio { pending: bool, transcription_id: i64, text: String, trial_remains_num: Option<i32>, trial_remains_until_date: Option<i32> })`.
- `grammers_mtsender::InvocationError::Rpc(grammers_mtsender::RpcError { code: i32, name: String, value: Option<u32>, caused_by: Option<u32> })`. grammers normalizes `FLOOD_WAIT_31` → `name = "FLOOD_WAIT"`, `value = Some(31)`.
- `client.get_me().await? -> grammers_client::peer::User`, `user.is_premium() -> bool`.
- `client.invoke(&request).await -> Result<R::Return, InvocationError>`.
- `PeerRef: Into<tl::enums::InputPeer>`.
- `DocumentAttributeAudio.duration: i32`, `DocumentAttributeVideo.duration: f64`.
- `MediaType` derives `#[serde(rename_all = "lowercase")]` → `VideoNote` serializes as `"videonote"`. The tool response therefore uses a `String` mapped to `"voice"` / `"video_note"` to match the feature spec.

---

## Task 1: Error variants

**Files:**
- Modify: `src/error.rs` (enum `Error` near line 36; tests after line 39)

- [ ] **Step 1: Write the failing tests**

Add these tests inside `mod tests` in `src/error.rs` (after `test_invalid_input_error_display`, before the closing `}` of the module):

```rust
    #[test]
    fn test_premium_required_error_display() {
        let error = Error::PremiumRequired;
        assert_eq!(
            error.to_string(),
            "transcription requires Telegram Premium on the connected account"
        );
    }

    #[test]
    fn test_transcription_failed_error_display() {
        let error = Error::TranscriptionFailed("TRANSCRIPTION_FAILED".to_string());
        assert_eq!(
            error.to_string(),
            "transcription failed: TRANSCRIPTION_FAILED"
        );
    }

    #[test]
    fn test_voice_too_long_error_display() {
        let error = Error::VoiceTooLong;
        assert_eq!(
            error.to_string(),
            "audio exceeds Telegram's transcription length limit"
        );
    }

    #[test]
    fn test_not_transcribable_error_display() {
        let error = Error::NotTranscribable {
            media_type: "photo".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "message is not transcribable (media type: photo); only voice and video_note are supported"
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib error::tests::test_premium_required_error_display`
Expected: FAIL — `no variant named PremiumRequired` (compile error).

- [ ] **Step 3: Add the variants**

In `src/error.rs`, add to `enum Error` immediately after the `DownloadFailed(String)` variant (before the closing `}` at line ~37):

```rust

    #[error("transcription requires Telegram Premium on the connected account")]
    PremiumRequired,

    #[error("transcription failed: {0}")]
    TranscriptionFailed(String),

    #[error("audio exceeds Telegram's transcription length limit")]
    VoiceTooLong,

    #[error(
        "message is not transcribable (media type: {media_type}); only voice and video_note are supported"
    )]
    NotTranscribable { media_type: String },
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib error::tests`
Expected: PASS (all error tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add src/error.rs
git commit -m "feat: add transcription error variants

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Config — `transcription_cost`

**Files:**
- Modify: `src/config.rs` (default fns ~line 73; `default_rate_limit_config` ~line 104; `RateLimitConfig` struct ~line 265)
- Modify: `src/config/tests.rs` (literal at line 132; add a default test)

- [ ] **Step 1: Write the failing test**

Add to `src/config/tests.rs` (anywhere among the other `#[test]` fns). The file has `use super::*;`, so the private `default_rate_limit_config` is in scope unqualified — the same way `default_media_download_cost()` is already called at `src/config/tests.rs:135`:

```rust
#[test]
fn test_default_rate_limit_has_transcription_cost() {
    let config = default_rate_limit_config();
    assert_eq!(config.transcription_cost, 5);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test config -- --test-threads=1 test_default_rate_limit_has_transcription_cost`
Expected: FAIL — `no field transcription_cost`.

- [ ] **Step 3: Implement**

In `src/config.rs`, add a default fn next to `default_media_download_cost` (line ~73):

```rust
fn default_transcription_cost() -> u32 {
    5
}
```

Add the field to `RateLimitConfig` (after `media_download_cost`, line ~272):

```rust
    /// Tokens charged per transcribe_voice_message call (Telegram's weekly
    /// transcription quota makes these calls precious; searches cost 1).
    #[serde(default = "default_transcription_cost")]
    pub transcription_cost: u32,
```

Add the field to `default_rate_limit_config()` (line ~105):

```rust
fn default_rate_limit_config() -> RateLimitConfig {
    RateLimitConfig {
        max_tokens: default_max_tokens(),
        refill_rate: default_refill_rate(),
        media_download_cost: default_media_download_cost(),
        transcription_cost: default_transcription_cost(),
    }
}
```

Update the literal in `src/config/tests.rs:132` (`rate_limiting: RateLimitConfig { ... }`) to add `transcription_cost: default_transcription_cost(),` alongside `media_download_cost`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test config -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add src/config.rs src/config/tests.rs
git commit -m "feat: add rate_limiting.transcription_cost config (default 5)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Domain types `TranscriptionState` / `TranscriptionOutcome`

**Files:**
- Create: `src/telegram/types/transcription.rs`
- Modify: `src/telegram/types.rs` (mod decl + re-export)
- Modify: `src/telegram.rs` (re-export, line ~10)

- [ ] **Step 1: Write the failing test**

Create `src/telegram/types/transcription.rs`:

```rust
//! Domain types for voice/video-note transcription.

use super::media::MediaType;

/// One observation of a transcription's progress (from one `transcribeAudio` call).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionState {
    pub transcription_id: i64,
    pub text: String,
    pub pending: bool,
}

/// The final result handed back to the MCP handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionOutcome {
    pub text: String,
    /// True if the timeout elapsed while the transcription was still pending.
    pub partial: bool,
    pub media_type: MediaType,
    pub duration_seconds: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_holds_fields() {
        let outcome = TranscriptionOutcome {
            text: "привет".to_string(),
            partial: false,
            media_type: MediaType::Voice,
            duration_seconds: Some(7),
        };
        assert_eq!(outcome.media_type, MediaType::Voice);
        assert_eq!(outcome.duration_seconds, Some(7));
        assert!(!outcome.partial);
    }
}
```

- [ ] **Step 2: Wire the module + re-exports**

In `src/telegram/types.rs` add the module declaration alongside the others (after `pub mod params;`):

```rust
pub mod transcription;
```

and add the re-export alongside the others:

```rust
pub use transcription::{TranscriptionOutcome, TranscriptionState};
```

In `src/telegram.rs`, extend the `pub use types::{ ... }` block (line ~10-13) to include `TranscriptionOutcome, TranscriptionState` (keep alphabetical-ish ordering):

```rust
pub use types::{
    Channel, ChannelId, ChannelName, HistoryParams, MediaFilter, MediaType, Message, MessageId,
    QueryMetadata, SearchParams, SearchResult, TranscriptionOutcome, TranscriptionState, UserId,
    Username,
};
```

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test --lib telegram::types::transcription`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add src/telegram/types/transcription.rs src/telegram/types.rs src/telegram.rs
git commit -m "feat: add TranscriptionState/TranscriptionOutcome domain types

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Pure transcription helpers (orchestrator, RPC mapping, media check)

**Files:**
- Create: `src/telegram/transcription.rs`
- Modify: `src/telegram.rs` (mod decl)

This task holds all the unit-tested logic. No grammers network calls.

- [ ] **Step 1: Create the module with implementation + tests**

Create `src/telegram/transcription.rs`:

```rust
//! Pure, testable helpers for voice/video-note transcription:
//! the poll-until-complete orchestrator, RPC-error mapping, and the
//! media-type guard. None of these touch the network.

use crate::error::Error;
use crate::telegram::types::{MediaType, TranscriptionState};
use grammers_mtsender::InvocationError;
use std::future::Future;
use std::time::Duration;
use tokio::time::Instant;

/// Seconds between re-invocations of `transcribeAudio` while pending.
pub(crate) const POLL_INTERVAL_SECS: u64 = 2;

/// Reject media that Telegram cannot transcribe.
///
/// Only voice messages and video notes (round videos) carry transcribable
/// audio. Everything else returns [`Error::NotTranscribable`] naming the
/// actual type (lowercased, matching the existing `NoVisualMedia` style).
pub(crate) fn ensure_transcribable(media_type: MediaType) -> Result<(), Error> {
    match media_type {
        MediaType::Voice | MediaType::VideoNote => Ok(()),
        other => Err(Error::NotTranscribable {
            media_type: format!("{:?}", other).to_lowercase(),
        }),
    }
}

/// Map a grammers `InvocationError` from `transcribeAudio` to a typed [`Error`].
///
/// `FLOOD_WAIT` / `FLOOD_PREMIUM_WAIT` (quota exhaustion) reuse the existing
/// `RateLimit` variant so retry-after reporting is consistent with the rest of
/// the connector.
pub(crate) fn map_transcribe_rpc_error(err: InvocationError) -> Error {
    if let InvocationError::Rpc(rpc) = &err {
        match rpc.name.as_str() {
            "PREMIUM_ACCOUNT_REQUIRED" => return Error::PremiumRequired,
            "TRANSCRIPTION_FAILED" => return Error::TranscriptionFailed(rpc.name.clone()),
            "MSG_VOICE_TOO_LONG" => return Error::VoiceTooLong,
            "FLOOD_WAIT" | "FLOOD_PREMIUM_WAIT" => {
                return Error::RateLimit {
                    retry_after_seconds: rpc.value.unwrap_or(0) as u64,
                };
            }
            _ => {}
        }
    }
    Error::TelegramApi(err.to_string())
}

/// Poll a pending transcription until it completes or `timeout` elapses.
///
/// `initial` is the result of the first `transcribeAudio` call. If it is already
/// complete, returns immediately. Otherwise it re-invokes via `poll` every
/// `interval` until a non-pending state arrives (`partial = false`) or the
/// deadline passes (`partial = true`, returning the latest accumulated state).
/// Because the first call already succeeded and seeds `latest`, a transient
/// `poll` failure degrades gracefully to the latest partial rather than erroring.
pub(crate) async fn poll_until_complete<F, Fut>(
    initial: TranscriptionState,
    timeout: Duration,
    interval: Duration,
    mut poll: F,
) -> (TranscriptionState, bool)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<TranscriptionState, Error>>,
{
    if !initial.pending {
        return (initial, false);
    }

    let deadline = Instant::now() + timeout;
    let mut latest = initial;

    while Instant::now() < deadline {
        tokio::time::sleep(interval).await;
        match poll().await {
            Ok(state) => {
                latest = state;
                if !latest.pending {
                    return (latest, false);
                }
            }
            // Transient failure mid-poll: return what we have so far as partial.
            Err(_) => return (latest, true),
        }
    }

    (latest, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpc(name: &str, value: Option<u32>) -> InvocationError {
        InvocationError::Rpc(grammers_mtsender::RpcError {
            code: 400,
            name: name.to_string(),
            value,
            caused_by: None,
        })
    }

    fn state(text: &str, pending: bool) -> TranscriptionState {
        TranscriptionState {
            transcription_id: 1,
            text: text.to_string(),
            pending,
        }
    }

    // ---- ensure_transcribable ----

    #[test]
    fn voice_is_transcribable() {
        assert!(ensure_transcribable(MediaType::Voice).is_ok());
    }

    #[test]
    fn video_note_is_transcribable() {
        assert!(ensure_transcribable(MediaType::VideoNote).is_ok());
    }

    #[test]
    fn photo_is_rejected_naming_type() {
        let err = ensure_transcribable(MediaType::Photo).unwrap_err();
        assert!(matches!(err, Error::NotTranscribable { .. }));
        assert!(err.to_string().contains("photo"));
    }

    // ---- map_transcribe_rpc_error ----

    #[test]
    fn maps_premium_required() {
        assert!(matches!(
            map_transcribe_rpc_error(rpc("PREMIUM_ACCOUNT_REQUIRED", None)),
            Error::PremiumRequired
        ));
    }

    #[test]
    fn maps_transcription_failed() {
        assert!(matches!(
            map_transcribe_rpc_error(rpc("TRANSCRIPTION_FAILED", None)),
            Error::TranscriptionFailed(_)
        ));
    }

    #[test]
    fn maps_voice_too_long() {
        assert!(matches!(
            map_transcribe_rpc_error(rpc("MSG_VOICE_TOO_LONG", None)),
            Error::VoiceTooLong
        ));
    }

    #[test]
    fn maps_flood_wait_to_rate_limit_with_seconds() {
        match map_transcribe_rpc_error(rpc("FLOOD_WAIT", Some(31))) {
            Error::RateLimit { retry_after_seconds } => assert_eq!(retry_after_seconds, 31),
            other => panic!("expected RateLimit, got {other:?}"),
        }
    }

    #[test]
    fn maps_unknown_to_telegram_api() {
        assert!(matches!(
            map_transcribe_rpc_error(rpc("SOMETHING_ELSE", None)),
            Error::TelegramApi(_)
        ));
    }

    // ---- poll_until_complete ----

    #[tokio::test]
    async fn immediate_non_pending_returns_without_polling() {
        let mut calls = 0u32;
        let (final_state, partial) = poll_until_complete(
            state("done", false),
            Duration::from_secs(30),
            Duration::from_secs(2),
            || {
                calls += 1;
                async { Ok(state("unused", false)) }
            },
        )
        .await;
        assert_eq!(calls, 0, "poll must not run for an already-complete initial state");
        assert!(!partial);
        assert_eq!(final_state.text, "done");
    }

    #[tokio::test(start_paused = true)]
    async fn pending_then_completed_returns_full_text() {
        let mut calls = 0u32;
        let (final_state, partial) = poll_until_complete(
            state("partial", true),
            Duration::from_secs(30),
            Duration::from_secs(2),
            || {
                calls += 1;
                let done = calls >= 2;
                async move {
                    Ok(TranscriptionState {
                        transcription_id: 1,
                        text: if done { "full text".to_string() } else { "partial".to_string() },
                        pending: !done,
                    })
                }
            },
        )
        .await;
        assert!(!partial);
        assert_eq!(final_state.text, "full text");
        assert_eq!(calls, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn pending_until_timeout_returns_partial() {
        let (final_state, partial) = poll_until_complete(
            state("so far", true),
            Duration::from_secs(6),
            Duration::from_secs(2),
            || async { Ok(state("so far", true)) },
        )
        .await;
        assert!(partial);
        assert_eq!(final_state.text, "so far");
    }
}
```

- [ ] **Step 2: Wire the module**

In `src/telegram.rs`, add the module declaration after `pub mod converters;`:

```rust
pub mod transcription;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --lib telegram::transcription`
Expected: PASS (the 3 mapping cases + 3 guard cases + 3 orchestrator cases). The paused-time tests complete instantly.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add src/telegram/transcription.rs src/telegram.rs
git commit -m "feat: add transcription orchestrator, RPC mapping, media guard

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Trait extension + production `TelegramClient` impl

**Files:**
- Modify: `src/telegram/trait_def.rs` (add 2 trait methods + import)
- Modify: `src/telegram/client.rs` (struct field, `new()`, trait impl, private helper)
- Modify: `src/telegram/converters.rs` (add `extract_audio_duration`)

No new unit tests here — this is grammers-bound production code verified by `cargo build`/`clippy`. Adding the trait methods regenerates the `mockall` mock used by later tasks. Existing tests stay green because nothing calls the new methods yet.

- [ ] **Step 1: Add `extract_audio_duration` to converters**

In `src/telegram/converters.rs`, after `detect_document_type` (line ~91), add:

```rust
/// Extract the duration (seconds) of a voice message or round video from its
/// document attributes. Returns `None` for media without an audio/video
/// attribute. Used by transcription metadata.
pub fn extract_audio_duration(media: &Media) -> Option<u32> {
    let Media::Document(doc) = media else {
        return None;
    };
    let Some(tl::enums::Document::Document(raw)) = doc.raw.document.as_ref() else {
        return None;
    };
    for attr in &raw.attributes {
        match attr {
            tl::enums::DocumentAttribute::Audio(a) => return Some(a.duration.max(0) as u32),
            tl::enums::DocumentAttribute::Video(v) => return Some(v.duration.max(0.0) as u32),
            _ => {}
        }
    }
    None
}
```

- [ ] **Step 2: Add the trait methods**

In `src/telegram/trait_def.rs`, extend the imports to include the new types:

```rust
use crate::telegram::types::{
    Channel, HistoryParams, MediaDownload, Message, SearchParams, SearchResult,
    TranscriptionOutcome,
};
```

Add to `trait TelegramClientTrait` (before the closing `}`):

```rust
    /// Transcribe a voice / video-note message's audio via `messages.transcribeAudio`.
    ///
    /// Resolves the peer once, validates the media type (rejecting non-voice /
    /// non-video_note with `Error::NotTranscribable`), invokes `TranscribeAudio`,
    /// then polls by re-invoking until the transcription completes or
    /// `timeout_secs` elapses (returning a partial result on timeout).
    async fn transcribe_audio(
        &self,
        channel_ref: &str,
        message_id: i32,
        timeout_secs: u32,
    ) -> Result<TranscriptionOutcome, Error>;

    /// Cached Telegram Premium flag for the connected account. Returns the cached
    /// value; if unknown, performs one `get_me()` and caches it. Returns `None`
    /// only when Premium status could not be determined.
    async fn is_premium(&self) -> Option<bool>;
```

- [ ] **Step 3: Add the premium cache field to the struct + `new()`**

In `src/telegram/client.rs`, add to `struct TelegramClient` (after `timeouts`):

```rust
    /// Cached Premium flag for the connected account (None = not yet determined).
    premium: tokio::sync::RwLock<Option<bool>>,
```

In `TelegramClient::new()`, add to the returned struct literal (after `timeouts: config.timeouts.clone(),`):

```rust
            premium: tokio::sync::RwLock::new(None),
```

- [ ] **Step 4: Implement the trait methods + a private invoke helper**

In `src/telegram/client.rs`, update the converters import (line ~5-8) to add `extract_audio_duration`:

```rust
use crate::telegram::converters::{
    convert_media_filter, convert_media_to_type, convert_message, convert_peer_to_channel,
    extract_audio_duration, matches_media_filter, select_size_candidate, size_candidates,
};
```

Add `TranscriptionOutcome, TranscriptionState` to the types import (line ~10-12):

```rust
use crate::telegram::types::{
    HistoryParams, MediaDownload, MediaType, QueryMetadata, SearchParams, SearchResult,
    TranscriptionOutcome, TranscriptionState,
};
```

Add `use grammers_client::tl;` to the imports (near the other `grammers_client` uses, line ~14).

Add a private helper to the inherent `impl TelegramClient` block (near `resolve_peer`, before its closing `}` at line ~207):

```rust
    /// Invoke `messages.transcribeAudio` once and parse the result into a
    /// [`TranscriptionState`]. Bounded by the history timeout budget.
    async fn invoke_transcribe(
        &self,
        peer: tl::enums::InputPeer,
        msg_id: i32,
    ) -> Result<TranscriptionState, Error> {
        use crate::telegram::transcription::map_transcribe_rpc_error;

        let request = tl::functions::messages::TranscribeAudio { peer, msg_id };
        let result = with_timeout("transcribe_audio", self.timeouts.history_secs, async {
            self.client
                .invoke(&request)
                .await
                .map_err(map_transcribe_rpc_error)
        })
        .await?;

        let tl::enums::messages::TranscribedAudio::Audio(t) = result;
        Ok(TranscriptionState {
            transcription_id: t.transcription_id,
            text: t.text,
            pending: t.pending,
        })
    }
```

Add the two trait methods to the `impl TelegramClientTrait for TelegramClient` block (after `download_message_media`, before the block's closing `}`):

```rust
    async fn is_premium(&self) -> Option<bool> {
        if let Some(cached) = *self.premium.read().await {
            return Some(cached);
        }
        match self.client.get_me().await {
            Ok(me) => {
                let premium = me.is_premium();
                *self.premium.write().await = Some(premium);
                Some(premium)
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to determine Premium status");
                None
            }
        }
    }

    async fn transcribe_audio(
        &self,
        channel_ref: &str,
        message_id: i32,
        timeout_secs: u32,
    ) -> Result<TranscriptionOutcome, Error> {
        use crate::telegram::transcription::{
            POLL_INTERVAL_SECS, ensure_transcribable, poll_until_complete,
        };

        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }

        // Resolve once; reuse the InputPeer for every poll (no repeated dialog walk).
        let peer = self.resolve_peer(channel_ref).await?;
        let peer_ref = peer
            .to_ref()
            .await
            .ok_or_else(|| Error::TelegramApi("Failed to convert peer to PeerRef".to_string()))?;
        let input_peer: tl::enums::InputPeer = peer_ref.clone().into();

        // Fetch the message to validate media type and read duration.
        let messages = with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
            self.client
                .get_messages_by_id(peer_ref, &[message_id])
                .await
                .map_err(|e| Error::TelegramApi(format!("Failed to get message: {}", e)))
        })
        .await?;
        let msg = messages.into_iter().next().flatten().ok_or_else(|| {
            Error::InvalidInput(format!(
                "Message {} not found in channel {}",
                message_id, channel_ref
            ))
        })?;
        let media = msg.media().ok_or_else(|| Error::NotTranscribable {
            media_type: "none".to_string(),
        })?;
        let media_type = convert_media_to_type(&media);
        ensure_transcribable(media_type)?;
        let duration_seconds = extract_audio_duration(&media);

        // Initial transcribeAudio call.
        let initial = self.invoke_transcribe(input_peer.clone(), message_id).await?;

        // Poll (re-invoke) until complete or timeout.
        let (final_state, partial) = poll_until_complete(
            initial,
            StdDuration::from_secs(timeout_secs as u64),
            StdDuration::from_secs(POLL_INTERVAL_SECS),
            || {
                let peer = input_peer.clone();
                async move { self.invoke_transcribe(peer, message_id).await }
            },
        )
        .await;

        Ok(TranscriptionOutcome {
            text: final_state.text,
            partial,
            media_type,
            duration_seconds,
        })
    }
```

- [ ] **Step 5: Build + lint to verify it compiles**

Run: `cargo build && cargo clippy -- -D warnings`
Expected: clean. If the compiler reports the `TranscribedAudio` enum variant name differs from `Audio`, use the name it prints (single-constructor enum). If `peer_ref.clone().into()` is ambiguous, annotate the target type as shown.

- [ ] **Step 6: Run the full test suite (existing tests stay green)**

Run: `cargo test`
Expected: PASS — no behavior change to existing tools; the mock now exposes `expect_transcribe_audio` / `expect_is_premium`.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add src/telegram/trait_def.rs src/telegram/client.rs src/telegram/converters.rs
git commit -m "feat: implement transcribe_audio + is_premium on TelegramClient

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: `premium` field in `check_mcp_status`

**Files:**
- Modify: `src/mcp/tools/types/responses.rs` (`StatusResponse` struct + its serialize test)
- Modify: `src/mcp/server.rs` (`check_mcp_status_impl`, line ~100)
- Modify: `src/mcp/tests/status.rs` (all 4 tests call `is_premium` now)

- [ ] **Step 1: Update the failing test**

In `src/mcp/tests/status.rs`, update `check_status_returns_connection_info` to expect Premium and assert it. Replace the body's mock setup + assertions:

```rust
#[tokio::test]
async fn check_status_returns_connection_info() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_is_connected().return_once(|| true);
    mock_client.expect_is_premium().return_once(|| Some(true));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_available_tokens().return_once(|| 45.5);

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let result = server
        .check_mcp_status(RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_ok());
    let response: StatusResponse = serde_json::from_str(&result.unwrap()).unwrap();
    assert!(response.telegram_connected);
    assert_eq!(response.rate_limiter_tokens, 45.5);
    assert_eq!(response.server_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(response.premium, Some(true));
}
```

Add `mock_client.expect_is_premium().return_once(|| Some(false));` (value irrelevant) to the other three tests (`check_status_reports_disconnected`, `check_status_includes_session_counters`, `check_status_age_is_none_before_first_write`) right after their `expect_is_connected(...)` line.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib mcp::tests::status`
Expected: FAIL — `no field premium` on `StatusResponse` (compile error), and/or mock missing `expect_is_premium` until both sides land.

- [ ] **Step 3: Add the field + populate it**

In `src/mcp/tools/types/responses.rs`, add to `StatusResponse` (after `session_uptime_secs`):

```rust

    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[schemars(description = "Whether the connected account has Telegram Premium (null if unknown)")]
    pub premium: Option<bool>,
```

Update the `status_response_serializes` test literal in the same file (line ~236) to add `premium: Some(true),` after `session_uptime_secs: 60,`.

In `src/mcp/server.rs`, update `check_mcp_status_impl` (line ~100) to populate the field:

```rust
        let response = StatusResponse {
            telegram_connected: connected,
            rate_limiter_tokens: tokens,
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            requests_received: self.metrics.requests_received(),
            responses_written: self.metrics.responses_written(),
            last_response_write_age_secs: self.metrics.last_write_age_secs(),
            session_started_at: self.metrics.session_started_at_rfc3339(),
            session_uptime_secs: self.metrics.uptime_secs(),
            premium: self.telegram_client.is_premium().await,
        };
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib mcp::tests::status responses`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add src/mcp/tools/types/responses.rs src/mcp/server.rs src/mcp/tests/status.rs
git commit -m "feat: report premium flag in check_mcp_status

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: MCP request/response types for the tool

**Files:**
- Modify: `src/mcp/tools/types/requests.rs` (new request + tests)
- Modify: `src/mcp/tools/types/responses.rs` (new response)
- Modify: `src/mcp/tools/types.rs` (re-exports)

- [ ] **Step 1: Write the failing request tests**

Add to `mod tests` in `src/mcp/tools/types/requests.rs`:

```rust
    #[test]
    fn transcribe_request_deserializes_with_flexible_scalars() {
        let json = r#"{"channel_id": 123456, "message_id": "42", "timeout_seconds": "60"}"#;
        let request: TranscribeVoiceMessageRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.channel_id, "123456");
        assert_eq!(request.message_id, 42);
        assert_eq!(request.timeout_seconds, Some(60));
    }

    #[test]
    fn transcribe_request_timeout_defaults_to_none() {
        let json = r#"{"channel_id": "news", "message_id": 42}"#;
        let request: TranscribeVoiceMessageRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.timeout_seconds, None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib requests::tests::transcribe_request_deserializes_with_flexible_scalars`
Expected: FAIL — `cannot find type TranscribeVoiceMessageRequest`.

- [ ] **Step 3: Add the request type**

In `src/mcp/tools/types/requests.rs`, after `GetMessageMediaRequest` (line ~139):

```rust
/// Request for transcribe_voice_message tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TranscribeVoiceMessageRequest {
    #[schemars(description = "Channel ID or username (required)")]
    #[serde(deserialize_with = "flexible_string")]
    pub channel_id: String,

    #[schemars(description = "Message ID within the channel")]
    #[serde(deserialize_with = "flexible_i64")]
    pub message_id: i64,

    #[schemars(
        description = "Seconds to wait for transcription to complete (default: 30, max: 120)"
    )]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub timeout_seconds: Option<u32>,
}
```

- [ ] **Step 4: Add the response type**

In `src/mcp/tools/types/responses.rs`, after the `StatusResponse` struct (or near the other response structs):

```rust
/// Response for transcribe_voice_message tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TranscribeVoiceMessageResponse {
    #[schemars(description = "The transcription text (may be partial)")]
    pub text: String,

    #[schemars(description = "True if the timeout elapsed before Telegram finished transcribing")]
    pub partial: bool,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[schemars(description = "Audio duration in seconds (from message metadata), if available")]
    pub duration_seconds: Option<u32>,

    #[schemars(description = "Media type: \"voice\" or \"video_note\"")]
    pub media_type: String,
}
```

- [ ] **Step 5: Add re-exports**

In `src/mcp/tools/types.rs`, add `TranscribeVoiceMessageRequest` to the `requests::{ ... }` re-export and `TranscribeVoiceMessageResponse` to the `responses::{ ... }` re-export (keep alphabetical ordering):

```rust
pub use requests::{
    GenerateLinkRequest, GetChannelInfoRequest, GetChannelsRequest, GetLastResponsesRequest,
    GetMessageByLinkRequest, GetMessageMediaRequest, GetRecentMessagesRequest, OpenMessageRequest,
    SearchRequest, TranscribeVoiceMessageRequest,
};
pub use responses::{
    BufferedResponseEntry, ChannelsResponse, GetMessageMediaResponse, LastResponsesResponse,
    MessageLinkResponse, MessageResponse, OpenMessageResponse, SearchResponse, StatusResponse,
    TranscribeVoiceMessageResponse,
};
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --lib requests::tests`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add src/mcp/tools/types/requests.rs src/mcp/tools/types/responses.rs src/mcp/tools/types.rs
git commit -m "feat: add transcribe_voice_message request/response types

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: Server wiring — tool #11 + cost builder

**Files:**
- Modify: `src/mcp/server.rs` (imports, struct field, `new()`, builder, `_impl`, `#[tool]` wrapper)
- Modify: `src/mcp/tools.rs` (doc comment "10 MCP tools" → "11")

No standalone test in this task; Task 9 adds the handler tests. End state must compile + existing tests pass.

- [ ] **Step 1: Imports + struct field + default + builder**

In `src/mcp/server.rs`:

Add `Error` and the new types to imports. Add after the existing `use crate::rate_limiter::RateLimiterTrait;`:

```rust
use crate::error::Error;
```

Extend the big `use crate::mcp::tools::{ ... }` block to include `TranscribeVoiceMessageRequest, TranscribeVoiceMessageResponse` (insert alphabetically; e.g. after `StatusResponse,`):

```rust
    StatusResponse, TranscribeVoiceMessageRequest, TranscribeVoiceMessageResponse,
    parse_channel_id, parse_message_id, parse_optional_channel_id,
```

Add the field to `struct McpServer` (after `media_download_cost: u32,`):

```rust
    transcription_cost: u32,
```

Add the default to `new()` (after `media_download_cost: 5,`):

```rust
            transcription_cost: 5,
```

Add the builder after `with_media_download_cost` (line ~67):

```rust
    /// Set the rate-limiter cost charged per transcribe_voice_message call
    /// (`[rate_limiting] transcription_cost`, default 5).
    pub fn with_transcription_cost(mut self, cost: u32) -> Self {
        self.transcription_cost = cost;
        self
    }
```

- [ ] **Step 2: Add the `_impl` method**

In `src/mcp/server.rs`, inside the inherent `impl` block that holds the other `*_impl` methods, after `get_message_media_impl` (before the block's closing `}` at line ~507):

```rust
    async fn transcribe_voice_message_impl(
        &self,
        request: TranscribeVoiceMessageRequest,
    ) -> Result<String, String> {
        const DEFAULT_TIMEOUT: u32 = 30;
        const MAX_TIMEOUT: u32 = 120;

        let message_id = parse_message_id(request.message_id)?;
        let timeout_secs = request
            .timeout_seconds
            .unwrap_or(DEFAULT_TIMEOUT)
            .clamp(1, MAX_TIMEOUT);

        // Premium fast-fail: only a definitive `false` short-circuits (before
        // spending a rate-limit token or a transcribeAudio call). Unknown
        // (`None`) falls through to the RPC-error path.
        if self.telegram_client.is_premium().await == Some(false) {
            return Err(Error::PremiumRequired.to_string());
        }

        // Transcription is precious (Telegram weekly quota); charge more than a search.
        self.rate_limiter
            .acquire(self.transcription_cost)
            .await
            .map_err(|e| e.to_string())?;

        let outcome = self
            .telegram_client
            .transcribe_audio(&request.channel_id, message_id.get() as i32, timeout_secs)
            .await
            .map_err(|e| e.to_string())?;

        // The feature contract specifies "voice" / "video_note"; MediaType's
        // lowercase serialization would emit "videonote", so map explicitly.
        let media_type = match outcome.media_type {
            crate::telegram::types::MediaType::Voice => "voice",
            crate::telegram::types::MediaType::VideoNote => "video_note",
            other => {
                return Err(format!(
                    "unexpected media type for transcription: {:?}",
                    other
                ));
            }
        }
        .to_string();

        let response = TranscribeVoiceMessageResponse {
            text: outcome.text,
            partial: outcome.partial,
            duration_seconds: outcome.duration_seconds,
            media_type,
        };

        tracing::info!(
            channel = %request.channel_id,
            message_id = message_id.get(),
            media_type = %response.media_type,
            partial = response.partial,
            duration_seconds = ?response.duration_seconds,
            "Transcription results"
        );

        serde_json::to_string(&response).map_err(|e| e.to_string())
    }
```

- [ ] **Step 3: Add the `#[tool]` wrapper**

In `src/mcp/server.rs`, inside the `#[tool_router] impl` block, after `get_message_media` (before the block's closing `}` at line ~733):

```rust
    /// Tool 11: transcribe_voice_message - Transcribe a voice/video-note message to text
    #[tool(
        description = "Transcribe a voice message or video note (round video) to text using Telegram's server-side transcription (no local ML). REQUIRES Telegram Premium on the connected account; check_mcp_status reports `premium`. Charged transcription_cost rate-limit tokens (more than a search). Returns partial text with partial=true if the wait times out."
    )]
    pub async fn transcribe_voice_message(
        &self,
        Parameters(request): Parameters<TranscribeVoiceMessageRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let request_id = id.0.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "transcribe_voice_message",
            request_id = %request_id,
            channel_id = %request.channel_id,
            message_id = request.message_id,
            timeout_seconds = ?request.timeout_seconds,
            "Tool invocation started"
        );
        let result = self.transcribe_voice_message_impl(request).await;
        log_tool_outcome("transcribe_voice_message", &request_id, started, &result);
        result
    }
```

- [ ] **Step 4: Update the tool-count doc comment**

In `src/mcp/tools.rs`, change `//! This module contains all 10 MCP tools.` to `//! This module contains all 11 MCP tools.`

- [ ] **Step 5: Build, lint, and run existing tests**

Run: `cargo build && cargo clippy -- -D warnings && cargo test`
Expected: clean + PASS. (No new tests yet; existing tests unaffected.)

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add src/mcp/server.rs src/mcp/tools.rs
git commit -m "feat: wire transcribe_voice_message tool into MCP server

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: Handler tests

**Files:**
- Create: `src/mcp/tests/transcription.rs`
- Modify: `src/mcp/tests.rs` (mod decl)

- [ ] **Step 1: Wire the test module**

In `src/mcp/tests.rs`, add (keep alphabetical with the others):

```rust
#[path = "tests/transcription.rs"]
mod transcription;
```

- [ ] **Step 2: Write the handler tests**

Create `src/mcp/tests/transcription.rs`:

```rust
//! Tests for transcribe_voice_message tool

use crate::error::Error;
use crate::mcp::server::McpServer;
use crate::mcp::tools::{TranscribeVoiceMessageRequest, TranscribeVoiceMessageResponse};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{MediaType, TranscriptionOutcome};
use mockall::predicate::eq;
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

fn request(channel: &str, message_id: i64, timeout: Option<u32>) -> TranscribeVoiceMessageRequest {
    TranscribeVoiceMessageRequest {
        channel_id: channel.to_string(),
        message_id,
        timeout_seconds: timeout,
    }
}

fn server(
    client: MockTelegramClientTrait,
    limiter: MockRateLimiterTrait,
) -> McpServer<MockTelegramClientTrait, MockRateLimiterTrait> {
    McpServer::new(Arc::new(client), Arc::new(limiter)).with_transcription_cost(5)
}

#[tokio::test]
async fn returns_transcription_text() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| Some(true));
    client
        .expect_transcribe_audio()
        .withf(|ch, id, t| ch == "news" && *id == 42 && *t == 30)
        .return_once(|_, _, _| {
            Ok(TranscriptionOutcome {
                text: "привет мир".to_string(),
                partial: false,
                media_type: MediaType::Voice,
                duration_seconds: Some(5),
            })
        });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().with(eq(5)).return_once(|_| Ok(()));

    let result = server(client, limiter)
        .transcribe_voice_message(
            Parameters(request("news", 42, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("should succeed");

    let resp: TranscribeVoiceMessageResponse = serde_json::from_str(&result).unwrap();
    assert_eq!(resp.text, "привет мир");
    assert!(!resp.partial);
    assert_eq!(resp.media_type, "voice");
    assert_eq!(resp.duration_seconds, Some(5));
}

#[tokio::test]
async fn returns_partial_flag_and_video_note_type() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| Some(true));
    client.expect_transcribe_audio().return_once(|_, _, _| {
        Ok(TranscriptionOutcome {
            text: "часть".to_string(),
            partial: true,
            media_type: MediaType::VideoNote,
            duration_seconds: None,
        })
    });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().with(eq(5)).return_once(|_| Ok(()));

    let result = server(client, limiter)
        .transcribe_voice_message(
            Parameters(request("chan", 1, Some(60))),
            RequestId(NumberOrString::Number(2)),
        )
        .await
        .expect("should succeed");

    let resp: TranscribeVoiceMessageResponse = serde_json::from_str(&result).unwrap();
    assert!(resp.partial);
    assert_eq!(resp.media_type, "video_note");
    assert_eq!(resp.duration_seconds, None);
}

#[tokio::test]
async fn premium_absent_fast_fails_without_calling_transcribe() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| Some(false));
    // No expect_transcribe_audio: mockall panics if it is called.
    // No expect_acquire: the rate limiter must not be charged on fast-fail.
    let limiter = MockRateLimiterTrait::new();

    let err = server(client, limiter)
        .transcribe_voice_message(
            Parameters(request("news", 42, None)),
            RequestId(NumberOrString::Number(3)),
        )
        .await
        .expect_err("should fail without Premium");

    assert!(err.contains("Premium"), "error was: {err}");
}

#[tokio::test]
async fn unknown_premium_proceeds() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| None);
    client.expect_transcribe_audio().return_once(|_, _, _| {
        Ok(TranscriptionOutcome {
            text: "ok".to_string(),
            partial: false,
            media_type: MediaType::Voice,
            duration_seconds: None,
        })
    });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().with(eq(5)).return_once(|_| Ok(()));

    let result = server(client, limiter)
        .transcribe_voice_message(
            Parameters(request("news", 42, None)),
            RequestId(NumberOrString::Number(4)),
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn rejects_non_voice_media() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| Some(true));
    client.expect_transcribe_audio().return_once(|_, _, _| {
        Err(Error::NotTranscribable {
            media_type: "photo".to_string(),
        })
    });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().with(eq(5)).return_once(|_| Ok(()));

    let err = server(client, limiter)
        .transcribe_voice_message(
            Parameters(request("news", 7, None)),
            RequestId(NumberOrString::Number(5)),
        )
        .await
        .expect_err("should reject");

    assert!(err.contains("not transcribable"), "error was: {err}");
    assert!(err.contains("photo"), "error was: {err}");
}

#[tokio::test]
async fn surfaces_voice_too_long() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| Some(true));
    client
        .expect_transcribe_audio()
        .return_once(|_, _, _| Err(Error::VoiceTooLong));
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().with(eq(5)).return_once(|_| Ok(()));

    let err = server(client, limiter)
        .transcribe_voice_message(
            Parameters(request("chan", 1, None)),
            RequestId(NumberOrString::Number(6)),
        )
        .await
        .expect_err("should fail");

    assert!(err.contains("length limit"), "error was: {err}");
}

#[tokio::test]
async fn surfaces_flood_wait_retry_after() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| Some(true));
    client
        .expect_transcribe_audio()
        .return_once(|_, _, _| Err(Error::RateLimit { retry_after_seconds: 42 }));
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().with(eq(5)).return_once(|_| Ok(()));

    let err = server(client, limiter)
        .transcribe_voice_message(
            Parameters(request("chan", 1, None)),
            RequestId(NumberOrString::Number(7)),
        )
        .await
        .expect_err("should fail");

    assert!(err.contains("retry after 42"), "error was: {err}");
}
```

- [ ] **Step 3: Run the handler tests**

Run: `cargo test --lib mcp::tests::transcription`
Expected: PASS (7 tests).

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add src/mcp/tests/transcription.rs src/mcp/tests.rs
git commit -m "test: handler tests for transcribe_voice_message

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 10: main.rs wiring (cost + eager Premium warm-up)

**Files:**
- Modify: `src/main.rs` (`run_mcp_server`, line ~99)

- [ ] **Step 1: Warm Premium + apply the cost builder**

In `src/main.rs::run_mcp_server`, before constructing the server, add the warm-up (the `TelegramClientTrait` is already imported at the top of `main.rs`):

```rust
    // Warm the Premium flag so check_mcp_status reports it from the first request.
    let _ = telegram_client.is_premium().await;

    // Create MCP server
    let server = McpServer::new(Arc::new(telegram_client), Arc::new(rate_limiter))
        .with_observability(&config.observability)
        .with_media_download_cost(config.rate_limiting.media_download_cost)
        .with_transcription_cost(config.rate_limiting.transcription_cost);
```

(Place the `is_premium()` call after `let rate_limiter = ...` and before `let server = ...`.)

- [ ] **Step 2: Build + full gate**

Run: `cargo build && cargo clippy -- -D warnings && cargo test`
Expected: clean + PASS (whole suite).

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add src/main.rs
git commit -m "feat: warm Premium flag at startup and apply transcription_cost

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 11: Documentation

**Files:**
- Modify: `README.md` (tool table + Premium callout + tool count)
- Modify: `CHANGELOG.md` (new entry)
- Modify: `docs/tasklist.md` (phase row)

- [ ] **Step 1: README**

Locate the tool reference table in `README.md` (search for an existing tool name such as `get_message_media`). Add a row for `transcribe_voice_message` matching the table's columns, e.g.:

```
| `transcribe_voice_message` | Transcribe a voice message or video note to text via Telegram's server-side transcription. **Requires Telegram Premium.** | `channel_id`, `message_id`, `timeout_seconds?` |
```

Add a callout near the tool table or features section:

```markdown
> **Requires Telegram Premium:** `transcribe_voice_message` uses Telegram's
> server-side `messages.transcribeAudio`, which is only available on accounts
> with Telegram Premium and is subject to Telegram's weekly transcription
> quota. Without Premium the tool returns a clear error. `check_mcp_status`
> reports a `premium` flag so you can tell in advance.
```

If the README states a tool count (e.g. "10 tools"), bump it to 11.

- [ ] **Step 2: CHANGELOG**

Add to `CHANGELOG.md` under `## [Unreleased]` (create the section if absent), in the existing style:

```markdown
### Added
- `transcribe_voice_message` MCP tool: transcribes voice messages and video
  notes to text via Telegram's server-side `messages.transcribeAudio` (no local
  ML). Requires Telegram Premium; polls until completion or `timeout_seconds`
  (default 30, max 120), returning partial text on timeout. Charged
  `rate_limiting.transcription_cost` tokens (default 5).
- `premium` flag in `check_mcp_status` output (eagerly detected at startup).
- `rate_limiting.transcription_cost` config option (default 5).
```

- [ ] **Step 3: tasklist**

Add a row to the progress table in `docs/tasklist.md` (after phase 22):

```
| 23 | Audio Transcription | ✅ Complete | <N> | transcribe_voice_message via messages.transcribeAudio; Premium-gated; poll-by-reinvoke |
```

Set `<N>` to the final test count from `cargo test` output. Update the "Overall Progress" line to `23/23`.

- [ ] **Step 4: Final full verification**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: clean + PASS.

- [ ] **Step 5: Commit**

```bash
git add README.md CHANGELOG.md docs/tasklist.md
git commit -m "docs: document transcribe_voice_message tool and Premium requirement

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Done-when

- `cargo fmt --check && cargo clippy -- -D warnings && cargo test` all pass.
- `transcribe_voice_message` appears in the MCP tool list (11 tools total).
- All test-plan cases from the spec are covered:
  - orchestrator: immediate / pending→completed / pending→timeout (Task 4)
  - RPC mapping per error name (Task 4)
  - non-voice rejection via `ensure_transcribable` (Task 4) and at the handler (Task 9)
  - Premium-absent fast-fail without calling transcribe/limiter (Task 9)
  - complete + partial JSON shaping (Task 9)
  - each RPC-mapped error → string (Task 9)
  - `premium` in `check_mcp_status` (Task 6)
- README + CHANGELOG + tasklist updated.
