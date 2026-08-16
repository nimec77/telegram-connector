//! Tests for channel discovery/subscription helpers (`channels`).

use super::*;
use grammers_client::peer::Peer;
use grammers_mtsender::SenderPool;
use grammers_session::storages::MemorySession;

/// Build an inert `Client` for offline `Peer::from_raw` construction: the
/// `SenderPool` runner is never spawned, so no I/O can happen. Same trick as
/// the `converters::channel` tests.
fn inert_client() -> Client {
    let session = Arc::new(MemorySession::default());
    let SenderPool { handle, .. } = SenderPool::new(session, 1);
    Client::new(handle)
}

/// A raw `channel#...` TL object. `broadcast`/`megagroup` are the two flags
/// that decide which `Peer` variant `Peer::from_raw` produces.
fn raw_channel(id: i64, broadcast: bool) -> tl::types::Channel {
    tl::types::Channel {
        creator: false,
        left: false,
        broadcast,
        verified: false,
        megagroup: !broadcast,
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
        stories_unavailable: false,
        signature_profiles: false,
        autotranslation: false,
        broadcast_messages_allowed: false,
        monoforum: false,
        forum_tabs: false,
        id,
        access_hash: Some(1234),
        title: "Test Channel".to_string(),
        username: Some("testchannel".to_string()),
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
    }
}

/// A raw small-group `chat#...` TL object (chat-kind, not channel-kind).
fn raw_small_group(id: i64) -> tl::types::Chat {
    tl::types::Chat {
        creator: false,
        left: false,
        deactivated: false,
        call_active: false,
        call_not_empty: false,
        noforwards: false,
        id,
        title: "Small Group".to_string(),
        photo: tl::enums::ChatPhoto::Empty,
        participants_count: 3,
        date: 0,
        version: 1,
        migrated_to: None,
        admin_rights: None,
        default_banned_rights: None,
    }
}

#[test]
fn subscribed_peer_keys_collects_chat_and_channel_keys_only() {
    let my_results = vec![
        tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: 111 }),
        tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 222 }),
        tl::enums::Peer::User(tl::types::PeerUser { user_id: 333 }),
    ];

    let keys = subscribed_peer_keys(&my_results);

    assert_eq!(keys.len(), 2);
    assert!(keys.contains(&SubscriptionKey::Chat(111)));
    assert!(keys.contains(&SubscriptionKey::Channel(222)));
    assert!(!keys.contains(&SubscriptionKey::Chat(333)));
    assert!(!keys.contains(&SubscriptionKey::Channel(333)));
}

#[test]
fn subscribed_peer_keys_empty_when_no_matches() {
    assert!(subscribed_peer_keys(&[]).is_empty());
}

#[test]
fn subscribed_peer_keys_do_not_collide_across_namespaces() {
    // `PeerChat.chat_id` and `PeerChannel.channel_id` are independent id
    // namespaces: a subscribed small group with id 111 must never mark a
    // never-joined channel that happens to also have id 111 as subscribed.
    let my_results = vec![tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: 111 })];
    let keys = subscribed_peer_keys(&my_results);

    let channel_with_same_bare_id = tl::enums::Chat::Channel(raw_channel(111, true));
    assert!(
        !keys.contains(&chat_subscription_key(&channel_with_same_bare_id)),
        "a chat-namespace id must not mark a channel-namespace peer subscribed"
    );

    let group_with_same_bare_id = tl::enums::Chat::Chat(raw_small_group(111));
    assert!(
        keys.contains(&chat_subscription_key(&group_with_same_bare_id)),
        "the actual subscribed small group must still be recognised"
    );
}

#[test]
fn chat_subscription_key_routes_each_variant_to_its_namespace() {
    assert_eq!(
        chat_subscription_key(&tl::enums::Chat::Chat(raw_small_group(7))),
        SubscriptionKey::Chat(7)
    );
    assert_eq!(
        chat_subscription_key(&tl::enums::Chat::Channel(raw_channel(7, true))),
        SubscriptionKey::Channel(7)
    );
    assert_eq!(
        chat_subscription_key(&tl::enums::Chat::Channel(raw_channel(7, false))),
        SubscriptionKey::Channel(7)
    );
}

