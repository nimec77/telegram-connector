//! Configuration data and the filesystem/environment loading boundary.
//!
//! [`Config::load_from`] expands environment references and validates operational
//! limits. Direct serde deserialization only supplies defaults and decodes fields;
//! callers using it must invoke the relevant validation methods themselves.
#![warn(missing_docs)]

pub(crate) mod defaults;
mod env;
mod loading;
mod types;
mod validation;

pub use types::{
    Config, LimitsConfig, LoggingConfig, MediaArtifactsConfig, ObservabilityConfig, OutputFormat,
    RateLimitConfig, SearchConfig, ServerConfig, TelegramConfig, TimeoutConfig,
    TranscriptionConfig,
};
pub(crate) use validation::MIN_IMAGE_BASE64_BYTES;

#[cfg(test)]
#[path = "config/tests.rs"]
mod tests;
