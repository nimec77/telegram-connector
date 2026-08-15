//! Raw-TL history/search pagers.
//!
//! Replicates the pagination of grammers' `MessageIter` / `SearchIter` /
//! `GlobalSearchIter` (pinned rev, `client/messages.rs`) while keeping the
//! response envelope's `chats`+`users`, which the high-level iterators
//! discard behind a crate-private `PeerMap`. Same requests, same order, same
//! stop conditions — zero additional network calls; the envelope feeds
//! forward attribution (see `telegram/envelope.rs`).

use super::raw_page::{
    RawPage, chat_peer_for_message, fill_buffer, input_peer_for_message, unpack_page,
};
use crate::telegram::envelope::EntityLookup;
use chrono::{DateTime, Utc};
use grammers_client::Client;
use grammers_client::tl;
use grammers_mtsender::InvocationError;
use grammers_session::types::PeerRef;
use std::collections::VecDeque;
use std::sync::Arc;

/// grammers' MAX_LIMIT: server pages cap at 100 messages.
pub(super) const PAGE_LIMIT: i32 = 100;

/// Raw date of a message (0 for Empty — mirrors grammers `date_timestamp`).
fn raw_date(raw: &tl::enums::Message) -> i32 {
    match raw {
        tl::enums::Message::Message(m) => m.date,
        tl::enums::Message::Service(m) => m.date,
        tl::enums::Message::Empty(_) => 0,
    }
}

fn history_request(
    peer: tl::enums::InputPeer,
    offset_id: i32,
) -> tl::functions::messages::GetHistory {
    tl::functions::messages::GetHistory {
        peer,
        offset_id,
        offset_date: 0,
        add_offset: 0,
        limit: PAGE_LIMIT,
        max_id: 0,
        min_id: 0,
        hash: 0,
    }
}

fn advance_history_offsets(request: &mut tl::functions::messages::GetHistory, page: &RawPage) {
    if let Some(last) = page.messages.last() {
        request.offset_id = last.id();
        request.offset_date = raw_date(last);
    }
}

fn advance_search_offsets(request: &mut tl::functions::messages::Search, page: &RawPage) {
    if let Some(last) = page.messages.last() {
        request.offset_id = last.id();
        request.max_date = raw_date(last);
    }
}

/// Map a time window onto `messages.SearchGlobal`'s `min_date`/`max_date`,
/// widened by one second at each end.
///
/// The TL schema types both as `int` (i32 unix seconds) and treats `0` as
/// "unbounded". Out-of-range instants clamp rather than error: a degraded
/// bound costs a slower search, a rejected one costs the caller their result.
///
/// Neither the TL schema nor grammers documents whether the bounds are
/// inclusive or exclusive, and the ±1 makes the answer stop mattering: the
/// server window is a strict superset of the requested one under *either*
/// semantics, so no message can be dropped server-side that the client-side
/// window checks in `ops_search` would have kept. Without it, an exclusive
/// server silently drops a message posted on the exact second of an explicit
/// `from_date`/`to_date` (the README's own example is that shape), and the
/// retained client-side guard never sees it to recover it. Cost is at most two
/// extra seconds of index — unmeasurable against a sub-second search.
fn window_bounds(from: DateTime<Utc>, to: Option<DateTime<Utc>>) -> (i32, i32) {
    let clamp = |ts: i64| ts.clamp(0, i32::MAX as i64) as i32;
    (
        clamp(from.timestamp().saturating_sub(1)),
        to.map_or(0, |t| clamp(t.timestamp().saturating_add(1))),
    )
}

/// Write a time window onto a `SearchGlobal` request.
///
/// Split out of `RawGlobalSearchPager::new` so the assignment itself is
/// testable: `new` needs a live `Client`, which no unit test can build, and a
/// transposition here (`min_date` ← the upper bound) would compile, pass, and
/// silently reinstate the unbounded global walk.
fn apply_window(
    request: &mut tl::functions::messages::SearchGlobal,
    from: DateTime<Utc>,
    to: Option<DateTime<Utc>>,
) {
    let (min_date, max_date) = window_bounds(from, to);
    request.min_date = min_date;
    request.max_date = max_date;
}

/// Raw `messages.GetHistory` pager (get_recent_messages path).
pub(super) struct RawHistoryPager {
    client: Client,
    request: tl::functions::messages::GetHistory,
    buffer: VecDeque<(tl::enums::Message, Arc<EntityLookup>)>,
    last_chunk: bool,
    /// Messages in the page just fetched, taken exactly once by the caller so a
    /// round trip is counted once rather than once per yielded message.
    last_page_size: Option<usize>,
}

