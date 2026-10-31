//! An actual grammers client whose runner is retained but never polled: no I/O.
use super::*;
use grammers_mtsender::SenderPool;

pub(crate) async fn stalled_client() -> (TelegramClient, impl Send, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("session.db");
    let session = Arc::new(SqliteSession::open(&path).await.expect("temporary session"));
    let pool = SenderPool::new(Arc::clone(&session), 1);
    let client = TelegramClient {
        client: Client::new(pool.handle),
        session,
        session_path: path,
        timeouts: TimeoutConfig {
            resolve_secs: 2,
            history_secs: 5,
            search_secs: 5,
            download_secs: 5,
        },
        max_download_bytes: 1024,
        search_deadline_secs: 20,
        account_status: tokio::sync::RwLock::new(AccountStatus::default()),
        _runner_handle: tokio::spawn(async {}),
    };
    (client, pool.runner, dir)
}
