//! Configuration representation, serde defaults and credential accessors.

use super::defaults;
use secrecy::{ExposeSecret, SecretString};
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
    #[serde(default = "defaults::default_rate_limit_config")]
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

/// MTProto connection/session settings; credentials are required only for setup.
#[derive(Debug, Clone, Deserialize)]
pub struct TelegramConfig {
    /// API ID from <https://my.telegram.org> (always required for connection)
    pub api_id: i32,
    /// API hash from <https://my.telegram.org> (required only for --setup)
    #[serde(default, deserialize_with = "deserialize_optional_secret_string")]
    pub api_hash: Option<SecretString>,
    /// Phone number for authentication (required only for --setup)
    #[serde(default, deserialize_with = "deserialize_optional_secret_string")]
    pub phone_number: Option<SecretString>,
    /// Session file path (always used)
    #[serde(default = "defaults::default_session_file")]
    pub session_file: PathBuf,
    /// Per-call-type timeout budgets for grammers network operations.
    #[serde(default = "defaults::default_timeout_config")]
    pub timeouts: TimeoutConfig,
    /// Hard cap (bytes) on a single media download; transfers exceeding it are
    /// rejected before and during streaming. Default 20 MiB. Pairs with
    /// `[rate_limiting] media_download_cost`, which prices the download (AD-6).
    #[serde(default = "defaults::default_max_download_bytes")]
    /// Accepted range is 1..=80 MiB; four process-wide raw slots share 80 MiB.
    pub max_download_bytes: u64,
}

/// Timeout budgets (seconds) applied to grammers calls in [`crate::telegram::client`].
///
/// Call categories have separate budgets. Multi-iteration walks
/// (`iter_messages.next()`, `search_iter.next()`) measure *total* elapsed time across all
/// iterations, not per-iteration.
#[derive(Debug, Clone, Deserialize)]
pub struct TimeoutConfig {
    /// Total text-send budget, starting before local admission.
    #[serde(default = "defaults::default_send_secs")]
    pub send_secs: u64,
    /// Total media-send budget including staging, upload and final acknowledgement.
    #[serde(default = "defaults::default_upload_send_secs")]
    pub upload_send_secs: u64,
    /// Resolve/dialog lookups, account checks and authentication RPCs. Interactive
    /// code/password entry is not timed; each login RPC gets this budget.
    #[serde(default = "defaults::default_resolve_secs")]
    pub resolve_secs: u64,
    /// History/message fetches, subscribed-channel enumeration and transcription RPCs.
    #[serde(default = "defaults::default_history_secs")]
    pub history_secs: u64,
    /// Search walks (single-channel `search_iter`, global `search_all_messages`).
    #[serde(default = "defaults::default_search_secs")]
    pub search_secs: u64,
    /// Wall-clock budget for a full in-memory media download.
    #[serde(default = "defaults::default_download_secs")]
    pub download_secs: u64,
}

impl TelegramConfig {
    /// Check whether nonempty authentication credentials are present (api_hash, phone_number).
    /// Note: api_id is always required for connection, not just setup
    pub fn has_auth_credentials(&self) -> bool {
        self.auth_credentials().is_some()
    }

    /// Authentication credentials as `(api_hash, phone_number)`, or `None`
    /// when either is absent or empty.
    ///
    /// This is the single definition of "credentials are present";
    /// [`Self::has_auth_credentials`] is its predicate form. The returned strings
    /// expose secret material to the caller; do not log or include them in errors.
    pub fn auth_credentials(&self) -> Option<(&str, &str)> {
        fn non_empty(secret: &SecretString) -> Option<&str> {
            Some(secret.expose_secret()).filter(|value| !value.is_empty())
        }
        Some((
            self.api_hash.as_ref().and_then(non_empty)?,
            self.phone_number.as_ref().and_then(non_empty)?,
        ))
    }
}

// Helper function for deserializing optional SecretString
fn deserialize_optional_secret_string<'de, D>(
    deserializer: D,
) -> Result<Option<SecretString>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt
        .filter(|s| !s.is_empty())
        .map(|s| SecretString::new(s.into_boxed_str())))
}

/// Cooperative search accumulation deadline, in addition to network timeouts.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    /// Wall-clock budget for a search's accumulation loop. On expiry the
    /// search returns the results gathered so far with `timed_out`/`partial`
    /// set — never an error. Must stay below `[telegram.timeouts] search_secs`
    /// (default 120) to be reachable.
    #[serde(default = "defaults::default_search_deadline_seconds")]
    pub deadline_seconds: u64,
}

/// Account token-bucket capacity, refill speed and weighted operation costs.
#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    /// Charge for an admitted outgoing text operation; never refunded.
    #[serde(default = "defaults::default_send_message_cost")]
    pub send_message_cost: u32,
    /// Charge for an admitted media operation, independent of upload part count.
    #[serde(default = "defaults::default_send_media_cost")]
    pub send_media_cost: u32,
    /// Initial and maximum token capacity; builders do not validate this automatically.
    #[serde(default = "defaults::default_max_tokens")]
    pub max_tokens: u32,
    /// Tokens restored per elapsed second on acquisition or snapshot.
    #[serde(default = "defaults::default_refill_rate")]
    pub refill_rate: f64,
    /// Tokens charged per get_message_media call (searches cost 1).
    #[serde(default = "defaults::default_media_download_cost")]
    pub media_download_cost: u32,
    /// Tokens charged per transcribe_voice_message call (Telegram's weekly
    /// transcription quota makes these calls precious; searches cost 1).
    #[serde(default = "defaults::default_transcription_cost")]
    pub transcription_cost: u32,
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

impl Default for SendingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_image_bytes: 10_485_760,
            max_media_bytes: 104_857_600,
            max_concurrent_media: 2,
            max_staging_bytes: 209_715_200,
            staging_root: None,
            upload_roots: Default::default(),
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
