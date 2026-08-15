//! Tests for raw-TL envelope interpretation (`raw_page`).

use super::*;
use crate::telegram::client::raw_pager::PAGE_LIMIT;
use crate::test_helpers::{raw_tl_channel, raw_tl_message, raw_tl_messages_slice};

#[test]
fn unpack_slice_computes_last_chunk_and_keeps_envelope() {
    let page = unpack_page(
        raw_tl_messages_slice(vec![raw_tl_message(500, 1_700_000_000, 11)], Some(7)),
        100,
    );
    assert!(
        !page.last_chunk,
        "id 500 > limit 100 → more pages may exist"
    );
    assert_eq!(page.next_rate, Some(7));
    assert_eq!(page.messages.len(), 1);
    assert_eq!(page.chats.len(), 1);

    let tail = unpack_page(
        raw_tl_messages_slice(vec![raw_tl_message(90, 1_700_000_000, 11)], None),
        100,
    );
    assert!(
        tail.last_chunk,
        "highest id <= limit → nothing older can exist"
    );

    let empty = unpack_page(raw_tl_messages_slice(vec![], None), 100);
    assert!(empty.last_chunk);
}

#[test]
fn unpack_messages_variant_is_always_last_chunk() {
    let res = tl::enums::messages::Messages::Messages(tl::types::messages::Messages {
        messages: vec![raw_tl_message(3, 1_700_000_000, 11)],
        topics: vec![],
        chats: vec![],
        users: vec![],
    });
    assert!(unpack_page(res, 100).last_chunk);
}

#[test]
fn global_offset_peer_resolves_access_hash_from_envelope() {
    let page = unpack_page(
        raw_tl_messages_slice(vec![raw_tl_message(500, 1_700_000_000, 11)], Some(9)),
        100,
    );
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
        last_chunk: false,
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
        last_chunk: false,
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

    let page = unpack_page(res, PAGE_LIMIT);
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
    let page = unpack_page(
        tl::enums::messages::Messages::NotModified(tl::types::messages::MessagesNotModified {
            count: 0,
        }),
        PAGE_LIMIT,
    );

    assert!(page.messages.is_empty());
    assert!(page.last_chunk);
}
