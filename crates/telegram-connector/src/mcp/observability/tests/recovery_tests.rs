use super::*;
use std::time::SystemTime;

fn response(payload: &str) -> BufferedResponse {
    BufferedResponse {
        request_id: "7".into(),
        tool_name: "search_messages".into(),
        written_at: SystemTime::now(),
        size_bytes: payload.len(),
        payload: payload.into(),
    }
}
fn selected(buffer: &ResponseBuffer, key: &str) -> Result<BufferedResponse, Error> {
    buffer
        .select_keys(&[key.into()])
        .expect("selection")
        .responses
        .remove(0)
        .1
}

#[tokio::test(start_paused = true)]
async fn task8_keyed_recovery_expires_exactly_ten_minutes_after_completion() {
    let buffer = Arc::new(ResponseBuffer::new(2, 100));
    let reservation = buffer.reserve_key("key").expect("reserve");
    tokio::time::advance(Duration::from_secs(1_000)).await;
    assert!(matches!(
        selected(&buffer, "key"),
        Err(Error::ResponsePending)
    ));
    reservation.complete(response("{}")).expect("complete");
    tokio::time::advance(Duration::from_secs(599)).await;
    assert!(selected(&buffer, "key").is_ok());
    assert!(matches!(
        buffer.reserve_key("key"),
        Err(Error::DuplicateRecoveryKey)
    ));
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(matches!(
        selected(&buffer, "key"),
        Err(Error::ResponseUnavailable)
    ));
    assert!(buffer.reserve_key("key").is_ok());
}

#[test]
fn task8_pending_reservations_cannot_be_evicted_and_cancellation_releases_capacity() {
    let buffer = Arc::new(ResponseBuffer::new(2, 100));
    let first = buffer.reserve_key("a").expect("first");
    let second = buffer.reserve_key("b").expect("second");
    assert!(matches!(
        buffer.reserve_key("c"),
        Err(Error::RecoveryCapacityReached)
    ));
    assert!(matches!(
        buffer.reserve_key("a"),
        Err(Error::DuplicateRecoveryKey)
    ));
    drop(first);
    assert!(matches!(
        selected(&buffer, "a"),
        Err(Error::ResponseUnavailable)
    ));
    let third = buffer
        .reserve_key("c")
        .expect("cancelled slot becomes evictable");
    assert!(matches!(
        selected(&buffer, "b"),
        Err(Error::ResponsePending)
    ));
    second.complete(response("{}")).expect("complete");
    third.complete(response("{}")).expect("complete");
    let _fourth = buffer.reserve_key("d").expect("evict oldest completed");
    assert!(matches!(
        selected(&buffer, "b"),
        Err(Error::ResponseUnavailable)
    ));
    assert!(selected(&buffer, "c").is_ok());
}

#[test]
fn task8_oversized_keyed_entries_remain_explicit_size_preserving_stubs() {
    let buffer = Arc::new(ResponseBuffer::new(2, 10));
    buffer
        .reserve_key("big")
        .expect("reserve")
        .complete(response(&"x".repeat(100)))
        .expect("complete");
    let entry = selected(&buffer, "big").expect("retained stub");
    assert_eq!(entry.size_bytes, 100);
    assert_eq!(entry.payload, OVERSIZED_PAYLOAD_STUB);
    assert!(matches!(
        buffer.reserve_key("big"),
        Err(Error::DuplicateRecoveryKey)
    ));
    let restarted = ResponseBuffer::new(2, 10);
    assert!(matches!(
        selected(&restarted, "big"),
        Err(Error::ResponseUnavailable)
    ));
    assert!(matches!(
        Arc::new(ResponseBuffer::new(0, 10)).reserve_key("key"),
        Err(Error::RecoveryCapacityReached)
    ));
}

#[test]
fn task8_key_selection_rejects_duplicates_and_excessive_result_fanout() {
    let buffer = ResponseBuffer::new(2, 100);
    assert!(buffer.select_keys(&[]).is_err());
    assert!(buffer.select_keys(&["a".into(), "a".into()]).is_err());
    assert!(
        buffer
            .select_keys(&["a".into(), "b".into(), "c".into()])
            .is_err()
    );
    assert!(buffer.select_keys(&["invalid\n".into()]).is_err());
    assert!(buffer.select_keys(&[" ".into()]).is_ok());
}
