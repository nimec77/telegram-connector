use super::*;

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

#[test]
fn poisoned_buffer_does_not_replay_interrupted_state() {
    let buffer = std::sync::Arc::new(ResponseBuffer::new(5, usize::MAX));
    buffer.push(entry("old"));
    poison(&buffer);
    assert!(matches!(
        buffer.try_snapshot(None),
        Err(StateUnavailable::ResponseBuffer)
    ));
    assert_eq!(buffer.try_len(), Err(StateUnavailable::ResponseBuffer));
    assert_eq!(
        buffer.try_push(entry("new")),
        Err(StateUnavailable::ResponseBuffer)
    );
    buffer.push(entry("new"));
    assert!(
        buffer.last(None).is_empty(),
        "must not replay interrupted state"
    );
}

#[tokio::test]
async fn poisoned_replay_is_a_tool_error() {
    use crate::mcp::server::McpServer;
    use crate::mcp::tools::GetLastResponsesRequest;
    use crate::rate_limiter::MockRateLimiterTrait;
    use crate::telegram::MockTelegramClientTrait;
    use rmcp::handler::server::common::RequestId;
    use rmcp::handler::server::wrapper::Parameters;
    use rmcp::model::NumberOrString;
    let server = McpServer::new(
        std::sync::Arc::new(MockTelegramClientTrait::new()),
        std::sync::Arc::new(MockRateLimiterTrait::new()),
    );
    let buffer = server.response_buffer();
    buffer.push(entry("1"));
    assert!(
        std::thread::spawn(move || {
            let _guard = buffer.entries.lock().expect("healthy replay");
            panic!("intentional replay poison");
        })
        .join()
        .is_err()
    );
    let result = server
        .get_last_responses(
            Parameters(GetLastResponsesRequest {
                recovery_keys: None,
                n: None,
                include_binary: None,
            }),
            RequestId(NumberOrString::Number(1)),
        )
        .await;
    assert!(
        result.is_err(),
        "unavailable replay must not succeed as empty history"
    );
}

pub(crate) fn poison(buffer: &std::sync::Arc<ResponseBuffer>) {
    let poisoner = std::sync::Arc::clone(buffer);
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.entries.lock().expect("healthy replay");
            panic!("intentional replay poison");
        })
        .join()
        .is_err()
    );
}
