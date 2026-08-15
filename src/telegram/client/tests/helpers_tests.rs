//! Tests for client-wide shared helpers (cursor wire bounds; from Task 9 on,
//! also the empty-identifier guard).

use super::*;
use crate::telegram::types::MessageId;

fn mid(id: i64) -> MessageId {
    MessageId::new(id).expect("positive test id")
}

#[test]
fn cursor_wire_bounds_passes_in_range_ids() {
    let (before, after) = cursor_wire_bounds(Some(mid(10)), Some(mid(5))).expect("in range");
    assert_eq!(before, Some(10));
    assert_eq!(after, Some(5));
}

#[test]
fn cursor_wire_bounds_none_stays_none() {
    let (before, after) = cursor_wire_bounds(None, None).expect("ok");
    assert!(before.is_none() && after.is_none());
}

#[test]
fn cursor_wire_bounds_rejects_beyond_i32_naming_the_field() {
    let big = i64::from(i32::MAX) + 1;
    let err = cursor_wire_bounds(Some(mid(big)), None).unwrap_err();
    assert!(err.to_string().contains("before_id"), "got: {err}");
    let err = cursor_wire_bounds(None, Some(mid(big))).unwrap_err();
    assert!(err.to_string().contains("after_id"), "got: {err}");
}
