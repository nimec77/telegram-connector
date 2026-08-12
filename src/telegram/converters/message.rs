//! Message assembly: forward header, link preview, and `convert_message`.
//!
//! Sub-domain of `converters` (LM-4). Depends on `media` for media-type
//! detection and video/audio info.

use super::channel::{channel_identity, peer_identity};
use super::media::{convert_media_to_type, extract_audio_info, extract_video_info};
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
/// This is the single conversion path for every fetch route; the raw pagers
/// call it with the full envelope, the grammers-`Message` wrapper below calls
/// it with what the high-level API exposes. Pure function of its inputs — no
/// client, no network (the zero-extra-call invariant is structural).
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
        grouped_id: raw_grouped_id(raw),
        link,
        reactions,
        reactions_total,
        album: None,
    })
}

/// Convert a grammers high-level Message (call paths that never see the raw
/// envelope: get_message_by_id, get_messages_batch, get_channel_stats).
///
/// Seeds the entity map from the only peers the high-level API exposes —
/// sender and chat — so sender resolution is unchanged; forwards on these
/// paths stay ids-only because `Message.peers` is crate-private in the
/// pinned grammers rev (see `envelope.rs`).
pub fn convert_message(
    msg: &grammers_client::message::Message,
    peer: &grammers_client::peer::Peer,
) -> Option<Message> {
    let mut entities = EntityLookup::empty();
    if let Some(sender) = msg.sender() {
        entities.insert_peer(sender);
    }
    if let Some(chat) = msg.peer() {
        entities.insert_peer(chat);
    }
    convert_raw_message(&msg.raw, peer, &entities)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telegram::envelope::EntityLookup;
    use crate::test_helpers::{raw_tl_channel, raw_tl_user};
    use grammers_client::Client;
    use grammers_client::peer::{Community, Peer};
    use grammers_mtsender::SenderPool;
    use grammers_session::storages::MemorySession;
    use std::sync::Arc;

    /// Fwd header fixture: unset fields None/false, `date` fixed.
    fn fwd_header(
        from_id: Option<tl::enums::Peer>,
        from_name: Option<&str>,
        post_author: Option<&str>,
    ) -> tl::types::MessageFwdHeader {
        tl::types::MessageFwdHeader {
            imported: false,
            saved_out: false,
            from_id,
            from_name: from_name.map(|s| s.to_string()),
            date: 1_700_000_000,
            channel_post: Some(1863),
            post_author: post_author.map(|s| s.to_string()),
            saved_from_peer: None,
            saved_from_msg_id: None,
            saved_from_id: None,
            saved_from_name: None,
            saved_date: None,
            psa_type: None,
        }
    }

    fn channel_fwd_peer(id: i64) -> Option<tl::enums::Peer> {
        Some(tl::enums::Peer::Channel(tl::types::PeerChannel {
            channel_id: id,
        }))
    }

    #[test]
    fn forward_from_enveloped_channel_carries_name_and_username() {
        let entities = EntityLookup::from_envelope(
            &[tl::enums::Chat::Channel(raw_tl_channel(
                1783384254,
                "Военкор",
                Some("voenkor_ru"),
            ))],
            &[],
        );
        let info = extract_forward_info(
            &fwd_header(channel_fwd_peer(1783384254), None, None),
            &entities,
        );
        assert_eq!(info.channel_id.map(|c| c.get()), Some(1783384254));
        assert_eq!(
            info.channel_name.as_ref().map(|n| n.as_str()),
            Some("Военкор")
        );
        assert_eq!(
            info.channel_username.as_ref().map(|u| u.as_str()),
            Some("voenkor_ru")
        );
        assert_eq!(info.sender_name, None);
        assert_eq!(info.original_message_id.map(|m| m.get()), Some(1863));
    }

    #[test]
    fn forward_from_private_channel_carries_name_without_username() {
        let entities = EntityLookup::from_envelope(
            &[tl::enums::Chat::Channel(raw_tl_channel(
                77,
                "Приватный",
                None,
            ))],
            &[],
        );
        let info = extract_forward_info(&fwd_header(channel_fwd_peer(77), None, None), &entities);
        assert_eq!(
            info.channel_name.as_ref().map(|n| n.as_str()),
            Some("Приватный")
        );
        assert_eq!(info.channel_username, None);
    }

    #[test]
    fn forward_from_user_populates_sender_name_only() {
        let entities = EntityLookup::from_envelope(
            &[],
            &[tl::enums::User::User(raw_tl_user(
                42,
                Some("Иван"),
                Some("Петров"),
                None,
            ))],
        );
        let from = Some(tl::enums::Peer::User(tl::types::PeerUser { user_id: 42 }));
        let info = extract_forward_info(&fwd_header(from, None, None), &entities);
        assert_eq!(info.sender_name.as_deref(), Some("Иван Петров"));
        assert_eq!(info.channel_id, None);
        assert!(info.channel_name.is_none());
        assert!(info.channel_username.is_none());
    }

    #[test]
    fn forward_from_hidden_sender_uses_from_name() {
        let info = extract_forward_info(
            &fwd_header(None, Some("Скрытый Автор"), None),
            &EntityLookup::empty(),
        );
        assert_eq!(info.sender_name.as_deref(), Some("Скрытый Автор"));
        assert_eq!(info.channel_id, None);
        assert!(info.channel_name.is_none());
    }

    #[test]
    fn forward_carries_post_author_for_signed_posts() {
        let entities = EntityLookup::from_envelope(
            &[tl::enums::Chat::Channel(raw_tl_channel(9, "Канал", None))],
            &[],
        );
        let info = extract_forward_info(
            &fwd_header(channel_fwd_peer(9), None, Some("И. Петров")),
            &entities,
        );
        assert_eq!(info.post_author.as_deref(), Some("И. Петров"));
    }

    #[test]
    fn envelope_miss_degrades_to_ids_only() {
        let info = extract_forward_info(
            &fwd_header(channel_fwd_peer(1783384254), None, None),
            &EntityLookup::empty(),
        );
        assert_eq!(info.channel_id.map(|c| c.get()), Some(1783384254));
        assert!(info.channel_name.is_none());
        assert!(info.channel_username.is_none());
        assert_eq!(info.sender_name, None);
        assert_eq!(info.original_message_id.map(|m| m.get()), Some(1863));
    }

    /// Raw channel-post message with a forward header, as it arrives inside a
    /// `messages.Messages` envelope. Flags false, unset options None.
    fn raw_forwarded_message(id: i32, fwd: tl::types::MessageFwdHeader) -> tl::enums::Message {
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
            id,
            from_id: None,
            from_boosts_applied: None,
            from_rank: None,
            peer_id: tl::enums::Peer::Channel(tl::types::PeerChannel {
                channel_id: 1144180066,
            }),
            saved_peer_id: None,
            fwd_from: Some(tl::enums::MessageFwdHeader::Header(fwd)),
            via_bot_id: None,
            via_business_bot_id: None,
            guestchat_via_from: None,
            reply_to: None,
            date: 1_700_000_100,
            message: "переслано".to_string(),
            media: None,
            reply_markup: None,
            entities: None,
            views: Some(10),
            forwards: Some(2),
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
    fn convert_raw_message_enriches_forward_from_envelope() {
        let peer = public_channel_peer(1144180066, "swodki");
        let entities = EntityLookup::from_envelope(
            &[tl::enums::Chat::Channel(raw_tl_channel(
                1783384254,
                "Военкор",
                Some("voenkor_ru"),
            ))],
            &[],
        );
        let raw =
            raw_forwarded_message(610121, fwd_header(channel_fwd_peer(1783384254), None, None));

        let msg = convert_raw_message(&raw, &peer, &entities).expect("converts");
        let fwd = msg.forwarded_from.expect("forward attribution present");
        assert_eq!(
            fwd.channel_name.as_ref().map(|n| n.as_str()),
            Some("Военкор")
        );
        assert_eq!(
            fwd.channel_username.as_ref().map(|u| u.as_str()),
            Some("voenkor_ru")
        );
        assert_eq!(msg.text, "переслано");
        assert_eq!(msg.views, Some(10));
        assert_eq!(msg.link, "https://t.me/swodki/610121");
    }

    #[test]
    fn convert_raw_message_without_forward_leaves_field_absent_in_json() {
        let peer = public_channel_peer(1144180066, "swodki");
        let mut raw_inner = match raw_forwarded_message(610122, fwd_header(None, None, None)) {
            tl::enums::Message::Message(m) => m,
            _ => unreachable!(),
        };
        raw_inner.fwd_from = None;
        let raw = tl::enums::Message::Message(raw_inner);

        let msg = convert_raw_message(&raw, &peer, &EntityLookup::empty()).expect("converts");
        assert!(msg.forwarded_from.is_none());
        let json = serde_json::to_value(&msg).expect("serializes");
        assert!(json.get("forwarded_from").is_none(), "absent, not null");
    }

    #[test]
    fn convert_raw_message_resolves_sender_from_envelope() {
        let peer = public_channel_peer(1144180066, "swodki");
        let entities = EntityLookup::from_envelope(
            &[],
            &[tl::enums::User::User(raw_tl_user(
                42,
                Some("Иван"),
                Some("Петров"),
                None,
            ))],
        );
        let mut raw_inner = match raw_forwarded_message(610123, fwd_header(None, None, None)) {
            tl::enums::Message::Message(m) => m,
            _ => unreachable!(),
        };
        raw_inner.fwd_from = None;
        raw_inner.from_id = Some(tl::enums::Peer::User(tl::types::PeerUser { user_id: 42 }));
        let raw = tl::enums::Message::Message(raw_inner);

        let msg = convert_raw_message(&raw, &peer, &entities).expect("converts");
        assert_eq!(msg.sender_id.map(|u| u.get()), Some(42));
        assert_eq!(
            msg.sender_name.as_deref(),
            Some("Иван"),
            "first-name-only parity with the high-level path"
        );
    }

    #[test]
    fn convert_raw_message_refuses_empty_placeholder() {
        let peer = public_channel_peer(1144180066, "swodki");
        let raw = tl::enums::Message::Empty(tl::types::MessageEmpty {
            id: 1,
            peer_id: None,
        });
        assert!(convert_raw_message(&raw, &peer, &EntityLookup::empty()).is_none());
    }

    #[test]
    fn forward_from_legacy_group_carries_title_without_id() {
        let entities = EntityLookup::from_envelope(
            &[tl::enums::Chat::Chat(tl::types::Chat {
                creator: false,
                left: false,
                deactivated: false,
                call_active: false,
                call_not_empty: false,
                noforwards: false,
                id: 31,
                title: "Группа".to_string(),
                photo: tl::enums::ChatPhoto::Empty,
                participants_count: 0,
                date: 0,
                version: 0,
                migrated_to: None,
                admin_rights: None,
                default_banned_rights: None,
            })],
            &[],
        );
        let from = Some(tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: 31 }));
        let info = extract_forward_info(&fwd_header(from, None, None), &entities);
        assert_eq!(
            info.channel_id, None,
            "chat-namespace ids stay unemitted, as today"
        );
        assert_eq!(
            info.channel_name.as_ref().map(|n| n.as_str()),
            Some("Группа")
        );
    }

    /// Inert client for offline Peer construction (same trick as the
    /// channel-converter tests).
    fn inert_client() -> Client {
        let session = Arc::new(MemorySession::default());
        let SenderPool { handle, .. } = SenderPool::new(session, 1);
        Client::new(handle)
    }

    fn public_channel_peer(id: i64, username: &str) -> Peer {
        let client = inert_client();
        let raw = tl::types::Channel {
            creator: false,
            left: false,
            broadcast: true,
            verified: false,
            megagroup: false,
            restricted: false,
            signatures: false,
            min: false,
            scam: false,
            has_link: false,
            has_geo: false,
            slowmode_enabled: false,
            call_active: false,
            call_not_empty: false,
            fake: false,
            gigagroup: false,
            noforwards: false,
            join_to_send: false,
            join_request: false,
            forum: false,
            stories_hidden: false,
            stories_hidden_min: false,
            stories_unavailable: true,
            signature_profiles: false,
            autotranslation: false,
            broadcast_messages_allowed: false,
            monoforum: false,
            forum_tabs: false,
            id,
            access_hash: Some(0),
            title: "Test Channel".to_string(),
            username: Some(username.to_string()),
            photo: tl::enums::ChatPhoto::Empty,
            date: 0,
            restriction_reason: None,
            admin_rights: None,
            banned_rights: None,
            default_banned_rights: None,
            participants_count: None,
            usernames: None,
            stories_max_id: None,
            color: None,
            profile_color: None,
            emoji_status: None,
            level: None,
            subscription_until_date: None,
            bot_verification_icon: None,
            send_paid_messages_stars: None,
            linked_monoforum_id: None,
            linked_community_id: None,
        };
        Peer::from_raw(&client, tl::enums::Chat::Channel(raw))
    }

    fn private_community_peer(id: i64) -> Peer {
        let client = inert_client();
        let raw = tl::types::Community {
            creator: false,
            left: false,
            min: false,
            collapsed_in_dialogs: false,
            id,
            access_hash: Some(0),
            title: "Test Community".to_string(),
            photo: tl::enums::ChatPhoto::Empty,
            date: 0,
            admin_rights: None,
            default_banned_rights: None,
        };
        Peer::Community(Community::from_raw(
            &client,
            tl::enums::Chat::Community(raw),
        ))
    }

    #[test]
    fn build_message_link_uses_public_form_when_username_exists() {
        let peer = public_channel_peer(1144180066, "swodki");
        let link = build_message_link(&peer, MessageId::new(610121).expect("valid id"));
        assert_eq!(link.as_deref(), Some("https://t.me/swodki/610121"));
    }

    #[test]
    fn build_message_link_falls_back_to_internal_form() {
        let peer = private_community_peer(521440428);
        let link = build_message_link(&peer, MessageId::new(5).expect("valid id"));
        assert_eq!(link.as_deref(), Some("https://t.me/c/521440428/5"));
    }

    #[test]
    fn extract_reactions_itemizes_emoji_and_totals_everything() {
        let raw = tl::enums::MessageReactions::Reactions(tl::types::MessageReactions {
            min: false,
            can_see_list: false,
            reactions_as_tags: false,
            results: vec![
                tl::enums::ReactionCount::Count(tl::types::ReactionCount {
                    chosen_order: None,
                    reaction: tl::enums::Reaction::Emoji(tl::types::ReactionEmoji {
                        emoticon: "🔥".to_string(),
                    }),
                    count: 41,
                }),
                tl::enums::ReactionCount::Count(tl::types::ReactionCount {
                    chosen_order: None,
                    reaction: tl::enums::Reaction::CustomEmoji(tl::types::ReactionCustomEmoji {
                        document_id: 7,
                    }),
                    count: 2,
                }),
            ],
            recent_reactions: None,
            top_reactors: None,
        });

        let (itemized, total) = extract_reactions(Some(&raw));
        let itemized = itemized.expect("emoji reactions present");
        assert_eq!(itemized.len(), 1, "custom emoji is not itemized");
        assert_eq!(itemized[0].emoji, "🔥");
        assert_eq!(itemized[0].count, 41);
        assert_eq!(total, Some(43), "total counts every reaction kind");
    }

    #[test]
    fn extract_reactions_none_when_absent() {
        assert_eq!(extract_reactions(None), (None, None));
    }

    #[test]
    fn timestamp_from_raw_refuses_empty_placeholder() {
        let raw = tl::enums::Message::Empty(tl::types::MessageEmpty {
            id: 609784,
            peer_id: None,
        });
        assert_eq!(
            timestamp_from_raw(&raw),
            None,
            "Empty must never yield a date (B1/B8)"
        );
    }

    #[test]
    fn timestamp_from_raw_converts_service_date() {
        let raw = tl::enums::Message::Service(tl::types::MessageService {
            out: false,
            mentioned: false,
            media_unread: false,
            reactions_are_possible: false,
            silent: false,
            post: true,
            legacy: false,
            id: 610119,
            from_id: None,
            peer_id: tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 1 }),
            saved_peer_id: None,
            reply_to: None,
            date: 1_700_000_000,
            action: tl::enums::MessageAction::Empty,
            reactions: None,
            ttl_period: None,
        });
        let ts = timestamp_from_raw(&raw).expect("real date converts");
        assert_eq!(ts.timestamp(), 1_700_000_000);
    }
}
