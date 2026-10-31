//! Account token-bucket admission, independent of protocol in-flight admission.
//!
//! No network operations or automatic retries occur here. The client trait does
//! not acquire this limiter; MCP entry points own validation/acquisition ordering.

use crate::config::RateLimitConfig;
use crate::error::{Error, StateUnavailable};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Token bucket for rate limiting
struct TokenBucket {
    max_tokens: f64,
    available_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl TokenBucket {
    fn new(max_tokens: u32, refill_rate: f64) -> Self {
        Self {
            max_tokens: max_tokens as f64,
            available_tokens: max_tokens as f64,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    /// Refill tokens based on elapsed time
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        let tokens_to_add = elapsed * self.refill_rate;
        self.available_tokens = (self.available_tokens + tokens_to_add).min(self.max_tokens);
        self.last_refill = now;
    }

    /// Try to acquire tokens; on failure returns (available, retry_after_seconds)
    fn try_acquire(&mut self, tokens: u32) -> Result<(), (f64, u64)> {
        self.refill();

        let tokens_f64 = tokens as f64;
        if self.available_tokens >= tokens_f64 {
            self.available_tokens -= tokens_f64;
            Ok(())
        } else {
            // Calculate how long to wait for tokens to refill
            let tokens_needed = tokens_f64 - self.available_tokens;
            let retry_after = (tokens_needed / self.refill_rate).ceil() as u64;
            Err((self.available_tokens, retry_after))
        }
    }

    /// Return previously-acquired tokens. Clamped at capacity, so returning
    /// more than was taken can never inflate the bucket.
    fn refund(&mut self, tokens: u32) {
        self.refill();
        self.available_tokens = (self.available_tokens + f64::from(tokens)).min(self.max_tokens);
    }

    fn available(&self) -> f64 {
        self.available_tokens
    }
}

/// Rate limiter using token bucket algorithm
pub struct RateLimiter {
    bucket: Arc<Mutex<TokenBucket>>,
    capacity: f64,
    refill_rate: f64,
}

impl RateLimiter {
    /// Create a full token bucket without I/O. Call [`RateLimitConfig::validate`]
    /// first for hand-built/deserialized configuration; construction does not
    /// validate or wait. Available-token reads refill against elapsed time.
    pub fn new(config: &RateLimitConfig) -> Self {
        let bucket = TokenBucket::new(config.max_tokens, config.refill_rate);
        Self {
            bucket: Arc::new(Mutex::new(bucket)),
            capacity: config.max_tokens as f64,
            refill_rate: config.refill_rate,
        }
    }

    /// Compatibility snapshot: returns zero if the bucket is unavailable.
    ///
    /// This conservatively advertises no spendable tokens, but loses the distinction
    /// between exhaustion and failure. Use [`Self::try_available_tokens`] for diagnostics.
    pub fn available_tokens(&self) -> f64 {
        self.try_available_tokens().unwrap_or(0.0)
    }

    /// Get the available tokens after refill.
    ///
    /// # Errors
    /// Returns [`StateUnavailable::RateLimiter`] if the bucket lock is poisoned.
    pub fn try_available_tokens(&self) -> Result<f64, StateUnavailable> {
        let mut bucket = self.bucket()?;
        bucket.refill();
        Ok(bucket.available())
    }

    fn bucket(&self) -> Result<std::sync::MutexGuard<'_, TokenBucket>, StateUnavailable> {
        self.bucket
            .lock()
            .map_err(|_| StateUnavailable::RateLimiter)
    }
}

/// Injectable account admission policy, deliberately unsealed for external limiters.
/// Custom implementations own waiting/storage behavior; the production bucket
/// refuses immediately rather than sleeping or retrying.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait RateLimiterTrait: Send + Sync {
    /// Acquire account tokens before any corresponding RPC.
    ///
    /// # Errors
    /// Production returns [`Error::RateLimit`] with a retry hint when tokens are
    /// insufficient, or [`Error::Unavailable`] when synchronized state is
    /// poisoned, including zero-token requests. Failed acquisition spends no
    /// tokens. No retry/wait is performed by the production implementation.
    async fn acquire(&self, tokens: u32) -> Result<(), Error>;

    /// Return tokens taken by an `acquire` whose work did not happen.
    ///
    /// Batch tools acquire pessimistically for every requested item, then
    /// refund the items that produced nothing, so admission control stays real
    /// while the net charge matches the work actually performed. Implementations may
    /// refuse best-effort refunds when synchronized state is unavailable; a refund
    /// must never reset an unavailable limiter to a usable bucket.
    fn refund(&self, tokens: u32);

    /// Get available tokens (legacy implementations may omit availability information).
    fn available_tokens(&self) -> f64;

    /// Fallible snapshot. The default preserves existing implementations; implementations
    /// with fallible storage should override it to report unavailable state.
    ///
    /// # Errors
    /// Returns the component failure when the implementation cannot read its state.
    fn try_available_tokens(&self) -> Result<f64, StateUnavailable> {
        Ok(self.available_tokens())
    }

    /// Bucket capacity (max tokens).
    fn capacity(&self) -> f64;

    /// Refill rate in tokens per second.
    fn refill_rate(&self) -> f64;
}

#[async_trait::async_trait]
impl RateLimiterTrait for RateLimiter {
    async fn acquire(&self, tokens: u32) -> Result<(), Error> {
        let mut bucket = self.bucket()?;
        bucket
            .try_acquire(tokens)
            .map_err(|(available, retry_after_seconds)| Error::RateLimit {
                retry_after_seconds,
                detail: format!(": requested {tokens} tokens, {available:.2} available"),
            })
    }

    fn refund(&self, tokens: u32) {
        // Best effort only: refusing a refund cannot reopen failed admission.
        if let Ok(mut bucket) = self.bucket() {
            bucket.refund(tokens);
        }
    }

    fn available_tokens(&self) -> f64 {
        RateLimiter::available_tokens(self)
    }

    fn try_available_tokens(&self) -> Result<f64, StateUnavailable> {
        RateLimiter::try_available_tokens(self)
    }

    fn capacity(&self) -> f64 {
        self.capacity
    }

    fn refill_rate(&self) -> f64 {
        self.refill_rate
    }
}

#[cfg(test)]
#[path = "rate_limiter/tests.rs"]
mod tests;