#[test]
fn supports_full_channel_rpc_accepts_broadcasts() {
    let client = inert_client();
    let peer = Peer::from_raw(&client, tl::enums::Chat::Channel(raw_channel(1, true)));

    assert!(
        matches!(peer, Peer::Channel(_)),
        "broadcast routes to Peer::Channel"
    );
    assert!(supports_full_channel_rpc(&peer));
}

#[test]
fn supports_full_channel_rpc_accepts_megagroups() {
    // grammers routes a non-broadcast `Chat::Channel` to `Peer::Group`, but a
    // megagroup is still channel-kind in TL: its `PeerRef` converts to a real
    // `InputChannel`, so `channels.GetFullChannel` applies.
    let client = inert_client();
    let peer = Peer::from_raw(&client, tl::enums::Chat::Channel(raw_channel(2, false)));

    assert!(
        matches!(peer, Peer::Group(_)),
        "grammers routes megagroups to Peer::Group"
    );
    assert!(
        supports_full_channel_rpc(&peer),
        "megagroups must reach GetFullChannel"
    );
}

#[test]
fn supports_full_channel_rpc_rejects_small_groups() {
    // A small group is chat-kind: `From<&PeerRef> for InputChannel` yields
    // `InputChannel::Empty` for it, so it must not reach the RPC.
    let client = inert_client();
    let peer = Peer::from_raw(&client, tl::enums::Chat::Chat(raw_small_group(3)));

    assert!(matches!(peer, Peer::Group(_)));
    assert!(!supports_full_channel_rpc(&peer));
}

#[test]
fn validate_channel_identifier_rejects_empty() {
    let err = validate_channel_identifier("").expect_err("empty identifier must be rejected");
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(
        err.to_string()
            .contains("Channel identifier cannot be empty")
    );
}

#[test]
fn validate_channel_identifier_accepts_non_empty() {
    assert!(validate_channel_identifier("@news").is_ok());
}

#[test]
fn total_counts_every_channel_while_the_page_is_cut_out_in_passing() {
    // B6: the walk continues past the page so `total` is the genuine
    // subscription count, not the page length.
    use crate::test_helpers::create_test_channel_named;

    let mut builder = ChannelPageBuilder::new(1, 2);
    for id in 1..=5 {
        builder.admit(create_test_channel_named(id, &format!("Канал {id}"), true));
    }
    let page = builder.finish();

    assert_eq!(page.total, 5, "total must count every channel walked");
    assert_eq!(page.channels.len(), 2, "the page honours limit");
    assert_eq!(
        page.channels[0].id.get(),
        2,
        "offset 1 skips the first channel"
    );
}

#[test]
fn an_offset_past_the_end_yields_an_empty_page_with_a_real_total() {
    use crate::test_helpers::create_test_channel_named;

    let mut builder = ChannelPageBuilder::new(10, 5);
    for id in 1..=3 {
        builder.admit(create_test_channel_named(id, "Канал", true));
    }
    let page = builder.finish();

    assert!(page.channels.is_empty());
    assert_eq!(page.total, 3);
}

#[test]
fn an_empty_chat_result_is_skipped_rather_than_shown_as_unknown() {
    let subscribed = std::collections::HashSet::new();
    let chat = tl::enums::Chat::Empty(tl::types::ChatEmpty { id: 7 });

    assert_eq!(classify_search_hit(&chat, &subscribed), SearchHit::Skip);
}

#[test]
fn a_chat_in_my_results_classifies_as_subscribed() {
    let my_results = vec![tl::enums::Peer::Channel(tl::types::PeerChannel {
        channel_id: 11,
    })];
    let subscribed = subscribed_peer_keys(&my_results);
    let chat = tl::enums::Chat::Channel(crate::test_helpers::raw_tl_channel(11, "Канал", None));

    assert_eq!(
        classify_search_hit(&chat, &subscribed),
        SearchHit::Subscribed
    );
}

#[test]
fn a_numeric_collision_across_namespaces_does_not_mark_a_channel_subscribed() {
    // PeerChat.chat_id and PeerChannel.channel_id are independent namespaces;
    // a bare i64 key would wrongly match here.
    let my_results = vec![tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: 11 })];
    let subscribed = subscribed_peer_keys(&my_results);
    let chat = tl::enums::Chat::Channel(crate::test_helpers::raw_tl_channel(11, "Канал", None));

    assert_eq!(
        classify_search_hit(&chat, &subscribed),
        SearchHit::Discovered,
        "a chat-namespace id must not match a channel-namespace id"
    );
}
