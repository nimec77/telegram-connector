use super::*;
use crate::sending::test_support::Fixture;

#[test]
fn sending_privacy_cancelled_local_preparation_cannot_acquire_or_start_network_work() {
    let fixture = Fixture::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .expect("isolated runtime");
    runtime.block_on(async {
        let mut client=MockTelegramClientTrait::new();client.expect_send_media().times(0);
        let mut limiter=MockRateLimiterTrait::new();limiter.expect_acquire().times(0);
        let server=McpServer::new(Arc::new(client),Arc::new(limiter)).with_sending(fixture.config.clone()).await.expect("store initialized");
        let(start_tx,start_rx)=tokio::sync::oneshot::channel();let(release_tx,release_rx)=std::sync::mpsc::channel();
        let blocker=tokio::task::spawn_blocking(move||{let _=start_tx.send(());let _=release_rx.recv_timeout(Duration::from_secs(10));});
        start_rx.await.expect("single blocking worker held");
        let request=serde_json::from_value(json!({"recipient":{"kind":"saved_messages"},"source":{"root":"outgoing","relative_path":"image.jpg"},"media":{"kind":"image"}})).expect("request");
        let mut preparation=Box::pin(server.send_media_data(request));
        let mut cx=std::task::Context::from_waker(futures::task::noop_waker_ref());
        assert!(preparation.as_mut().poll(&mut cx).is_pending(),"copy must queue behind held worker");
        drop(preparation);release_tx.send(()).expect("release");blocker.await.expect("worker returned");
        server.outgoing_store.as_ref().expect("store").close(tokio::time::Instant::now()+Duration::from_secs(5)).await.expect("cancelled preparation drains");
        assert_eq!(std::fs::read_dir(&fixture.staging).expect("staging root").count(),0);
    });
}

#[tokio::test]
async fn sending_privacy_direct_operation_cannot_bypass_enable_flag() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_send_message().times(0);
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().times(0);
    let server = McpServer::new(Arc::new(client), Arc::new(limiter));
    let request = serde_json::from_value(
        json!({"recipient":{"kind":"saved_messages"},"text":"private payload"}),
    )
    .expect("request");
    let failure = server
        .send_message_data(request)
        .await
        .expect_err("disabled operation");
    assert_eq!(
        failure.kind(),
        crate::telegram::types::sending::SendFailureKind::SendingDisabled
    );
    assert!(!format!("{failure:?} {failure}").contains("private payload"));
}
