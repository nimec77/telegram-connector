//! Message assembly: forward header, link preview, and `convert_raw_message`.
//!
//! Sub-domain of `converters` (LM-4). Depends on `media` for media-type
//! detection and video/audio info.

use super::channel::{channel_identity, peer_identity};
use super::media::{
    convert_media_to_type, extract_audio_info, extract_document_info, extract_poll_info,
    extract_video_info,
};
use crate::link::MessageLink;
use crate::telegram::envelope::EntityLookup;
use crate::telegram::types::{
    ChannelId, ChannelName, ForwardInfo, LinkPreview, MediaType, Message, MessageId,
    MessageReaction, UserId, Username,
};
use chrono::{DateTime, Utc};
use grammers_client::media::Media;
use grammers_client::tl;

/// A message's date at the grammers boundary as the domain's `DateTime<Utc>`.
///
/// Telegram dates are whole-second `i32`s on the raw TL variants, so reading
/// them directly loses nothing (grammers' own `date()` reads the same field).
/// `MessageEmpty` placeholders have no real date and yield `None` — never an
/// epoch-0 fabrication (work-order B1/B8). Single owner of the raw→chrono
/// conversion — date-type churn lands here.
pub(crate) fn message_timestamp(msg: &grammers_client::message::Message) -> Option<DateTime<Utc>> {
    timestamp_from_raw(&msg.raw)
}

/// Raw-enum seam for [`message_timestamp`] — constructible offline, so the
/// Empty-refusal is directly testable (same pattern as `client/guard.rs`).
/// Also the timestamp reader for the raw-pager fetch paths.
pub(crate) fn timestamp_from_raw(raw: &tl::enums::Message) -> Option<DateTime<Utc>> {
    let secs = match raw {
        tl::enums::Message::Message(m) => m.date,
        tl::enums::Message::Service(m) => m.date,
        tl::enums::Message::Empty(_) => return None,
    };
    DateTime::from_timestamp(secs as i64, 0)
}

