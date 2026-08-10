//! Message assembly: forward header, link preview, and `convert_message`.
//!
//! Sub-domain of `converters` (LM-4). Depends on `media` for media-type
//! detection and video/audio info.

use super::channel::peer_identity;
use super::media::{convert_media_to_type, extract_audio_info, extract_video_info};
use crate::telegram::types::{
    ChannelId, ForwardInfo, LinkPreview, MediaType, Message, MessageId, UserId,
};
use chrono::{DateTime, Utc};
use grammers_client::media::Media;
use grammers_client::tl;

/// A message's date at the grammers boundary as the domain's `DateTime<Utc>`.
///
/// grammers 0.10 reports dates as jiff `Timestamp`s; the domain model stays on
/// chrono. Telegram dates are whole-second `i32`s, so converting via seconds
/// loses nothing, and `None` is unreachable for any date Telegram can send.
/// Single owner of the jiff→chrono conversion — date-type churn lands here.
pub(crate) fn message_timestamp(msg: &grammers_client::message::Message) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(msg.date().as_second(), 0)
}

/// Extract forward attribution from a raw forward header.
///
/// Drops down to the raw TL `MessageFwdHeader` because grammers' high-level API
/// exposes only `forward_header()` (the raw enum). `channel_name`/`channel_username`
/// are left `None`: `from_id` is an ID-only TL `Peer`, and the resolved
/// title/username are not available without an extra resolve call.
pub(crate) fn extract_forward_info(header: &tl::types::MessageFwdHeader) -> ForwardInfo {
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

/// Cap on a link preview's `description`, in Unicode scalar values. Internal
/// presentation limit; not operator-configurable (AD-6 KISS — left a named const).
const LINK_PREVIEW_DESCRIPTION_MAX_CHARS: usize = 500;

/// Extract a link preview from a raw webpage media block.
///
/// Only the `WebPage::Page` variant carries content; `Empty`/`Pending`/`NotModified`
/// yield `None`. `description` is truncated to [`LINK_PREVIEW_DESCRIPTION_MAX_CHARS`]
/// Unicode scalar values so multi-byte (e.g. Cyrillic) text is never split mid-codepoint.
pub(crate) fn extract_link_preview(media: &tl::types::MessageMediaWebPage) -> Option<LinkPreview> {
    match &media.webpage {
        tl::enums::WebPage::Page(page) => Some(LinkPreview {
            url: page.url.clone(),
            site_name: page.site_name.clone(),
            title: page.title.clone(),
            description: page
                .description
                .as_ref()
                .map(|d| d.chars().take(LINK_PREVIEW_DESCRIPTION_MAX_CHARS).collect()),
        }),
        _ => None,
    }
}

/// Convert grammers Message to our Message type
pub fn convert_message(
    msg: &grammers_client::message::Message,
    peer: &grammers_client::peer::Peer,
) -> Option<Message> {
    // A MessageEmpty placeholder (deleted / never-existed id) must never map
    // to a domain Message — it has an epoch-0 date and empty text (B1).
    if matches!(msg.raw, tl::enums::Message::Empty(_)) {
        return None;
    }

    let (channel_id, channel_name, channel_username) = peer_identity(peer)?;

    let message_id = MessageId::new(msg.id() as i64).ok()?;

    // Get sender info; msg.sender() is Option<&Peer> (None for anonymous posts).
    let (sender_id, sender_name) = match msg.sender() {
        Some(sender) => {
            let id = sender.id().bare_id().and_then(|i| UserId::new(i).ok());
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
        timestamp: message_timestamp(msg)?,
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
