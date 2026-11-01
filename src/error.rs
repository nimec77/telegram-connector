//! Typed domain/client failures and fixed unavailable-state identities.
//!
//! Canonical MCP boundaries map variants to safe ToolError values; legacy adapters
//! preserve text errors. Application setup/configuration use contextual `anyhow`. String payloads
//! below are not automatically redacted: constructors/callers must exclude secrets.
#![warn(missing_docs)]

use thiserror::Error;

/// Domain/client operation failure. Existing exhaustive matches must handle new variants.
#[derive(Error, Debug, Clone)]
pub enum Error {
    /// A recovery key is already reserved or retained.
    #[error("recovery key is already pending or retained")]
    DuplicateRecoveryKey,
    /// All recovery slots are occupied by pending work, or recovery is disabled.
    #[error("recovery capacity is unavailable")]
    RecoveryCapacityReached,
    /// A selected operation has not completed.
    #[error("response is still pending")]
    ResponsePending,
    /// A selected response is unknown, expired, cancelled or evicted.
    #[error("response is unavailable")]
    ResponseUnavailable,
    /// Response metadata exceeds the applicable serialized output budget.
    #[error("response metadata exceeds the serialized output budget")]
    ResponseTooLarge,
    /// Failure in local response assembly or a worker task; no provider payload.
    #[error("internal error: {0}")]
    Internal(&'static str),
    /// The requested desktop operation is unavailable on this platform.
    #[error("open_message_in_telegram is only supported on macOS")]
    UnsupportedPlatform,
    /// The local desktop launcher refused or could not execute the request.
    #[error("{message}")]
    DesktopOpenFailed {
        /// Safe process failure description.
        message: String,
        /// Validated link attempted by the launcher.
        link: String,
    },
    /// Every channel attempt failed; retain each typed cause until output conversion.
    #[error("{}", display_channel_failures(.0))]
    AllChannelsFailed(Vec<(String, Error)>),

    /// A valid message identifier has no live message in the resolved channel.
    #[error("invalid input: Message {message_id} not found or deleted in channel {channel}")]
    MessageNotFound {
        /// Supplied channel reference.
        channel: String,
        /// Positive message ID.
        message_id: i64,
    },
    /// No eligible channel/group matches the supplied reference.
    #[error("invalid input: Channel not found: {0}")]
    ChannelNotFound(String),
    /// A bare ID or title identifies more than one distinct channel/group.
    #[error("ambiguous channel reference: {0}; use an unambiguous username")]
    AmbiguousChannel(String),
    /// More than one subscribed channel/group has the requested title.
    #[error("ambiguous title: {count} subscribed chats are named '{identifier}'")]
    AmbiguousTitle {
        /// Original title.
        identifier: String,
        /// Number of distinct matches.
        count: usize,
    },
    /// A resolved peer is outside the channel/group tool domain.
    #[error("not a channel or group")]
    UnsupportedPeer,

    /// Shared state was interrupted; its protected contents cannot be trusted.
    #[error(transparent)]
    Unavailable(#[from] StateUnavailable),

    /// Authentication/session setup failure; interactive authentication redacts upstream details.
    #[error("authentication failed: {0}")]
    Auth(String),
    /// Telegram API or transport failure in the established client error format.
    #[error("telegram API error: {0}")]
    TelegramApi(String),
    /// Local token admission or upstream FLOOD_WAIT refusal; no automatic retry occurs.
    #[error("rate limit exceeded{detail}, retry after {retry_after_seconds} seconds")]
    RateLimit {
        /// Suggested wait before retry, in seconds.
        retry_after_seconds: u64,
        /// Pre-rendered local deficit detail (leading ": "), or empty for upstream waits.
        detail: String,
    },
    /// Invalid arguments or a referenced entity that cannot be found/represented.
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// An async operation exceeded its configured budget; its future was dropped.
    #[error("operation '{operation}' timed out after {secs}s")]
    Timeout {
        /// Stable operation identity, such as `transcribe_audio`.
        operation: String,
        /// Configured budget in seconds, not a measured elapsed duration.
        secs: u64,
    },
    /// Reported or streamed media exceeded the download cap.
    #[error("media too large: {size_bytes} bytes exceeds limit of {max_bytes} bytes")]
    MediaTooLarge {
        /// Reported size or observed transfer size at refusal.
        size_bytes: u64,
        /// Configured byte cap.
        max_bytes: u64,
    },
    /// The message has neither a supported image nor a usable visual thumbnail.
    #[error("message has no visual media (media type: {media_type})")]
    NoVisualMedia {
        /// Observed media type in the existing caller-facing vocabulary.
        media_type: String,
    },
    /// Media transfer, decode or encode failure.
    #[error("media download failed: {0}")]
    DownloadFailed(String),
    /// Download succeeded, but bounded image shrinking could not meet the payload cap.
    /// Distinct from DownloadFailed for MCP per-item reason tokens.
    #[error("image could not be reduced below the {limit}-byte payload cap")]
    PayloadCapExceeded {
        /// Requested maximum base64 payload bytes.
        limit: usize,
    },
    /// The connected account lacks Telegram Premium for transcription.
    #[error("transcription requires Telegram Premium on the connected account")]
    PremiumRequired,
    /// Telegram refused or failed transcription.
    #[error("transcription failed: {0}")]
    TranscriptionFailed(String),
    /// Audio exceeds Telegram's transcription length limit.
    #[error("audio exceeds Telegram's transcription length limit")]
    VoiceTooLong,
    /// Only voice messages and video notes are accepted for transcription.
    #[error(
        "message is not transcribable (media type: {media_type}); only voice and video_note are supported"
    )]
    NotTranscribable {
        /// Actual media type in the existing caller-facing vocabulary.
        media_type: String,
    },
}

