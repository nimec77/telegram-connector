//! Upload-only pool lifecycle: queued binary parts cannot outlive the media lease.
use super::send_diagnostics::private_rpc;
use crate::sending::StagedUpload;
use grammers_client::Client;
use grammers_mtsender::{SenderPool, SenderPoolHandle};
use grammers_session::storages::SqliteSession;
use std::sync::Arc;

pub(super) struct UploadPool {
    pub client: Client,
    handle: SenderPoolHandle,
    runner: Option<tokio::task::JoinHandle<()>>,
    lease: Arc<StagedUpload>,
    runtime: tokio::runtime::Handle,
}
impl UploadPool {
    pub(super) fn start(
        session: Arc<SqliteSession>,
        api_id: i32,
        lease: Arc<StagedUpload>,
    ) -> Self {
        let pool = SenderPool::new(session, api_id);
        let handle = pool.handle.thin.clone();
        let client = Client::new(pool.handle);
        let runner = tokio::spawn(private_rpc(async move {
            pool.runner.run().await;
        }));
        Self::track(client, handle, runner, lease)
    }
    pub(super) fn track(
        client: Client,
        handle: SenderPoolHandle,
        runner: tokio::task::JoinHandle<()>,
        lease: Arc<StagedUpload>,
    ) -> Self {
        Self {
            client,
            handle,
            runner: Some(runner),
            lease,
            runtime: tokio::runtime::Handle::current(),
        }
    }
}
impl Drop for UploadPool {
    fn drop(&mut self) {
        // Quit closes connections and joins their tasks. Never abort the caller's
        // original pool, and never release binary admission on waiter cancellation.
        let _ = self.handle.quit();
        if let Some(runner) = self.runner.take() {
            let lease = Arc::clone(&self.lease);
            self.runtime.spawn(async move {
                let _ = runner.await;
                drop(lease);
            });
        }
    }
}

#[cfg(all(test, unix))]
#[path = "tests/upload_pool_tests.rs"]
mod tests;
