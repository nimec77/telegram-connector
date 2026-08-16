//! Tests for client-wide shared helpers (cursor wire bounds; from Task 9 on,
//! also the empty-identifier guard; from Task 7 of audit-stage-4 on, also
//! `assemble_search_result`).

use super::search_budget::SearchBudget;
use super::*;
use crate::telegram::types::MessageId;

fn mid(id: i64) -> MessageId {
    MessageId::new(id).expect("positive test id")
}

fn at_secs(secs: i64) -> chrono::DateTime<chrono::Utc> {
    use chrono::TimeZone;
    chrono::Utc
        .timestamp_opt(secs, 0)
        .single()
        .expect("valid timestamp")
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

#[test]
fn empty_channel_identifier_is_rejected() {
    let err = validate_channel_identifier("").expect_err("empty must be rejected");
    assert!(
        err.to_string()
            .contains("Channel identifier cannot be empty"),
        "got: {err}"
    );
}

#[test]
fn non_empty_channel_identifier_passes() {
    assert!(validate_channel_identifier("durov").is_ok());
}

#[test]
fn assembled_result_counts_distinct_channels_not_messages() {
    use crate::test_helpers::create_test_message;

    let budget = SearchBudget::new(0);
    let messages = vec![
        create_test_message(1, "текст", 100),
        create_test_message(2, "текст", 100),
        create_test_message(3, "текст", 200),
    ];
    let result = assemble_search_result(
        messages,
        &budget,
        false,
        "запрос".to_string(),
        at_secs(1_000),
        None,
        Some(2),
        42,
    );

    assert_eq!(result.returned, 3);
    assert_eq!(result.query_metadata.channels_in_results, 2);
    assert_eq!(result.query_metadata.channels_scanned, Some(2));
    assert_eq!(result.search_time_ms, 42);
}

#[test]
fn an_empty_result_reports_zero_channels_in_results() {
    // History's old hand-rolled `if messages.is_empty() { 0 } else { 1 }`
    // must stay equivalent to the unique-count for the single-channel case.
    let budget = SearchBudget::new(0);
    let result = assemble_search_result(
        Vec::new(),
        &budget,
        false,
        String::new(),
        at_secs(1_000),
        None,
        Some(1),
        7,
    );

    assert_eq!(result.returned, 0);
    assert_eq!(result.query_metadata.channels_in_results, 0);
}

#[test]
fn partial_is_paired_with_timed_out_not_with_has_more() {
    // A full page is not a timeout: `has_more` true must leave `partial` false.
    let budget = SearchBudget::new(0);
    let result = assemble_search_result(
        Vec::new(),
        &budget,
        true,
        String::new(),
        at_secs(1_000),
        None,
        None,
        1,
    );

    assert!(result.has_more);
    assert!(!result.query_metadata.partial);
    assert!(!result.query_metadata.timed_out);
}
