use super::*;

fn config(root: PathBuf) -> StoreConfig {
    StoreConfig {
        root,
        ttl: Duration::from_secs(600),
        max_entries: 100,
        max_bytes: 1024,
    }
}
async fn store() -> (tempfile::TempDir, Arc<ArtifactStore>) {
    let temp = tempfile::tempdir().expect("private fixture");
    let store = ArtifactStore::try_new(config(temp.path().join("artifacts")))
        .await
        .expect("initialize store");
    (temp, store)
}
async fn job<T: Send + 'static>(
    store: &Arc<ArtifactStore>,
    f: impl FnOnce(&ArtifactStore) -> T + Send + 'static,
) -> T {
    let store = Arc::clone(store);
    crate::mcp::runtime::image_job(move || f(&store))
        .await
        .expect("artifact worker")
}

#[tokio::test]
async fn inserts_complete_bytes_with_dimensions_and_opaque_ids() {
    let (_temp, store) = store().await;
    let before = chrono::Utc::now();
    let item = job(&store, |s| s.insert(b"verified JPEG bytes", 30, 20))
        .await
        .expect("insert");
    assert_eq!((item.width, item.height, item.size_bytes), (30, 20, 19));
    assert!(item.expires_at >= before + chrono::Duration::seconds(599));
    assert!(item.expires_at <= chrono::Utc::now() + chrono::Duration::seconds(600));
    let id = item
        .uri
        .strip_prefix("telegram-media://artifact/")
        .expect("private URI");
    assert_eq!(id.len(), 48);
    assert!(
        id.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert_eq!(
        std::fs::read(&item.path).expect("complete export"),
        b"verified JPEG bytes"
    );
    let uri = item.uri.clone();
    assert_eq!(
        job(&store, move |s| s.read(&uri, 19))
            .await
            .expect("read")
            .bytes,
        b"verified JPEG bytes"
    );
    assert_eq!(
        job(&store, |s| s.snapshot()).await.expect("usage"),
        StoreSnapshot {
            entries: 1,
            bytes: 19
        }
    );
}

#[tokio::test]
async fn strict_uri_lookup_and_cross_store_isolation() {
    let (_temp, store) = store().await;
    let (_other_temp, other) = self::store().await;
    let item = job(&store, |s| s.insert(b"jpeg", 1, 1))
        .await
        .expect("insert");
    let uri = item.uri.clone();
    assert_eq!(
        job(&other, move |s| s.read(&uri, 100))
            .await
            .expect_err("separate manifest"),
        StoreError::Missing
    );
    for uri in [
        "file:///tmp/image.jpg".into(),
        format!("{}?x=1", item.uri),
        format!("{}/../x", item.uri),
        item.uri.to_uppercase(),
        "telegram-media://artifact/%2e%2e".into(),
    ] {
        assert_eq!(
            job(&store, move |s| s.read(&uri, 100))
                .await
                .expect_err("strict URI"),
            StoreError::InvalidUri
        );
    }
}

#[tokio::test]
async fn exact_caps_evict_oldest_and_oversize_preserves_healthy_entries() {
    let temp = tempfile::tempdir().expect("fixture");
    let mut cfg = config(temp.path().join("store"));
    cfg.max_entries = 2;
    cfg.max_bytes = 8;
    let store = ArtifactStore::try_new(cfg).await.expect("store");
    let first = job(&store, |s| s.insert(b"1111", 1, 1))
        .await
        .expect("first");
    let second = job(&store, |s| s.insert(b"2222", 1, 1))
        .await
        .expect("second");
    assert_eq!(
        job(&store, |s| s.snapshot()).await.expect("usage"),
        StoreSnapshot {
            entries: 2,
            bytes: 8
        }
    );
    assert_eq!(
        job(&store, |s| s.insert(b"123456789", 1, 1))
            .await
            .expect_err("oversize"),
        StoreError::TooLarge
    );
    assert!(first.path.exists());
    assert!(second.path.exists());
    let _third = job(&store, |s| s.insert(b"3333", 1, 1))
        .await
        .expect("third");
    assert!(!first.path.exists());
    assert!(second.path.exists());
    let uri = second.uri;
    assert_eq!(
        job(&store, move |s| s.read(&uri, 3))
            .await
            .expect_err("pre-allocation read bound"),
        StoreError::TooLarge
    );
    assert_eq!(
        job(&store, |s| s.snapshot()).await.expect("usage"),
        StoreSnapshot {
            entries: 2,
            bytes: 8
        }
    );
}
#[tokio::test]
async fn entry_cap_evicts_even_when_byte_capacity_remains() {
    let temp = tempfile::tempdir().expect("fixture");
    let mut cfg = config(temp.path().join("store"));
    cfg.max_entries = 1;
    let store = ArtifactStore::try_new(cfg).await.expect("store");
    let first = job(&store, |s| s.insert(b"1", 1, 1)).await.expect("first");
    job(&store, |s| s.insert(b"2", 1, 1)).await.expect("second");
    assert!(!first.path.exists());
}
#[tokio::test]
async fn invalid_config_is_refused_without_initializing_storage() {
    let temp = tempfile::tempdir().expect("fixture");
    for (ttl, entries, bytes) in [
        (0, 1, 1),
        (3601, 1, 1),
        (1, 0, 1),
        (1, 1001, 1),
        (1, 1, 0),
        (1, 1, 1_073_741_825),
    ] {
        let mut cfg = config(temp.path().join("store"));
        cfg.ttl = Duration::from_secs(ttl);
        cfg.max_entries = entries;
        cfg.max_bytes = bytes;
        assert!(matches!(
            ArtifactStore::try_new(cfg).await,
            Err(StoreError::Unavailable)
        ));
        assert!(!temp.path().join("store").exists());
    }
    for root in [PathBuf::new(), PathBuf::from("relative")] {
        assert!(matches!(
            ArtifactStore::try_new(config(root)).await,
            Err(StoreError::Unavailable)
        ));
    }
}
#[tokio::test]
async fn ttl_never_refreshes_and_expired_reads_fail_immediately() {
    let (_temp, store) = store().await;
    tokio::time::pause();
    // Keep the runtime runnable while real blocking I/O completes: only explicit advances count.
    let clock_guard = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    let item = job(&store, |s| s.insert(b"jpeg", 1, 1))
        .await
        .expect("insert");
    let uri = item.uri.clone();
    let expiry = job(&store, move |s| s.read(&uri, 4))
        .await
        .expect("read")
        .expires_at;
    tokio::time::advance(Duration::from_secs(599)).await;
    let uri = item.uri.clone();
    assert_eq!(
        job(&store, move |s| s.read(&uri, 4))
            .await
            .expect("read near expiry")
            .expires_at,
        expiry
    );
    tokio::time::advance(Duration::from_secs(1)).await;
    let uri = item.uri;
    assert_eq!(
        job(&store, move |s| s.read(&uri, 4))
            .await
            .expect_err("expired"),
        StoreError::Missing
    );
    clock_guard.abort();
}
#[tokio::test]
async fn foreign_entries_fail_closed_without_deleting_data() {
    let (temp, store) = store().await;
    let item = job(&store, |s| s.insert(b"jpeg", 1, 1))
        .await
        .expect("insert");
    let foreign = temp.path().join("artifacts/foreign");
    std::fs::write(&foreign, b"foreign").expect("foreign fixture");
    assert_eq!(
        job(&store, |s| s.cleanup())
            .await
            .expect_err("foreign entry"),
        StoreError::Unavailable
    );
    assert_eq!(
        job(&store, |s| s.insert(b"x", 1, 1))
            .await
            .expect_err("failed closed"),
        StoreError::Unavailable
    );
    assert!(item.path.exists());
    assert!(foreign.exists());
}
#[tokio::test]
async fn failed_deletion_refuses_mutation_without_freeing_quota() {
    let (_temp, store) = store().await;
    let item = job(&store, |s| s.insert(b"jpeg", 1, 1))
        .await
        .expect("insert");
    std::fs::remove_file(&item.path).expect("remove fixture");
    std::fs::create_dir(&item.path).expect("replace with nonregular entry");
    let uri = item.uri;
    assert_eq!(
        job(&store, move |s| s.remove(&uri))
            .await
            .expect_err("refuse foreign"),
        StoreError::Unavailable
    );
    assert_eq!(
        job(&store, |s| s.snapshot())
            .await
            .expect_err("unavailable usage"),
        StoreError::Unavailable
    );
    assert_eq!(
        job(&store, |s| s.insert(b"x", 1, 1))
            .await
            .expect_err("no mutations"),
        StoreError::Unavailable
    );
    assert!(item.path.is_dir());
}

#[tokio::test]
async fn missing_file_is_missing_and_explicit_removal_releases_reservation() {
    let (_temp, store) = store().await;
    let item = job(&store, |s| s.insert(b"jpeg", 1, 1))
        .await
        .expect("insert");
    std::fs::remove_file(&item.path).expect("remove fixture");
    let uri = item.uri.clone();
    assert_eq!(
        job(&store, move |s| s.read(&uri, 4))
            .await
            .expect_err("missing"),
        StoreError::Missing
    );
    let uri = item.uri;
    job(&store, move |s| s.remove(&uri))
        .await
        .expect("release absent owned file");
    assert_eq!(
        job(&store, |s| s.snapshot()).await.expect("usage"),
        StoreSnapshot {
            entries: 0,
            bytes: 0
        }
    );
}
#[cfg(unix)]
#[tokio::test]
async fn linked_and_replaced_artifacts_are_refused_without_deleting_foreign_data() {
    use std::os::unix::fs::symlink;
    for hardlink in [false, true] {
        let (temp, store) = store().await;
        let item = job(&store, |s| s.insert(b"jpeg", 1, 1))
            .await
            .expect("insert");
        let foreign = temp.path().join("foreign.jpg");
        if hardlink {
            std::fs::hard_link(&item.path, &foreign).expect("hardlink");
        } else {
            std::fs::write(&foreign, b"jpeg").expect("foreign file");
            std::fs::remove_file(&item.path).expect("remove owned");
            symlink(&foreign, &item.path).expect("symlink");
        }
        let uri = item.uri;
        assert_eq!(
            job(&store, move |s| s.read(&uri, 4))
                .await
                .expect_err("linked entry"),
            StoreError::Unavailable
        );
        assert_eq!(
            job(&store, |s| s.cleanup())
                .await
                .expect_err("refuse cleanup"),
            StoreError::Unavailable
        );
        assert_eq!(std::fs::read(&foreign).expect("foreign survives"), b"jpeg");
        assert!(std::fs::symlink_metadata(&item.path).is_ok());
    }
}
#[tokio::test]
async fn poisoned_state_is_unavailable_and_never_recovered() {
    let (_temp, store) = store().await;
    let poison = Arc::clone(&store);
    assert!(
        std::thread::spawn(move || {
            let _guard = poison.state.lock().expect("healthy state");
            panic!("interrupt store mutation");
        })
        .join()
        .is_err()
    );
    assert_eq!(
        job(&store, |s| s.snapshot()).await.expect_err("poison"),
        StoreError::Unavailable
    );
    assert_eq!(
        job(&store, |s| s.insert(b"jpeg", 1, 1))
            .await
            .expect_err("poison"),
        StoreError::Unavailable
    );
    assert_eq!(
        job(&store, |s| s.cleanup()).await.expect_err("poison"),
        StoreError::Unavailable
    );
}
#[tokio::test]
async fn periodic_cleanup_uses_weak_ownership_and_drop_removes_only_known_files() {
    let (temp, store) = store().await;
    let item = job(&store, |s| s.insert(b"jpeg", 1, 1))
        .await
        .expect("insert");
    tokio::time::pause();
    let clock_guard = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(600)).await;
    for _ in 0..10000 {
        if !item.path.exists() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        !item.path.exists(),
        "periodic cleanup must delete expired bytes without a caller operation"
    );
    let item = job(&store, |s| s.insert(b"new", 1, 1))
        .await
        .expect("new insert");
    let foreign = temp.path().join("artifacts/foreign");
    std::fs::write(&foreign, b"keep").expect("foreign fixture");
    let weak = Arc::downgrade(&store);
    drop(store);
    for _ in 0..10000 {
        if !item.path.exists() && !temp.path().join("artifacts/.lock").exists() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        weak.upgrade().is_none(),
        "periodic task must not retain store"
    );
    assert!(!item.path.exists());
    assert!(!temp.path().join("artifacts/.lock").exists());
    assert_eq!(std::fs::read(&foreign).expect("foreign survives"), b"keep");
    clock_guard.abort();
}
#[tokio::test]
async fn nonempty_stale_and_second_process_roots_fail_without_deletion() {
    let (temp, store) = store().await;
    let root = temp.path().join("artifacts");
    assert!(matches!(
        ArtifactStore::try_new(config(root.clone())).await,
        Err(StoreError::Unavailable)
    ));
    let stale = tempfile::tempdir().expect("stale root");
    std::fs::write(stale.path().join(".lock"), b"stale").expect("stale fixture");
    assert!(matches!(
        ArtifactStore::try_new(config(stale.path().to_path_buf())).await,
        Err(StoreError::Unavailable)
    ));
    assert_eq!(
        std::fs::read(stale.path().join(".lock")).expect("stale preserved"),
        b"stale"
    );
    assert!(root.join(".lock").exists());
    drop(store);
}
#[cfg(unix)]
#[tokio::test]
async fn privacy_modes_and_unprotected_ancestors_are_enforced() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    let (temp, store) = store().await;
    let item = job(&store, |s| s.insert(b"jpeg", 1, 1))
        .await
        .expect("insert");
    assert_eq!(
        std::fs::metadata(temp.path().join("artifacts"))
            .expect("root")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for file in [&item.path, &temp.path().join("artifacts/.lock")] {
        assert_eq!(
            std::fs::metadata(file).expect("file").permissions().mode() & 0o777,
            0o600
        );
    }
    let unsafe_parent = temp.path().join("unsafe");
    std::fs::create_dir(&unsafe_parent).expect("unsafe parent");
    std::fs::set_permissions(&unsafe_parent, std::fs::Permissions::from_mode(0o777))
        .expect("permissions");
    assert!(matches!(
        ArtifactStore::try_new(config(unsafe_parent.join("private"))).await,
        Err(StoreError::Unavailable)
    ));
    assert_eq!(
        std::fs::metadata(&unsafe_parent)
            .expect("parent")
            .permissions()
            .mode()
            & 0o777,
        0o777
    );
    let alias = temp.path().join("alias");
    symlink(temp.path().join("artifacts"), &alias).expect("root symlink");
    assert!(matches!(
        ArtifactStore::try_new(config(alias)).await,
        Err(StoreError::Unavailable)
    ));
}
#[cfg(target_os = "macos")]
#[tokio::test]
async fn macos_granting_directory_acl_is_rejected_and_preserved() {
    let temp = tempfile::tempdir().expect("fixture");
    let parent = temp.path().join("granted");
    std::fs::create_dir(&parent).expect("parent");
    let result = std::process::Command::new("/bin/chmod")
        .args([
            "+a",
            "everyone allow read,search,file_inherit,directory_inherit",
        ])
        .arg(&parent)
        .output()
        .expect("grant disposable ACL");
    assert!(result.status.success(), "ACL fixture failed");
    assert!(matches!(
        ArtifactStore::try_new(config(parent.join("private"))).await,
        Err(StoreError::Unavailable)
    ));
    let acl = std::process::Command::new("/bin/ls")
        .args(["-lde"])
        .arg(&parent)
        .output()
        .expect("ACL listing");
    assert!(String::from_utf8_lossy(&acl.stdout).contains(" allow "));
    assert!(!parent.join("private/.lock").exists());
}
#[cfg(target_os = "macos")]
#[tokio::test]
async fn shared_sticky_root_is_refused_but_private_child_is_accepted() {
    assert!(matches!(
        ArtifactStore::try_new(config(PathBuf::from("/private/tmp"))).await,
        Err(StoreError::Unavailable)
    ));
    let temp = tempfile::tempdir_in("/private/tmp").expect("private child");
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private fixture root");
    let store = ArtifactStore::try_new(config(temp.path().to_path_buf()))
        .await
        .expect("private root below sticky ancestor");
    job(&store, |s| s.insert(b"jpeg", 1, 1))
        .await
        .expect("write under private root");
}

