//! Serde default-value providers for config fields.
//!
//! Unit of `config` (LM-6). Referenced by `#[serde(default = "...")]`
//! attributes via the `use defaults::*;` glob in `config.rs`.

use super::{
    LimitsConfig, LoggingConfig, ObservabilityConfig, RateLimitConfig, SearchConfig, ServerConfig,
    TimeoutConfig, TranscriptionConfig,
};
use std::path::PathBuf;

pub(crate) fn default_session_file() -> PathBuf {
    let dirs = directories::ProjectDirs::from("", "", "telegram-connector")
        .expect("Could not determine config directory");
    dirs.config_dir().join("session.bin")
}

pub(crate) fn default_hours_back() -> u32 {
    48
}

pub(crate) fn default_max_results_default() -> u32 {
    20
}

pub(crate) fn default_max_results_limit() -> u32 {
    100
}

pub(crate) fn default_search_deadline_seconds() -> u64 {
    20
}

pub(crate) fn default_max_tokens() -> u32 {
    60
}

pub(crate) fn default_refill_rate() -> f64 {
    2.0
}

pub(crate) fn default_log_level() -> String {
    "info".to_string()
}

pub(crate) fn default_log_format() -> String {
    "compact".to_string()
}

pub(crate) fn default_file_enabled() -> bool {
    true
}

pub(crate) fn default_log_path() -> PathBuf {
    let dirs = directories::ProjectDirs::from("", "", "telegram-connector")
        .expect("Could not determine config directory");
    dirs.config_dir().join("logs")
}

pub(crate) fn default_max_log_days() -> u32 {
    7
}

pub(crate) fn default_shutdown_timeout() -> u64 {
    5
}

pub(crate) fn default_resolve_secs() -> u64 {
    30
}

pub(crate) fn default_history_secs() -> u64 {
    60
}

pub(crate) fn default_search_secs() -> u64 {
    120
}

pub(crate) fn default_download_secs() -> u64 {
    120
}

pub(crate) fn default_media_download_cost() -> u32 {
    3
}

pub(crate) fn default_transcription_cost() -> u32 {
    5
}

pub(crate) fn default_max_download_bytes() -> u64 {
    20 * 1024 * 1024
}

pub(crate) fn default_transcription_default_timeout() -> u32 {
    30
}

pub(crate) fn default_transcription_max_timeout() -> u32 {
    120
}

pub(crate) fn default_transcription_config() -> TranscriptionConfig {
    TranscriptionConfig {
        default_timeout_seconds: default_transcription_default_timeout(),
        max_timeout_seconds: default_transcription_max_timeout(),
    }
}

pub(crate) fn default_max_buffered_payload_bytes() -> usize {
    262_144
}

pub(crate) fn default_timeout_config() -> TimeoutConfig {
    TimeoutConfig {
        resolve_secs: default_resolve_secs(),
        history_secs: default_history_secs(),
        search_secs: default_search_secs(),
        download_secs: default_download_secs(),
    }
}

pub(crate) fn default_server_config() -> ServerConfig {
    ServerConfig {
        shutdown_timeout_seconds: default_shutdown_timeout(),
    }
}

pub(crate) fn default_search_config() -> SearchConfig {
    SearchConfig {
        default_hours_back: default_hours_back(),
        max_results_default: default_max_results_default(),
        max_results_limit: default_max_results_limit(),
        deadline_seconds: default_search_deadline_seconds(),
    }
}

pub(crate) fn default_rate_limit_config() -> RateLimitConfig {
    RateLimitConfig {
        max_tokens: default_max_tokens(),
        refill_rate: default_refill_rate(),
        media_download_cost: default_media_download_cost(),
        transcription_cost: default_transcription_cost(),
    }
}

pub(crate) fn default_logging_config() -> LoggingConfig {
    LoggingConfig {
        level: default_log_level(),
        format: default_log_format(),
        file_enabled: default_file_enabled(),
        file_path: default_log_path(),
        max_log_days: default_max_log_days(),
    }
}

pub(crate) fn default_slow_write_threshold_ms() -> u64 {
    500
}

pub(crate) fn default_response_buffer_size() -> usize {
    10
}

pub(crate) fn default_observability_config() -> ObservabilityConfig {
    ObservabilityConfig::default()
}

pub(crate) fn default_response_byte_budget() -> u64 {
    40_000
}

pub(crate) fn default_media_batch_max_total_bytes() -> u64 {
    8 * 1024 * 1024
}

pub(crate) fn default_limits_config() -> LimitsConfig {
    LimitsConfig {
        response_byte_budget: default_response_byte_budget(),
        media_batch_max_total_bytes: default_media_batch_max_total_bytes(),
    }
}
