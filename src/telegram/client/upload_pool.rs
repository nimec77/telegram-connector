//! Upload-only runtime ownership: all vendor tasks die before binary admission releases.
use super::send_diagnostics::private_rpc;
use crate::sending::StagedUpload;
use crate::telegram::sending::SendDeadline;
use crate::telegram::types::sending::{SendFailure, SendFailureKind};
use grammers_client::Client;
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

pub(super) struct UploadPool {
    pub client: Client,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

// Keep the lease behind the runtime even if the vendor driver unwinds. A
// current-thread runtime has no concurrently executing async worker: its shutdown
// synchronously destroys its owned tasks and queues before this guard releases.
struct RuntimeLease {
    runtime: Option<tokio::runtime::Runtime>,
    lease: Option<Arc<StagedUpload>>,
    origin: tokio::runtime::Handle,
}
impl Drop for RuntimeLease {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(Duration::ZERO);
        }
        if let Some(lease) = self.lease.take() {
            // Snapshot destruction can schedule owned filesystem cleanup. Perform
            // it on the original executor, not a foreign thread after its runtime died.
            self.origin.spawn_blocking(move || drop(lease));
        }
    }
}

impl UploadPool {
    pub(super) fn start(
        session: Arc<SqliteSession>,
        api_id: i32,
        lease: Arc<StagedUpload>,
        deadline: SendDeadline,
    ) -> Result<Self, SendFailure> {
        let pool = SenderPool::new(session, api_id);
        let client = Client::new(pool.handle);
        Self::supervise(client, lease, deadline.instant(), pool.runner.run())
    }
    pub(super) fn supervise<F>(
        client: Client,
        lease: Arc<StagedUpload>,
        deadline: tokio::time::Instant,
        driver: F,
    ) -> Result<Self, SendFailure>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let unavailable = || SendFailure::not_sent(SendFailureKind::Unavailable, None);
        if tokio::time::Instant::now() >= deadline {
            return Err(SendFailure::not_sent(SendFailureKind::Timeout, None));
        }
        let origin = tokio::runtime::Handle::try_current().map_err(|_| unavailable())?;
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let at = deadline.into_std();
        std::thread::Builder::new()
            .name("telegram-upload".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .ok();
                let scope = RuntimeLease {
                    runtime,
                    lease: Some(lease),
                    origin,
                };
                if let Some(runtime) = &scope.runtime {
                    runtime.block_on(async move {
                        tokio::select! {
                            biased;
                            _=stopped=>{},
                            _=tokio::time::sleep_until(tokio::time::Instant::from_std(at))=>{},
                            _=private_rpc(driver)=>{},
                        }
                    });
                } else {
                    // A failed runtime build must destroy any already queued part
                    // bodies in the unpolled driver before the lease can be released.
                    drop(driver);
                }
                // scope tears down the complete task set before releasing the lease.
            })
            .map_err(|_| unavailable())?;
        Ok(Self {
            client,
            stop: Some(stop),
        })
    }
}
impl Drop for UploadPool {
    fn drop(&mut self) {
        // Interrupt even an inline stalled connection/InitConnection future.
        // Never abort or change the caller's original read/final-send pool.
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

#[cfg(all(test, unix))]
#[path = "tests/upload_pool_tests.rs"]
mod tests;
