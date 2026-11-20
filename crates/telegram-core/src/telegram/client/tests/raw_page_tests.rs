//! Tests for raw-TL envelope interpretation (`raw_page`).

use super::*;
use crate::telegram::client::raw_pager::PAGE_LIMIT;
use crate::test_helpers::{raw_tl_channel, raw_tl_message, raw_tl_messages_slice};

#[test]
fn unpack_slice_keeps_envelope_and_separates_scope_exhaustion() {
    let page = unpack_page(raw_tl_messages_slice(
        vec![raw_tl_message(500, 1_700_000_000, 11)],
        Some(7),
    ));
    assert!(
        !page.single_peer_exhausted(PAGE_LIMIT),
        "id 500 > limit 100 → more pages may exist"
    );
    assert!(!page.global_exhausted());
    assert_eq!(page.next_rate, Some(7));
    assert_eq!(page.messages.len(), 1);
    assert_eq!(page.chats.len(), 1);

    let tail = unpack_page(raw_tl_messages_slice(
        vec![raw_tl_message(90, 1_700_000_000, 11)],
        None,
    ));
    assert!(
        tail.single_peer_exhausted(PAGE_LIMIT),
        "highest id <= limit → nothing older can exist"
    );

    assert!(
        !tail.global_exhausted(),
        "low IDs only exhaust single-peer walks"
    );

    let empty = unpack_page(raw_tl_messages_slice(vec![], None));
    assert!(empty.single_peer_exhausted(PAGE_LIMIT));
    assert!(empty.global_exhausted());
}

#[test]
fn unpack_messages_variant_is_always_last_chunk() {
    let res = tl::enums::messages::Messages::Messages(tl::types::messages::Messages {
        messages: vec![raw_tl_message(3, 1_700_000_000, 11)],
        topics: vec![],
        chats: vec![],
        users: vec![],
    });
    let page = unpack_page(res);
    assert!(page.single_peer_exhausted(PAGE_LIMIT));
    assert!(page.global_exhausted());
}

#[test]
fn global_offset_peer_resolves_access_hash_from_envelope() {
    let page = unpack_page(raw_tl_messages_slice(
        vec![raw_tl_message(500, 1_700_000_000, 11)],
        Some(9),
    ));
    let input = input_peer_for_message(&page.messages[0], &page);
    match input {
        tl::enums::InputPeer::Channel(c) => assert_eq!(c.channel_id, 11),
        other => panic!("expected InputPeer::Channel, got {other:?}"),
    }

    // Envelope miss → Empty, mirroring grammers' unwrap_or fallback.
    let missing = raw_tl_message(501, 1_700_000_000, 999);
    assert!(matches!(
        input_peer_for_message(&missing, &page),
        tl::enums::InputPeer::Empty
    ));
}

#[test]
fn global_offset_peer_resolves_community_and_forbidden_chats() {
    // Communities and Forbidden channels are channel-namespace too; a
    // page ending in one must still produce a real offset peer, not
    // Empty (would drift SearchGlobal pagination).
    let community_page = RawPage {
        messages: vec![raw_tl_message(500, 1_700_000_000, 21)],
        chats: vec![tl::enums::Chat::Community(
            crate::test_helpers::raw_tl_community(21, "Группа"),
        )],
        users: vec![],
        next_rate: None,
        kind: PageKind::Slice,
    };
    match input_peer_for_message(&community_page.messages[0], &community_page) {
        tl::enums::InputPeer::Channel(c) => assert_eq!(c.channel_id, 21),
        other => panic!("expected InputPeer::Channel for community, got {other:?}"),
    }

    let forbidden_page = RawPage {
        messages: vec![raw_tl_message(501, 1_700_000_000, 22)],
        chats: vec![tl::enums::Chat::ChannelForbidden(
            tl::types::ChannelForbidden {
                broadcast: true,
                megagroup: false,
                monoforum: false,
                id: 22,
                access_hash: 77,
                title: "Закрытый".to_string(),
                until_date: None,
            },
        )],
        users: vec![],
        next_rate: None,
        kind: PageKind::Slice,
    };
    match input_peer_for_message(&forbidden_page.messages[0], &forbidden_page) {
        tl::enums::InputPeer::Channel(c) => {
            assert_eq!(c.channel_id, 22);
            assert_eq!(c.access_hash, 77, "forbidden variant's hash is carried");
        }
        other => panic!("expected InputPeer::Channel for forbidden, got {other:?}"),
    }
}

