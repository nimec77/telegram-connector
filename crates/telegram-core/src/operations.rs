//! Account-scoped operations with admission, absolute deadlines and cancellation.

use crate::error::Error;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{Instant, sleep_until};
pub use tokio_util::sync::CancellationToken;

mod batch;
mod channels;
#[doc(hidden)]
pub mod fanout;
mod history;
#[cfg(feature = "image-preview")]
mod media;
#[cfg(feature = "image-preview")]
#[doc(hidden)]
pub mod preview;
#[cfg(feature = "image-preview")]
pub use media::{
    JoinedMediaSource, MAX_PREVIEW_JPEG_BYTES, MediaPreview, MediaPreviewBatch, MediaPreviewItem,
    MediaPreviewRequest,
};
mod search;
mod sending;
mod validation;
pub use batch::BatchOperation;
pub(crate) use history::HistoryIdBounds;
pub use history::{HistoryIdPage, HistoryIdRange, HistoryOperation, HistoryScope};
pub use search::{SearchOperation, SearchPage, SearchScope};
#[doc(hidden)]
pub use validation::{parse_channel_id, parse_channel_reference, validate_date_window};

/// Transport-supplied deadline and cancellation for one request.
pub struct OperationContext {
    deadline: Instant,
    cancellation: CancellationToken,
}

impl OperationContext {
    /// Start with the caller's absolute deadline and cancellation handle.
    pub fn new(deadline: Instant, cancellation: CancellationToken) -> Self {
        Self {
            deadline,
            cancellation,
        }
    }

    /// Absolute local deadline supplied by the transport.
    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Cancels the operation without changing the linked account.
    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    pub(crate) fn bound(&self, limit: Duration, name: &'static str) -> BoundedOperation<'_> {
        let started = Instant::now();
        let deadline = started
            .checked_add(limit)
            .unwrap_or(self.deadline)
            .min(self.deadline);
        let remaining = deadline.saturating_duration_since(started);
        let seconds = remaining
            .as_secs()
            .saturating_add(u64::from(remaining.subsec_nanos() != 0));
        BoundedOperation {
            deadline,
            cancellation: &self.cancellation,
            name,
            seconds,
        }
    }
}

pub(crate) struct BoundedOperation<'a> {
    deadline: Instant,
    cancellation: &'a CancellationToken,
    name: &'static str,
    seconds: u64,
}

impl BoundedOperation<'_> {
    fn timeout(&self) -> Error {
        Error::Timeout {
            operation: self.name.into(),
            secs: self.seconds,
        }
    }

    fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }

    pub(crate) fn check(&self) -> Result<(), Error> {
        if self.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if self.expired() {
            return Err(self.timeout());
        }
        Ok(())
    }

    pub(crate) async fn run<T>(
        &self,
        work: impl Future<Output = Result<T, Error>>,
    ) -> Result<T, Error> {
        self.check()?;
        tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => Err(Error::Cancelled),
            _ = sleep_until(self.deadline) => Err(self.timeout()),
            result = work => result,
        }
    }
}

/// Bounds applied in addition to the caller's request deadline.
#[derive(Clone, Copy)]
pub struct AccountLimits {
    /// Maximum total time for an admitted search, including channel resolution.
    pub search_timeout: Duration,
    /// Maximum total time for recent history and batch lookups.
    pub history_timeout: Duration,
}

impl Default for AccountLimits {
    fn default() -> Self {
        Self {
            search_timeout: Duration::from_secs(crate::config::defaults::default_search_secs()),
            history_timeout: Duration::from_secs(crate::config::defaults::default_history_secs()),
        }
    }
}

/// Charged operations using one account's client and shared limiter.
pub struct AccountOperations<T, R> {
    client: Arc<T>,
    limiter: Arc<R>,
    limits: AccountLimits,
}

impl<T: TelegramClientTrait, R: RateLimiterTrait> AccountOperations<T, R> {
    /// Share the supplied client's account budget and operation limits.
    pub fn new(client: Arc<T>, limiter: Arc<R>, limits: AccountLimits) -> Self {
        Self {
            client,
            limiter,
            limits,
        }
    }

    /// Create a search context when the transport has only a cancellation token.
    ///
    /// # Errors
    /// Returns an invalid-input error if the configured timeout cannot be represented.
    pub fn search_context(
        &self,
        cancellation: CancellationToken,
    ) -> Result<OperationContext, Error> {
        with_timeout(self.limits.search_timeout, cancellation)
    }

    /// Create a context bounded by the configured history and batch timeout.
    ///
    /// # Errors
    /// Returns an invalid-input error if the configured timeout cannot be represented.
    pub fn history_context(
        &self,
        cancellation: CancellationToken,
    ) -> Result<OperationContext, Error> {
        with_timeout(self.limits.history_timeout, cancellation)
    }
}

fn with_timeout(
    timeout: Duration,
    cancellation: CancellationToken,
) -> Result<OperationContext, Error> {
    let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
        Error::InvalidInput("operation timeout exceeds the local clock range".into())
    })?;
    Ok(OperationContext::new(deadline, cancellation))
}

#[cfg(test)]
#[path = "operations/tests/search.rs"]
mod tests;

#[cfg(test)]
#[path = "operations/tests/history_batch.rs"]
mod history_batch_tests;
