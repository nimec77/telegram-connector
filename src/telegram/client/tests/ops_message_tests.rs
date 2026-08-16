//! Unit tests for the batch-partition invariant.

use super::*;
use crate::telegram::envelope::EntityLookup;
use crate::test_helpers::{raw_tl_channel, raw_tl_message};
use grammers_client::Client;
use grammers_client::peer::Peer;
use grammers_mtsender::SenderPool;
use grammers_session::storages::MemorySession;
use std::collections::HashMap;
use std::sync::Arc;

fn inert_peer() -> (Client, Peer) {
    let session = Arc::new(MemorySession::default());
    let SenderPool { handle, .. } = SenderPool::new(session, 1);
    let client = Client::new(handle);
    let peer = Peer::from_raw(
        &client,
        tl::enums::Chat::Channel(raw_tl_channel(11, "Канал", None)),
    );
    (client, peer)
}

#[test]
fn every_requested_id_lands_in_exactly_one_bucket() {
    // The batch invariant: no id may be silently dropped.
    let (_client, peer) = inert_peer();
    let entities = EntityLookup::from_envelope(&[], &[]);
    let mut by_id = HashMap::new();
    by_id.insert(1, raw_tl_message(1, 1_000, 11));
    // id 2 is absent entirely (never existed)
    by_id.insert(
        3,
        tl::enums::Message::Empty(tl::types::MessageEmpty {
            id: 3,
            peer_id: None,
        }),
    );

    let batch = partition_batch(&[1, 2, 3], &mut by_id, &peer, &entities, "@канал");

    assert_eq!(batch.messages.len(), 1, "only id 1 exists");
    assert_eq!(batch.missing_ids, vec![2, 3]);
    assert_eq!(
        batch.messages.len() + batch.missing_ids.len(),
        3,
        "every requested id must land in exactly one bucket"
    );
}

#[test]
fn a_message_empty_placeholder_is_missing_not_a_fabricated_message() {
    // grammers wraps a deleted id in a MessageEmpty-backed object rather than
    // omitting it; converting it blind would fabricate an epoch-0 message.
    let (_client, peer) = inert_peer();
    let entities = EntityLookup::from_envelope(&[], &[]);
    let mut by_id = HashMap::new();
    by_id.insert(
        9,
        tl::enums::Message::Empty(tl::types::MessageEmpty {
            id: 9,
            peer_id: None,
        }),
    );

    let batch = partition_batch(&[9], &mut by_id, &peer, &entities, "@канал");

    assert!(batch.messages.is_empty());
    assert_eq!(batch.missing_ids, vec![9]);
}

#[test]
fn a_present_message_that_fails_conversion_is_reported_missing_not_dropped() {
    // A bare id-0 message: it is present in the map and is NOT a
    // `MessageEmpty` (it's a `Message::Service`), so it clears the
    // `is_empty_variant` guard and `convert_raw_message`'s own `MessageEmpty`
    // check — but `MessageId::new(0)` rejects non-positive ids, so
    // `convert_raw_message` still returns `None`. This exercises the branch
    // that would otherwise silently drop the id instead of reporting it
    // missing.
    let (_client, peer) = inert_peer();
    let entities = EntityLookup::from_envelope(&[], &[]);
    let mut by_id = HashMap::new();
    by_id.insert(0, raw_tl_message(0, 1_000, 11));

    let batch = partition_batch(&[0], &mut by_id, &peer, &entities, "@канал");

    assert!(batch.messages.is_empty());
    assert_eq!(batch.missing_ids, vec![0]);
}

#[test]
fn requested_order_is_preserved_in_missing_ids() {
    let (_client, peer) = inert_peer();
    let entities = EntityLookup::from_envelope(&[], &[]);
    let mut by_id = HashMap::new();

    let batch = partition_batch(&[5, 3, 9], &mut by_id, &peer, &entities, "@канал");

    assert_eq!(batch.missing_ids, vec![5, 3, 9]);
}
