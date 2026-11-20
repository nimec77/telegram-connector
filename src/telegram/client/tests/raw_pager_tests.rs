//! Tests for the raw-TL pagers: offset advancement and search-window math.

use super::*;
use crate::UtcTimestamp;
use crate::test_helpers::{raw_tl_message, raw_tl_messages_slice};

#[test]
fn history_offsets_advance_from_last_message() {
    let mut request = history_request(tl::enums::InputPeer::Empty, 0);
    let page = unpack_page(raw_tl_messages_slice(
        vec![
            raw_tl_message(500, 1_700_000_500, 11),
            raw_tl_message(499, 1_700_000_400, 11),
        ],
        None,
    ));
    advance_history_offsets(&mut request, &page);
    assert_eq!(request.offset_id, 499);
    assert_eq!(request.offset_date, 1_700_000_400);
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
    let page = unpack_page(raw_tl_messages_slice(
        vec![
            raw_tl_message(500, 1_700_000_500, 11),
            raw_tl_message(499, 1_700_000_400, 11),
        ],
        None,
    ));
    advance_search_offsets(&mut request, &page);
    assert_eq!(request.offset_id, 499);
    assert_eq!(request.max_date, 1_700_000_400);
}

#[test]
fn window_bounds_maps_both_ends_widened_by_a_second() {
    let from = UtcTimestamp::from_unix_seconds(1_700_000_000).expect("valid ts");
    let to = UtcTimestamp::from_unix_seconds(1_700_086_400).expect("valid ts");
    let (min_date, max_date) = window_bounds(from, Some(to));
    // ±1 makes the server window a superset under both inclusive and
    // exclusive semantics; the client-side checks still filter exactly.
    assert_eq!(min_date, 1_699_999_999);
    assert_eq!(max_date, 1_700_086_401);
}

#[test]
fn window_bounds_open_upper_end_is_unbounded_sentinel() {
    let from = UtcTimestamp::from_unix_seconds(1_700_000_000).expect("valid ts");
    let (min_date, max_date) = window_bounds(from, None);
    assert_eq!(min_date, 1_699_999_999);
    // 0 is the protocol's "no upper bound", not "the epoch" — and the
    // widening must not turn it into 1.
    assert_eq!(max_date, 0);
}

#[test]
fn window_bounds_clamps_pre_epoch_lower_end_to_unbounded() {
    let from = UtcTimestamp::from_unix_seconds(-86_400).expect("valid ts");
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
    let from = UtcTimestamp::from_unix_seconds(i32::MAX as i64 + 1_000).expect("valid ts");
    let (min_date, _) = window_bounds(from, None);
    assert_eq!(min_date, i32::MAX);
}

#[test]
fn window_bounds_saturates_the_upper_end_at_i32_max() {
    // The +1 widening is the edge here: an upper bound already at the i32
    // ceiling must clamp, not wrap to a negative "max_date" — which the
    // server would read as a nonsense window rather than an open one.
    let from = UtcTimestamp::from_unix_seconds(1_700_000_000).expect("valid ts");
    let at_ceiling = UtcTimestamp::from_unix_seconds(i32::MAX as i64).expect("valid ts");
    let (_, max_date) = window_bounds(from, Some(at_ceiling));
    assert_eq!(max_date, i32::MAX);

    let beyond = UtcTimestamp::from_unix_seconds(i32::MAX as i64 + 1_000).expect("valid ts");
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

    let from = UtcTimestamp::from_unix_seconds(1_700_000_000).expect("valid ts");
    apply_window(&mut request, from, None);
    assert_eq!(request.min_date, 1_699_999_999, "lower bound is min_date");
    assert_eq!(
        request.max_date, 0,
        "open-ended window leaves max_date at 0"
    );

    let to = UtcTimestamp::from_unix_seconds(1_700_086_400).expect("valid ts");
    apply_window(&mut request, from, Some(to));
    assert_eq!(request.min_date, 1_699_999_999, "lower bound is min_date");
    assert_eq!(request.max_date, 1_700_086_401, "upper bound is max_date");
}

