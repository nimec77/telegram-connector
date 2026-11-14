use super::staging::StagingStore;
use super::test_support::Fixture;
use crate::telegram::sending::SendDeadline;
use crate::telegram::types::sending::{SendFailureKind, SendMediaKind};
use std::os::unix::fs::PermissionsExt as _;
use std::time::Duration;
use tokio::io::AsyncReadExt as _;

fn deadline() -> SendDeadline {
    SendDeadline::new(30).expect("deadline")
}

#[tokio::test]
async fn sending_staging_snapshot_is_private_immutable_and_streamed_from_the_snapshot() {
    let fixture = Fixture::new();
    let store = StagingStore::try_new(&fixture.config).await.expect("store");
    let staged = store
        .stage(
            &fixture.source("image.jpg"),
            &SendMediaKind::Image,
            deadline(),
        )
        .await
        .expect("snapshot");
    assert_eq!(staged.size_bytes(), fixture.bytes.len() as u64);
    assert_eq!(staged.mime_type(), "image/jpeg");
    assert!(staged.upload_name().ends_with(".jpg"));
    assert!(!staged.upload_name().contains('/'));
    for entry in std::fs::read_dir(&fixture.staging).expect("private staging") {
        assert_eq!(
            entry
                .expect("entry")
                .metadata()
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    assert_eq!(
        std::fs::read(fixture.source.join("image.jpg")).expect("unchanged source"),
        fixture.bytes
    );
    std::fs::write(
        fixture.source.join("image.jpg"),
        b"source changed after snapshot",
    )
    .expect("change source");
    let mut reader = staged.reader().await.expect("verified snapshot reader");
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .expect("stream snapshot");
    assert_eq!(bytes, fixture.bytes);
    drop(reader);
    drop(staged);
    store
        .close(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
        .expect("cleanup");
    assert_eq!(
        std::fs::read_dir(&fixture.staging)
            .expect("staging")
            .count(),
        0
    );
}

#[tokio::test]
async fn sending_staging_slots_and_byte_quota_refuse_promptly_without_overcommit() {
    let fixture = Fixture::new();
    let store = StagingStore::try_new(&fixture.config).await.expect("store");
    let a = store
        .stage(
            &fixture.source("image.jpg"),
            &SendMediaKind::Image,
            deadline(),
        )
        .await
        .expect("first");
    let b = store
        .stage(
            &fixture.source("image.jpg"),
            &SendMediaKind::Image,
            deadline(),
        )
        .await
        .expect("second");
    assert_eq!(
        store
            .stage(
                &fixture.source("image.jpg"),
                &SendMediaKind::Image,
                deadline()
            )
            .await
            .expect_err("no third slot")
            .kind(),
        SendFailureKind::SendCapacityReached
    );
    drop(a);
    drop(b);
    store
        .close(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
        .expect("cleanup");
    let mut fixture = Fixture::new();
    let cap = fixture.bytes.len() as u64;
    fixture.config.settings.max_image_bytes = cap;
    fixture.config.settings.max_media_bytes = cap;
    fixture.config.settings.max_staging_bytes = cap;
    let store = StagingStore::try_new(&fixture.config)
        .await
        .expect("bounded store");
    let held = store
        .stage(
            &fixture.source("image.jpg"),
            &SendMediaKind::Image,
            deadline(),
        )
        .await
        .expect("exact cap");
    assert_eq!(
        store
            .stage(
                &fixture.source("image.jpg"),
                &SendMediaKind::Image,
                deadline()
            )
            .await
            .expect_err("byte quota")
            .kind(),
        SendFailureKind::SendCapacityReached
    );
    drop(held);
    store
        .close(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
        .expect("cleanup");
}

#[tokio::test]
async fn sending_staging_cap_plus_one_and_type_mismatch_fail_without_source_changes() {
    let mut fixture = Fixture::new();
    fixture.config.settings.max_image_bytes = fixture.bytes.len() as u64 - 1;
    let store = StagingStore::try_new(&fixture.config).await.expect("store");
    assert_eq!(
        store
            .stage(
                &fixture.source("image.jpg"),
                &SendMediaKind::Image,
                deadline()
            )
            .await
            .expect_err("too large")
            .kind(),
        SendFailureKind::MediaTooLarge
    );
    assert_eq!(
        store
            .stage(
                &fixture.source("image.jpg"),
                &SendMediaKind::Voice {
                    duration_seconds: 1
                },
                deadline()
            )
            .await
            .expect_err("not voice")
            .kind(),
        SendFailureKind::UnsupportedMedia
    );
    assert_eq!(
        std::fs::read(fixture.source.join("image.jpg")).expect("source"),
        fixture.bytes
    );
    store
        .close(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
        .expect("cleanup");
}
