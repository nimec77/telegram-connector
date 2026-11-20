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
    let store = StagingStore::try_new(&fixture.config, &fixture.protected_paths)
        .await
        .expect("store");
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
    let store = StagingStore::try_new(&fixture.config, &fixture.protected_paths)
        .await
        .expect("store");
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

#[tokio::test]
async fn sending_staging_large_metadata_reader_starts_at_zero_and_preserves_all_bytes() {
    use tokio::io::AsyncReadExt as _;
    let fixture = Fixture::new();
    let store = StagingStore::try_new(&fixture.config, &fixture.protected_paths)
        .await
        .expect("store");
    for (bytes, kind, mime) in [
        (
            crate::sending::media_fixtures::metadata_jpeg(),
            SendMediaKind::Image,
            "image/jpeg",
        ),
        (
            crate::sending::media_fixtures::tagged_mp3(4, 0, &vec![0; 131_072]),
            SendMediaKind::Audio {
                duration_seconds: 1,
                title: None,
                performer: None,
            },
            "audio/mpeg",
        ),
    ] {
        std::fs::write(fixture.source.join("metadata.bin"), &bytes).expect("source");
        let staged = store
            .stage(
                &fixture.source("metadata.bin"),
                &kind,
                SendDeadline::new(30).expect("budget"),
            )
            .await
            .expect("metadata snapshot");
        assert_eq!(staged.mime_type(), mime);
        assert_eq!(staged.size_bytes(), bytes.len() as u64);
        std::fs::write(fixture.source.join("metadata.bin"), b"changed source").expect("replace");
        let mut reader = staged.reader().await.expect("verified reader");
        let mut actual = Vec::new();
        reader
            .read_to_end(&mut actual)
            .await
            .expect("read full snapshot");
        assert_eq!(
            actual, bytes,
            "probe cursor must not leave the upload at EOF"
        );
    }
    store
        .close(Instant::now() + Duration::from_secs(5))
        .await
        .expect("cleanup");
}

#[tokio::test]
async fn sending_staging_large_metadata_counts_against_the_entire_file_cap() {
    for (bytes, kind) in [
        (
            crate::sending::media_fixtures::metadata_jpeg(),
            SendMediaKind::Image,
        ),
        (
            crate::sending::media_fixtures::tagged_mp3(4, 0, &vec![0; 131_072]),
            SendMediaKind::Audio {
                duration_seconds: 1,
                title: None,
                performer: None,
            },
        ),
    ] {
        let mut fixture = Fixture::new();
        std::fs::write(fixture.source.join("metadata.bin"), &bytes).expect("source");
        let cap = bytes.len() as u64;
        fixture.config.max_image_bytes = cap;
        fixture.config.max_media_bytes = cap;
        let store = StagingStore::try_new(&fixture.config, &fixture.protected_paths)
            .await
            .expect("store");
        let staged = store
            .stage(
                &fixture.source("metadata.bin"),
                &kind,
                SendDeadline::new(30).expect("budget"),
            )
            .await
            .expect("exact file cap");
        drop(staged);
        let mut oversized = bytes.clone();
        oversized.push(0);
        std::fs::write(fixture.source.join("metadata.bin"), &oversized).expect("cap plus one");
        assert_eq!(
            store
                .stage(
                    &fixture.source("metadata.bin"),
                    &kind,
                    SendDeadline::new(30).expect("budget")
                )
                .await
                .expect_err("metadata still consumes file quota")
                .kind(),
            SendFailureKind::MediaTooLarge
        );
        store
            .close(Instant::now() + Duration::from_secs(5))
            .await
            .expect("cleanup");
    }
}
