use super::*;

#[test]
fn new_metrics_start_at_zero() {
    let metrics = SessionMetrics::new();
    assert_eq!(metrics.requests_received(), 0);
    assert_eq!(metrics.responses_written(), 0);
    assert_eq!(metrics.last_write_age_secs(), None);
    assert!(metrics.abandoned_requests().is_empty());
}

#[test]
fn record_request_increments_and_tracks_in_flight() {
    let metrics = SessionMetrics::new();
    metrics.record_request("1", "search_messages");
    metrics.record_request("2", "check_mcp_status");
    assert_eq!(metrics.requests_received(), 2);
    let mut abandoned = metrics.abandoned_requests();
    abandoned.sort();
    assert_eq!(
        abandoned,
        vec![
            ("1".to_string(), "search_messages".to_string()),
            ("2".to_string(), "check_mcp_status".to_string()),
        ]
    );
}

#[test]
fn record_response_written_clears_in_flight_and_returns_info() {
    let metrics = SessionMetrics::new();
    metrics.record_request("1", "search_messages");
    let in_flight = metrics.record_response_written("1");
    assert_eq!(metrics.responses_written(), 1);
    assert_eq!(
        in_flight.expect("in-flight entry").tool_name,
        "search_messages"
    );
    assert!(metrics.abandoned_requests().is_empty());
    assert_eq!(metrics.last_write_age_secs(), Some(0));
}

#[test]
fn record_response_written_unknown_id_still_counts() {
    let metrics = SessionMetrics::new();
    let in_flight = metrics.record_response_written("99");
    assert!(in_flight.is_none());
    assert_eq!(metrics.responses_written(), 1);
}

#[test]
fn session_started_at_is_rfc3339() {
    let metrics = SessionMetrics::new();
    let stamp = metrics.session_started_at_rfc3339();
    assert!(chrono::DateTime::parse_from_rfc3339(&stamp).is_ok());
}

#[test]
fn log_summary_does_not_panic() {
    let metrics = SessionMetrics::new();
    metrics.record_request("1", "search_messages");
    metrics.log_summary("test");
}
