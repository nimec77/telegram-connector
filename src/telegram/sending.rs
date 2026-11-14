//! One absolute outgoing-operation deadline and fixed safe stage diagnostics.

use crate::telegram::types::sending::{SendFailure, SendFailureKind};
use std::future::Future;
use std::time::Duration;
use tokio::time::Instant;

/// A budget started before staging, preserved through every network stage.
#[derive(Debug, Clone, Copy)]
pub struct SendDeadline {
    at: Instant,
}

/// Stages after final dispatch cannot claim that expiry prevented delivery.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Pre-dispatch stages are integrated in subsequent tasks"
    )
)]
pub(crate) enum SendStage {
    LocalPreparation,
    Resolve,
    ValidateTargets,
    Upload,
    FinalSend,
    Receipt,
}

impl SendDeadline {
    /// Start a finite nonzero budget. Copying this value never restarts it.
    ///
    /// # Errors
    /// Rejects zero and durations that cannot be represented by the clock.
    pub fn new(timeout_secs: u64) -> Result<Self, SendFailure> {
        let at = Instant::now()
            .checked_add(Duration::from_secs(timeout_secs))
            .filter(|_| timeout_secs != 0)
            .ok_or_else(|| SendFailure::not_sent(SendFailureKind::InvalidInput, None))?;
        Ok(Self { at })
    }
    /// Absolute Tokio deadline.
    pub fn instant(self) -> Instant {
        self.at
    }
    /// Expiry is inclusive; a result exactly at the deadline is too late.
    pub fn expired(self) -> bool {
        Instant::now() >= self.at
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "Send stages integrated in subsequent tasks")
    )]
    pub(crate) async fn run<T>(
        self,
        stage: SendStage,
        future: impl Future<Output = Result<T, SendFailure>>,
    ) -> Result<T, SendFailure> {
        let failure = || match stage {
            SendStage::FinalSend | SendStage::Receipt => SendFailure::unknown(),
            _ => SendFailure::not_sent(SendFailureKind::Timeout, None),
        };
        let mut future = std::pin::pin!(future);
        let guarded = std::future::poll_fn(|cx| {
            if self.expired() {
                std::task::Poll::Ready(Err(failure()))
            } else {
                future.as_mut().poll(cx)
            }
        });
        let result = tokio::time::timeout_at(self.at, guarded).await;
        if self.expired() {
            return Err(failure());
        }
        result.unwrap_or_else(|_| Err(failure()))
    }
}
