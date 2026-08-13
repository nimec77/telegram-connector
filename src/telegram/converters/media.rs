//! Media classification, sizing, and video/audio info extraction.
//!
//! Sub-domain of `converters` (LM-4): grammers media -> our `MediaType`,
//! `MediaFilter` mapping, zero-cost video/audio metadata, and photo size
//! candidate selection.

use crate::telegram::types::{
    AudioInfo, AudioKind, DocumentInfo, MediaFilter, MediaType, PollInfo, PollOption,
    SizeCandidate, VideoInfo, VideoKind,
};
use grammers_client::media::{Document, Media, PhotoSize};
use grammers_client::tl;
use std::collections::HashMap;

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
    let mut title = None;
    let mut performer = None;
    for attr in &raw.attributes {
        if let tl::enums::DocumentAttribute::Audio(a) = attr {
            duration_seconds = a.duration.max(0) as u32;
            title = a.title.clone();
            performer = a.performer.clone();
            break;
        }
    }

    Some(AudioInfo {
        duration_seconds,
        file_size_bytes: raw.size.max(0) as u64,
        kind,
        mime_type: Some(raw.mime_type.clone()),
        title,
        performer,
    })
}

/// Derive `DocumentInfo` from a generic document's attributes. Returns `None`
/// for every other media class, including the document-backed ones (video,
/// audio, voice, animation, sticker) that already have a dedicated info
/// object. Same zero-cost raw-TL source as [`extract_video_info`].
pub fn extract_document_info(media: &Media) -> Option<DocumentInfo> {
    if convert_media_to_type(media) != MediaType::Document {
        return None;
    }
    let Media::Document(doc) = media else {
        return None;
    };
    let Some(tl::enums::Document::Document(raw)) = doc.raw.document.as_ref() else {
        return None;
    };

    let file_name = raw.attributes.iter().find_map(|attr| match attr {
        tl::enums::DocumentAttribute::Filename(f) => Some(f.file_name.clone()),
        _ => None,
    });

    Some(DocumentInfo {
        file_name,
        file_size_bytes: raw.size.max(0) as u64,
        mime_type: Some(raw.mime_type.clone()),
    })
}

/// Derive `PollInfo` from poll media. Returns `None` for every other media
/// class. Answers are matched to their vote counts by the `option` bytes key
/// that both `PollAnswer` and `PollAnswerVoters` carry — never by position,
/// which Telegram does not guarantee. Undisclosed results degrade to
/// text-only options; nothing is fabricated and no call is made.
pub fn extract_poll_info(media: &Media) -> Option<PollInfo> {
    let Media::Poll(poll) = media else {
        return None;
    };

    let voters_by_option: HashMap<&[u8], u64> = poll
        .iter_voters_summary()
        .map(|voters| {
            voters
                .map(|v| {
                    let count = v.voters.and_then(|n| u64::try_from(n).ok()).unwrap_or(0);
                    (v.option.as_slice(), count)
                })
                .collect()
        })
        .unwrap_or_default();

    let options = poll
        .iter_answers()
        .filter_map(|answer| {
            let tl::enums::PollAnswer::Answer(answer) = answer else {
                return None;
            };
            let tl::enums::TextWithEntities::Entities(text) = &answer.text;
            Some(PollOption {
                text: text.text.clone(),
                voters: voters_by_option.get(answer.option.as_slice()).copied(),
            })
        })
        .collect();

    let tl::enums::TextWithEntities::Entities(question) = poll.question();

    Some(PollInfo {
        question: question.text.clone(),
        options,
        total_voters: poll.total_voters().and_then(|v| u64::try_from(v).ok()),
        closed: poll.closed(),
        // No accessor for this one in the pinned rev; `raw` is public and the
        // repo already reads document attributes the same way.
        multiple_choice: poll.raw.multiple_choice,
        quiz: poll.is_quiz(),
    })
}

/// Shared core for the high-level and raw filter entry points.
fn media_matches_filter(
    media: Option<&Media>,
    text: &str,
    pinned: bool,
    filter: &MediaFilter,
) -> bool {
    let Some(media) = media else {
        // No media - only match if filter is Url (check text for URLs) or Pinned
        return match filter {
            MediaFilter::Url => text.contains("http://") || text.contains("https://"),
            MediaFilter::Pinned => pinned,
            _ => false,
        };
    };

    let media_type = convert_media_to_type(media);

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
            text.contains("http://") || text.contains("https://")
        }
        MediaFilter::Pinned => pinned,
    }
}

/// Check if a message's media matches the given filter (for client-side filtering)
///
/// Used by `get_recent_messages` since `iter_messages` doesn't support server-side filtering.
pub fn matches_media_filter(msg: &grammers_client::message::Message, filter: &MediaFilter) -> bool {
    media_matches_filter(msg.media().as_ref(), msg.text(), msg.pinned(), filter)
}

/// Raw-message twin of [`matches_media_filter`], for the raw history pager path.
pub(crate) fn matches_media_filter_raw(raw: &tl::enums::Message, filter: &MediaFilter) -> bool {
    let (media, text, pinned) = match raw {
        tl::enums::Message::Message(m) => (
            m.media.clone().and_then(Media::from_raw),
            m.message.as_str(),
            m.pinned,
        ),
        _ => (None, "", false),
    };
    media_matches_filter(media.as_ref(), text, pinned, filter)
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
