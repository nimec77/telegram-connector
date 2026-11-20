use super::sending_protocol::offline_client::{OfflineClient, SendGate};
use super::sending_protocol::{handler, native_wire, policy, text};
use super::*;
use crate::sending::test_support::Fixture;
use crate::telegram::sending::SendDeadline;
use crate::telegram::types::sending::{SendFailureKind, SendMediaKind};
use std::sync::atomic::Ordering;

#[tokio::test]
async fn sending_shutdown_zero_grace_does_not_claim_unsent_after_simulated_dispatch() {
    let gate = Arc::new(SendGate::default());
    let mut client = OfflineClient::new(true);
    client.gate = Some(Arc::clone(&gate));
    let client = Arc::new(client);
    let server = handler(Arc::clone(&client), policy(), 4).await;
    let buffer = server.response_buffer();
    let mut wire = native_wire(server, 0);
    let mut req = modern(json!(1), "tools/call");
    req["params"]["name"] = json!("send_message");
    req["params"]["arguments"] = text("shutdown-call");
    wire.send(req).await;
    gate.started.notified().await;
    wire.close().await;
    assert_eq!(gate.dispatched.load(Ordering::SeqCst), 1);
    assert_eq!(gate.dropped.load(Ordering::SeqCst), 1);
    let selected = buffer
        .select_keys(&["shutdown-call".into()])
        .expect("recovery snapshot");
    assert!(matches!(
        selected.responses[0].1,
        Err(crate::error::Error::ResponseUnavailable)
    ));
}

#[tokio::test]
async fn sending_shutdown_stops_admission_without_granting_an_extra_cleanup_deadline() {
    let fixture = Fixture::new();
    let server = handler(
        Arc::new(OfflineClient::new(true)),
        fixture.config.clone(),
        4,
    )
    .await;
    let store = server.outgoing_store.as_ref().expect("store").clone();
    let held = store
        .stage(
            &fixture.source("image.jpg"),
            &SendMediaKind::Image,
            SendDeadline::new(120).expect("budget"),
        )
        .await
        .expect("held snapshot");
    let mut wire = native_wire(server, 0);
    wire.send(modern(json!(1), "tools/list")).await;
    assert_eq!(wire.recv().await["id"], 1);
    wire.close().await;
    assert_eq!(
        store
            .stage(
                &fixture.source("image.jpg"),
                &SendMediaKind::Image,
                SendDeadline::new(120).expect("budget")
            )
            .await
            .expect_err("closed admission")
            .kind(),
        SendFailureKind::Unavailable
    );
    drop(held);
    assert!(
        store
            .close(tokio::time::Instant::now() + Duration::from_secs(10))
            .await
            .is_err(),
        "a later close cannot create a new grace period"
    );
}

#[tokio::test]
async fn sending_shutdown_saturated_output_keeps_completed_receipt_and_original_drain_budget() {
    let client = Arc::new(OfflineClient::new(true));
    let server = handler(Arc::clone(&client), policy(), 4).await;
    let buffer = server.response_buffer();
    let mut wire = super::sending_protocol::native_wire_with_capacity(server, 1, 256);
    let mut request = modern(json!(1), "tools/call");
    request["params"]["name"] = json!("send_message");
    request["params"]["arguments"] = text("blocked-output");
    wire.send(request).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while client.sent.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("simulated dispatch");
    wire.shutdown
        .take()
        .expect("shutdown sender")
        .send(())
        .expect("signal");
    let Wire {
        task,
        read: _read,
        write: _write,
        ..
    } = wire;
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("no second output grace")
        .expect("runtime task");
    let selected = buffer
        .select_keys(&["blocked-output".into()])
        .expect("recovery");
    let response = selected.responses[0]
        .1
        .as_ref()
        .expect("completed before output loss");
    let value: Value = serde_json::from_str(&response.payload).expect("retained frame");
    assert_eq!(
        value["result"]["structuredContent"]["data"]["delivery_state"],
        "sent"
    );
    assert_eq!(client.sent.load(Ordering::SeqCst), 1);
}