impl RawHistoryPager {
    pub(super) fn new(client: &Client, peer: PeerRef) -> Self {
        Self {
            client: client.clone(),
            request: history_request(peer.into(), 0),
            buffer: VecDeque::new(),
            last_chunk: false,
            last_page_size: None,
        }
    }

    pub(super) fn offset_id(mut self, offset: i32) -> Self {
        self.request.offset_id = offset;
        self
    }

    pub(super) fn take_last_page_size(&mut self) -> Option<usize> {
        self.last_page_size.take()
    }

    pub(super) async fn next(
        &mut self,
    ) -> Result<Option<(tl::enums::Message, Arc<EntityLookup>)>, InvocationError> {
        if let Some(item) = self.buffer.pop_front() {
            return Ok(Some(item));
        }
        if self.last_chunk {
            return Ok(None);
        }
        let page = unpack_page(self.client.invoke(&self.request).await?, self.request.limit);
        self.last_chunk = page.last_chunk;
        // Measured on the page, not the buffer: the field reports raw messages
        // walked, which only coincides with the buffer length while the buffer
        // is provably empty here and nothing filters on the way in.
        let page_size = page.messages.len();
        advance_history_offsets(&mut self.request, &page);
        fill_buffer(&mut self.buffer, page);
        self.last_page_size = Some(page_size);
        Ok(self.buffer.pop_front())
    }
}

/// Raw `messages.Search` pager (search_messages single-channel path).
pub(super) struct RawChannelSearchPager {
    client: Client,
    request: tl::functions::messages::Search,
    buffer: VecDeque<(tl::enums::Message, Arc<EntityLookup>)>,
    last_chunk: bool,
    /// Messages in the page just fetched, taken exactly once by the caller so a
    /// round trip is counted once rather than once per yielded message.
    last_page_size: Option<usize>,
}

impl RawChannelSearchPager {
    pub(super) fn new(client: &Client, peer: PeerRef) -> Self {
        Self {
            client: client.clone(),
            request: tl::functions::messages::Search {
                peer: peer.into(),
                q: String::new(),
                from_id: None,
                saved_peer_id: None,
                saved_reaction: None,
                top_msg_id: None,
                filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
                min_date: 0,
                max_date: 0,
                offset_id: 0,
                add_offset: 0,
                limit: PAGE_LIMIT,
                max_id: 0,
                min_id: 0,
                hash: 0,
            },
            buffer: VecDeque::new(),
            last_chunk: false,
            last_page_size: None,
        }
    }

    pub(super) fn query(mut self, query: &str) -> Self {
        self.request.q = query.to_string();
        self
    }

    pub(super) fn filter(mut self, filter: tl::enums::MessagesFilter) -> Self {
        self.request.filter = filter;
        self
    }

    pub(super) fn offset_id(mut self, offset: i32) -> Self {
        self.request.offset_id = offset;
        self
    }

    pub(super) fn take_last_page_size(&mut self) -> Option<usize> {
        self.last_page_size.take()
    }

    pub(super) async fn next(
        &mut self,
    ) -> Result<Option<(tl::enums::Message, Arc<EntityLookup>)>, InvocationError> {
        if let Some(item) = self.buffer.pop_front() {
            return Ok(Some(item));
        }
        if self.last_chunk {
            return Ok(None);
        }
        let page = unpack_page(self.client.invoke(&self.request).await?, self.request.limit);
        self.last_chunk = page.last_chunk;
        // Measured on the page, not the buffer — see `RawHistoryPager::next`.
        let page_size = page.messages.len();
        advance_search_offsets(&mut self.request, &page);
        fill_buffer(&mut self.buffer, page);
        self.last_page_size = Some(page_size);
        Ok(self.buffer.pop_front())
    }
}

/// Raw `messages.SearchGlobal` pager (search_messages all-channels path).
///
/// Yields each message with its envelope entities and its own chat as a
/// high-level `Peer` (built from the same envelope), which the ops layer
/// needs for identity and link building.
pub(super) struct RawGlobalSearchPager {
    client: Client,
    request: tl::functions::messages::SearchGlobal,
    buffer: VecDeque<(
        tl::enums::Message,
        Arc<EntityLookup>,
        Option<grammers_client::peer::Peer>,
    )>,
    last_chunk: bool,
    /// Messages in the page just fetched, taken exactly once by the caller so a
    /// round trip is counted once rather than once per yielded message.
    last_page_size: Option<usize>,
}