/// Extract forward attribution from a raw forward header, resolving the
/// source's display data from the response envelope's entity map.
///
/// Zero network calls: `entities` is built from the same response the message
/// arrived in. A map miss degrades to the ids-only form — nothing is
/// fabricated and nothing is resolved on demand. Raw TL is required here
/// because the pinned grammers rev keeps `Message.peers` crate-private (see
/// `envelope.rs` module docs).
pub(crate) fn extract_forward_info(
    header: &tl::types::MessageFwdHeader,
    entities: &EntityLookup,
) -> ForwardInfo {
    let info = header.from_id.as_ref().and_then(|peer| entities.get(peer));

    let (channel_id, channel_name, channel_username, user_sender_name) = match &header.from_id {
        Some(tl::enums::Peer::Channel(ch)) => (
            ChannelId::new(ch.channel_id).ok(),
            info.and_then(|i| i.display_name.as_deref())
                .and_then(|n| ChannelName::new(n).ok()),
            info.and_then(|i| i.username.as_deref())
                .and_then(|u| Username::new(u).ok()),
            None,
        ),
        // Legacy groups: chat-namespace ids were never emitted; keep that,
        // but surface the title now that the envelope provides it.
        Some(tl::enums::Peer::Chat(_)) => (
            None,
            info.and_then(|i| i.display_name.as_deref())
                .and_then(|n| ChannelName::new(n).ok()),
            None,
            None,
        ),
        Some(tl::enums::Peer::User(_)) => {
            (None, None, None, info.and_then(|i| i.display_name.clone()))
        }
        None => (None, None, None, None),
    };

    ForwardInfo {
        channel_id,
        channel_name,
        channel_username,
        // Hidden senders (`from_name`, no `from_id`) win, as they always have;
        // otherwise a user-source forward carries the user's display name.
        sender_name: header.from_name.clone().or(user_sender_name),
        post_author: header.post_author.clone(),
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

/// Permalink for a message, from data already in hand (work-order D1):
/// public `t.me/<username>` form when the channel has one, members-only
/// `t.me/c/…` otherwise. Same builder as generate_message_link (B2).
pub(crate) fn build_message_link(
    peer: &grammers_client::peer::Peer,
    message_id: MessageId,
) -> Option<String> {
    let identity = channel_identity(peer)?;
    Some(MessageLink::new(identity.id, message_id, identity.username.as_deref()).https_link)
}

/// Itemized standard-emoji reactions plus an all-kinds total (work-order D2).
/// Custom-emoji and paid reactions count toward the total but are not
/// itemized (no renderable emoji string).
pub(crate) fn extract_reactions(
    reactions: Option<&tl::enums::MessageReactions>,
) -> (Option<Vec<MessageReaction>>, Option<u64>) {
    let Some(tl::enums::MessageReactions::Reactions(r)) = reactions else {
        return (None, None);
    };
    let mut itemized = Vec::new();
    let mut total = 0u64;
    for result in &r.results {
        let tl::enums::ReactionCount::Count(rc) = result;
        let count = u64::try_from(rc.count).unwrap_or(0);
        total += count;
        if let tl::enums::Reaction::Emoji(e) = &rc.reaction {
            itemized.push(MessageReaction {
                emoji: e.emoticon.clone(),
                count,
            });
        }
    }
    (Some(itemized).filter(|v| !v.is_empty()), Some(total))
}

/// Raw-field readers mirroring grammers' own thin accessors (each is a
/// `match` on the three message variants; Service/Empty yield the neutral
/// value exactly as the high-level methods do).
fn raw_text(raw: &tl::enums::Message) -> &str {
    match raw {
        tl::enums::Message::Message(m) => &m.message,
        _ => "",
    }
}

fn raw_from_id(raw: &tl::enums::Message) -> Option<&tl::enums::Peer> {
    match raw {
        tl::enums::Message::Message(m) => m.from_id.as_ref(),
        tl::enums::Message::Service(m) => m.from_id.as_ref(),
        tl::enums::Message::Empty(_) => None,
    }
}

fn raw_media(raw: &tl::enums::Message) -> Option<Media> {
    match raw {
        tl::enums::Message::Message(m) => m.media.clone().and_then(Media::from_raw),
        _ => None,
    }
}

fn raw_forward_header(raw: &tl::enums::Message) -> Option<&tl::types::MessageFwdHeader> {
    match raw {
        tl::enums::Message::Message(m) => m
            .fwd_from
            .as_ref()
            .map(|tl::enums::MessageFwdHeader::Header(h)| h),
        _ => None,
    }
}

fn raw_views(raw: &tl::enums::Message) -> Option<i32> {
    match raw {
        tl::enums::Message::Message(m) => m.views,
        _ => None,
    }
}

fn raw_forwards(raw: &tl::enums::Message) -> Option<i32> {
    match raw {
        tl::enums::Message::Message(m) => m.forwards,
        _ => None,
    }
}

fn raw_reply_to_message_id(raw: &tl::enums::Message) -> Option<i32> {
    match raw {
        tl::enums::Message::Message(tl::types::Message {
            reply_to: Some(tl::enums::MessageReplyHeader::Header(header)),
            ..
        }) => header.reply_to_msg_id,
        _ => None,
    }
}

fn raw_grouped_id(raw: &tl::enums::Message) -> Option<i64> {
    match raw {
        tl::enums::Message::Message(m) => m.grouped_id,
        _ => None,
    }
}

/// Convert a raw TL message to our domain Message, resolving senders and
/// forward attribution from the response envelope's entity map.
///
/// This is the single conversion path for every fetch route: every fetch
/// path supplies a real response envelope, and `EntityLookup` has no
/// production constructor other than `from_envelope`. Pure function of its
/// inputs — no client, no network (the zero-extra-call invariant is
/// structural).
pub(crate) fn convert_raw_message(
    raw: &tl::enums::Message,
    peer: &grammers_client::peer::Peer,
    entities: &EntityLookup,
) -> Option<Message> {
    // A MessageEmpty placeholder (deleted / never-existed id) must never map
    // to a domain Message — it has an epoch-0 date and empty text (B1).
    if matches!(raw, tl::enums::Message::Empty(_)) {
        return None;
    }

    let (channel_id, channel_name, channel_username) = peer_identity(peer)?;
    let message_id = MessageId::new(raw.id() as i64).ok()?;

    // Sender from the raw from_id + envelope. grammers' private-DM fallback
    // (peer-as-sender when from_id is absent) is unreachable here: every
    // fetch path targets channel/group peers, where an absent from_id means
    // an anonymous post — (None, None), as before.
    let (sender_id, sender_name) = match raw_from_id(raw) {
        Some(from) => (
            grammers_session::types::PeerId::from(from.clone())
                .bare_id()
                .and_then(|i| UserId::new(i).ok()),
            entities.get(from).and_then(|info| info.sender_name()),
        ),
        None => (None, None),
    };

    // Check for media and detect its type (computed once; reused for link preview)
    let media = raw_media(raw);
    let (has_media, media_type) = match &media {
        Some(m) => (true, convert_media_to_type(m)),
        None => (false, MediaType::None),
    };

    // Enrichment (all derived from data already in hand — no network calls):
    let link_preview = match &media {
        Some(Media::WebPage(wp)) => extract_link_preview(&wp.raw),
        _ => None,
    };

    // Zero-cost media metadata derived from the in-hand document attributes.
    let video_info = media.as_ref().and_then(extract_video_info);
    let audio_info = media.as_ref().and_then(extract_audio_info);
    let document_info = media.as_ref().and_then(extract_document_info);
    let poll_info = media.as_ref().and_then(extract_poll_info);

    let forwarded_from = raw_forward_header(raw).map(|h| extract_forward_info(h, entities));

    let views = raw_views(raw).and_then(|v| u64::try_from(v).ok());
    let forwards = raw_forwards(raw).and_then(|v| u64::try_from(v).ok());
    let reply_to_message_id =
        raw_reply_to_message_id(raw).and_then(|id| MessageId::new(id as i64).ok());

    let raw_reactions = match raw {
        tl::enums::Message::Message(m) => m.reactions.as_ref(),
        _ => None,
    };
    let (reactions, reactions_total) = extract_reactions(raw_reactions);
    let link = build_message_link(peer, message_id)?;

    Some(Message {
        id: message_id,
        channel_id,
        channel_name,
        channel_username,
        text: raw_text(raw).to_string(),
        timestamp: timestamp_from_raw(raw)?,
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
        document_info,
        poll_info,
        grouped_id: raw_grouped_id(raw),
        link,
        reactions,
        reactions_total,
        album: None,
    })
}

#[cfg(test)]
#[path = "../tests/message_tests.rs"]
mod tests;
