//! Converter tests: thumbnail size selection, raw media filter, forward extraction, link previews.
use super::media::matches_media_filter_raw;
use super::message::{extract_forward_info, extract_link_preview};
use super::*;
use crate::telegram::envelope::EntityLookup;
use crate::telegram::types::{MediaFilter, SizeCandidate};
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

/// Raw message with optional media for the raw filter tests. Flags false,
/// unset options None.
fn raw_message_with_media(
    media: Option<tl::enums::MessageMedia>,
    text: &str,
) -> tl::enums::Message {
    tl::enums::Message::Message(tl::types::Message {
        out: false,
        mentioned: false,
        media_unread: false,
        silent: false,
        post: true,
        from_scheduled: false,
        legacy: false,
        edit_hide: false,
        pinned: false,
        noforwards: false,
        video_processing_pending: false,
        paid_suggested_post_stars: false,
        paid_suggested_post_ton: false,
        invert_media: false,
        offline: false,
        id: 1,
        from_id: None,
        from_boosts_applied: None,
        from_rank: None,
        peer_id: tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 1 }),
        saved_peer_id: None,
        fwd_from: None,
        via_bot_id: None,
        via_business_bot_id: None,
        guestchat_via_from: None,
        reply_to: None,
        date: 1_700_000_000,
        message: text.to_string(),
        media,
        reply_markup: None,
        entities: None,
        views: None,
        forwards: None,
        replies: None,
        edit_date: None,
        post_author: None,
        grouped_id: None,
        restriction_reason: None,
        ttl_period: None,
        reactions: None,
        quick_reply_shortcut_id: None,
        effect: None,
        factcheck: None,
        report_delivery_until_date: None,
        paid_message_stars: None,
        suggested_post: None,
        schedule_repeat_period: None,
        summary_from_language: None,
        rich_message: None,
    })
}

#[test]
fn raw_filter_matches_photo_media() {
    let media = tl::enums::MessageMedia::Photo(tl::types::MessageMediaPhoto {
        spoiler: false,
        photo: Some(tl::enums::Photo::Empty(tl::types::PhotoEmpty { id: 1 })),
        ttl_seconds: None,
        live_photo: false,
        video: None,
    });
    let raw = raw_message_with_media(Some(media), "");
    assert!(matches_media_filter_raw(&raw, &MediaFilter::Photo));
    assert!(!matches_media_filter_raw(&raw, &MediaFilter::Video));
}

#[test]
fn raw_filter_url_matches_text_without_media() {
    let raw = raw_message_with_media(None, "see https://example.com");
    assert!(matches_media_filter_raw(&raw, &MediaFilter::Url));
    assert!(!matches_media_filter_raw(&raw, &MediaFilter::Photo));
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
