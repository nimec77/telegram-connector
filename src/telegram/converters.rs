//! Type conversion helpers for grammers types to our domain types

use crate::telegram::types::{
    AudioInfo, AudioKind, Channel, ChannelId, ChannelName, ForwardInfo, LinkPreview, MediaFilter,
    MediaType, Message, MessageId, SizeCandidate, UserId, Username, VideoInfo, VideoKind,
};
use chrono::{DateTime, Utc};
use grammers_client::media::{Document, Media, PhotoSize};
use grammers_client::tl;

/// Convert our MediaFilter enum to grammers MessagesFilter for server-side filtering
pub fn convert_media_filter(filter: &MediaFilter) -> tl::enums::MessagesFilter {
    match filter {
        MediaFilter::Photo => tl::enums::MessagesFilter::InputMessagesFilterPhotos,
        MediaFilter::Video => tl::enums::MessagesFilter::InputMessagesFilterVideo,
        MediaFilter::PhotoVideo => tl::enums::MessagesFilter::InputMessagesFilterPhotoVideo,
        MediaFilter::Document => tl::enums::MessagesFilter::InputMessagesFilterDocument,
        MediaFilter::Audio => tl::enums::MessagesFilter::InputMessagesFilterMusic,
        MediaFilter::Voice => tl::enums::MessagesFilter::InputMessagesFilterVoice,
        MediaFilter::VideoNote => tl::enums::MessagesFilter::InputMessagesFilterRoundVideo,
        MediaFilter::Gif => tl::enums::MessagesFilter::InputMessagesFilterGif,
        MediaFilter::Url => tl::enums::MessagesFilter::InputMessagesFilterUrl,
        MediaFilter::Pinned => tl::enums::MessagesFilter::InputMessagesFilterPinned,
    }
}

/// Convert grammers Media to our MediaType enum
///
/// Inspects the media type and document attributes to determine the correct MediaType.
pub fn convert_media_to_type(media: &Media) -> MediaType {
    match media {
        Media::Photo(_) => MediaType::Photo,
        Media::Sticker(_) => MediaType::Sticker,
        Media::Contact(_) => MediaType::Contact,
        Media::Poll(_) => MediaType::Poll,
        Media::Geo(_) | Media::GeoLive(_) => MediaType::Location,
        Media::Venue(_) => MediaType::Venue,
        Media::Dice(_) => MediaType::Dice,
        Media::WebPage(_) => MediaType::None, // WebPage previews are not considered media
        Media::Document(doc) => detect_document_type(doc),
        _ => MediaType::Document, // Fallback for any new/unknown variants
    }
}

/// Detect the specific type of a Document based on its attributes
fn detect_document_type(doc: &Document) -> MediaType {
    // Access the raw document to inspect attributes
    let raw_doc = match &doc.raw.document {
        Some(tl::enums::Document::Document(d)) => d,
        _ => return MediaType::Document,
    };

    // Check attributes to determine document subtype
    for attr in &raw_doc.attributes {
        match attr {
            tl::enums::DocumentAttribute::Video(v) => {
                // Round video = VideoNote, otherwise Video
                return if v.round_message {
                    MediaType::VideoNote
                } else {
                    MediaType::Video
                };
            }
            tl::enums::DocumentAttribute::Audio(a) => {
                // Voice message vs music/audio
                return if a.voice {
                    MediaType::Voice
                } else {
                    MediaType::Audio
                };
            }
            tl::enums::DocumentAttribute::Animated => {
                return MediaType::Animation;
            }
            tl::enums::DocumentAttribute::Sticker(_) => {
                // Should be caught by Media::Sticker, but just in case
                return MediaType::Sticker;
            }
            _ => {}
        }
    }

    // Check MIME type for GIFs that might not have Animated attribute
    let mime = raw_doc.mime_type.as_str();
    if mime == "image/gif" || (mime == "video/mp4" && doc.is_animated()) {
        return MediaType::Animation;
    }

    // Default to generic document
    MediaType::Document
}

/// Extract the duration (seconds) of a voice message or round video from its
/// document attributes. Returns `None` for media without an audio/video
/// attribute. Used by transcription metadata.
pub fn extract_audio_duration(media: &Media) -> Option<u32> {
    let Media::Document(doc) = media else {
        return None;
    };
    let Some(tl::enums::Document::Document(raw)) = doc.raw.document.as_ref() else {
        return None;
    };
    for attr in &raw.attributes {
        match attr {
            tl::enums::DocumentAttribute::Audio(a) => return Some(a.duration.max(0) as u32),
            tl::enums::DocumentAttribute::Video(v) => return Some(v.duration.max(0.0) as u32),
            _ => {}
        }
    }
    None
}

