use super::*;
use crate::sending::test_support::Fixture;
use std::time::Duration;

#[test]
fn sending_staging_copy_enforces_streamed_size_when_metadata_lies() {
    for (bytes, declared, cap, want) in [
        (
            b"abc".as_slice(),
            2,
            10,
            SendFailureKind::UploadSourceInvalid,
        ),
        (
            b"ab".as_slice(),
            3,
            10,
            SendFailureKind::UploadSourceInvalid,
        ),
        (b"abc".as_slice(), 2, 2, SendFailureKind::MediaTooLarge),
    ] {
        let mut input = bytes;
        let mut output = Vec::new();
        assert_eq!(
            copy_stream(&mut input, &mut output, declared, cap, || Ok(()))
                .expect_err("lying length")
                .kind(),
            want
        );
    }
}

#[tokio::test]
async fn sending_staging_cancelled_started_worker_keeps_slot_and_byte_reservation() {
    let fixture = Fixture::new();
    let store = StagingStore::try_new(&fixture.config).await.expect("store");
    let source = fixture.source("image.jpg");
    let bytes = fixture.bytes.len() as u64;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let running = Arc::clone(&store);
    let task = tokio::spawn(async move {
        running
            .stage_using(
                &source,
                &SendMediaKind::Image,
                SendDeadline::new(30).expect("budget"),
                move |mut job| {
                    job.lease.bytes(bytes)?;
                    let _ = started_tx.send(());
                    let _ = release_rx.recv_timeout(Duration::from_secs(10));
                    Err(SendFailure::not_sent(SendFailureKind::Timeout, None))
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), started_rx)
        .await
        .expect("worker starts")
        .expect("started");
    task.abort();
    assert!(task.await.expect_err("cancelled").is_cancelled());
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
            .expect_err("started cancelled copy still occupies slot")
            .kind(),
        SendFailureKind::SendCapacityReached
    );
    assert_eq!(
        store.owner.quota.state.lock().expect("quota").bytes,
        bytes * 2
    );
    release_tx.send(()).expect("release worker");
    drop(held);
    store
        .close(Instant::now() + Duration::from_secs(5))
        .await
        .expect("cleanup");
}

#[tokio::test]
async fn sending_staging_failed_cleanup_keeps_quota_and_never_deletes_replacement() {
    let fixture = Fixture::new();
    let store = StagingStore::try_new(&fixture.config).await.expect("store");
    let held = store
        .stage(
            &fixture.source("image.jpg"),
            &SendMediaKind::Image,
            SendDeadline::new(30).expect("budget"),
        )
        .await
        .expect("snapshot");
    let path = held.path.clone();
    std::fs::rename(&path, fixture.staging.join("renamed-owned-file"))
        .expect("disposable mutation");
    std::fs::create_dir(&path).expect("unowned replacement directory");
    drop(held);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let changed = store.owner.quota.changed.notified();
            if !store.owner.failed.lock().expect("failures").is_empty() {
                break;
            }
            changed.await;
        }
    })
    .await
    .expect("cleanup finishes");
    let second = store
        .stage(
            &fixture.source("image.jpg"),
            &SendMediaKind::Image,
            SendDeadline::new(30).expect("budget"),
        )
        .await
        .expect("remaining slot");
    assert_eq!(
        store
            .stage(
                &fixture.source("image.jpg"),
                &SendMediaKind::Image,
                SendDeadline::new(30).expect("budget")
            )
            .await
            .expect_err("failed cleanup still occupies slot")
            .kind(),
        SendFailureKind::SendCapacityReached
    );
    assert!(path.is_dir());
    drop(second);
    assert!(
        store
            .close(Instant::now() + Duration::from_secs(5))
            .await
            .is_err()
    );
}