#[test]
fn identical_message_offsets_keep_each_history_request_channel_scoped() {
    for channel_id in [11, 22] {
        let peer = tl::enums::InputPeer::Channel(tl::types::InputPeerChannel {
            channel_id,
            access_hash: channel_id * 10,
        });
        let mut request = history_request(peer, 600);
        assert_eq!(
            request.offset_id, 600,
            "before cursor remains exclusive RPC offset"
        );
        let page = unpack_page(raw_tl_messages_slice(
            vec![raw_tl_message(500, 1_700_000_000, channel_id)],
            None,
        ));
        advance_history_offsets(&mut request, &page);
        assert_eq!(request.offset_id, 500);
        match request.peer {
            tl::enums::InputPeer::Channel(peer) => {
                assert_eq!(peer.channel_id, channel_id);
                assert_eq!(peer.access_hash, channel_id * 10);
            }
            other => panic!("cursor lost channel identity: {other:?}"),
        }
    }
}

use crate::telegram::albums::AlbumMode;
use crate::telegram::client::walk::{BelowCutoff, Fetched, Flow, MessageWalk, WalkConfig};
use crate::telegram::types::PeerNamespace;
use crate::test_helpers::{raw_tl_channel, raw_tl_message_grouped, raw_tl_user};
use std::sync::Mutex;

struct ScriptedSearch {
    pages: Mutex<VecDeque<tl::enums::messages::Messages>>,
    requests: Mutex<Vec<tl::functions::messages::SearchGlobal>>,
}

impl GlobalSearchFetch for ScriptedSearch {
    async fn fetch(
        &self,
        request: &tl::functions::messages::SearchGlobal,
    ) -> Result<tl::enums::messages::Messages, InvocationError> {
        self.requests
            .lock()
            .expect("request script")
            .push(request.clone());
        Ok(self
            .pages
            .lock()
            .expect("page script")
            .pop_front()
            .expect("unexpected fetch"))
    }
}

fn scripted_pager(
    pages: Vec<tl::enums::messages::Messages>,
) -> RawGlobalSearchPager<ScriptedSearch> {
    let grammers_mtsender::SenderPool { handle, .. } = grammers_mtsender::SenderPool::new(
        Arc::new(grammers_session::storages::MemorySession::default()),
        1,
    );
    RawGlobalSearchPager::with_fetch(
        &Client::new(handle),
        UtcTimestamp::from_unix_seconds(1_000).expect("cutoff"),
        Some(UtcTimestamp::from_unix_seconds(2_000).expect("upper bound")),
        ScriptedSearch {
            pages: Mutex::new(pages.into()),
            requests: Mutex::new(vec![]),
        },
    )
}

fn global_message(id: i32, date: i32, namespace: PeerNamespace) -> tl::enums::Message {
    let mut raw = raw_tl_message_grouped(id, date, 11, 1);
    if let tl::enums::Message::Message(message) = &mut raw {
        message.grouped_id = None;
        message.peer_id = match namespace {
            PeerNamespace::User => tl::enums::Peer::User(tl::types::PeerUser { user_id: 11 }),
            PeerNamespace::Chat => tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: 11 }),
            PeerNamespace::Channel => {
                tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 11 })
            }
        };
    }
    raw
}

fn global_slice(
    messages: Vec<tl::enums::Message>,
    next_rate: Option<i32>,
) -> tl::enums::messages::Messages {
    let mut response = raw_tl_messages_slice(messages, next_rate);
    if let tl::enums::messages::Messages::Slice(page) = &mut response {
        page.chats = vec![
            tl::enums::Chat::Channel(raw_tl_channel(11, "Channel", None)),
            tl::enums::Chat::Forbidden(tl::types::ChatForbidden {
                id: 11,
                title: "Group".into(),
            }),
        ];
        page.users = vec![tl::enums::User::User(raw_tl_user(
            11,
            Some("Private"),
            None,
            None,
        ))];
    }
    response
}

