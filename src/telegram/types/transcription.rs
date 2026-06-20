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
