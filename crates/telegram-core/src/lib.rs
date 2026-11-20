//! Telegram account operations, session ownership, and domain data.

pub mod config;
pub mod deadline;
pub mod error;
pub mod link;
pub mod operations;
pub mod rate_limiter;
pub mod sending;
pub mod telegram;
pub mod timestamp;

pub use telegram::{TelegramClient, TelegramClientTrait};
pub use timestamp::UtcTimestamp;

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod test_helpers;