fn terminal(messages: Vec<tl::enums::Message>) -> tl::enums::messages::Messages {
    let tl::enums::messages::Messages::Slice(page) = global_slice(messages, None) else {
        unreachable!()
    };
    tl::enums::messages::Messages::Messages(tl::types::messages::Messages {
        messages: page.messages,
        topics: vec![],
        chats: page.chats,
        users: page.users,
    })
}

fn global_walk(limit: usize, deadline_secs: u64) -> MessageWalk<'static> {
    MessageWalk::new(
        WalkConfig {
            cutoff_time: UtcTimestamp::from_unix_seconds(1_000).expect("cutoff"),
            to_date: Some(UtcTimestamp::from_unix_seconds(2_000).expect("upper bound")),
            after_bound: None,
            media_filter: None,
            below_cutoff: BelowCutoff::Skip,
        },
        AlbumMode::Separate,
        limit,
        deadline_secs,
    )
}

async fn step_global(
    pager: &mut RawGlobalSearchPager<ScriptedSearch>,
    walk: &mut MessageWalk<'_>,
) -> Flow {
    let next = pager.next().await.expect("scripted page");
    let page_size = pager.take_last_page_size();
    match next {
        Some((raw, entities, peer)) => walk.step(
            Some(Fetched {
                raw,
                entities,
                peer: peer.as_ref(),
            }),
            page_size,
        ),
        None => walk.step(None, page_size),
    }
}

#[tokio::test]
async fn global_next_continues_a_full_mixed_peer_slice_after_low_first_id() {
    let messages = (0..100)
        .map(|index| {
            global_message(
                if index == 0 { 9 } else { 2_000 - index },
                999,
                [
                    PeerNamespace::User,
                    PeerNamespace::Chat,
                    PeerNamespace::Channel,
                ][index as usize % 3],
            )
        })
        .collect();
    let mut pager = scripted_pager(vec![
        global_slice(messages, Some(7)),
        terminal(vec![global_message(1_800, 1_500, PeerNamespace::Channel)]),
    ]);
    let mut walk = global_walk(10, 0);
    while step_global(&mut pager, &mut walk).await == Flow::Continue {}
    let (page, budget) = walk.into_parts();
    assert_eq!(
        page.into_messages()
            .iter()
            .map(|m| m.id.get())
            .collect::<Vec<_>>(),
        [1_800],
        "later qualifying result survives a low ID in another peer"
    );
    assert_eq!(budget.pages_fetched(), 2);
    assert_eq!(budget.messages_scanned(), 101);
    let requests = pager.fetch.requests.lock().expect("requests");
    assert_eq!(requests.len(), 2);
    for request in requests.iter() {
        assert_eq!((request.min_date, request.max_date), (999, 2_001));
        assert!(!request.users_only && !request.groups_only && !request.broadcasts_only);
    }
    assert_eq!(
        requests[1].offset_id, 1_901,
        "offset uses the actual final raw message"
    );
    assert_eq!(requests[1].offset_rate, 7);
    assert!(
        matches!(requests[1].offset_peer, tl::enums::InputPeer::User(ref p) if p.user_id == 11)
    );
}

#[tokio::test]
async fn global_next_short_slices_without_next_rate_advance_each_peer_namespace() {
    for namespace in [
        PeerNamespace::User,
        PeerNamespace::Chat,
        PeerNamespace::Channel,
    ] {
        let mut pager = scripted_pager(vec![
            global_slice(vec![global_message(500, 1_500, namespace)], None),
            global_slice(vec![], None),
        ]);
        let (raw, _, peer) = pager.next().await.expect("first fetch").expect("result");
        assert_eq!(raw.id(), 500);
        assert!(peer.is_some());
        assert_eq!(pager.take_last_page_size(), Some(1));
        assert_eq!(pager.take_last_page_size(), None);
        assert!(pager.next().await.expect("empty fetch").is_none());
        assert_eq!(pager.take_last_page_size(), Some(0));
        assert!(pager.next().await.expect("exhausted").is_none());
        assert_eq!(pager.take_last_page_size(), None);
        let requests = pager.fetch.requests.lock().expect("requests");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].offset_id, 500);
        assert_eq!(requests[1].offset_rate, 0);
        match (&requests[1].offset_peer, namespace) {
            (tl::enums::InputPeer::User(peer), PeerNamespace::User) => assert_eq!(peer.user_id, 11),
            (tl::enums::InputPeer::Chat(peer), PeerNamespace::Chat) => assert_eq!(peer.chat_id, 11),
            (tl::enums::InputPeer::Channel(peer), PeerNamespace::Channel) => {
                assert_eq!(peer.channel_id, 11)
            }
            other => panic!("offset lost namespace: {other:?}"),
        }
    }
}

