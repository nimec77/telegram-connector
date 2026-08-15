//! Converter tests: audio and video metadata extraction.
use super::*;
use crate::telegram::types::{AudioKind, VideoKind};
use grammers_client::media::{Document, Media};
use grammers_client::tl;

pub(super) fn video_doc(
    round_message: bool,
    duration: f64,
    w: i32,
    h: i32,
    size: i64,
    mime: &str,
    with_thumb: bool,
) -> Media {
    let thumbs = with_thumb.then(|| {
        vec![tl::enums::PhotoSize::Empty(tl::types::PhotoSizeEmpty {
            r#type: "i".to_string(),
        })]
    });
    Media::Document(Document::from_raw_media(tl::types::MessageMediaDocument {
        nopremium: false,
        spoiler: false,
        video: false,
        round: false,
        voice: false,
        document: Some(tl::enums::Document::Document(tl::types::Document {
            id: 1,
            access_hash: 0,
            file_reference: Vec::new(),
            date: 0,
            mime_type: mime.to_string(),
            size,
            thumbs,
            video_thumbs: None,
            dc_id: 0,
            attributes: vec![tl::enums::DocumentAttribute::Video(
                tl::types::DocumentAttributeVideo {
                    round_message,
                    supports_streaming: false,
                    nosound: false,
                    duration,
                    w,
                    h,
                    preload_prefix_size: None,
                    video_start_ts: None,
                    video_codec: None,
                },
            )],
        })),
        alt_documents: None,
        video_cover: None,
        video_timestamp: None,
        ttl_seconds: None,
    }))
}

fn gif_doc(size: i64, with_thumb: bool) -> Media {
    let thumbs = with_thumb.then(|| {
        vec![tl::enums::PhotoSize::Empty(tl::types::PhotoSizeEmpty {
            r#type: "i".to_string(),
        })]
    });
    Media::Document(Document::from_raw_media(tl::types::MessageMediaDocument {
        nopremium: false,
        spoiler: false,
        video: false,
        round: false,
        voice: false,
        document: Some(tl::enums::Document::Document(tl::types::Document {
            id: 1,
            access_hash: 0,
            file_reference: Vec::new(),
            date: 0,
            mime_type: "image/gif".to_string(),
            size,
            thumbs,
            video_thumbs: None,
            dc_id: 0,
            attributes: vec![tl::enums::DocumentAttribute::Animated],
        })),
        alt_documents: None,
        video_cover: None,
        video_timestamp: None,
        ttl_seconds: None,
    }))
}

pub(super) fn audio_doc(
    voice: bool,
    duration: i32,
    size: i64,
    mime: &str,
    title: Option<&str>,
    performer: Option<&str>,
) -> Media {
    Media::Document(Document::from_raw_media(tl::types::MessageMediaDocument {
        nopremium: false,
        spoiler: false,
        video: false,
        round: false,
        voice: false,
        document: Some(tl::enums::Document::Document(tl::types::Document {
            id: 1,
            access_hash: 0,
            file_reference: Vec::new(),
            date: 0,
            mime_type: mime.to_string(),
            size,
            thumbs: None,
            video_thumbs: None,
            dc_id: 0,
            attributes: vec![tl::enums::DocumentAttribute::Audio(
                tl::types::DocumentAttributeAudio {
                    voice,
                    duration,
                    title: title.map(|s| s.to_string()),
                    performer: performer.map(|s| s.to_string()),
                    waveform: None,
                },
            )],
        })),
        alt_documents: None,
        video_cover: None,
        video_timestamp: None,
        ttl_seconds: None,
    }))
}

#[test]
fn extract_video_info_regular_video() {
    let media = video_doc(false, 30.0, 1920, 1080, 5_000_000, "video/mp4", true);
    let info = extract_video_info(&media).expect("video info present");
    assert_eq!(info.kind, VideoKind::Video);
    assert_eq!(info.duration_seconds, 30);
    assert_eq!(info.width, 1920);
    assert_eq!(info.height, 1080);
    assert_eq!(info.file_size_bytes, 5_000_000);
    assert!(info.has_thumbnail);
    assert_eq!(info.mime_type.as_deref(), Some("video/mp4"));
}

#[test]
fn extract_video_info_round_message_is_video_note() {
    let media = video_doc(true, 5.0, 240, 240, 100_000, "video/mp4", true);
    let info = extract_video_info(&media).expect("video info present");
    assert_eq!(info.kind, VideoKind::VideoNote);
}

#[test]
fn extract_video_info_gif_is_animation_with_zero_dims() {
    let media = gif_doc(20_000, true);
    let info = extract_video_info(&media).expect("video info present");
    assert_eq!(info.kind, VideoKind::Animation);
    assert_eq!(info.duration_seconds, 0);
    assert_eq!(info.width, 0);
    assert_eq!(info.height, 0);
    assert!(info.has_thumbnail);
}

#[test]
fn extract_video_info_without_thumbs_is_false() {
    let media = video_doc(false, 10.0, 640, 480, 1_000, "video/mp4", false);
    let info = extract_video_info(&media).expect("video info present");
    assert!(!info.has_thumbnail);
}

#[test]
fn extract_video_info_none_for_audio() {
    let media = audio_doc(true, 7, 1000, "audio/ogg", None, None);
    assert!(extract_video_info(&media).is_none());
}

#[test]
fn extract_audio_info_voice() {
    let media = audio_doc(true, 7, 1000, "audio/ogg", None, None);
    let info = extract_audio_info(&media).expect("audio info present");
    assert_eq!(info.kind, AudioKind::Voice);
    assert_eq!(info.duration_seconds, 7);
    assert_eq!(info.file_size_bytes, 1000);
    assert_eq!(info.mime_type.as_deref(), Some("audio/ogg"));
}

#[test]
fn extract_audio_info_music() {
    let media = audio_doc(false, 200, 4_000_000, "audio/mpeg", None, None);
    let info = extract_audio_info(&media).expect("audio info present");
    assert_eq!(info.kind, AudioKind::Audio);
}

#[test]
fn extract_audio_info_none_for_video() {
    let media = video_doc(false, 30.0, 1920, 1080, 5_000_000, "video/mp4", true);
    assert!(extract_audio_info(&media).is_none());
}

#[test]
fn audio_info_carries_title_and_performer() {
    let media = audio_doc(
        false,
        184,
        7_340_032,
        "audio/mpeg",
        Some("Ноктюрн"),
        Some("Шопен"),
    );

    let info = extract_audio_info(&media).expect("audio info present");

    assert_eq!(info.title.as_deref(), Some("Ноктюрн"));
    assert_eq!(info.performer.as_deref(), Some("Шопен"));
    assert_eq!(info.duration_seconds, 184);
}

#[test]
fn audio_info_without_id3_metadata_omits_title_and_performer() {
    let media = audio_doc(true, 12, 4096, "audio/ogg", None, None);

    let info = extract_audio_info(&media).expect("audio info present");

    assert_eq!(info.title, None);
    assert_eq!(info.performer, None);
    assert_eq!(info.kind, AudioKind::Voice);
}

#[test]
fn audio_info_omits_absent_title_from_json() {
    let media = audio_doc(true, 12, 4096, "audio/ogg", None, None);
    let info = extract_audio_info(&media).expect("audio info present");

    let json = serde_json::to_value(&info).expect("serializes");

    assert!(json.get("title").is_none());
    assert!(json.get("performer").is_none());
}
