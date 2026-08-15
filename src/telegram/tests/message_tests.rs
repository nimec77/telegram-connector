//! Tests for raw-TL → domain message conversion (forward enrichment,
//! links, reactions, timestamps).

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
    let raw = raw_forwarded_message(610121, fwd_header(channel_fwd_peer(1783384254), None, None));

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

#[test]
fn batch_style_conversion_enriches_forward_from_a_shared_envelope() {
    // One envelope shared by every message in a getMessages response — the
    // shape fetch_messages_by_id returns. Both messages must attribute.
    let peer = public_channel_peer(1144180066, "swodki");
    let entities = EntityLookup::from_envelope(
        &[tl::enums::Chat::Channel(raw_tl_channel(
            1783384254,
            "Pavel Zloi",
            Some("evilfreelancer"),
        ))],
        &[],
    );

    for id in [610121, 610122] {
        let raw = raw_forwarded_message(id, fwd_header(channel_fwd_peer(1783384254), None, None));
        let msg = convert_raw_message(&raw, &peer, &entities).expect("converts");
        let fwd = msg.forwarded_from.expect("forward attribution present");
        assert_eq!(
            fwd.channel_name.as_ref().map(|n| n.as_str()),
            Some("Pavel Zloi")
        );
        assert_eq!(
            fwd.channel_username.as_ref().map(|u| u.as_str()),
            Some("evilfreelancer")
        );
    }
}
