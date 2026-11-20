//! Real raw-TL media for the private operation snapshot; no account or network.
use super::VisualMediaMessage;
use crate::telegram::types::VideoKind;
use grammers_client::media::{Document, Media, Photo};
use grammers_client::tl;
use grammers_session::types::PeerId;

pub(super) fn source(media: Option<Media>) -> VisualMediaMessage {
    VisualMediaMessage {
        media,
        caption: Some("Фото 🦀".into()),
        peer_id: PeerId::channel_unchecked(100),
    }
}
pub(super) fn cached_size(tag: &str, w: i32, h: i32, bytes: &[u8]) -> tl::enums::PhotoSize {
    tl::types::PhotoCachedSize {
        r#type: tag.into(),
        w,
        h,
        bytes: bytes.to_vec(),
    }
    .into()
}
pub(super) fn photo(sizes: Vec<tl::enums::PhotoSize>) -> Media {
    Media::Photo(Photo::from_raw(
        tl::types::Photo {
            has_stickers: false,
            id: 1,
            access_hash: 0,
            file_reference: Vec::new(),
            date: 0,
            sizes,
            video_sizes: None,
            dc_id: 2,
        }
        .into(),
    ))
}
pub(super) fn video(kind: VideoKind, thumbs: Option<Vec<tl::enums::PhotoSize>>) -> Media {
    let mut attributes = vec![
        tl::types::DocumentAttributeVideo {
            round_message: kind == VideoKind::VideoNote,
            supports_streaming: false,
            nosound: kind == VideoKind::Animation,
            duration: 7.5,
            w: 1920,
            h: 1080,
            preload_prefix_size: None,
            video_start_ts: None,
            video_codec: None,
        }
        .into(),
    ];
    if kind == VideoKind::Animation {
        attributes.push(tl::enums::DocumentAttribute::Animated);
    }
    Media::Document(Document::from_raw_media(tl::types::MessageMediaDocument {
        nopremium: false,
        spoiler: false,
        video: false,
        round: false,
        voice: false,
        document: Some(
            tl::types::Document {
                id: 1,
                access_hash: 0,
                file_reference: Vec::new(),
                date: 0,
                mime_type: "video/mp4".into(),
                size: 123_456,
                thumbs,
                video_thumbs: None,
                dc_id: 2,
                attributes,
            }
            .into(),
        ),
        alt_documents: None,
        video_cover: None,
        video_timestamp: None,
        ttl_seconds: None,
    }))
}
