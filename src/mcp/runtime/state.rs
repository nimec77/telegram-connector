//! Bounded request ownership shared by admission, handlers, and the writer.
use crate::config::ServerConfig;
use crate::mcp::observability::SessionMetrics;
use rmcp::model::{ErrorCode, ErrorData, ProgressToken, RequestId};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, Notify, watch};
use tokio::time::Instant;

pub(super) struct Operation {
    pub token: Option<ProgressToken>,
    pub cancelled: watch::Sender<bool>,
    pub last_progress: Mutex<f64>,
}
impl Operation {
    pub async fn cancelled(&self) {
        let mut rx = self.cancelled.subscribe();
        let _ = rx.wait_for(|cancelled| *cancelled).await;
    }
}

#[derive(Clone, Copy)]
pub(super) enum Phase {
    Running,
    Draining(Instant),
    Stopped,
}

pub(super) struct State {
    pub config: ServerConfig,
    pub metrics: Arc<SessionMetrics>,
    pub operations: Mutex<HashMap<RequestId, Arc<Operation>>>,
    pub phase: watch::Sender<Phase>,
    admission_closed: AtomicBool,
    pub changed: Notify,
    pub write_gate: Mutex<()>,
    pub failure: Mutex<Option<std::io::Error>>,
}
impl State {
    pub fn new(config: ServerConfig, metrics: Arc<SessionMetrics>) -> Arc<Self> {
        Arc::new(Self {
            config,
            metrics,
            operations: Mutex::new(HashMap::new()),
            phase: watch::channel(Phase::Running).0,
            admission_closed: AtomicBool::new(false),
            changed: Notify::new(),
            write_gate: Mutex::new(()),
            failure: Mutex::new(None),
        })
    }
    pub fn drain(&self) {
        self.admission_closed.store(true, Ordering::Release);
        self.input_eof();
        self.changed.notify_waiters();
    }
    // Physical EOF starts the deadline independently of a blocked bootstrap
    // write. Already-buffered input remains admissible until consumed EOF.
    pub fn input_eof(&self) {
        self.phase.send_if_modified(|phase| {
            if matches!(phase, Phase::Running) {
                *phase = Phase::Draining(
                    Instant::now() + Duration::from_secs(self.config.shutdown_timeout_seconds),
                );
                true
            } else {
                false
            }
        });
    }
    pub fn stopping(&self) -> bool {
        !matches!(*self.phase.borrow(), Phase::Running)
    }
    pub async fn admit(
        &self,
        id: RequestId,
        token: Option<ProgressToken>,
    ) -> Result<(), ErrorData> {
        let mut entries = self.operations.lock().await;
        if self.admission_closed.load(Ordering::Acquire)
            || matches!(*self.phase.borrow(), Phase::Stopped)
            || entries.len() >= self.config.max_in_flight_requests
        {
            return Err(ErrorData::new(
                ErrorCode(1001),
                "Server is busy or draining",
                None,
            ));
        }
        if entries.contains_key(&id) {
            return Err(ErrorData::invalid_request(
                "Request ID is already active",
                None,
            ));
        }
        if let Some(token) = &token
            && entries
                .values()
                .any(|entry| entry.token.as_ref() == Some(token))
        {
            return Err(ErrorData::invalid_params(
                "Progress token is already active",
                None,
            ));
        }
        entries.insert(
            id,
            Arc::new(Operation {
                token,
                cancelled: watch::channel(false).0,
                last_progress: Mutex::new(f64::NEG_INFINITY),
            }),
        );
        Ok(())
    }
    pub async fn finish(&self, id: &RequestId) {
        if let Some(operation) = self.operations.lock().await.remove(id) {
            // Close queued notification futures, including those that already
            // captured this operation before its terminal response was written.
            operation.cancelled.send_replace(true);
        }
        self.changed.notify_waiters();
    }
    pub async fn cancel(&self, id: &RequestId) {
        if let Some(operation) = self.operations.lock().await.remove(id) {
            operation.cancelled.send_replace(true);
            self.metrics
                .discard_request(&id.clone().into_json_value().to_string());
        }
        self.changed.notify_waiters();
    }
    pub async fn fail(&self, error: std::io::Error) {
        let mut failure = self.failure.lock().await;
        if failure.is_none() {
            *failure = Some(error);
        }
        drop(failure);
        self.stop().await;
    }
    pub async fn stop(&self) {
        self.admission_closed.store(true, Ordering::Release);
        self.phase.send_replace(Phase::Stopped);
        for (id, operation) in self.operations.lock().await.drain() {
            operation.cancelled.send_replace(true);
            self.metrics
                .discard_request(&id.into_json_value().to_string());
        }
        self.changed.notify_waiters();
    }
    pub async fn stopped(&self) {
        let mut rx = self.phase.subscribe();
        loop {
            let phase = *rx.borrow_and_update();
            match phase {
                Phase::Stopped => return,
                Phase::Draining(deadline) => {
                    tokio::select! {
                        _ = tokio::time::sleep_until(deadline) => { self.stop().await; return; }
                        result = rx.changed() => { if result.is_err() { return; } }
                    }
                }
                Phase::Running => {
                    if rx.changed().await.is_err() {
                        return;
                    }
                }
            }
        }
    }
    pub async fn drained(&self) {
        let mut rx = self.phase.subscribe();
        loop {
            let changed = self.changed.notified();
            if self.admission_closed.load(Ordering::Acquire)
                && self.operations.lock().await.is_empty()
            {
                return;
            }
            tokio::select! {
                _ = changed => {}
                _ = self.stopped() => return,
                _ = rx.changed() => {}
            }
        }
    }
}
