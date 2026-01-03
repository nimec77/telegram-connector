//! Media type enums for message content and search filtering.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// All Telegram media types (for message content)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum MediaType {
    #[default]
    None, // Text-only message
    Photo,     // Image
    Video,     // Video file
    Document,  // Generic file
    Audio,     // Audio file (music)
    Voice,     // Voice message
    VideoNote, // Round video message
    Animation, // GIF
    Sticker,   // Sticker
    Contact,   // Shared contact
    Location,  // GPS location
    Venue,     // Location with venue info
    Poll,      // Poll/quiz
    Dice,      // Dice/dart/etc game
}

/// Media filter for search (maps to Telegram's InputMessagesFilter).
///
/// **Important:** This is metadata-based filtering, NOT content recognition.
/// - `Photo` returns messages WITH photos attached
/// - It does NOT search for objects/text inside photos
/// - No OCR, no speech-to-text, no image recognition
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaFilter {
    /// Photos only (inputMessagesFilterPhotos)
    Photo,
    /// Videos only (inputMessagesFilterVideo)
    Video,
    /// Photos and videos combined (inputMessagesFilterPhotoVideo)
    PhotoVideo,
    /// Documents/files (inputMessagesFilterDocument)
    Document,
    /// Music files (inputMessagesFilterMusic)
    Audio,
    /// Voice messages (inputMessagesFilterVoice)
    Voice,
    /// Round video messages (inputMessagesFilterRoundVideo)
    VideoNote,
    /// Animated GIFs (inputMessagesFilterGif)
    Gif,
    /// Messages with URLs (inputMessagesFilterUrl)
    Url,
    /// Pinned messages only (inputMessagesFilterPinned)
    Pinned,
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // MediaType Tests
    // =========================================================================

    #[test]
    fn media_type_default_is_none() {
        assert_eq!(MediaType::default(), MediaType::None);
    }

    #[test]
    fn media_type_serde_lowercase() {
        let json = serde_json::to_string(&MediaType::VideoNote).unwrap();
        assert_eq!(json, "\"videonote\"");
    }

    #[test]
    fn media_type_all_variants_serialize() {
        let variants = vec![
            MediaType::None,
            MediaType::Photo,
            MediaType::Video,
            MediaType::Document,
            MediaType::Audio,
            MediaType::Voice,
            MediaType::VideoNote,
            MediaType::Animation,
            MediaType::Sticker,
            MediaType::Contact,
            MediaType::Location,
            MediaType::Venue,
            MediaType::Poll,
            MediaType::Dice,
        ];

        for variant in variants {
            let json = serde_json::to_string(&variant);
            assert!(json.is_ok());
        }
    }

    // =========================================================================
    // MediaFilter Tests
    // =========================================================================

    #[test]
    fn media_filter_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&MediaFilter::PhotoVideo).unwrap(),
            "\"photo_video\""
        );
        assert_eq!(
            serde_json::to_string(&MediaFilter::VideoNote).unwrap(),
            "\"video_note\""
        );
    }

    #[test]
    fn media_filter_deserializes_from_snake_case() {
        let filter: MediaFilter = serde_json::from_str("\"photo_video\"").unwrap();
        assert_eq!(filter, MediaFilter::PhotoVideo);
    }

    #[test]
    fn media_filter_all_variants_serialize() {
        let variants = vec![
            MediaFilter::Photo,
            MediaFilter::Video,
            MediaFilter::PhotoVideo,
            MediaFilter::Document,
            MediaFilter::Audio,
            MediaFilter::Voice,
            MediaFilter::VideoNote,
            MediaFilter::Gif,
            MediaFilter::Url,
            MediaFilter::Pinned,
        ];

        for variant in variants {
            let json = serde_json::to_string(&variant);
            assert!(json.is_ok(), "Failed to serialize {:?}", variant);
        }
    }

    #[test]
    fn media_filter_roundtrip() {
        let original = MediaFilter::PhotoVideo;
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: MediaFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }
}
