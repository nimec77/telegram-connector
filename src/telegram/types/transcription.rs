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
    /// Resolved source of the transcribed message.
    pub channel_id: super::ids::ChannelId,
    /// Source namespace.
    pub peer_namespace: super::ids::PeerNamespace,
    /// Message ID inside that source.
    pub message_id: super::ids::MessageId,
    pub text: String,
    /// True when expiry or a polling failure prevented a complete result.
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
            channel_id: crate::telegram::types::ChannelId::new(100).expect("fixture channel"),
            peer_namespace: crate::telegram::types::PeerNamespace::Channel,
            message_id: crate::telegram::types::MessageId::new(1).expect("fixture message"),
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
