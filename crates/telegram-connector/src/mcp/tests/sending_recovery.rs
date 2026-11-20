use super::sending_protocol::offline_client::{OfflineClient, SendGate};
use super::sending_protocol::{call, handler, native_wire, policy, text};
use super::*;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn sending_recovery_completed_evicted_expired_and_restarted_entries_do_not_imply_unsent() {
    let client = Arc::new(OfflineClient::new(true));
    let mut wire = native_wire(handler(Arc::clone(&client), policy(), 1).await, 1);
    let original = call(&mut wire, 7, "send_message", text("first")).await;
    let recovered = call(
        &mut wire,
        8,
        "get_last_responses",
        json!({"recovery_keys":["first"]}),
    )
    .await;
    assert_eq!(
        recovered["result"]["structuredContent"]["data"]["responses"][0]["response"],
        original
    );
    let second = call(&mut wire, 7, "send_message", text("second")).await;
    assert_eq!(
        second["result"]["structuredContent"]["data"]["message_id"],
        43
    );
    let evicted = call(
        &mut wire,
        9,
        "get_last_responses",
        json!({"recovery_keys":["first"]}),
    )
    .await;
    assert_eq!(
        evicted["result"]["structuredContent"]["data"]["responses"][0]["error"]["code"],
        "response_unavailable"
    );
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(601)).await;
    let expired = call(
        &mut wire,
        10,
        "get_last_responses",
        json!({"recovery_keys":["second"]}),
    )
    .await;
    assert_eq!(
        expired["result"]["structuredContent"]["data"]["responses"][0]["error"]["code"],
        "response_unavailable"
    );
    tokio::time::resume();
    assert_eq!(client.sent.load(Ordering::SeqCst), 2);
    wire.close().await;
    let mut restarted = native_wire(handler(Arc::clone(&client), policy(), 1).await, 1);
    let lost = call(
        &mut restarted,
        1,
        "get_last_responses",
        json!({"recovery_keys":["second"]}),
    )
    .await;
    assert_eq!(
        lost["result"]["structuredContent"]["data"]["responses"][0]["error"]["code"],
        "response_unavailable"
    );
    assert_eq!(client.sent.load(Ordering::SeqCst), 2);
    assert!(!lost.to_string().contains("not_sent"));
    restarted.close().await;
}

#[tokio::test]
async fn sending_recovery_cancel_after_simulated_dispatch_suppresses_response_and_marks_recovery_inconclusive()
 {
    let gate = Arc::new(SendGate::default());
    let mut client = OfflineClient::new(true);
    client.gate = Some(Arc::clone(&gate));
    let client = Arc::new(client);
    let mut wire = native_wire(handler(Arc::clone(&client), policy(), 4).await, 1);
    let mut req = modern(json!(1), "tools/call");
    req["params"]["name"] = json!("send_message");
    req["params"]["arguments"] = text("cancelled");
    wire.send(req).await;
    gate.started.notified().await;
    assert_eq!(gate.dispatched.load(Ordering::SeqCst), 1);
    let pending = call(
        &mut wire,
        2,
        "get_last_responses",
        json!({"recovery_keys":["cancelled"]}),
    )
    .await;
    assert_eq!(
        pending["result"]["structuredContent"]["data"]["responses"][0]["error"]["code"],
        "response_pending"
    );
    wire.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}))
        .await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while gate.dropped.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("operation dropped");
    let lost = call(
        &mut wire,
        3,
        "get_last_responses",
        json!({"recovery_keys":["cancelled"]}),
    )
    .await;
    assert_eq!(
        lost["id"], 3,
        "no cancelled call response may leak ahead of recovery"
    );
    assert_eq!(
        lost["result"]["structuredContent"]["data"]["responses"][0]["error"]["code"],
        "response_unavailable"
    );
    assert_eq!(client.sent.load(Ordering::SeqCst), 1);
    assert!(!lost.to_string().contains("not_sent"));
    wire.close().await;
}
