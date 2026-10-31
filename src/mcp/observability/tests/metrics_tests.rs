use super::*;

#[test]
fn new_metrics_start_at_zero() {
    let metrics = SessionMetrics::new();
    assert_eq!(metrics.requests_received(), 0);
    assert_eq!(metrics.responses_written(), 0);
    assert_eq!(metrics.last_write_age_secs(), None);
    assert_eq!(metrics.try_last_write_age_secs(), Ok(None));
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

pub(crate) fn poison(metrics: &std::sync::Arc<SessionMetrics>, last_write: bool) {
    let metrics = std::sync::Arc::clone(metrics);
    assert!(
        std::thread::spawn(move || {
            if last_write {
                let _guard = metrics.last_write.lock().expect("healthy last write");
                panic!("intentional last-write poison");
            } else {
                let mut guard = metrics.in_flight.lock().expect("healthy correlation");
                guard.clear();
                panic!("intentional correlation poison");
            }
        })
        .join()
        .is_err()
    );
}

#[test]
fn poisoned_metrics_do_not_reuse_interrupted_state() {
    for last_write in [false, true] {
        let metrics = std::sync::Arc::new(SessionMetrics::new());
        metrics.record_request("1", "search_messages");
        metrics.record_response_written("other");
        poison(&metrics, last_write);
        if last_write {
            assert_eq!(
                metrics.last_write_age_secs(),
                None,
                "unavailable is not a timestamp"
            );
        } else {
            assert!(
                metrics.abandoned_requests().is_empty(),
                "must not return interrupted correlation"
            );
        }
        if last_write {
            assert_eq!(
                metrics.try_last_write_age_secs(),
                Err(StateUnavailable::LastWrite)
            );
        } else {
            assert_eq!(
                metrics.try_abandoned_requests(),
                Err(StateUnavailable::InFlight)
            );
        }
        metrics.record_request("2", "get_last_responses");
        let request = metrics.record_response_written("2");
        assert_eq!(metrics.requests_received(), 2);
        assert_eq!(metrics.responses_written(), 2);
        if last_write {
            assert_eq!(
                request.expect("healthy independent correlation").tool_name,
                "get_last_responses"
            );
        } else {
            assert!(request.is_none());
            assert_eq!(metrics.last_write_age_secs(), Some(0));
        }
        metrics.log_summary("poison test");
    }
}

#[tokio::test]
async fn poisoned_last_write_status_retains_counters_and_reports_unavailable() {
    use crate::mcp::server::McpServer;
    use crate::rate_limiter::MockRateLimiterTrait;
    use crate::telegram::{AccountStatus, MockTelegramClientTrait};
    use rmcp::handler::server::common::RequestId;
    use rmcp::model::NumberOrString;
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_cached_account_status()
        .returning(AccountStatus::default);
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_try_available_tokens().return_const(Ok(2.0));
    limiter.expect_capacity().return_const(5.0);
    limiter.expect_refill_rate().return_const(1.0);
    let server = McpServer::new(std::sync::Arc::new(client), std::sync::Arc::new(limiter));
    let metrics = server.metrics();
    metrics.record_request("1", "search_messages");
    metrics.record_response_written("1");
    poison(&metrics, true);
    let response = server
        .check_mcp_status(RequestId(NumberOrString::Number(1)))
        .await
        .expect("status remains usable");
    let json: serde_json::Value = serde_json::from_str(&response).expect("JSON");
    assert!(json["last_response_write_age_secs"].is_null());
    assert!(json["last_response_write_unavailable"].as_str().is_some());
    assert_eq!(json["requests_received"], 1);
    assert_eq!(json["responses_written"], 1);
    assert_eq!(json["rate_limiter"]["tokens"], 2.0);
}
