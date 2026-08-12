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
                    detail: String::new(),
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
            Error::RateLimit {
                retry_after_seconds,
                detail,
            } => {
                assert_eq!(retry_after_seconds, 31);
                assert_eq!(detail, "");
            }
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
        assert_eq!(
            calls, 0,
            "poll must not run for an already-complete initial state"
        );
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
                        text: if done {
                            "full text".to_string()
                        } else {
                            "partial".to_string()
                        },
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
