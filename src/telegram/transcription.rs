//! Pure, testable helpers for voice/video-note transcription:
//! the poll-until-complete orchestrator, RPC-error mapping, and the
//! media-type guard. None of these touch the network.
//!
//! [`TranscriptionDeadline`] is a compatibility path required by the public
//! [`crate::TelegramClientTrait::transcribe_audio`] signature.

use crate::error::Error;
use crate::telegram::types::{MediaType, TranscriptionState};
use grammers_mtsender::InvocationError;
use std::future::Future;
use std::time::Duration;
use tokio::time::Instant;

/// One absolute budget shared by every stage of a transcription request.
#[derive(Debug, Clone, Copy)]
pub struct TranscriptionDeadline {
    at: Instant,
    timeout_secs: u32,
}

impl TranscriptionDeadline {
    /// Start the budget after local validation and rate-limit acquisition.
    ///
    /// Zero is an already-expired budget: production transcription returns a
    /// typed timeout before any stage can start. Copying a deadline preserves its
    /// absolute instant; it does not renew the budget. Use the validated MCP
    /// transcription settings for configured requests.
    pub fn new(timeout_secs: u32) -> Self {
        Self {
            at: Instant::now() + Duration::from_secs(u64::from(timeout_secs)),
            timeout_secs,
        }
    }

    /// Absolute Tokio instant shared across all stages.
    pub fn instant(self) -> Instant {
        self.at
    }

    /// Original requested budget, retained for timeout diagnostics.
    pub fn timeout_secs(self) -> u32 {
        self.timeout_secs
    }

    fn expired(self) -> bool {
        Instant::now() >= self.at
    }

    fn timeout_error(self) -> Error {
        Error::Timeout {
            operation: "transcribe_audio".to_string(),
            secs: u64::from(self.timeout_secs),
        }
    }

    /// Cancel a stage at the shared deadline; completion at expiry is too late.
    /// Existing per-RPC timeout wrappers remain inside this outer bound.
    pub(crate) async fn run<T>(
        self,
        future: impl Future<Output = Result<T, Error>>,
    ) -> Result<T, Error> {
        let mut future = std::pin::pin!(future);
        let guarded = std::future::poll_fn(|cx| {
            // Check every resumption, before a compound future can advance to
            // another RPC when its current await completes at expiry.
            if self.expired() {
                std::task::Poll::Ready(Err(self.timeout_error()))
            } else {
                future.as_mut().poll(cx)
            }
        });
        let result = tokio::time::timeout_at(self.at, guarded).await;
        // A stage can also finish after using the remaining time in one poll.
        // Keep the same strict expiry rule for that completion.
        if self.expired() {
            return Err(self.timeout_error());
        }
        match result {
            Ok(result) => result,
            Err(_) => Err(self.timeout_error()),
        }
    }
}

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

/// Poll a pending transcription using the operation's existing absolute deadline.
///
/// `initial` is the result of the first `transcribeAudio` call. If it is already
/// complete, returns immediately. Otherwise it re-invokes via `poll` every
/// `interval` until a non-pending state arrives (`partial = false`) or the
/// deadline passes (`partial = true`, returning the latest accumulated state).
/// Because the first call already succeeded and seeds `latest`, a transient
/// `poll` failure degrades gracefully to the latest partial rather than erroring.
pub(crate) async fn poll_until_complete<F, Fut>(
    initial: TranscriptionState,
    deadline: TranscriptionDeadline,
    interval: Duration,
    mut poll: F,
) -> (TranscriptionState, bool)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<TranscriptionState, Error>>,
{
    if deadline.expired() {
        return (initial, true);
    }
    if !initial.pending {
        return (initial, false);
    }
    let mut latest = initial;
    loop {
        let wake_at = (Instant::now() + interval).min(deadline.instant());
        tokio::time::sleep_until(wake_at).await;
        // Do not even construct a new RPC future once the deadline is reached.
        if deadline.expired() {
            return (latest, true);
        }
        match deadline.run(poll()).await {
            Ok(state) => {
                latest = state;
                if !latest.pending {
                    return (latest, false);
                }
            }
            // Expiry or a transient poll failure preserves the last observed text.
            Err(_) => return (latest, true),
        }
    }
}

#[cfg(test)]
#[path = "tests/transcription_tests.rs"]
mod tests;