impl RawGlobalSearchPager {
    /// The time window is a constructor argument, not an optional builder step:
    /// without server-side bounds the pager walks the entire global index
    /// backwards discarding out-of-window results — measured at 44.86 s for a
    /// rare media filter over a 24 h window. An omitted builder call would
    /// reinstate that silently, so the unbounded request is not constructible.
    pub(super) fn new(client: &Client, from: DateTime<Utc>, to: Option<DateTime<Utc>>) -> Self {
        let mut request = tl::functions::messages::SearchGlobal {
            folder_id: None,
            q: String::new(),
            filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
            // Overwritten by `apply_window` below; the literal cannot call it.
            min_date: 0,
            max_date: 0,
            offset_rate: 0,
            offset_peer: tl::enums::InputPeer::Empty,
            offset_id: 0,
            limit: PAGE_LIMIT,
            broadcasts_only: false,
            groups_only: false,
            users_only: false,
            community: None,
        };
        apply_window(&mut request, from, to);
        Self {
            client: client.clone(),
            request,
            buffer: VecDeque::new(),
            last_chunk: false,
            last_page_size: None,
        }
    }

    pub(super) fn query(mut self, query: &str) -> Self {
        self.request.q = query.to_string();
        self
    }

    pub(super) fn filter(mut self, filter: tl::enums::MessagesFilter) -> Self {
        self.request.filter = filter;
        self
    }

    pub(super) fn take_last_page_size(&mut self) -> Option<usize> {
        self.last_page_size.take()
    }