/// Derive `VideoInfo` from a video-class media's document attributes. Returns
/// `None` for non-video media. Reads raw TL attributes because the high-level
/// grammers `Document` API does not expose video duration / pixel dimensions.
/// `image/gif` animations may carry no `Video` attribute, in which case
/// duration/width/height stay `0` (design decision). No network calls.
pub fn extract_video_info(media: &Media) -> Option<VideoInfo> {
    let kind = match convert_media_to_type(media) {
        MediaType::Video => VideoKind::Video,
        MediaType::VideoNote => VideoKind::VideoNote,
        MediaType::Animation => VideoKind::Animation,
        _ => return None,
    };
    let Media::Document(doc) = media else {
        return None;
    };
    let Some(tl::enums::Document::Document(raw)) = doc.raw.document.as_ref() else {
        return None;
    };

    let mut duration_seconds = 0;
    let mut width = 0;
    let mut height = 0;
    for attr in &raw.attributes {
        if let tl::enums::DocumentAttribute::Video(v) = attr {
            duration_seconds = v.duration.max(0.0) as u32;
            width = v.w.max(0) as u32;
            height = v.h.max(0) as u32;
            break;
        }
    }

    Some(VideoInfo {
        duration_seconds,
        width,
        height,
        file_size_bytes: raw.size.max(0) as u64,
        kind,
        has_thumbnail: raw.thumbs.as_ref().is_some_and(|t| !t.is_empty()),
        mime_type: Some(raw.mime_type.clone()),
    })
}

/// Derive `AudioInfo` from an audio-class media's document attributes. Returns
/// `None` for non-audio media. Same zero-cost raw-TL source as
/// [`extract_video_info`].
pub fn extract_audio_info(media: &Media) -> Option<AudioInfo> {
    let kind = match convert_media_to_type(media) {
        MediaType::Audio => AudioKind::Audio,
        MediaType::Voice => AudioKind::Voice,
        _ => return None,
    };
    let Media::Document(doc) = media else {
        return None;
    };
    let Some(tl::enums::Document::Document(raw)) = doc.raw.document.as_ref() else {
        return None;
    };

    let mut duration_seconds = 0;
    for attr in &raw.attributes {
        if let tl::enums::DocumentAttribute::Audio(a) = attr {
            duration_seconds = a.duration.max(0) as u32;
            break;
        }
    }

    Some(AudioInfo {
        duration_seconds,
        file_size_bytes: raw.size.max(0) as u64,
        kind,
        mime_type: Some(raw.mime_type.clone()),
    })
}

/// Check if a message's media matches the given filter (for client-side filtering)
///
/// Used by `get_recent_messages` since `iter_messages` doesn't support server-side filtering.
pub fn matches_media_filter(msg: &grammers_client::message::Message, filter: &MediaFilter) -> bool {
    let Some(media) = msg.media() else {
        // No media - only match if filter is Url (check text for URLs) or Pinned
        return match filter {
            MediaFilter::Url => msg.text().contains("http://") || msg.text().contains("https://"),
            MediaFilter::Pinned => msg.pinned(),
            _ => false,
        };
    };

    let media_type = convert_media_to_type(&media);

    match filter {
        MediaFilter::Photo => media_type == MediaType::Photo,
        MediaFilter::Video => media_type == MediaType::Video,
        MediaFilter::PhotoVideo => media_type == MediaType::Photo || media_type == MediaType::Video,
        MediaFilter::Document => media_type == MediaType::Document,
        MediaFilter::Audio => media_type == MediaType::Audio,
        MediaFilter::Voice => media_type == MediaType::Voice,
        MediaFilter::VideoNote => media_type == MediaType::VideoNote,
        MediaFilter::Gif => media_type == MediaType::Animation,
        MediaFilter::Url => {
            // Message has media AND contains URL
            msg.text().contains("http://") || msg.text().contains("https://")
        }
        MediaFilter::Pinned => msg.pinned(),
    }
}

