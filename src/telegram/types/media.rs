//! Media type enums for message content and search filtering.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// All Telegram media types (for message content)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
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

/// Video-class media metadata, derived entirely from a message's document
/// attributes (no network calls). Present only for video / video_note /
/// animation media.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VideoInfo {
    pub duration_seconds: u32,
    pub width: u32,
    pub height: u32,
    pub file_size_bytes: u64,
    pub kind: VideoKind,
    pub has_thumbnail: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mime_type: Option<String>,
}

/// Closed set of video-class kinds. Dedicated (not reused `MediaType`) so the
/// advertised JSON schema is exactly `video | video_note | animation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VideoKind {
    Video,
    VideoNote,
    Animation,
}

/// Audio-class media metadata (zero-cost, same source as `VideoInfo`).
/// Present only for audio (music) / voice media. Pairs with the transcription
/// tool: duration tells the client whether a voice message is worth a quota call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AudioInfo {
    pub duration_seconds: u32,
    pub file_size_bytes: u64,
    pub kind: AudioKind,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mime_type: Option<String>,
}

/// Closed set of audio-class kinds (`audio | voice`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioKind {
    Audio,
    Voice,
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

/// Raw media bytes downloaded from Telegram plus source metadata.
///
/// Produced by `TelegramClientTrait::download_message_media`; consumed by the
/// MCP-layer image pipeline. Carries no grammers types so it can flow through
/// the mockable trait boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaDownload {
    /// Raw downloaded image bytes (JPEG as served by Telegram).
    pub bytes: Vec<u8>,
    /// What the source message's media was (Photo, Video, Animation, VideoNote).
    pub media_type: MediaType,
    /// True when `bytes` is a video-like media's thumbnail, not the media itself.
    pub is_thumbnail: bool,
    /// Message caption (`msg.text()` on media messages), None when empty.
    pub caption: Option<String>,
    /// Pixel width of the downloaded size variant, if Telegram reported it.
    pub width: Option<u32>,
    /// Pixel height of the downloaded size variant, if Telegram reported it.
    pub height: Option<u32>,
    /// Byte size of the downloaded size variant.
    pub source_size_bytes: u64,
    /// Zero-cost video metadata for video-class media (`None` for photos).
    pub video_info: Option<VideoInfo>,
}

/// A downloadable size variant of a photo or thumbnail, decoupled from
/// grammers `PhotoSize` so size selection is a pure, testable function.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeCandidate {
    pub width: u32,
    pub height: u32,
    pub size_bytes: u64,
    /// Telegram thumbnail type tag (e.g. "m", "x", "y") used to map the
    /// selection back to the grammers PhotoSize to download.
    pub photo_type: String,
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
    fn media_type_serde_snake_case() {
        let json = serde_json::to_string(&MediaType::VideoNote).unwrap();
        assert_eq!(json, "\"video_note\"");
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

    #[test]
    fn media_download_construction() {
        let download = MediaDownload {
            bytes: vec![0xff, 0xd8],
            media_type: MediaType::Photo,
            is_thumbnail: false,
            caption: Some("a chart".to_string()),
            width: Some(1280),
            height: Some(720),
            source_size_bytes: 2,
            video_info: None,
        };
        assert_eq!(download.media_type, MediaType::Photo);
        assert!(!download.is_thumbnail);
    }

    #[test]
    fn size_candidate_construction() {
        let candidate = SizeCandidate {
            width: 800,
            height: 600,
            size_bytes: 50_000,
            photo_type: "x".to_string(),
        };
        assert_eq!(candidate.width.max(candidate.height), 800);
    }

    // =========================================================================
    // VideoInfo / AudioInfo Tests
    // =========================================================================

    #[test]
    fn video_kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&VideoKind::VideoNote).unwrap(),
            "\"video_note\""
        );
        assert_eq!(
            serde_json::to_string(&VideoKind::Animation).unwrap(),
            "\"animation\""
        );
        assert_eq!(
            serde_json::to_string(&VideoKind::Video).unwrap(),
            "\"video\""
        );
    }

    #[test]
    fn audio_kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&AudioKind::Voice).unwrap(),
            "\"voice\""
        );
        assert_eq!(
            serde_json::to_string(&AudioKind::Audio).unwrap(),
            "\"audio\""
        );
    }

    #[test]
    fn video_info_omits_mime_type_when_none() {
        let info = VideoInfo {
            duration_seconds: 0,
            width: 0,
            height: 0,
            file_size_bytes: 100,
            kind: VideoKind::Animation,
            has_thumbnail: false,
            mime_type: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("mime_type").is_none());
        assert_eq!(json["kind"], "animation");
        assert_eq!(json["duration_seconds"], 0);
    }

    #[test]
    fn audio_info_roundtrips() {
        let info = AudioInfo {
            duration_seconds: 12,
            file_size_bytes: 2048,
            kind: AudioKind::Voice,
            mime_type: Some("audio/ogg".to_string()),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: AudioInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, info);
    }
}
