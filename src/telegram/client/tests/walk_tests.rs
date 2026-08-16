//! Unit tests for the synchronous accumulation-loop decision machine.

use super::*;
use crate::telegram::envelope::EntityLookup;
use crate::test_helpers::{raw_tl_channel, raw_tl_message};
use chrono::{DateTime, TimeZone, Utc};
use grammers_client::Client;
use grammers_client::peer::Peer;
use grammers_mtsender::SenderPool;
use grammers_session::storages::MemorySession;
use std::sync::Arc;

/// A `Client` that never touches the network: the `SenderPool` runner is
/// destructured away and never spawned, so `Peer::from_raw` works offline.
fn inert_client() -> Client {
    let session = Arc::new(MemorySession::default());
    let SenderPool { handle, .. } = SenderPool::new(session, 1);
    Client::new(handle)
}

fn channel_peer(client: &Client, id: i64) -> Peer {
    Peer::from_raw(
        client,
        grammers_client::tl::enums::Chat::Channel(raw_tl_channel(id, "Канал", None)),
    )
}

fn no_entities() -> Arc<EntityLookup> {
    Arc::new(EntityLookup::from_envelope(&[], &[]))
}

fn at(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0)
        .single()
        .expect("valid timestamp")
}

/// Config admitting everything at or after `cutoff`, with no other bounds.
fn open_config(cutoff: DateTime<Utc>) -> WalkConfig<'static> {
    WalkConfig {
        cutoff_time: cutoff,
        to_date: None,
        after_bound: None,
        media_filter: None,
        below_cutoff: BelowCutoff::Stop,
    }
}

fn fetched<'p>(id: i32, date: i32, peer: &'p Peer) -> Fetched<'p> {
    Fetched {
        raw: raw_tl_message(id, date, 11),
        entities: no_entities(),
        peer: Some(peer),
    }
}

#[test]
fn an_empty_round_trip_still_counts_a_page() {
    // The `None` fetch means the pager is exhausted, but the round trip that
    // discovered that still cost the caller latency — `pages_fetched` reports
    // round trips, so accounting must happen before the terminal Stop.
    let mut walk = MessageWalk::new(open_config(at(1_000)), false, 10, 0);

    assert_eq!(walk.step(None, Some(0)), Flow::Stop);
    assert_eq!(walk.pages_fetched(), 1);
    assert_eq!(walk.messages_scanned(), 0);
}

#[test]
fn a_full_page_stops_the_walk_and_latches_has_more() {
    let client = inert_client();
    let peer = channel_peer(&client, 11);
    let mut walk = MessageWalk::new(open_config(at(1_000)), false, 1, 0);

    assert_eq!(
        walk.step(Some(fetched(2, 2_000, &peer)), Some(2)),
        Flow::Continue
    );
    assert_eq!(walk.step(Some(fetched(1, 1_500, &peer)), None), Flow::Stop);

    let (page, budget) = walk.into_parts();
    assert!(page.has_more(), "a refused message must latch has_more");
    assert_eq!(page.into_messages().len(), 1);
    assert_eq!(budget.pages_fetched(), 1);
    assert_eq!(budget.messages_scanned(), 2);
}
