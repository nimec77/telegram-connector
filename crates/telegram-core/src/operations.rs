//! Account-scoped operations with admission, absolute deadlines and cancellation.

use crate::error::Error;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{Instant, sleep_until};
pub use tokio_util::sync::CancellationToken;

#[doc(hidden)]
pub mod fanout;
mod search;
mod validation;
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

    fn bound(&self, limit: Duration, name: &'static str) -> BoundedOperation<'_> {
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

struct BoundedOperation<'a> {
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

    async fn run<T>(&self, work: impl Future<Output = Result<T, Error>>) -> Result<T, Error> {
        if self.cancellation.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if self.expired() {
            return Err(self.timeout());
        }
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
}

impl Default for AccountLimits {
    fn default() -> Self {
        Self {
            search_timeout: Duration::from_secs(crate::config::defaults::default_search_secs()),
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
        let deadline = Instant::now()
            .checked_add(self.limits.search_timeout)
            .ok_or_else(|| {
                Error::InvalidInput("search timeout exceeds the local clock range".into())
            })?;
        Ok(OperationContext::new(deadline, cancellation))
    }
}

#[cfg(test)]
#[path = "operations/tests/search.rs"]
mod tests;
