use super::*;
use rmcp::RoleServer;
use rmcp::model::{
    JsonRpcMessage, JsonRpcNotification, JsonRpcResponse, JsonRpcVersion2_0, NumberOrString,
    ProgressNotificationParam, ProgressToken, RequestId, ServerNotification, ServerResult,
};
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::Transport;
use std::sync::Arc;
use std::time::Duration;

/// Channel-free fake: receive() pops from a queue, send() records JSON.
struct FakeTransport {
    incoming: VecDeque<RxJsonRpcMessage<RoleServer>>,
    sent: Arc<Mutex<Vec<String>>>,
    fail_sends: bool,
}

impl FakeTransport {
    fn new(incoming: Vec<RxJsonRpcMessage<RoleServer>>) -> Self {
        Self {
            incoming: incoming.into(),
            sent: Arc::new(Mutex::new(Vec::new())),
            fail_sends: false,
        }
    }
}

impl Transport<RoleServer> for FakeTransport {
    type Error = std::io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let sent = Arc::clone(&self.sent);
        let fail = self.fail_sends;
        async move {
            if fail {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "peer gone",
                ));
            }
            let json = serde_json::to_string(&item).map_err(std::io::Error::other)?;
            sent.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(json);
            Ok(())
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        self.incoming.pop_front()
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn call_tool_request(id: i64, tool: &str) -> RxJsonRpcMessage<RoleServer> {
    serde_json::from_value(serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": tool, "arguments": {} }
    }))
    .expect("valid call-tool request")
}

fn tool_response(id: i64) -> TxJsonRpcMessage<RoleServer> {
    JsonRpcMessage::Response(JsonRpcResponse {
        jsonrpc: JsonRpcVersion2_0,
        id: RequestId::Number(id),
        result: ServerResult::empty(()),
    })
}

fn instrumented(
    incoming: Vec<RxJsonRpcMessage<RoleServer>>,
) -> (
    InstrumentedTransport<FakeTransport>,
    Arc<SessionMetrics>,
    Arc<ResponseBuffer>,
) {
    let metrics = Arc::new(SessionMetrics::new());
    let buffer = Arc::new(ResponseBuffer::new(10, usize::MAX));
    let transport = InstrumentedTransport::new(
        FakeTransport::new(incoming),
        Arc::clone(&metrics),
        Arc::clone(&buffer),
        Duration::from_millis(500),
    );
    (transport, metrics, buffer)
}

#[tokio::test]
async fn receive_counts_requests_and_tracks_in_flight() {
    let (mut transport, metrics, _) = instrumented(vec![call_tool_request(1, "search_messages")]);
    let message = transport.receive().await;
    assert!(message.is_some());
    assert_eq!(metrics.requests_received(), 1);
    assert_eq!(
        metrics.abandoned_requests(),
        vec![("1".to_string(), "search_messages".to_string())]
    );
}

#[tokio::test]
async fn receive_eof_returns_none() {
    let (mut transport, metrics, _) = instrumented(vec![]);
    assert!(transport.receive().await.is_none());
    assert_eq!(metrics.requests_received(), 0);
}

#[tokio::test]
async fn send_response_updates_metrics_and_buffer() {
    let (mut transport, metrics, buffer) =
        instrumented(vec![call_tool_request(1, "search_messages")]);
    // Grab a clone of the sent-records handle before any sends happen.
    let sent_handle = Arc::clone(&transport.inner.sent);
    transport.receive().await;
    transport.send(tool_response(1)).await.expect("send ok");

    assert_eq!(metrics.responses_written(), 1);
    assert!(metrics.abandoned_requests().is_empty());
    assert_eq!(metrics.last_write_age_secs(), Some(0));

    let buffered = buffer.last(None);
    assert_eq!(buffered.len(), 1);
    assert_eq!(buffered[0].request_id, "1");
    assert_eq!(buffered[0].tool_name, "search_messages");
    assert!(buffered[0].payload.contains("jsonrpc"));
    assert_eq!(buffered[0].size_bytes, buffered[0].payload.len());

    // Verify the buffered recovery copy is byte-identical to what the inner transport wrote.
    let sent_entries = sent_handle.lock().unwrap_or_else(PoisonError::into_inner);
    assert_eq!(sent_entries.len(), 1);
    assert_eq!(sent_entries[0], buffered[0].payload);
}

#[tokio::test]
async fn send_skips_buffering_for_get_last_responses() {
    let (mut transport, metrics, buffer) =
        instrumented(vec![call_tool_request(2, "get_last_responses")]);
    transport.receive().await;
    transport.send(tool_response(2)).await.expect("send ok");
    assert_eq!(metrics.responses_written(), 1);
    assert!(buffer.is_empty());
}

#[tokio::test]
async fn send_error_propagates_and_skips_metrics() {
    let (mut transport, metrics, buffer) =
        instrumented(vec![call_tool_request(1, "search_messages")]);
    transport.receive().await;
    transport.inner.fail_sends = true;
    let result = transport.send(tool_response(1)).await;
    assert!(result.is_err());
    assert_eq!(metrics.responses_written(), 0);
    // Failed write leaves the request in-flight (it was never answered).
    assert_eq!(metrics.abandoned_requests().len(), 1);
    assert!(buffer.is_empty());
}

