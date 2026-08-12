use super::message::{extract_forward_info, extract_link_preview};
use super::*;
use crate::telegram::envelope::EntityLookup;
use crate::telegram::types::{AudioKind, SizeCandidate, VideoKind};
use grammers_client::media::{Document, Media};
use grammers_client::tl;

fn candidate(width: u32, height: u32, size_bytes: u64, tag: &str) -> SizeCandidate {
    SizeCandidate {
        width,
        height,
        size_bytes,
        photo_type: tag.to_string(),
    }
}

#[test]
fn selects_smallest_candidate_that_satisfies_max_dimension() {
    let candidates = vec![
        candidate(320, 180, 10_000, "m"),
        candidate(1280, 720, 100_000, "x"),
        candidate(2560, 1440, 400_000, "y"),
    ];
    let selected = select_size_candidate(&candidates, 1280).unwrap();
    assert_eq!(selected.photo_type, "x");
}

#[test]
fn falls_back_to_largest_when_none_satisfies() {
    let candidates = vec![
        candidate(320, 180, 10_000, "m"),
        candidate(800, 450, 40_000, "x"),
    ];
    let selected = select_size_candidate(&candidates, 1280).unwrap();
    assert_eq!(selected.photo_type, "x");
}

#[test]
fn empty_candidates_returns_none() {
    assert!(select_size_candidate(&[], 1280).is_none());
}

#[test]
fn longest_side_is_what_counts() {
    // 720x1280 portrait qualifies for max_dimension 1280 via its height.
    let candidates = vec![
        candidate(720, 1280, 90_000, "x"),
        candidate(1440, 2560, 300_000, "y"),
    ];
    let selected = select_size_candidate(&candidates, 1280).unwrap();
    assert_eq!(selected.photo_type, "x");
}

#[test]
fn tie_on_longest_side_picks_first_candidate() {
    // Both candidates have longest_side == 1280; min_by_key returns the first of equals.
    let candidates = vec![
        candidate(1280, 720, 100_000, "x"),
        candidate(720, 1280, 90_000, "y"),
    ];
    let selected = select_size_candidate(&candidates, 1280).unwrap();
    assert_eq!(selected.photo_type, "x");
}

#[test]
fn single_candidate_below_threshold_is_returned_via_fallback() {
    // No candidate satisfies max_dimension; fallback returns the largest (only) one.
    let candidates = vec![candidate(320, 180, 10_000, "m")];
    let selected = select_size_candidate(&candidates, 1280).unwrap();
    assert_eq!(selected.photo_type, "m");
}

fn fwd_header() -> tl::types::MessageFwdHeader {
    tl::types::MessageFwdHeader {
        imported: false,
        saved_out: false,
        from_id: None,
        from_name: None,
        date: 1_700_000_000,
        channel_post: None,
        post_author: None,
        saved_from_peer: None,
        saved_from_msg_id: None,
        saved_from_id: None,
        saved_from_name: None,
        saved_date: None,
        psa_type: None,
    }
}

fn webpage_media(
    url: &str,
    site_name: Option<&str>,
    title: Option<&str>,
    description: Option<String>,
) -> tl::types::MessageMediaWebPage {
    tl::types::MessageMediaWebPage {
        force_large_media: false,
        force_small_media: false,
        manual: false,
        safe: false,
        webpage: tl::enums::WebPage::Page(tl::types::WebPage {
            has_large_media: false,
            video_cover_photo: false,
            id: 1,
            url: url.to_string(),
            display_url: url.to_string(),
            hash: 0,
            r#type: None,
            site_name: site_name.map(|s| s.to_string()),
            title: title.map(|s| s.to_string()),
            description,
            photo: None,
            embed_url: None,
            embed_type: None,
            embed_width: None,
            embed_height: None,
            duration: None,
            author: None,
            document: None,
            cached_page: None,
            attributes: None,
        }),
    }
}

#[test]
fn forward_from_channel_without_envelope_extracts_ids_only() {
    let mut header = fwd_header();
    header.from_id = Some(tl::enums::Peer::Channel(tl::types::PeerChannel {
        channel_id: 555,
    }));
    header.channel_post = Some(42);

    let info = extract_forward_info(&header, &EntityLookup::empty());
    assert_eq!(info.channel_id.map(|c| c.get()), Some(555));
    assert_eq!(info.original_message_id.map(|m| m.get()), Some(42));
    assert_eq!(info.original_date.unwrap().timestamp(), 1_700_000_000);
    assert!(info.channel_name.is_none());
    assert!(info.channel_username.is_none());
    assert!(info.sender_name.is_none());
}

#[test]
fn forward_from_hidden_user_has_name_only() {
    let mut header = fwd_header();
    header.from_name = Some("Hidden User".to_string());

    let info = extract_forward_info(&header, &EntityLookup::empty());
    assert_eq!(info.sender_name.as_deref(), Some("Hidden User"));
    assert!(info.channel_id.is_none());
    assert!(info.original_message_id.is_none());
}

#[test]
fn forward_with_zero_date_has_no_original_date() {
    let mut header = fwd_header();
    header.date = 0;

    let info = extract_forward_info(&header, &EntityLookup::empty());
    assert!(info.original_date.is_none());
}

#[test]
fn link_preview_extracted_and_description_truncated_to_500_chars() {
    let media = webpage_media(
        "https://example.com/article",
        Some("Example"),
        Some("Headline"),
        Some("я".repeat(600)),
    );

    let preview = extract_link_preview(&media).unwrap();
    assert_eq!(preview.url, "https://example.com/article");
    assert_eq!(preview.site_name.as_deref(), Some("Example"));
    assert_eq!(preview.title.as_deref(), Some("Headline"));
    assert_eq!(preview.description.as_ref().unwrap().chars().count(), 500);
}

#[test]
fn link_preview_empty_webpage_returns_none() {
    let media = tl::types::MessageMediaWebPage {
        force_large_media: false,
        force_small_media: false,
        manual: false,
        safe: false,
        webpage: tl::enums::WebPage::Empty(tl::types::WebPageEmpty { id: 0, url: None }),
    };
    assert!(extract_link_preview(&media).is_none());
}

fn video_doc(
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

fn audio_doc(voice: bool, duration: i32, size: i64, mime: &str) -> Media {
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
                    title: None,
                    performer: None,
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
    let media = audio_doc(true, 7, 1000, "audio/ogg");
    assert!(extract_video_info(&media).is_none());
}

#[test]
fn extract_audio_info_voice() {
    let media = audio_doc(true, 7, 1000, "audio/ogg");
    let info = extract_audio_info(&media).expect("audio info present");
    assert_eq!(info.kind, AudioKind::Voice);
    assert_eq!(info.duration_seconds, 7);
    assert_eq!(info.file_size_bytes, 1000);
    assert_eq!(info.mime_type.as_deref(), Some("audio/ogg"));
}

#[test]
fn extract_audio_info_music() {
    let media = audio_doc(false, 200, 4_000_000, "audio/mpeg");
    let info = extract_audio_info(&media).expect("audio info present");
    assert_eq!(info.kind, AudioKind::Audio);
}

#[test]
fn extract_audio_info_none_for_video() {
    let media = video_doc(false, 30.0, 1920, 1080, 5_000_000, "video/mp4", true);
    assert!(extract_audio_info(&media).is_none());
}