    pub(super) async fn next(
        &mut self,
    ) -> Result<
        Option<(
            tl::enums::Message,
            Arc<EntityLookup>,
            Option<grammers_client::peer::Peer>,
        )>,
        InvocationError,
    > {
        if let Some(item) = self.buffer.pop_front() {
            return Ok(Some(item));
        }
        if self.last_chunk {
            return Ok(None);
        }
        // Order matters: advance offsets while `page` is whole, then
        // destructure — the message vec is consumed by the buffer fill while
        // chats/users are still needed for per-message chat peers.
        let page = unpack_page(self.client.invoke(&self.request).await?, self.request.limit);
        self.last_chunk = page.last_chunk;
        // Measured on the page, not the buffer — see `RawHistoryPager::next`.
        let page_size = page.messages.len();
        if let Some(last) = page.messages.last() {
            self.request.offset_rate = page.next_rate.unwrap_or(0);
            self.request.offset_id = last.id();
            self.request.offset_peer = input_peer_for_message(last, &page);
        }
        let RawPage {
            messages,
            chats,
            users,
            ..
        } = page;
        let entities = Arc::new(EntityLookup::from_envelope(&chats, &users));
        for message in messages {
            let chat = chat_peer_for_message(&self.client, &message, &chats, &users);
            self.buffer
                .push_back((message, Arc::clone(&entities), chat));
        }
        self.last_page_size = Some(page_size);
        Ok(self.buffer.pop_front())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::raw_tl_channel;
    use crate::test_helpers::{raw_tl_message, raw_tl_messages_slice};
    use chrono::DateTime;
    use grammers_client::tl;

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
    fn history_offsets_advance_from_last_message() {
        let mut request = history_request(tl::enums::InputPeer::Empty, 0);
        let page = unpack_page(
            raw_tl_messages_slice(
                vec![
                    raw_tl_message(500, 1_700_000_500, 11),
                    raw_tl_message(499, 1_700_000_400, 11),
                ],
                None,
            ),
            100,
        );
        advance_history_offsets(&mut request, &page);
        assert_eq!(request.offset_id, 499);
        assert_eq!(request.offset_date, 1_700_000_400);
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
    fn search_offsets_advance_from_last_message() {
        let mut request = tl::functions::messages::Search {
            peer: tl::enums::InputPeer::Empty,
            q: String::new(),
            from_id: None,
            saved_peer_id: None,
            saved_reaction: None,
            top_msg_id: None,
            filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
            min_date: 0,
            max_date: 0,
            offset_id: 0,
            add_offset: 0,
            limit: PAGE_LIMIT,
            max_id: 0,
            min_id: 0,
            hash: 0,
        };
        let page = unpack_page(
            raw_tl_messages_slice(
                vec![
                    raw_tl_message(500, 1_700_000_500, 11),
                    raw_tl_message(499, 1_700_000_400, 11),
                ],
                None,
            ),
            100,
        );
        advance_search_offsets(&mut request, &page);
        assert_eq!(request.offset_id, 499);
        assert_eq!(request.max_date, 1_700_000_400);
    }

    #[test]
    fn window_bounds_maps_both_ends_widened_by_a_second() {
        let from = DateTime::from_timestamp(1_700_000_000, 0).expect("valid ts");
        let to = DateTime::from_timestamp(1_700_086_400, 0).expect("valid ts");
        let (min_date, max_date) = window_bounds(from, Some(to));
        // ±1 makes the server window a superset under both inclusive and
        // exclusive semantics; the client-side checks still filter exactly.
        assert_eq!(min_date, 1_699_999_999);
        assert_eq!(max_date, 1_700_086_401);
    }

    #[test]
    fn window_bounds_open_upper_end_is_unbounded_sentinel() {
        let from = DateTime::from_timestamp(1_700_000_000, 0).expect("valid ts");
        let (min_date, max_date) = window_bounds(from, None);
        assert_eq!(min_date, 1_699_999_999);
        // 0 is the protocol's "no upper bound", not "the epoch" — and the
        // widening must not turn it into 1.
        assert_eq!(max_date, 0);
    }

    #[test]
    fn window_bounds_clamps_pre_epoch_lower_end_to_unbounded() {
        let from = DateTime::from_timestamp(-86_400, 0).expect("valid ts");
        let (min_date, max_date) = window_bounds(from, None);
        // A degraded bound costs latency; a rejected search costs the caller
        // their result. Degrade.
        assert_eq!(min_date, 0);
        assert_eq!(max_date, 0);
    }

    #[test]
    fn window_bounds_clamps_beyond_i32_range() {
        // Past 2038: saturates instead of wrapping into a negative i32, which
        // would silently widen the window to everything.
        let from = DateTime::from_timestamp(i32::MAX as i64 + 1_000, 0).expect("valid ts");
        let (min_date, _) = window_bounds(from, None);
        assert_eq!(min_date, i32::MAX);
    }

    #[test]
    fn window_bounds_saturates_the_upper_end_at_i32_max() {
        // The +1 widening is the edge here: an upper bound already at the i32
        // ceiling must clamp, not wrap to a negative "max_date" — which the
        // server would read as a nonsense window rather than an open one.
        let from = DateTime::from_timestamp(1_700_000_000, 0).expect("valid ts");
        let at_ceiling = DateTime::from_timestamp(i32::MAX as i64, 0).expect("valid ts");
        let (_, max_date) = window_bounds(from, Some(at_ceiling));
        assert_eq!(max_date, i32::MAX);

        let beyond = DateTime::from_timestamp(i32::MAX as i64 + 1_000, 0).expect("valid ts");
        let (_, max_date) = window_bounds(from, Some(beyond));
        assert_eq!(max_date, i32::MAX);
    }

    #[test]
    fn apply_window_lands_both_bounds_on_the_tl_request() {
        // The seam the pager's server-side bounding actually depends on: a
        // transposition here compiles and passes every other test while
        // reinstating the unbounded global walk.
        let mut request = tl::functions::messages::SearchGlobal {
            folder_id: None,
            q: String::new(),
            filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
            min_date: 0,
            max_date: 0,
            offset_rate: 0,
            offset_peer: tl::enums::InputPeer::Empty,
            offset_id: 0,
            limit: PAGE_LIMIT,
            broadcasts_only: false,
            groups_only: false,
            users_only: false,
            community: None,
        };

        let from = DateTime::from_timestamp(1_700_000_000, 0).expect("valid ts");
        apply_window(&mut request, from, None);
        assert_eq!(request.min_date, 1_699_999_999, "lower bound is min_date");
        assert_eq!(
            request.max_date, 0,
            "open-ended window leaves max_date at 0"
        );

        let to = DateTime::from_timestamp(1_700_086_400, 0).expect("valid ts");
        apply_window(&mut request, from, Some(to));
        assert_eq!(request.min_date, 1_699_999_999, "lower bound is min_date");
        assert_eq!(request.max_date, 1_700_086_401, "upper bound is max_date");
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
}