/// Convert grammers Peer to our Channel type
pub fn convert_peer_to_channel(peer: &grammers_client::peer::Peer) -> Option<Channel> {
    use grammers_client::peer::Peer;

    match peer {
        Peer::Channel(ch) => {
            let id = ChannelId::new(ch.id().bare_id()).ok()?;
            let name = ChannelName::new(ch.title()).ok()?;
            let username = ch
                .username()
                .and_then(|u| Username::new(u).ok())
                .unwrap_or_else(|| Username::new("unknown").unwrap());

            Some(Channel {
                id,
                name,
                username,
                description: None, // Not available from basic chat info
                member_count: 0,   // Would need additional API call
                is_verified: ch.raw.verified,
                is_public: ch.username().is_some(),
                is_subscribed: true, // We're iterating our dialogs, so we're subscribed
                last_message_date: None,
            })
        }
        Peer::Group(g) => {
            // Include groups as they behave like channels for our purposes
            let id = ChannelId::new(g.id().bare_id()).ok()?;
            let name = ChannelName::new(g.title().unwrap_or("Unknown")).ok()?;
            let username = g
                .username()
                .and_then(|u| Username::new(u).ok())
                .unwrap_or_else(|| Username::new("group").unwrap());

            Some(Channel {
                id,
                name,
                username,
                description: None,
                member_count: 0,
                is_verified: false,
                is_public: g.username().is_some(),
                is_subscribed: true,
                last_message_date: None,
            })
        }
        _ => {
            tracing::debug!(
                peer_id = peer.id().bare_id(),
                "Skipping non-channel/group peer in convert_peer_to_channel (likely a User)"
            );
            None
        }
    }
}

/// Pick the size variant to download: the smallest whose longest side is at
/// least `max_dimension` (no point downloading more pixels than will be
/// returned), or the largest available when none qualifies.
pub fn select_size_candidate(
    candidates: &[SizeCandidate],
    max_dimension: u32,
) -> Option<SizeCandidate> {
    candidates
        .iter()
        .filter(|c| c.width.max(c.height) >= max_dimension)
        .min_by_key(|c| c.width.max(c.height))
        .or_else(|| candidates.iter().max_by_key(|c| c.width.max(c.height)))
        .cloned()
}

/// Extract downloadable size candidates from grammers photo/document thumbs.
///
/// `Stripped` and `Path` variants are tiny inline previews / vector outlines,
/// not photo content; `Empty` is unavailable. All three are skipped.
pub fn size_candidates(thumbs: &[PhotoSize]) -> Vec<SizeCandidate> {
    thumbs
        .iter()
        .filter_map(|thumb| match thumb {
            PhotoSize::Size(s) => Some(SizeCandidate {
                width: s.width.max(0) as u32,
                height: s.height.max(0) as u32,
                size_bytes: s.size.max(0) as u64,
                photo_type: thumb.photo_type(),
            }),
            PhotoSize::Cached(s) => Some(SizeCandidate {
                width: s.width.max(0) as u32,
                height: s.height.max(0) as u32,
                size_bytes: s.bytes.len() as u64,
                photo_type: thumb.photo_type(),
            }),
            PhotoSize::Progressive(s) => Some(SizeCandidate {
                width: s.width.max(0) as u32,
                height: s.height.max(0) as u32,
                size_bytes: thumb.size() as u64,
                photo_type: thumb.photo_type(),
            }),
            PhotoSize::Empty(_) | PhotoSize::Stripped(_) | PhotoSize::Path(_) => None,
        })
        .collect()
}

/// Extract forward attribution from a raw forward header.
///
/// Drops down to the raw TL `MessageFwdHeader` because grammers' high-level API
/// exposes only `forward_header()` (the raw enum). `channel_name`/`channel_username`
/// are left `None`: `from_id` is an ID-only TL `Peer`, and the resolved
/// title/username are not available without an extra resolve call.
fn extract_forward_info(header: &tl::types::MessageFwdHeader) -> ForwardInfo {
    let channel_id = match &header.from_id {
        Some(tl::enums::Peer::Channel(ch)) => ChannelId::new(ch.channel_id).ok(),
        _ => None,
    };

    ForwardInfo {
        channel_id,
        channel_name: None,
        channel_username: None,
        sender_name: header.from_name.clone(),
        original_date: DateTime::<Utc>::from_timestamp(header.date as i64, 0)
            .filter(|dt| dt.timestamp() > 0),
        original_message_id: header
            .channel_post
            .and_then(|id| MessageId::new(id as i64).ok()),
    }
}