#[cfg(unix)]
#[tokio::test]
async fn staging_creation_failure_permanently_refuses_mutation() {
    use std::os::unix::fs::PermissionsExt as _;
    let (temp, store) = store().await;
    let root = temp.path().join("artifacts");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o500))
        .expect("inject write failure");
    assert_eq!(
        job(&store, |s| s.insert(b"jpeg", 1, 1))
            .await
            .expect_err("staging write"),
        StoreError::Unavailable
    );
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
        .expect("restore fixture permissions");
    assert_eq!(
        job(&store, |s| s.insert(b"jpeg", 1, 1))
            .await
            .expect_err("failed closed after write failure"),
        StoreError::Unavailable
    );
}
#[tokio::test]
async fn missing_ownership_lock_is_storage_failure_not_missing_artifact() {
    let (temp, store) = store().await;
    let item = job(&store, |s| s.insert(b"jpeg", 1, 1))
        .await
        .expect("insert");
    std::fs::remove_file(temp.path().join("artifacts/.lock")).expect("remove ownership lock");
    let uri = item.uri;
    assert_eq!(
        job(&store, move |s| s.read(&uri, 4))
            .await
            .expect_err("ownership failure"),
        StoreError::Unavailable
    );
}

#[tokio::test]
async fn queued_periodic_cleanup_does_not_retain_store() {
    use std::sync::Condvar;
    let (_temp, store) = store().await;
    let weak = Arc::downgrade(&store);
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let (started, mut observed) = tokio::sync::mpsc::unbounded_channel();
    let mut workers = Vec::new();
    for _ in 0..crate::mcp::runtime::IMAGE_PROCESSING_SLOTS {
        let release = Arc::clone(&release);
        let started = started.clone();
        workers.push(tokio::spawn(crate::mcp::runtime::image_job(move || {
            started.send(()).expect("worker started");
            let (lock, cv) = &*release;
            let mut released = lock.lock().expect("fixture lock");
            while !*released {
                released = cv.wait(released).expect("fixture wait");
            }
        })));
    }
    for _ in 0..crate::mcp::runtime::IMAGE_PROCESSING_SLOTS {
        observed.recv().await.expect("admitted blocker");
    }
    tokio::time::pause();
    let clock_guard = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(61)).await;
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }
    drop(store);
    let retained = weak.upgrade().is_some();
    let (lock, cv) = &*release;
    *lock.lock().expect("fixture lock") = true;
    cv.notify_all();
    for worker in workers {
        worker.await.expect("worker task").expect("blocking worker");
    }
    clock_guard.abort();
    assert!(
        !retained,
        "a cleanup awaiting worker admission retained the last store handle"
    );
}

