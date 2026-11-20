//! Compatibility exports for account admission and its injectable seam.

pub use telegram_core::rate_limiter::{RateLimiter, RateLimiterTrait};

#[cfg(test)]
pub use telegram_core::rate_limiter::MockRateLimiterTrait;

#[cfg(test)]
#[path = "rate_limiter_tests.rs"]
mod tests;
