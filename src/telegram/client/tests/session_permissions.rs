use super::*;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

fn config(path: &std::path::Path) -> TelegramConfig {
    toml::from_str(&format!(
        "api_id = 1\nsession_file = {:?}\n",
        path.to_string_lossy()
    ))
    .expect("disposable config")
}

#[tokio::test]
async fn task8_new_session_directories_database_and_wal_are_private() {
    let directory = tempfile::tempdir().expect("disposable session root");
    let parent = directory.path().join("private/account");
    let path = parent.join("session.db");
    let client = TelegramClient::new(
        &config(&path),
        &crate::config::defaults::default_search_config(),
    )
    .await
    .expect("offline client");
    for folder in [directory.path().join("private"), parent] {
        assert_eq!(
            std::fs::metadata(folder)
                .expect("directory")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    let database = libsql::Builder::new_local(&path)
        .build()
        .await
        .expect("offline database");
    let connection = database.connect().expect("SQLite connection");
    connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE permission_fixture (value INTEGER); INSERT INTO permission_fixture VALUES (1);").await.expect("WAL write");
    for suffix in ["", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        let metadata =
            std::fs::symlink_metadata(std::path::PathBuf::from(name)).expect("SQLite entry");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600, "{suffix}");
        assert_eq!(metadata.nlink(), 1);
    }
    drop(client);
}

#[tokio::test]
async fn task8_session_and_sidecar_symlinks_are_refused_without_modifying_targets() {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let directory = tempfile::tempdir().expect("disposable root");
        let path = directory.path().join("session.db");
        let target = directory.path().join("unrelated.db");
        std::fs::write(&target, []).expect("empty unrelated fixture");
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        std::os::unix::fs::symlink(&target, std::path::PathBuf::from(name)).expect("link fixture");
        let result = TelegramClient::new(
            &config(&path),
            &crate::config::defaults::default_search_config(),
        )
        .await;
        assert!(result.is_err(), "accepted {suffix} symlink");
        assert_eq!(
            std::fs::read(&target).expect("target unchanged"),
            Vec::<u8>::new()
        );
    }
}

#[tokio::test]
async fn task8_existing_session_hardlinks_are_refused_before_sqlite_opens_them() {
    let directory = tempfile::tempdir().expect("disposable root");
    let path = directory.path().join("session.db");
    let target = directory.path().join("unrelated.db");
    std::fs::write(&target, []).expect("empty unrelated fixture");
    std::fs::hard_link(&target, &path).expect("hardlink");
    let result = TelegramClient::new(
        &config(&path),
        &crate::config::defaults::default_search_config(),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(
        std::fs::metadata(target).expect("unchanged target").len(),
        0
    );
}
