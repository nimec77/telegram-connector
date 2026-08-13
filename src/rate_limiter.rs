use crate::config::RateLimitConfig;
use crate::error::Error;
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
    /// Create a new rate limiter from configuration
    pub fn new(config: &RateLimitConfig) -> Self {
        let bucket = TokenBucket::new(config.max_tokens, config.refill_rate);
        Self {
            bucket: Arc::new(Mutex::new(bucket)),
            capacity: config.max_tokens as f64,
            refill_rate: config.refill_rate,
        }
    }

    /// Get the number of available tokens (after refill)
    pub fn available_tokens(&self) -> f64 {
        let mut bucket = self.bucket.lock().unwrap();
        bucket.refill();
        bucket.available()
    }
}

/// Trait for rate limiting (allows mocking in tests)
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait RateLimiterTrait: Send + Sync {
    /// Acquire tokens, returning error if rate limit exceeded
    async fn acquire(&self, tokens: u32) -> Result<(), Error>;

    /// Return tokens taken by an `acquire` whose work did not happen.
    ///
    /// Batch tools acquire pessimistically for every requested item, then
    /// refund the items that produced nothing, so admission control stays real
    /// while the net charge matches the work actually performed.
    fn refund(&self, tokens: u32);

    /// Get available tokens
    fn available_tokens(&self) -> f64;

    /// Bucket capacity (max tokens).
    fn capacity(&self) -> f64;

    /// Refill rate in tokens per second.
    fn refill_rate(&self) -> f64;
}

#[async_trait::async_trait]
impl RateLimiterTrait for RateLimiter {
    async fn acquire(&self, tokens: u32) -> Result<(), Error> {
        let mut bucket = self.bucket.lock().unwrap();
        bucket
            .try_acquire(tokens)
            .map_err(|(available, retry_after_seconds)| Error::RateLimit {
                retry_after_seconds,
                detail: format!(": requested {tokens} tokens, {available:.2} available"),
            })
    }

    fn refund(&self, tokens: u32) {
        let mut bucket = self.bucket.lock().unwrap();
        bucket.refund(tokens);
    }

    fn available_tokens(&self) -> f64 {
        let mut bucket = self.bucket.lock().unwrap();
        bucket.refill();
        bucket.available()
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
