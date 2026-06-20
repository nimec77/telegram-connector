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

    /// Try to acquire tokens, return retry_after_seconds if insufficient
    fn try_acquire(&mut self, tokens: u32) -> Result<(), u64> {
        self.refill();

        let tokens_f64 = tokens as f64;
        if self.available_tokens >= tokens_f64 {
            self.available_tokens -= tokens_f64;
            Ok(())
        } else {
            // Calculate how long to wait for tokens to refill
            let tokens_needed = tokens_f64 - self.available_tokens;
            let retry_after = (tokens_needed / self.refill_rate).ceil() as u64;
            Err(retry_after)
        }
    }

    fn available(&self) -> f64 {
        self.available_tokens
    }
}

/// Rate limiter using token bucket algorithm
pub struct RateLimiter {
    bucket: Arc<Mutex<TokenBucket>>,
}

impl RateLimiter {
    /// Create a new rate limiter from configuration
    pub fn new(config: &RateLimitConfig) -> Self {
        let bucket = TokenBucket::new(config.max_tokens, config.refill_rate);
        Self {
            bucket: Arc::new(Mutex::new(bucket)),
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

    /// Get available tokens
    fn available_tokens(&self) -> f64;
}

#[async_trait::async_trait]
impl RateLimiterTrait for RateLimiter {
    async fn acquire(&self, tokens: u32) -> Result<(), Error> {
        let mut bucket = self.bucket.lock().unwrap();
        bucket
            .try_acquire(tokens)
            .map_err(|retry_after_seconds| Error::RateLimit {
                retry_after_seconds,
            })
    }

    fn available_tokens(&self) -> f64 {
        let mut bucket = self.bucket.lock().unwrap();
        bucket.refill();
        bucket.available()
    }
}

#[cfg(test)]
#[path = "rate_limiter/tests.rs"]
mod tests;
