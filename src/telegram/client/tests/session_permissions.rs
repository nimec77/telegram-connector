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

#[tokio::test]
async fn review_session_rejects_a_private_parent_below_an_unsafe_ancestor() {
    let root = tempfile::tempdir().expect("disposable root");
    let unsafe_parent = root.path().join("shared");
    let private = unsafe_parent.join("account");
    std::fs::create_dir_all(&private).expect("directories");
    std::fs::set_permissions(&unsafe_parent, std::fs::Permissions::from_mode(0o777))
        .expect("unsafe ancestor");
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700))
        .expect("private leaf");
    let path = private.join("session.db");
    let result = TelegramClient::new(
        &config(&path),
        &crate::config::defaults::default_search_config(),
    )
    .await;
    assert!(result.is_err(), "accepted replaceable session ancestry");
    assert!(
        !path.exists(),
        "created a database before rejecting ancestry"
    );
}

#[cfg(target_os = "macos")]
fn grant_acl(path: &std::path::Path, rule: &str) {
    let status = std::process::Command::new("/bin/chmod")
        .args(["+a", rule])
        .arg(path)
        .status()
        .expect("ACL fixture");
    assert!(status.success());
    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(0o600 | if path.is_dir() { 0o100 } else { 0 }),
    )
    .expect("private mode despite ACL");
}
#[cfg(target_os = "macos")]
fn has_allow_acl(path: &std::path::Path) -> bool {
    let output = std::process::Command::new("/bin/ls")
        .args(["-lde", "--"])
        .arg(path)
        .env("LC_ALL", "C")
        .output()
        .expect("ACL inspection");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .any(|line| line.contains(" allow "))
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn review_session_database_wal_and_shm_cannot_retain_read_granting_acls() {
    let root = tempfile::tempdir().expect("disposable root");
    let path = root.path().join("session.db");
    let _client = TelegramClient::new(
        &config(&path),
        &crate::config::defaults::default_search_config(),
    )
    .await
    .expect("offline client");
    let database = libsql::Builder::new_local(&path)
        .build()
        .await
        .expect("database");
    let connection = database.connect().expect("connection");
    connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE acl_fixture (value INTEGER); INSERT INTO acl_fixture VALUES (1);").await.expect("valid WAL");
    let paths: Vec<_> = ["", "-wal", "-shm"]
        .iter()
        .map(|suffix| {
            let mut name = path.as_os_str().to_os_string();
            name.push(suffix);
            std::path::PathBuf::from(name)
        })
        .collect();
    for entry in &paths {
        grant_acl(entry, "everyone allow read");
        assert!(has_allow_acl(entry));
    }
    let _second = TelegramClient::new(
        &config(&path),
        &crate::config::defaults::default_search_config(),
    )
    .await
    .expect("private reopen");
    for entry in paths {
        assert!(!has_allow_acl(&entry), "read ACL survived on session entry");
    }
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn review_session_rejects_access_granting_ancestor_acls() {
    let root = tempfile::tempdir().expect("disposable root");
    let parent = root.path().join("granted");
    std::fs::create_dir(&parent).expect("parent");
    grant_acl(
        &parent,
        "everyone allow read,write,search,delete_child,file_inherit,directory_inherit",
    );
    let private = parent.join("account");
    std::fs::create_dir(&private).expect("leaf");
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700))
        .expect("private leaf");
    let path = private.join("session.db");
    let result = TelegramClient::new(
        &config(&path),
        &crate::config::defaults::default_search_config(),
    )
    .await;
    assert!(result.is_err(), "accepted access-granting ancestor ACL");
    assert!(!path.exists());
    assert!(has_allow_acl(&parent), "existing parent ACL was modified");
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn final_review_shared_sticky_directory_cannot_be_the_immediate_session_parent() {
    let placeholder =
        tempfile::NamedTempFile::new_in("/private/tmp").expect("unique disposable name");
    let path = placeholder.path().to_path_buf();
    placeholder.close().expect("leave absent database");
    let result = crate::telegram::session_storage::open_private_session(&path).await;
    let accepted = result.is_ok();
    drop(result);
    let created = path.exists();
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut entry = path.as_os_str().to_os_string();
        entry.push(suffix);
        let _ = std::fs::remove_file(std::path::PathBuf::from(entry));
    }
    assert!(!accepted, "accepted an immediate shared sticky parent");
    assert!(
        !created,
        "created the database before rejecting the shared leaf"
    );
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn final_review_private_leaf_below_a_shared_sticky_ancestor_still_opens() {
    let directory = tempfile::tempdir_in("/private/tmp").expect("private disposable leaf");
    let path = directory.path().join("session.db");
    let session = crate::telegram::session_storage::open_private_session(&path)
        .await
        .expect("protected private leaf");
    assert_eq!(
        std::fs::metadata(&path)
            .expect("database")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    drop(session);
}