#[tokio::test]
async fn processed_jpeg_cap_is_checked_before_eviction() {
    let temp = tempfile::tempdir().expect("fixture");
    let mut cfg = config(temp.path().join("artifacts"));
    cfg.max_bytes = 64 * 1024 * 1024;
    let store = ArtifactStore::try_new(cfg).await.expect("store");
    let first = job(&store, |s| s.insert(b"healthy", 1, 1))
        .await
        .expect("healthy insert");
    assert_eq!(
        job(&store, |s| s.insert(&vec![0; 20 * 1024 * 1024 + 1], 1, 1))
            .await
            .expect_err("JPEG cap"),
        StoreError::TooLarge
    );
    assert!(first.path.exists());
    let exact = job(&store, |s| s.insert(&vec![0; 20 * 1024 * 1024], 1, 1))
        .await
        .expect("exact JPEG cap");
    assert_eq!(
        std::fs::metadata(exact.path).expect("complete file").len(),
        20 * 1024 * 1024
    );
}

#[tokio::test]
async fn explicit_close_finishes_before_last_handle_drop_and_clones_stay_closed() {
    let (temp, store) = store().await;
    let clone = Arc::clone(&store);
    let item = job(&store, |s| s.insert(b"jpeg", 1, 1))
        .await
        .expect("insert");
    let uri = item.uri;
    let foreign = temp.path().join("artifacts/foreign");
    std::fs::write(&foreign, b"keep").expect("foreign file");
    // Cleanup refusal must not prevent safe finalization of the known owned manifest.
    assert_eq!(
        job(&store, |s| s.cleanup())
            .await
            .expect_err("foreign entry"),
        StoreError::Unavailable
    );
    job(&store, |s| s.close()).await.expect("explicit close");
    assert!(
        !item.path.exists(),
        "close must finish owned-file cleanup before returning"
    );
    assert!(!temp.path().join("artifacts/.lock").exists());
    assert_eq!(std::fs::read(&foreign).expect("foreign survives"), b"keep");
    job(&clone, |s| s.close()).await.expect("idempotent close");
    assert_eq!(
        job(&clone, |s| s.insert(b"new", 1, 1))
            .await
            .expect_err("live clone cannot reopen"),
        StoreError::Unavailable
    );
    assert_eq!(
        job(&clone, move |s| s.read(&uri, 4))
            .await
            .expect_err("closed read"),
        StoreError::Unavailable
    );
    assert_eq!(
        job(&clone, |s| s.snapshot())
            .await
            .expect_err("closed usage"),
        StoreError::Unavailable
    );
    drop(store);
    assert!(!temp.path().join("artifacts/.lock").exists());
    assert_eq!(
        std::fs::read(&foreign).expect("foreign survives finalization"),
        b"keep"
    );
}
#[cfg(unix)]
#[tokio::test]
async fn explicit_close_refuses_replaced_owned_file_and_still_closes_all_clones() {
    use std::os::unix::fs::symlink;
    let (temp, store) = store().await;
    let item = job(&store, |s| s.insert(b"jpeg", 1, 1))
        .await
        .expect("insert");
    let foreign = temp.path().join("foreign.jpg");
    std::fs::write(&foreign, b"keep").expect("foreign file");
    std::fs::remove_file(&item.path).expect("remove owned");
    symlink(&foreign, &item.path).expect("replace owned path");
    assert_eq!(
        job(&store, |s| s.close())
            .await
            .expect_err("unsafe cleanup"),
        StoreError::Unavailable
    );
    assert_eq!(std::fs::read(&foreign).expect("foreign survives"), b"keep");
    assert!(
        std::fs::symlink_metadata(&item.path)
            .expect("replacement survives")
            .file_type()
            .is_symlink()
    );
    std::fs::remove_file(&item.path).expect("remove injected replacement");
    assert_eq!(
        job(&store, |s| s.insert(b"new", 1, 1))
            .await
            .expect_err("failed close remains closed"),
        StoreError::Unavailable
    );
}

