use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("telegram API error: {0}")]
    TelegramApi(String),

    #[error("rate limit exceeded{detail}, retry after {retry_after_seconds} seconds")]
    RateLimit {
        retry_after_seconds: u64,
        /// Pre-rendered deficit detail from the local limiter (leading ": "),
        /// or empty when the wait comes from Telegram (FLOOD_WAIT) and no
        /// token arithmetic exists.
        detail: String,
    },

    #[error("configuration error: {0}")]
    Config(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("MCP protocol error: {0}")]
    Mcp(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("operation '{operation}' timed out after {secs}s")]
    Timeout { operation: String, secs: u64 },

    #[error("media too large: {size_bytes} bytes exceeds limit of {max_bytes} bytes")]
    MediaTooLarge { size_bytes: u64, max_bytes: u64 },

    #[error("message has no visual media (media type: {media_type})")]
    NoVisualMedia { media_type: String },

    #[error("media download failed: {0}")]
    DownloadFailed(String),

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
    fn test_config_error_display() {
        let error = Error::Config("missing api_id".to_string());
        assert_eq!(error.to_string(), "configuration error: missing api_id");
    }

    #[test]
    fn test_network_error_display() {
        let error = Error::Network("connection timeout".to_string());
        assert_eq!(error.to_string(), "network error: connection timeout");
    }

    #[test]
    fn test_mcp_error_display() {
        let error = Error::Mcp("invalid request".to_string());
        assert_eq!(error.to_string(), "MCP protocol error: invalid request");
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
