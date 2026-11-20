//! Configuration representation, serde defaults and credential accessors.

use super::{RateLimitConfig, SearchConfig, TelegramConfig, defaults};
use serde::Deserialize;
use std::path::PathBuf;

/// Complete connector configuration; secrets remain wrapped in `SecretString`.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Explicit opt-in outgoing message/media policy.
    #[serde(default)]
    pub sending: SendingConfig,
    /// Optional private processed-preview storage.
    #[serde(default)]
    pub media_artifacts: MediaArtifactsConfig,
    /// MTProto session, credentials, download bounds and RPC budgets.
    pub telegram: TelegramConfig,
    /// Search accumulation budget and partial-result policy.
    #[serde(default = "defaults::default_search_config")]
    pub search: SearchConfig,
    /// Account token bucket and weighted operation costs.
    #[serde(default)]
    pub rate_limiting: RateLimitConfig,
    /// Tracing output and daily file retention settings.
    #[serde(default = "defaults::default_logging_config")]
    pub logging: LoggingConfig,
    /// Input, admission and graceful-drain limits.
    #[serde(default = "defaults::default_server_config")]
    pub server: ServerConfig,
    /// Response recovery storage and write diagnostics.
    #[serde(default = "defaults::default_observability_config")]
    pub observability: ObservabilityConfig,
    /// Default and maximum transcription request budgets.
    #[serde(default = "defaults::default_transcription_config")]
    pub transcription: TranscriptionConfig,
    /// MCP message-stream and image payload budgets.
    #[serde(default = "defaults::default_limits_config")]
    pub limits: LimitsConfig,
}

/// Application tool-output contract, independent from MCP protocol negotiation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    /// Versioned typed JSON, structured content and matching text fallback.
    #[default]
    JsonV1,
    /// Temporary compatibility output with historical text/image shapes.
    Legacy,
}

/// Bounds for one protocol session, independent of account charges and output budgets.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Application output contract; canonical JSON is the migration default.
    #[serde(default)]
    pub output_format: OutputFormat,
    /// One absolute drain budget after EOF or shutdown; zero cancels immediately.
    #[serde(default = "defaults::default_shutdown_timeout")]
    pub shutdown_timeout_seconds: u64,
    /// Maximum incoming NDJSON frame, including its newline (independent of output limits).
    #[serde(default = "defaults::default_max_request_bytes")]
    pub max_request_bytes: usize,
    /// Maximum admitted requests, including responses waiting to be written.
    #[serde(default = "defaults::default_max_in_flight_requests")]
    pub max_in_flight_requests: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        defaults::default_server_config()
    }
}

/// Opt-in outgoing file policy. Paths belong to the connector host.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SendingConfig {
    /// Enable immediate account sends, only with the canonical output contract.
    pub enabled: bool,
    /// Per-image maximum, at most 10 MiB.
    pub max_image_bytes: u64,
    /// Per-audio/voice/video maximum, at most 100 MiB.
    pub max_media_bytes: u64,
    /// One or two nonwaiting media-operation slots.
    pub max_concurrent_media: usize,
    /// Aggregate occupied/reserved staging bytes.
    pub max_staging_bytes: u64,
    /// A separate fresh private empty directory; required for configured media roots.
    pub staging_root: Option<PathBuf>,
    /// Explicit aliases for existing absolute prepared-attachment directories.
    pub upload_roots: std::collections::BTreeMap<String, PathBuf>,
}

impl SendingConfig {
    pub(crate) fn staging_config(&self) -> telegram_core::sending::StagingConfig {
        telegram_core::sending::StagingConfig {
            max_image_bytes: self.max_image_bytes,
            max_media_bytes: self.max_media_bytes,
            max_concurrent_media: self.max_concurrent_media,
            max_staging_bytes: self.max_staging_bytes,
            staging_root: self.staging_root.clone(),
            upload_roots: self.upload_roots.clone(),
        }
    }
}

impl Default for SendingConfig {
    fn default() -> Self {
        let staging = telegram_core::sending::StagingConfig::default();
        Self {
            enabled: false,
            max_image_bytes: staging.max_image_bytes,
            max_media_bytes: staging.max_media_bytes,
            max_concurrent_media: staging.max_concurrent_media,
            max_staging_bytes: staging.max_staging_bytes,
            staging_root: staging.staging_root,
            upload_roots: staging.upload_roots,
        }
    }
}