#[tokio::test]
async fn global_next_retains_private_user_group_and_channel_results_on_terminal_pages() {
    let mut pager = scripted_pager(vec![terminal(vec![
        global_message(9, 1_500, PeerNamespace::User),
        global_message(9, 1_500, PeerNamespace::Chat),
        global_message(9, 1_500, PeerNamespace::Channel),
    ])]);
    let mut walk = global_walk(10, 0);
    while step_global(&mut pager, &mut walk).await == Flow::Continue {}
    let (page, budget) = walk.into_parts();
    let messages = page.into_messages();
    assert_eq!(
        messages
            .iter()
            .map(|m| m.peer_namespace)
            .collect::<Vec<_>>(),
        [
            Some(PeerNamespace::User),
            Some(PeerNamespace::Chat),
            Some(PeerNamespace::Channel)
        ]
    );
    assert_eq!(messages.len(), 3);
    assert_eq!(budget.pages_fetched(), 1);
    assert_eq!(budget.messages_scanned(), 3);
    assert_eq!(pager.fetch.requests.lock().expect("requests").len(), 1);
}

#[tokio::test]
async fn global_next_exact_limit_probes_until_exhaustion_or_an_extra_qualifying_result() {
    for extra in [false, true] {
        let mut pager = scripted_pager(vec![
            global_slice(vec![global_message(9, 1_000, PeerNamespace::User)], Some(7)),
            global_slice(
                vec![
                    global_message(500, 2_001, PeerNamespace::Chat),
                    global_message(499, 999, PeerNamespace::Channel),
                ],
                None,
            ),
            terminal(if extra {
                vec![global_message(498, 2_000, PeerNamespace::Channel)]
            } else {
                vec![]
            }),
        ]);
        let mut walk = global_walk(1, 0);
        while step_global(&mut pager, &mut walk).await == Flow::Continue {}
        let (page, budget) = walk.into_parts();
        assert_eq!(
            page.has_more(),
            extra,
            "only a refused qualifying result sets has_more"
        );
        assert_eq!(
            page.into_messages()
                .iter()
                .map(|m| m.id.get())
                .collect::<Vec<_>>(),
            [9]
        );
        assert_eq!(
            budget.pages_fetched(),
            3,
            "the terminal empty round trip counts"
        );
        assert_eq!(budget.messages_scanned(), if extra { 4 } else { 3 });
    }
}

#[tokio::test(start_paused = true)]
async fn global_next_uses_one_walk_deadline_and_preserves_partial_results() {
    let mut pager = scripted_pager(vec![
        global_slice(
            vec![global_message(500, 1_500, PeerNamespace::User)],
            Some(7),
        ),
        global_slice(vec![global_message(499, 999, PeerNamespace::Chat)], None),
    ]);
    let mut walk = global_walk(1, 2);
    assert!(!walk.expired());
    assert_eq!(step_global(&mut pager, &mut walk).await, Flow::Continue);
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    assert!(!walk.expired());
    assert_eq!(step_global(&mut pager, &mut walk).await, Flow::Continue);
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    assert!(walk.expired(), "paging cannot restart the total deadline");
    let (page, budget) = walk.into_parts();
    assert!(
        !page.has_more(),
        "deadline alone cannot prove another qualifying result"
    );
    assert_eq!(page.into_messages().len(), 1);
    assert!(budget.timed_out());
    assert_eq!(budget.pages_fetched(), 2);
    assert_eq!(budget.messages_scanned(), 2);
    assert_eq!(pager.fetch.requests.lock().expect("requests").len(), 2);
}
