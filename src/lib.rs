//! Telegram domain types, an injectable MTProto client, and an MCP stdio server.
//!
//! [`TelegramClient`] owns the real Telegram session; [`TelegramClientTrait`] and
//! [`RateLimiterTrait`] let embedders substitute their own implementations.
//! [`mcp::McpServer`] validates tool inputs and applies charging, pagination and
//! output budgets above that seam. Typed [`Error`] values become safe versioned
//! tool errors on the wire; direct Rust convenience adapters retain string errors.
//! Diagnostics use tracing/stderr;
//! stdout belongs exclusively to MCP frames while serving.
//!
//! # Inert domain construction
//!
//! No account, filesystem or network access is needed for domain values and links.
//! Constructors validate input, and domain ID/name deserialization delegates to those
//! constructors. Directly deserialized configuration still needs operational validation.
//!
//! ```
//! use telegram_connector::{Error, MessageLink};
//! use telegram_connector::telegram::{ChannelId, MessageId, Username};
//!
//! let channel = ChannelId::new(123)?;
//! let message = MessageId::new(456)?;
//! let username = Username::new("example_channel")?;
//! assert!(matches!(ChannelId::new(0), Err(Error::InvalidInput(_))));
//! let link = MessageLink::new(channel, message, Some(username.as_str()));
//! assert_eq!(link.https_link, "https://t.me/example_channel/456");
//! # Ok::<(), Error>(())
//! ```
//!
//! # Inert configuration and server composition
//!
//! Parsing in memory does not open a session or expand environment variables.
//! [`Config::load_from`] additionally performs expansion and noncredential validation.
//! Validate manually when using serde directly, then pass configuration into builders.
//!
//! ```
//! use std::sync::Arc;
//! use telegram_connector::{Config, RateLimiter, RateLimiterTrait, TelegramClientTrait};
//! use telegram_connector::mcp::McpServer;
//!
//! let config: Config = toml::from_str("[telegram]\napi_id = 123")?;
//! config.rate_limiting.validate()?;
//! config.server.validate()?;
//! let limiter = RateLimiter::new(&config.rate_limiting);
//! assert_eq!(limiter.capacity(), f64::from(config.rate_limiting.max_tokens));
//! assert!(limiter.try_available_tokens()? > 0.0);
//!
//! // Compiled against the same public paths available to an external embedder.
//! fn compose<T: TelegramClientTrait + 'static>(client: Arc<T>, config: &Config)
//!     -> McpServer<T, RateLimiter>
//! {
//!     McpServer::new(client, Arc::new(RateLimiter::new(&config.rate_limiting)))
//!         .with_observability(&config.observability)
//!         .with_media_download_cost(config.rate_limiting.media_download_cost)
//!         .with_transcription_cost(config.rate_limiting.transcription_cost)
//!         .with_transcription_timeouts(
//!             config.transcription.default_timeout_seconds,
//!             config.transcription.max_timeout_seconds,
//!         )
//!         .with_response_byte_budget(config.limits.response_byte_budget)
//!         .with_media_batch_max_total_bytes(config.limits.media_batch_max_total_bytes)
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Run `cargo run --offline --locked --example inert_server` to execute this
//! composition with an example-local inert client and call the status tool.
//! Public surface policy and compatibility inventory: `docs/rust-api.md` in the
//! repository. Runtime shutdown and bootstrap limits: `docs/stdio-runtime.md`.

pub mod cli;
#[cfg(test)]
extern crate self as telegram_connector;
pub mod config;
pub mod error;
pub mod link;
pub mod logging;
pub mod mcp;
pub mod rate_limiter;
pub mod sending;
pub mod telegram;

// Test helpers (only compiled in test builds)
#[cfg(test)]
pub mod test_helpers;

pub use cli::Cli;
pub use config::{Config, MediaArtifactsConfig, ObservabilityConfig, ServerConfig};
pub use error::Error;
pub use link::MessageLink;
pub use rate_limiter::{RateLimiter, RateLimiterTrait};
pub use telegram::{TelegramClient, TelegramClientTrait};
