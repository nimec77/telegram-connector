//! Message assembly: forward header, link preview, and `convert_message`.
//!
//! Sub-domain of `converters` (LM-4). Depends on `media` for media-type
//! detection and video/audio info.

use super::media::{convert_media_to_type, extract_audio_info, extract_video_info};
use crate::telegram::types::{
    ChannelId, ChannelName, ForwardInfo, LinkPreview, MediaType, Message, MessageId, UserId,
    Username,
};
use chrono::{DateTime, Utc};
use grammers_client::media::Media;
use grammers_client::tl;

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

/// Extract a link preview from a raw webpage media block.
///
/// Only the `WebPage::Page` variant carries content; `Empty`/`Pending`/`NotModified`
/// yield `None`. `description` is truncated to 500 Unicode scalar values so multi-byte
/// (e.g. Cyrillic) text is never split mid-codepoint.
pub(crate) fn extract_link_preview(media: &tl::types::MessageMediaWebPage) -> Option<LinkPreview> {
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
