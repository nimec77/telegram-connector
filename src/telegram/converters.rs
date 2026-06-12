//! Type conversion helpers for grammers types to our domain types

use crate::telegram::types::{
    Channel, ChannelId, ChannelName, MediaFilter, MediaType, Message, MessageId, SizeCandidate,
    UserId, Username,
};
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

    // Check for media and detect its type
    let (has_media, media_type) = match msg.media() {
        Some(media) => (true, convert_media_to_type(&media)),
        None => (false, MediaType::None),
    };

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
    })
}