/// Extract a link preview from a raw webpage media block.
///
/// Only the `WebPage::Page` variant carries content; `Empty`/`Pending`/`NotModified`
/// yield `None`. `description` is truncated to 500 Unicode scalar values so multi-byte
/// (e.g. Cyrillic) text is never split mid-codepoint.
fn extract_link_preview(media: &tl::types::MessageMediaWebPage) -> Option<LinkPreview> {
    match &media.webpage {
        tl::enums::WebPage::Page(page) => Some(LinkPreview {
            url: page.url.clone(),
            site_name: page.site_name.clone(),
            title: page.title.clone(),
            description: page
                .description
                .as_ref()
                .map(|d| d.chars().take(500).collect()),
        }),
        _ => None,
    }
}

/// Convert grammers Message to our Message type
pub fn convert_message(
    msg: &grammers_client::message::Message,
    peer: &grammers_client::peer::Peer,
) -> Option<Message> {
    use grammers_client::peer::Peer;

    let (channel_id, channel_name, channel_username) = match peer {
        Peer::Channel(ch) => (
            ChannelId::new(ch.id().bare_id()).ok()?,
            ChannelName::new(ch.title()).ok()?,
            ch.username()
                .and_then(|u| Username::new(u).ok())
                .unwrap_or_else(|| Username::new("unknown").unwrap()),
        ),
        Peer::Group(g) => (
            ChannelId::new(g.id().bare_id()).ok()?,
            ChannelName::new(g.title().unwrap_or("Unknown")).ok()?,
            g.username()
                .and_then(|u| Username::new(u).ok())
                .unwrap_or_else(|| Username::new("group").unwrap()),
        ),
        Peer::User(u) => (
            ChannelId::new(u.id().bare_id()).ok()?,
            ChannelName::new(u.first_name().unwrap_or("User")).ok()?,
            u.username()
                .and_then(|un| Username::new(un).ok())
                .unwrap_or_else(|| Username::new("user").unwrap()),
        ),
    };

    let message_id = MessageId::new(msg.id() as i64).ok()?;

    // Get sender info
    // msg.sender() returns Result<&Peer, Option<PeerRef>> in newer grammers versions
    let (sender_id, sender_name) = match msg.sender() {
        Some(sender) => {
            let id = UserId::new(sender.id().bare_id()).ok();
            let name = sender.name().map(|s: &str| s.to_string());
            (id, name)
        }
        None => (None, None),
    };

    // Check for media and detect its type (computed once; reused for link preview)
    let media = msg.media();
    let (has_media, media_type) = match &media {
        Some(m) => (true, convert_media_to_type(m)),
        None => (false, MediaType::None),
    };

    // Enrichment (all derived from data already in `msg` — no network calls):
    let link_preview = match &media {
        Some(Media::WebPage(wp)) => extract_link_preview(&wp.raw),
        _ => None,
    };

    // Zero-cost media metadata derived from the in-hand document attributes.
    let video_info = media.as_ref().and_then(extract_video_info);
    let audio_info = media.as_ref().and_then(extract_audio_info);

    let forwarded_from = msg
        .forward_header()
        .map(|tl::enums::MessageFwdHeader::Header(header)| extract_forward_info(&header));

    let views = msg.view_count().and_then(|v| u64::try_from(v).ok());
    let forwards = msg.forward_count().and_then(|v| u64::try_from(v).ok());
    let reply_to_message_id = msg
        .reply_to_message_id()
        .and_then(|id| MessageId::new(id as i64).ok());

    Some(Message {
        id: message_id,
        channel_id,
        channel_name,
        channel_username,
        text: msg.text().to_string(),
        timestamp: msg.date(),
        sender_id,
        sender_name,
        has_media,
        media_type,
        forwarded_from,
        link_preview,
        views,
        forwards,
        reply_to_message_id,
        video_info,
        audio_info,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn forward_from_channel_extracts_ids_not_names() {
        let mut header = fwd_header();
        header.from_id = Some(tl::enums::Peer::Channel(tl::types::PeerChannel {
            channel_id: 555,
        }));
        header.channel_post = Some(42);

        let info = extract_forward_info(&header);
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

        let info = extract_forward_info(&header);
        assert_eq!(info.sender_name.as_deref(), Some("Hidden User"));
        assert!(info.channel_id.is_none());
        assert!(info.original_message_id.is_none());
    }

    #[test]
    fn forward_with_zero_date_has_no_original_date() {
        let mut header = fwd_header();
        header.date = 0;

        let info = extract_forward_info(&header);
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
}