#[test]
fn fetch_decode_builds_an_entity_map_from_the_response_envelope() {
    // THE load-bearing test for work order A. The bug was that
    // getMessages responses had their chats/users discarded, leaving
    // forwards ids-only. This asserts the decode keeps them, so a forward
    // source the account does not subscribe to is still attributable.
    let res =
        tl::enums::messages::Messages::ChannelMessages(tl::types::messages::ChannelMessages {
            inexact: false,
            pts: 1,
            count: 1,
            offset_id_offset: None,
            messages: vec![raw_tl_message(298716, 1_700_000_000, 1912881684)],
            topics: vec![],
            // The forward SOURCE — a channel we never asked about, present
            // only because the envelope names every entity its messages
            // reference.
            chats: vec![tl::enums::Chat::Channel(raw_tl_channel(
                1783384254,
                "Pavel Zloi",
                Some("evilfreelancer"),
            ))],
            users: vec![],
        });

    let page = unpack_page(res);
    let entities = EntityLookup::from_envelope(&page.chats, &page.users);

    let source = tl::enums::Peer::Channel(tl::types::PeerChannel {
        channel_id: 1783384254,
    });
    let info = entities
        .get(&source)
        .expect("envelope must name the forward source");
    assert_eq!(info.display_name.as_deref(), Some("Pavel Zloi"));
    assert_eq!(info.username.as_deref(), Some("evilfreelancer"));
}

#[test]
fn unpack_page_treats_not_modified_as_an_empty_final_page() {
    let page = unpack_page(tl::enums::messages::Messages::NotModified(
        tl::types::messages::MessagesNotModified { count: 0 },
    ));

    assert!(page.messages.is_empty());
    assert!(page.single_peer_exhausted(PAGE_LIMIT));
    assert!(page.global_exhausted());
}

#[test]
fn same_number_user_and_channel_select_their_own_envelope_identity() {
    use crate::test_helpers::raw_tl_user;
    use grammers_client::Client;
    use grammers_mtsender::SenderPool;
    use grammers_session::storages::MemorySession;
    use grammers_session::types::PeerId;
    use std::sync::Arc;

    let SenderPool { handle, .. } = SenderPool::new(Arc::new(MemorySession::default()), 1);
    let client = Client::new(handle);
    let chats = vec![tl::enums::Chat::Channel(raw_tl_channel(
        11, "Channel", None,
    ))];
    let users = vec![tl::enums::User::User(raw_tl_user(
        11,
        Some("User"),
        None,
        None,
    ))];
    let channel = raw_tl_message(1, 1_700_000_000, 11);
    let mut user = raw_tl_message(2, 1_700_000_000, 11);
    if let tl::enums::Message::Service(message) = &mut user {
        message.peer_id = tl::enums::Peer::User(tl::types::PeerUser { user_id: 11 });
    }
    let channel_peer = chat_peer_for_message(&client, &channel, &chats, &users).expect("channel");
    let user_peer = chat_peer_for_message(&client, &user, &chats, &users).expect("user");
    assert_eq!(channel_peer.id(), PeerId::channel_unchecked(11));
    assert_eq!(user_peer.id(), PeerId::user_unchecked(11));
    assert_ne!(channel_peer.id(), user_peer.id());
    // This characterizes the untyped numeric resolver's ambiguity without
    // asserting a live collision: its bare-id predicate accepts both.
    assert_eq!(channel_peer.id().bare_id(), user_peer.id().bare_id());
}

#[test]
fn global_mixed_peer_slice_is_not_exhausted_by_first_message_id() {
    let messages = (0..100)
        .map(|index| {
            let mut raw = raw_tl_message(
                if index == 0 { 9 } else { 2_000 - index },
                1_700_000_000,
                11,
            );
            if let tl::enums::Message::Service(message) = &mut raw {
                message.peer_id = match index % 3 {
                    0 => tl::enums::Peer::User(tl::types::PeerUser { user_id: 11 }),
                    1 => tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: 11 }),
                    _ => tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 11 }),
                };
            }
            raw
        })
        .collect();
    let page = unpack_page(raw_tl_messages_slice(messages, Some(7)));
    assert!(
        !page.global_exhausted(),
        "a low ID in one peer cannot exhaust global search"
    );
}

#[test]
fn channel_messages_keeps_single_peer_low_id_policy_without_exhausting_global_scope() {
    for id in [9, 500] {
        let page = unpack_page(tl::enums::messages::Messages::ChannelMessages(
            tl::types::messages::ChannelMessages {
                inexact: false,
                pts: 1,
                count: 1,
                offset_id_offset: None,
                messages: vec![raw_tl_message(id, 1_700_000_000, 11)],
                topics: vec![],
                chats: vec![],
                users: vec![],
            },
        ));
        assert_eq!(page.single_peer_exhausted(PAGE_LIMIT), id <= PAGE_LIMIT);
        assert!(!page.global_exhausted());
    }
}