/// Fixed identities for unavailable synchronized state. No panic payload or
/// protected contents are retained in this error; no poison recovery is attempted.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateUnavailable {
    /// Account admission and token snapshots cannot trust the token bucket.
    #[error("rate limiter unavailable: interrupted critical section")]
    RateLimiter,
    /// Last successful write timestamp is unavailable, rather than absent.
    #[error("last response write unavailable: interrupted critical section")]
    LastWrite,
    /// Request-to-response correlation is unavailable, rather than unmatched.
    #[error("request correlation unavailable: interrupted critical section")]
    InFlight,
    /// Enabled replay storage is unavailable, rather than empty or disabled.
    #[error("response replay unavailable: interrupted critical section")]
    ResponseBuffer,
}

// Legacy output serialization is intentionally textual. Canonical output maps variants
// to ToolError before serializing, so provider details cannot cross that boundary.
impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::ChannelNotFound(reference) => {
                serializer.serialize_str(&format!("Channel not found: {reference}"))
            }
            Self::UnsupportedPeer => serializer.serialize_str("Not a channel or group"),
            _ => serializer.collect_str(self),
        }
    }
}

fn display_channel_failures(failures: &[(String, Error)]) -> String {
    format!(
        "all {} channels failed: {}",
        failures.len(),
        failures
            .iter()
            .map(|(channel, error)| format!("{channel}: {error}"))
            .collect::<Vec<_>>()
            .join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_error_display() {
        let error = Error::Auth("invalid credentials".to_string());
        assert_eq!(
            error.to_string(),
            "authentication failed: invalid credentials"
        );
    }

    #[test]
    fn test_telegram_api_error_display() {
        let error = Error::TelegramApi("flood wait".to_string());
        assert_eq!(error.to_string(), "telegram API error: flood wait");
    }

    #[test]
    fn test_rate_limit_error_display() {
        let error = Error::RateLimit {
            retry_after_seconds: 5,
            detail: String::new(),
        };
        assert_eq!(
            error.to_string(),
            "rate limit exceeded, retry after 5 seconds"
        );
    }

    #[test]
    fn test_rate_limit_error_display_with_deficit_detail() {
        let error = Error::RateLimit {
            retry_after_seconds: 2,
            detail: ": requested 5 tokens, 2.40 available".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "rate limit exceeded: requested 5 tokens, 2.40 available, retry after 2 seconds"
        );
    }

    #[test]
    fn test_error_debug_format() {
        let error = Error::Auth("test".to_string());
        let debug_output = format!("{:?}", error);
        assert!(debug_output.contains("Auth"));
        assert!(debug_output.contains("test"));
    }

    #[test]
    fn test_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    }

    #[test]
    fn test_invalid_input_error_display() {
        let error = Error::InvalidInput("Channel ID must be positive".to_string());
        assert_eq!(
            error.to_string(),
            "invalid input: Channel ID must be positive"
        );
    }

    #[test]
    fn test_timeout_error_display() {
        let error = Error::Timeout {
            operation: "search_messages".to_string(),
            secs: 120,
        };
        assert_eq!(
            error.to_string(),
            "operation 'search_messages' timed out after 120s"
        );
    }

    #[test]
    fn test_media_too_large_error_display() {
        let error = Error::MediaTooLarge {
            size_bytes: 25_000_000,
            max_bytes: 20_971_520,
        };
        assert_eq!(
            error.to_string(),
            "media too large: 25000000 bytes exceeds limit of 20971520 bytes"
        );
    }

    #[test]
    fn test_no_visual_media_error_display() {
        let error = Error::NoVisualMedia {
            media_type: "poll".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "message has no visual media (media type: poll)"
        );
    }

    #[test]
    fn test_download_failed_error_display() {
        let error = Error::DownloadFailed("connection reset".to_string());
        assert_eq!(error.to_string(), "media download failed: connection reset");
    }

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
}