/// Bounds for the `transcribe_voice_message` `timeout_seconds` request param.
///
/// Sibling of the `transcription_cost` rate-limit knob; previously these were
/// hardcoded constants in the transcription tool (AD-6).
#[derive(Debug, Clone, Deserialize)]
pub struct TranscriptionConfig {
    /// Wait applied when a request omits `timeout_seconds`.
    #[serde(default = "defaults::default_transcription_default_timeout")]
    pub default_timeout_seconds: u32,
    /// Upper bound the request's `timeout_seconds` is clamped to.
    #[serde(default = "defaults::default_transcription_max_timeout")]
    pub max_timeout_seconds: u32,
}

/// Response-size limits applied at the MCP layer (`[limits]`, work-order B4).
#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    /// Cap (bytes) on a serialized message-stream response. When exceeded,
    /// trailing messages are dropped and `has_more`/`next_cursor` are set.
    /// At least one message is always returned, even over-budget.
    #[serde(default = "defaults::default_response_byte_budget")]
    pub response_byte_budget: u64,

    /// Cap (bytes of base64 payload, as sent to the client) on the total image
    /// payload of one `get_messages_media_batch` call. Base64 is 4/3 the size
    /// of the underlying JPEG, and base64 is what consumes client context, so
    /// the budget is counted in base64 bytes.
    #[serde(default = "defaults::default_media_batch_max_total_bytes")]
    pub media_batch_max_total_bytes: u64,
}

/// Process-wide tracing output and owned daily-log retention settings.
#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    /// Tracing filter used when `RUST_LOG` is absent or invalid.
    #[serde(default = "defaults::default_log_level")]
    pub level: String,
    /// Stderr format selector (`json` chooses JSON; other values use pretty text).
    #[serde(default = "defaults::default_log_format")]
    pub format: String,
    /// Enable file logging (default: true)
    #[serde(default = "defaults::default_file_enabled")]
    pub file_enabled: bool,
    /// Directory for log files (default: ~/.config/telegram-connector/logs/)
    #[serde(default = "defaults::default_log_path")]
    pub file_path: PathBuf,
    /// Number of days to retain log files (default: 7)
    #[serde(default = "defaults::default_max_log_days")]
    pub max_log_days: u32,
}

/// Transport observability settings (`[observability]` table).
///
/// Added after the 2026-06-12 lost-response incident (`docs/connetion-issue.md`)
/// so stdout write behavior is tunable in the field without a rebuild.
#[derive(Debug, Clone, Deserialize)]
pub struct ObservabilityConfig {
    /// WARN when a stdout write+flush exceeds this many milliseconds.
    /// 0 makes every write WARN (field diagnostic mode).
    #[serde(default = "defaults::default_slow_write_threshold_ms")]
    pub slow_write_threshold_ms: u64,

    /// Ring buffer capacity for `get_last_responses` (0 disables buffering).
    #[serde(default = "defaults::default_response_buffer_size")]
    pub response_buffer_size: usize,

    /// Responses larger than this many bytes are recorded in the ring buffer
    /// with a stub payload instead of the full body (get_message_media responses
    /// are ~1.5 MB of base64; replaying them via get_last_responses is useless).
    #[serde(default = "defaults::default_max_buffered_payload_bytes")]
    pub max_buffered_payload_bytes: usize,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            slow_write_threshold_ms: defaults::default_slow_write_threshold_ms(),
            response_buffer_size: defaults::default_response_buffer_size(),
            max_buffered_payload_bytes: defaults::default_max_buffered_payload_bytes(),
        }
    }
}

/// Private processed-JPEG cache and optional local export settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MediaArtifactsConfig {
    /// Initialize the private cache (disabled by default).
    pub enabled: bool,
    /// Permit explicit delivery of raw absolute local paths.
    pub file_export_enabled: bool,
    /// Dedicated empty private absolute directory; platform config child when absent.
    pub root: Option<PathBuf>,
    /// Logical lifetime in seconds, 1..=3600.
    pub ttl_seconds: u64,
    /// Reserved entries, 1..=1000.
    pub max_entries: usize,
    /// Reserved bytes, 1..=1073741824.
    pub max_total_bytes: u64,
}
impl Default for MediaArtifactsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            file_export_enabled: false,
            root: None,
            ttl_seconds: 600,
            max_entries: 100,
            max_total_bytes: 268_435_456,
        }
    }
}
