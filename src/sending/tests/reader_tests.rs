use super::*;
use crate::sending::staging::StagingStore;
use crate::sending::test_support::Fixture;
use crate::telegram::sending::SendDeadline;
use crate::telegram::types::sending::{SendFailureKind, SendMediaKind};
use std::time::Duration;
use tokio::time::Instant;

#[tokio::test]
async fn sending_staging_detached_read_job_and_completed_bytes_keep_the_snapshot_lease() {
    let fixture = Fixture::new();
    let store = StagingStore::try_new(&fixture.config).await.expect("store");
    let snapshot = store
        .stage(
            &fixture.source("image.jpg"),
            &SendMediaKind::Image,
            SendDeadline::new(30).expect("budget"),
        )
        .await
        .expect("snapshot");
    let mut reader = snapshot.reader().await.expect("reader");
    let job = reader.job(524_288).expect("bounded read job");
    drop(reader);
    drop(snapshot);
    let (start_tx, start_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let pending = tokio::task::spawn_blocking(move || {
        let _ = start_tx.send(());
        let _ = release_rx.recv_timeout(Duration::from_secs(10));
        job.run()
    });
    start_rx.await.expect("started");
    let held = store
        .stage(
            &fixture.source("image.jpg"),
            &SendMediaKind::Image,
            SendDeadline::new(30).expect("budget"),
        )
        .await
        .expect("second slot");
    assert_eq!(
        store
            .stage(
                &fixture.source("image.jpg"),
                &SendMediaKind::Image,
                SendDeadline::new(30).expect("budget")
            )
            .await
            .expect_err("read job keeps first slot")
            .kind(),
        SendFailureKind::SendCapacityReached
    );
    release_tx.send(()).expect("release");
    let completed = pending.await.expect("completed IO");
    assert_eq!(
        *completed.result.as_ref().expect("read bytes"),
        fixture.bytes.len()
    );
    assert_eq!(
        &completed.bytes[..fixture.bytes.len()],
        fixture.bytes.as_slice()
    );
    assert_eq!(
        store
            .stage(
                &fixture.source("image.jpg"),
                &SendMediaKind::Image,
                SendDeadline::new(30).expect("budget")
            )
            .await
            .expect_err("unconsumed bytes keep slot")
            .kind(),
        SendFailureKind::SendCapacityReached
    );
    drop(completed);
    drop(held);
    store
        .close(Instant::now() + Duration::from_secs(5))
        .await
        .expect("cleanup");
}

#[tokio::test]
async fn sending_staging_reader_cannot_return_buffered_bytes_after_operation_expiry() {
    use tokio::io::AsyncReadExt as _;
    let fixture = Fixture::new();
    let store = StagingStore::try_new(&fixture.config).await.expect("store");
    let snapshot = store
        .stage(
            &fixture.source("image.jpg"),
            &SendMediaKind::Image,
            SendDeadline::new(30).expect("budget"),
        )
        .await
        .expect("snapshot");
    let mut reader = snapshot.reader().await.expect("reader");
    let job = reader.job(524_288).expect("read job");
    reader.pending = Some(tokio::task::spawn_blocking(move || job.run()));
    let mut one = [0u8; 1];
    assert_eq!(reader.read(&mut one).await.expect("first byte"), 1);
    assert_eq!(one[0], fixture.bytes[0]);
    assert!(reader.bytes.len() > reader.offset);
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(30)).await;
    assert_eq!(
        reader
            .read(&mut one)
            .await
            .expect_err("no data at expiry")
            .kind(),
        io::ErrorKind::TimedOut
    );
    tokio::time::resume();
    drop(reader);
    drop(snapshot);
    store
        .close(Instant::now() + Duration::from_secs(5))
        .await
        .expect("cleanup");
}