#[tokio::test]
async fn successful_close_is_disarmed_before_another_store_reuses_empty_root() {
    let (temp, store) = store().await;
    job(&store, |s| s.insert(b"old", 1, 1))
        .await
        .expect("old insert");
    job(&store, |s| s.close()).await.expect("close old store");
    let replacement = ArtifactStore::try_new(config(temp.path().join("artifacts")))
        .await
        .expect("reuse empty private root");
    let item = job(&replacement, |s| s.insert(b"new", 1, 1))
        .await
        .expect("new store insert");
    job(&store, |s| s.close())
        .await
        .expect("old close remains a no-op");
    drop(store);
    let uri = item.uri;
    assert_eq!(
        job(&replacement, move |s| s.read(&uri, 3))
            .await
            .expect("new store remains owned")
            .bytes,
        b"new"
    );
    assert!(temp.path().join("artifacts/.lock").exists());
    job(&replacement, |s| s.close())
        .await
        .expect("close replacement");
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_root_separators_and_dots_do_not_hide_symlinks() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    let mut accepted = Vec::new();
    for suffix in ["", "/", "/.", "/./"] {
        let temp = tempfile::tempdir().expect("fixture");
        let real = temp.path().join("real");
        std::fs::create_dir(&real).expect("root");
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).expect("private");
        let alias = temp.path().join("alias");
        symlink(&real, &alias).expect("alias");
        let spelling = PathBuf::from(format!("{}{suffix}", alias.display()));
        if !matches!(
            ArtifactStore::try_new(config(spelling)).await,
            Err(StoreError::Unavailable)
        ) {
            accepted.push(suffix);
        }
    }
    assert!(
        accepted.is_empty(),
        "accepted symlink spellings {accepted:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn root_normalization_preserves_physical_parent_traversal() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    let temp = tempfile::tempdir().expect("fixture");
    let parent = temp.path().join("physical");
    std::fs::create_dir(&parent).expect("parent");
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).expect("private");
    let child = parent.join("child");
    std::fs::create_dir(&child).expect("child");
    let alias = temp.path().join("alias");
    symlink(&child, &alias).expect("ancestor alias");
    let store = ArtifactStore::try_new(config(alias.join("../store/.")))
        .await
        .expect("physical parent");
    let item = job(&store, |store| store.insert(b"jpeg", 1, 1))
        .await
        .expect("insert");
    assert_eq!(
        item.path.parent().expect("root"),
        parent.canonicalize().expect("canonical").join("store")
    );
    assert!(
        !temp.path().join("store").exists(),
        "parent components must not be collapsed lexically"
    );
}
