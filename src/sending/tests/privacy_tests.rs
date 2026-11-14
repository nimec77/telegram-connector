use super::staging::StagingStore;
use super::test_support::Fixture;
use crate::telegram::sending::SendDeadline;
use crate::telegram::types::sending::SendMediaKind;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::time::Duration;

#[tokio::test]
async fn sending_privacy_adjacent_sources_are_allowed_but_sensitive_roots_and_overlap_are_rejected()
{
    let fixture = Fixture::new();
    let store = StagingStore::try_new(&fixture.config)
        .await
        .expect("source is adjacent to logs, not inside them");
    store
        .close(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
        .expect("close");
    for case in 0..4 {
        let mut fixture = Fixture::new();
        let root = match case {
            0 => fixture.dir.path().to_path_buf(),
            1 => fixture.dir.path().join("logs"),
            2 => fixture.staging.clone(),
            _ => fixture.dir.path().join("secrets"),
        };
        if !root.exists() {
            std::fs::create_dir(&root).expect("fixture");
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
                .expect("private");
        }
        fixture
            .config
            .settings
            .upload_roots
            .insert("outgoing".into(), root);
        assert!(
            StagingStore::try_new(&fixture.config).await.is_err(),
            "protected case {case}"
        );
    }
}

#[tokio::test]
async fn sending_privacy_path_escapes_symlinks_hardlinks_and_nonregular_sources_are_refused() {
    let fixture = Fixture::new();
    symlink(
        fixture.source.join("image.jpg"),
        fixture.source.join("link.jpg"),
    )
    .expect("fixture symlink");
    std::fs::hard_link(
        fixture.source.join("image.jpg"),
        fixture.source.join("hard.jpg"),
    )
    .expect("fixture hardlink");
    std::fs::create_dir(fixture.source.join("directory")).expect("directory");
    symlink(&fixture.source, fixture.source.join("alias")).expect("intermediate symlink");
    let store = StagingStore::try_new(&fixture.config).await.expect("store");
    for name in [
        "../image.jpg",
        "/etc/passwd",
        "link.jpg",
        "hard.jpg",
        "directory",
        "alias/image.jpg",
        "",
    ] {
        assert!(
            store
                .stage(
                    &fixture.source(name),
                    &SendMediaKind::Image,
                    SendDeadline::new(30).expect("budget")
                )
                .await
                .is_err(),
            "{name}"
        );
    }
    store
        .close(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
        .expect("close");
}

#[tokio::test]
async fn sending_privacy_requires_fresh_private_root_and_protected_ancestry() {
    for case in 0..3 {
        let fixture = Fixture::new();
        match case {
            0 => {
                std::fs::write(fixture.staging.join("old-data"), b"do not sweep").expect("fixture")
            }
            1 => std::fs::set_permissions(&fixture.staging, std::fs::Permissions::from_mode(0o777))
                .expect("writable"),
            _ => {
                std::fs::set_permissions(&fixture.staging, std::fs::Permissions::from_mode(0o1777))
                    .expect("sticky is not a private parent")
            }
        }
        assert!(StagingStore::try_new(&fixture.config).await.is_err());
    }
}

#[tokio::test]
async fn sending_privacy_hostile_filename_is_sanitized_and_fifo_is_not_opened() {
    let fixture = Fixture::new();
    let name = "private\n..\\name.avi";
    std::fs::write(fixture.source.join(name), &fixture.bytes).expect("fixture");
    let fifo = fixture.source.join("fifo");
    assert!(
        std::process::Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()
            .expect("disposable FIFO")
            .success()
    );
    let store = StagingStore::try_new(&fixture.config).await.expect("store");
    let held = store
        .stage(
            &fixture.source(name),
            &SendMediaKind::Image,
            SendDeadline::new(30).expect("budget"),
        )
        .await
        .expect("JPEG under hostile name");
    assert!(held.upload_name().ends_with(".jpg"));
    assert!(
        held.upload_name()
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    );
    assert!(
        store
            .stage(
                &fixture.source("fifo"),
                &SendMediaKind::Image,
                SendDeadline::new(30).expect("budget")
            )
            .await
            .is_err()
    );
    drop(held);
    store
        .close(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
        .expect("close");
}

#[tokio::test]
async fn sending_privacy_source_identity_detects_truncation_growth_and_same_size_replacement() {
    let fixture = Fixture::new();
    let path = fixture.source.join("image.jpg");
    for case in 0..3 {
        std::fs::write(&path, &fixture.bytes).expect("original");
        let before = super::privacy::identity(&std::fs::metadata(&path).expect("metadata"))
            .expect("identity");
        match case {
            0 => std::fs::write(&path, b"x").expect("truncate"),
            1 => {
                use std::io::Write as _;
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .expect("open")
                    .write_all(b"growth")
                    .expect("grow");
            }
            _ => {
                std::fs::rename(&path, fixture.source.join("retired.jpg"))
                    .expect("retain old inode");
                std::fs::write(&path, &fixture.bytes).expect("same length replacement");
            }
        }
        assert!(super::privacy::same_file(&path, before).is_err());
    }
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn sending_privacy_rejects_granting_acl_on_disposable_source_root() {
    let fixture = Fixture::new();
    assert!(
        std::process::Command::new("/bin/chmod")
            .args(["+a", "everyone allow write"])
            .arg(&fixture.source)
            .status()
            .expect("fixture ACL")
            .success()
    );
    assert!(StagingStore::try_new(&fixture.config).await.is_err());
}