#[tokio::test]
async fn send_notification_passes_through_unaccounted() {
    // The early-return branch in send() (response_id == None) must pass through
    // without touching metrics or the buffer.
    let (mut transport, metrics, buffer) = instrumented(vec![]);
    let notification: TxJsonRpcMessage<RoleServer> =
        JsonRpcMessage::Notification(JsonRpcNotification {
            jsonrpc: JsonRpcVersion2_0,
            notification: ServerNotification::ProgressNotification(rmcp::model::Notification::new(
                ProgressNotificationParam::new(ProgressToken(NumberOrString::Number(1)), 50.0),
            )),
        });
    transport.send(notification).await.expect("send ok");
    assert_eq!(metrics.responses_written(), 0);
    assert!(buffer.is_empty());
    let sent_entries = transport
        .inner
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    assert_eq!(
        sent_entries.len(),
        1,
        "inner transport should have written the notification"
    );
}

#[tokio::test]
async fn send_slow_write_warns_with_zero_threshold() {
    // A zero threshold means every nonzero-elapsed write triggers the warn branch.
    // This test constructs InstrumentedTransport directly to use Duration::ZERO.
    let metrics = Arc::new(SessionMetrics::new());
    let buffer = Arc::new(ResponseBuffer::new(10, usize::MAX));
    let mut transport = InstrumentedTransport::new(
        FakeTransport::new(vec![call_tool_request(1, "search_messages")]),
        Arc::clone(&metrics),
        Arc::clone(&buffer),
        Duration::ZERO,
    );
    transport.receive().await;
    transport.send(tool_response(1)).await.expect("send ok");
    assert_eq!(metrics.responses_written(), 1);
}

#[test]
fn slow_write_predicate() {
    assert!(is_slow_write(
        Duration::from_millis(501),
        Duration::from_millis(500)
    ));
    assert!(!is_slow_write(
        Duration::from_millis(499),
        Duration::from_millis(500)
    ));
    // Threshold 0 = every (nonzero) write warns.
    assert!(is_slow_write(Duration::from_micros(1), Duration::ZERO));
}

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

fn entry(id: &str) -> BufferedResponse {
    BufferedResponse {
        request_id: id.to_string(),
        tool_name: "search_messages".to_string(),
        written_at: SystemTime::now(),
        size_bytes: 2,
        payload: "{}".to_string(),
    }
}

#[test]
fn buffer_returns_newest_first() {
    let buffer = ResponseBuffer::new(5, usize::MAX);
    buffer.push(entry("1"));
    buffer.push(entry("2"));
    let last = buffer.last(None);
    assert_eq!(last.len(), 2);
    assert_eq!(last[0].request_id, "2");
    assert_eq!(last[1].request_id, "1");
}

#[test]
fn buffer_evicts_oldest_at_capacity() {
    let buffer = ResponseBuffer::new(2, usize::MAX);
    buffer.push(entry("1"));
    buffer.push(entry("2"));
    buffer.push(entry("3"));
    let ids: Vec<String> = buffer
        .last(None)
        .into_iter()
        .map(|e| e.request_id)
        .collect();
    assert_eq!(ids, vec!["3".to_string(), "2".to_string()]);
    assert_eq!(buffer.len(), 2);
}

#[test]
fn buffer_capacity_zero_disables_buffering() {
    let buffer = ResponseBuffer::new(0, usize::MAX);
    buffer.push(entry("1"));
    assert!(buffer.last(None).is_empty());
    assert!(buffer.is_empty());
}

#[test]
fn buffer_last_caps_n_at_len() {
    let buffer = ResponseBuffer::new(5, usize::MAX);
    buffer.push(entry("1"));
    buffer.push(entry("2"));
    assert_eq!(buffer.last(Some(1)).len(), 1);
    assert_eq!(buffer.last(Some(1))[0].request_id, "2");
    assert_eq!(buffer.last(Some(10)).len(), 2);
}

#[test]
fn push_replaces_oversized_payload_with_stub() {
    let buffer = ResponseBuffer::new(5, 100);
    buffer.push(BufferedResponse {
        request_id: "1".to_string(),
        tool_name: "get_message_media".to_string(),
        written_at: SystemTime::now(),
        size_bytes: 200,
        payload: "x".repeat(200),
    });

    let entries = buffer.last(None);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].payload, OVERSIZED_PAYLOAD_STUB);
    // size_bytes still reports the real wire size.
    assert_eq!(entries[0].size_bytes, 200);
    // The stub must stay valid JSON so get_last_responses can embed it.
    assert!(serde_json::from_str::<serde_json::Value>(OVERSIZED_PAYLOAD_STUB).is_ok());
}

#[test]
fn push_keeps_payload_at_or_under_threshold() {
    let buffer = ResponseBuffer::new(5, 100);
    buffer.push(BufferedResponse {
        request_id: "1".to_string(),
        tool_name: "search_messages".to_string(),
        written_at: SystemTime::now(),
        size_bytes: 100,
        payload: "y".repeat(100),
    });

    let entries = buffer.last(None);
    assert_eq!(entries[0].payload, "y".repeat(100));
}
