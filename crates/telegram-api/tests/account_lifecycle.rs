use std::time::Duration;
use telegram_api::service::{AccountStore, LinkState, RetirementOutcome};

#[tokio::test]
async fn retired_generation_cleanup_cannot_touch_relinked_session() {
    let root = tempfile::tempdir().expect("temporary data root");
    let store = AccountStore::open(root.path()).await.expect("storage");
    let old = store
        .start("issuer", "owner")
        .await
        .expect("old generation");
    store.activate(&old, 17).await.expect("old link");
    std::fs::write(old.session_file(), b"old").expect("old session fixture");
    drop(store);
    let child = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args(["--ignored", "--exact", "account_retirement_child"])
        .env("TELEGRAM_TEST_RETIRED_ROOT", root.path())
        .status()
        .expect("retirement process");
    assert!(child.success());

    let store = AccountStore::open(root.path()).await.expect("reopen");
    assert_eq!(
        store.status("issuer", "owner").await.expect("status"),
        LinkState::RevocationPending
    );
    let new = store
        .start("issuer", "owner")
        .await
        .expect("new generation");
    store.activate(&new, 17).await.expect("new link");
    std::fs::write(new.session_file(), b"new").expect("new session fixture");

    let pending = store.pending_retirements().await.expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].session_file(), old.session_file());
    assert!(!pending[0].expired_at(100 + Duration::from_secs(86_399).as_secs()));
    assert!(pending[0].expired_at(100 + Duration::from_secs(86_400).as_secs()));
    store
        .finish_retirement(&pending[0], RetirementOutcome::ManualRevocation)
        .await
        .expect("purge retired session");
    assert!(!old.session_file().exists());
    assert_eq!(
        std::fs::read(new.session_file()).expect("new session"),
        b"new"
    );
    assert_eq!(
        store.status("issuer", "owner").await.expect("status"),
        LinkState::LinkedWithManualRevocation
    );
}

#[tokio::test]
async fn single_root_lock_precedes_session_open() {
    let root = tempfile::tempdir().expect("temporary data root");
    let store = AccountStore::open(root.path()).await.expect("first owner");
    let child = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args(["--ignored", "--exact", "account_root_child"])
        .env("TELEGRAM_TEST_LOCKED_ROOT", root.path())
        .status()
        .expect("run second service process");
    assert!(child.success());
    drop(store);
    AccountStore::open(root.path())
        .await
        .expect("lock released");
}

#[tokio::test]
async fn ownership_conflicts_and_stale_grants_are_transactional() {
    let root = tempfile::tempdir().expect("temporary data root");
    let store = AccountStore::open(root.path()).await.expect("storage");
    let first = store
        .start("issuer", "first")
        .await
        .expect("first generation");
    let second = store
        .start("issuer", "second")
        .await
        .expect("second generation");
    store.activate(&first, 17).await.expect("first link");
    assert!(store.activate(&second, 17).await.is_err());
    assert!(store.start("issuer", "first").await.is_err());
    let proposal = telegram_api::contracts::GrantUpdate {
        expected_revision: 0,
        read: telegram_api::contracts::ReadGrant::Channels(vec!["42".into()]),
        send_enabled: false,
    };
    let current = store
        .update_grant(&first, "registered", proposal.clone())
        .await
        .expect("first grant");
    assert_eq!(current.revision, 1);
    assert!(
        store
            .update_grant(&first, "registered", proposal)
            .await
            .is_err()
    );
    store.retire("issuer", "first", 100).await.expect("retire");
    assert!(store.grant(&first, "registered").await.is_err());
}

#[tokio::test]
#[ignore = "re-executed by single_root_lock_precedes_session_open"]
async fn account_root_child() {
    let path = std::env::var_os("TELEGRAM_TEST_LOCKED_ROOT").expect("disposable root");
    assert!(
        AccountStore::open(std::path::Path::new(&path))
            .await
            .is_err()
    );
}

#[tokio::test]
#[ignore = "re-executed by retired_generation_cleanup_cannot_touch_relinked_session"]
async fn account_retirement_child() {
    let root = std::env::var_os("TELEGRAM_TEST_RETIRED_ROOT").expect("disposable root");
    let store = AccountStore::open(std::path::Path::new(&root))
        .await
        .expect("open old link");
    assert!(
        store
            .retire("issuer", "owner", 100)
            .await
            .expect("retirement commit")
            .is_some()
    );
    std::process::exit(0);
}
