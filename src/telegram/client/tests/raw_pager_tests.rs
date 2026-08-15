//! Tests for the raw-TL pagers: offset advancement and search-window math.

use super::*;
use crate::test_helpers::{raw_tl_message, raw_tl_messages_slice};
use chrono::DateTime;

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
