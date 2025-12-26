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
        bucket.try_acquire(tokens).map_err(|retry_after_seconds| {
            Error::RateLimit {
                retry_after_seconds,
            }
        })
    }

    fn available_tokens(&self) -> f64 {
        let mut bucket = self.bucket.lock().unwrap();
        bucket.refill();
        bucket.available()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    fn test_config(max_tokens: u32, refill_rate: f64) -> RateLimitConfig {
        RateLimitConfig {
            max_tokens,
            refill_rate,
        }
    }

    // ========================================
    // Initialization Tests
    // ========================================

    #[test]
    fn new_limiter_has_max_tokens_available() {
        let config = test_config(50, 2.0);
        let limiter = RateLimiter::new(&config);
        assert_eq!(limiter.available_tokens(), 50.0);
    }

    #[test]
    fn available_tokens_returns_correct_initial_value() {
        let config = test_config(100, 5.0);
        let limiter = RateLimiter::new(&config);
        assert_eq!(limiter.available_tokens(), 100.0);
    }

    // ========================================
    // Acquire - Success Cases
    // ========================================

    #[tokio::test]
    async fn acquire_tokens_when_available_succeeds() {
        let config = test_config(50, 2.0);
        let limiter = RateLimiter::new(&config);

        let result = limiter.acquire(10).await;
        assert!(result.is_ok());
        let available = limiter.available_tokens();
        assert!(available >= 39.9 && available <= 40.1); // Allow for timing variance
    }

    #[tokio::test]
    async fn acquire_exactly_max_tokens_succeeds() {
        let config = test_config(50, 2.0);
        let limiter = RateLimiter::new(&config);

        let result = limiter.acquire(50).await;
        assert!(result.is_ok());
        let available = limiter.available_tokens();
        assert!(available >= 0.0 && available <= 0.1); // Allow for timing variance
    }

    #[tokio::test]
    async fn acquire_zero_tokens_is_noop() {
        let config = test_config(50, 2.0);
        let limiter = RateLimiter::new(&config);

        let result = limiter.acquire(0).await;
        assert!(result.is_ok());
        assert_eq!(limiter.available_tokens(), 50.0);
    }

    #[tokio::test]
    async fn multiple_acquires_reduce_tokens() {
        let config = test_config(50, 2.0);
        let limiter = RateLimiter::new(&config);

        limiter.acquire(10).await.unwrap();
        limiter.acquire(15).await.unwrap();
        limiter.acquire(5).await.unwrap();

        let available = limiter.available_tokens();
        assert!(available >= 19.9 && available <= 20.1); // Allow for timing variance
    }

    // ========================================
    // Acquire - Failure Cases
    // ========================================

    #[tokio::test]
    async fn acquire_more_than_available_fails() {
        let config = test_config(50, 2.0);
        let limiter = RateLimiter::new(&config);

        let result = limiter.acquire(60).await;
        assert!(result.is_err());

        match result {
            Err(Error::RateLimit {
                retry_after_seconds,
            }) => {
                // Should need to wait for 10 tokens at 2/sec = 5 seconds
                assert_eq!(retry_after_seconds, 5);
            }
            _ => panic!("Expected RateLimit error"),
        }
    }

    #[tokio::test]
    async fn multiple_acquires_eventually_deplete() {
        let config = test_config(50, 2.0);
        let limiter = RateLimiter::new(&config);

        // Deplete tokens
        limiter.acquire(20).await.unwrap();
        limiter.acquire(30).await.unwrap();

        // Next acquire should fail
        let result = limiter.acquire(5).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rate_limit_error_includes_retry_after() {
        let config = test_config(10, 5.0);
        let limiter = RateLimiter::new(&config);

        // Deplete all tokens
        limiter.acquire(10).await.unwrap();

        // Try to acquire more
        let result = limiter.acquire(20).await;
        match result {
            Err(Error::RateLimit {
                retry_after_seconds,
            }) => {
                // Need 20 tokens at 5/sec = 4 seconds
                assert_eq!(retry_after_seconds, 4);
            }
            _ => panic!("Expected RateLimit error with retry_after"),
        }
    }

    // ========================================
    // Refill Over Time Tests
    // ========================================

    #[tokio::test]
    async fn tokens_refill_after_waiting() {
        let config = test_config(50, 10.0); // 10 tokens/sec
        let limiter = RateLimiter::new(&config);

        // Deplete tokens
        limiter.acquire(50).await.unwrap();
        let available = limiter.available_tokens();
        assert!(available >= 0.0 && available <= 0.1); // Near zero with timing variance

        // Wait 1 second - should refill 10 tokens
        sleep(Duration::from_secs(1)).await;
        let available = limiter.available_tokens();
        assert!(available >= 9.0 && available <= 11.0); // Allow for timing variance
    }

    #[tokio::test]
    async fn refill_does_not_exceed_max_tokens() {
        let config = test_config(50, 10.0);
        let limiter = RateLimiter::new(&config);

        // Start with full tokens
        assert_eq!(limiter.available_tokens(), 50.0);

        // Wait 2 seconds - should still be capped at 50
        sleep(Duration::from_secs(2)).await;
        assert_eq!(limiter.available_tokens(), 50.0);
    }

    #[tokio::test]
    async fn partial_refill_works_correctly() {
        let config = test_config(100, 20.0); // 20 tokens/sec
        let limiter = RateLimiter::new(&config);

        // Use 50 tokens
        limiter.acquire(50).await.unwrap();
        let available = limiter.available_tokens();
        assert!(available >= 49.9 && available <= 50.1); // Allow for timing variance

        // Wait 0.5 seconds - should refill 10 tokens
        sleep(Duration::from_millis(500)).await;
        let available = limiter.available_tokens();
        assert!(available >= 59.0 && available <= 61.0); // 50 + 10 ± variance
    }

    #[tokio::test]
    async fn acquire_after_refill_succeeds() {
        let config = test_config(50, 10.0);
        let limiter = RateLimiter::new(&config);

        // Deplete tokens
        limiter.acquire(50).await.unwrap();

        // Wait for refill
        sleep(Duration::from_secs(1)).await;

        // Should be able to acquire refilled tokens
        let result = limiter.acquire(10).await;
        assert!(result.is_ok());
    }

    // ========================================
    // Edge Cases
    // ========================================

    #[tokio::test]
    async fn refill_rate_zero_never_refills() {
        let config = test_config(50, 0.0); // No refill
        let limiter = RateLimiter::new(&config);

        limiter.acquire(10).await.unwrap();
        sleep(Duration::from_secs(2)).await;

        // Should still have 40 tokens (no refill)
        assert_eq!(limiter.available_tokens(), 40.0);
    }

    #[tokio::test]
    async fn max_tokens_zero_always_rate_limited() {
        let config = test_config(0, 5.0);
        let limiter = RateLimiter::new(&config);

        let result = limiter.acquire(1).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn concurrent_acquires_are_thread_safe() {
        let config = test_config(100, 10.0);
        let limiter = Arc::new(RateLimiter::new(&config));

        let mut handles = vec![];
        for _ in 0..10 {
            let limiter_clone = Arc::clone(&limiter);
            let handle = tokio::spawn(async move {
                limiter_clone.acquire(10).await
            });
            handles.push(handle);
        }

        let mut successes = 0;
        for handle in handles {
            if handle.await.unwrap().is_ok() {
                successes += 1;
            }
        }

        // Should have exactly 10 successes (100 tokens / 10 per acquire)
        assert_eq!(successes, 10);
    }

    // ========================================
    // Property-Based Tests (using proptest)
    // ========================================

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_tokens_never_exceed_max(max_tokens in 1u32..1000, refill_rate in 0.1f64..100.0) {
            let config = test_config(max_tokens, refill_rate);
            let limiter = RateLimiter::new(&config);

            // Available tokens should never exceed max
            let available = limiter.available_tokens();
            assert!(available <= max_tokens as f64);
        }

        #[test]
        fn prop_tokens_never_negative(
            max_tokens in 1u32..1000,
            refill_rate in 0.1f64..100.0,
            acquire_amount in 1u32..100
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let config = test_config(max_tokens, refill_rate);
                let limiter = RateLimiter::new(&config);

                // Try to acquire
                let _ = limiter.acquire(acquire_amount).await;

                // Tokens should never be negative
                let available = limiter.available_tokens();
                assert!(available >= 0.0);
            });
        }

        #[test]
        fn prop_acquire_sequence_eventually_fails(max_tokens in 10u32..100) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let config = test_config(max_tokens, 0.0); // No refill
                let limiter = RateLimiter::new(&config);

                let mut total_acquired = 0;
                let mut failed = false;

                // Keep acquiring until we fail
                for _ in 0..200 {
                    if limiter.acquire(1).await.is_err() {
                        failed = true;
                        break;
                    }
                    total_acquired += 1;
                }

                // Should fail after max_tokens
                assert!(failed);
                assert_eq!(total_acquired, max_tokens);
            });
        }
    }

    // Note: prop_refill_eventually_succeeds was removed because it uses sleep()
    // which causes tests to hang/freeze. Refill behavior is already tested
    // by non-property tests: tokens_refill_after_waiting, acquire_after_refill_succeeds, etc.
}
