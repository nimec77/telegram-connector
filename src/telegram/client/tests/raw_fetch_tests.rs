//! Tests for the raw-TL by-id fetch (`raw_fetch`).

use super::*;
use crate::test_helpers::raw_tl_message;
use grammers_session::types::{PeerAuth, PeerId, PeerRef};

fn channel_ref(id: i64) -> PeerRef {
    PeerRef {
        id: PeerId::channel_unchecked(id),
        auth: PeerAuth::from_hash(0),
    }
}

fn chat_ref(id: i64) -> PeerRef {
    PeerRef {
        id: PeerId::chat_unchecked(id),
        auth: PeerAuth::default(),
    }
}

#[test]
fn channel_peer_routes_to_channels_get_messages() {
    let request = get_messages_request(channel_ref(1144180066), &[610121, 610122]);

    match request {
        GetMessagesRequest::Channel(r) => assert_eq!(r.id.len(), 2),
        GetMessagesRequest::Plain(_) => panic!("channel peer must use channels.GetMessages"),
    }
}

#[test]
fn non_channel_peer_routes_to_messages_get_messages() {
    let request = get_messages_request(chat_ref(521440428), &[7]);

    match request {
        GetMessagesRequest::Plain(r) => assert_eq!(r.id.len(), 1),
        GetMessagesRequest::Channel(_) => panic!("chat peer must use messages.GetMessages"),
    }
}

#[test]
fn index_messages_keys_by_id_regardless_of_response_order() {
    let messages = vec![
        raw_tl_message(610122, 1_700_000_100, 1144180066),
        raw_tl_message(610121, 1_700_000_000, 1144180066),
    ];

    let indexed = index_messages(messages, channel_ref(1144180066));

    assert_eq!(indexed.len(), 2);
    assert_eq!(indexed[&610121].id(), 610121);
    assert_eq!(indexed[&610122].id(), 610122);
}

#[test]
fn index_messages_drops_a_message_from_a_different_peer() {
    // messages.GetMessages resolves bare ids across every dialog, so a
    // response can name a chat we did not ask about.
    let messages = vec![
        raw_tl_message(610121, 1_700_000_000, 1144180066),
        raw_tl_message(610122, 1_700_000_100, 999_999),
    ];

    let indexed = index_messages(messages, channel_ref(1144180066));

    assert_eq!(indexed.len(), 1);
    assert!(indexed.contains_key(&610121));
    assert!(!indexed.contains_key(&610122));
}

#[test]
fn index_messages_keeps_empty_placeholders_for_the_caller_to_classify() {
    let messages = vec![tl::enums::Message::Empty(tl::types::MessageEmpty {
        id: 609784,
        peer_id: None,
    })];

    let indexed = index_messages(messages, channel_ref(1144180066));

    assert!(indexed.contains_key(&609784));
}
