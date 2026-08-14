use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::path::PathBuf;

pub(crate) mod defaults;
mod env;

use defaults::*;
use env::expand_env_vars;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub telegram: TelegramConfig,
    #[serde(default = "default_search_config")]
    pub search: SearchConfig,
    #[serde(default = "default_rate_limit_config")]
    pub rate_limiting: RateLimitConfig,
    #[serde(default = "default_logging_config")]
    pub logging: LoggingConfig,
    #[serde(default = "default_server_config")]
    pub server: ServerConfig,
    #[serde(default = "default_observability_config")]
    pub observability: ObservabilityConfig,
    #[serde(default = "default_transcription_config")]
    pub transcription: TranscriptionConfig,
    #[serde(default = "default_limits_config")]
    pub limits: LimitsConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramConfig {
    /// API ID from https://my.telegram.org (always required for connection)
    pub api_id: i32,
    /// API hash from https://my.telegram.org (required only for --setup)
    #[serde(default, deserialize_with = "deserialize_optional_secret_string")]
    pub api_hash: Option<SecretString>,
    /// Phone number for authentication (required only for --setup)
    #[serde(default, deserialize_with = "deserialize_optional_secret_string")]
    pub phone_number: Option<SecretString>,
    /// Session file path (always used)
    #[serde(default = "default_session_file")]
    pub session_file: PathBuf,
    /// Per-call-type timeout budgets for grammers network operations.
    #[serde(default = "default_timeout_config")]
    pub timeouts: TimeoutConfig,
    /// Hard cap (bytes) on a single media download; transfers exceeding it are
    /// rejected before and during streaming. Default 20 MiB. Pairs with
    /// `[rate_limiting] media_download_cost`, which prices the download (AD-6).
    #[serde(default = "default_max_download_bytes")]
    pub max_download_bytes: u64,
}

/// Timeout budgets (seconds) applied to grammers calls in [`crate::telegram::client`].
///
/// Three call categories share global budgets keyed by call type. Multi-iteration walks
/// (`iter_messages.next()`, `search_iter.next()`) measure *total* elapsed time across all
/// iterations, not per-iteration.
#[derive(Debug, Clone, Deserialize)]
pub struct TimeoutConfig {
    /// Resolve / dialog-lookup operations (`resolve_username`, `iter_dialogs`).
    #[serde(default = "default_resolve_secs")]
    pub resolve_secs: u64,
    /// History walks (`iter_messages`, `get_message_by_id` fetch).
    #[serde(default = "default_history_secs")]
    pub history_secs: u64,
    /// Search walks (single-channel `search_iter`, global `search_all_messages`).
    #[serde(default = "default_search_secs")]
    pub search_secs: u64,
    /// Wall-clock budget for a full in-memory media download.
    #[serde(default = "default_download_secs")]
    pub download_secs: u64,
}

impl TimeoutConfig {
    /// Reject zero values — a zero budget would cancel every call instantly.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.resolve_secs == 0 {
            anyhow::bail!("telegram.timeouts.resolve_secs must be > 0");
        }
        if self.history_secs == 0 {
            anyhow::bail!("telegram.timeouts.history_secs must be > 0");
        }
        if self.search_secs == 0 {
            anyhow::bail!("telegram.timeouts.search_secs must be > 0");
        }
        if self.download_secs == 0 {
            anyhow::bail!("telegram.timeouts.download_secs must be > 0");
        }
        Ok(())
    }
}

impl TelegramConfig {
    /// Check if authentication credentials are present (api_hash, phone_number)
    /// Note: api_id is always required for connection, not just setup
    pub fn has_auth_credentials(&self) -> bool {
        self.api_hash
            .as_ref()
            .is_some_and(|s| !s.expose_secret().is_empty())
            && self
                .phone_number
                .as_ref()
                .is_some_and(|s| !s.expose_secret().is_empty())
    }

    /// Get authentication credentials (panics if not present - call has_auth_credentials first)
    pub fn auth_credentials(&self) -> (&str, &str) {
        (
            self.api_hash
                .as_ref()
                .expect("api_hash required")
                .expose_secret(),
            self.phone_number
                .as_ref()
                .expect("phone_number required")
                .expose_secret(),
        )
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

/// Upper bound accepted for `[search] deadline_seconds`.
///
/// `SearchBudget::new` computes `Instant::now() + Duration::from_secs(deadline_secs)`,
/// which panics on overflow in every build profile. This cap keeps a pathological
/// TOML value from turning every search into a panic. It is deliberately permissive:
/// a useful value sits well below `[telegram.timeouts] search_secs` (default 120),
/// which is the actual practical ceiling.
const MAX_SEARCH_DEADLINE_SECONDS: u64 = 3600;

/// Floor below which an image would be downscaled past usefulness. Once a
/// batch's remaining budget drops under this, `Base64Budget::allowance`
/// returns `None` rather than emitting an unreadable thumbnail — which is why
/// `limits.media_batch_max_total_bytes` is validated against it here.
pub(crate) const MIN_IMAGE_BASE64_BYTES: usize = 32_768;

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    #[serde(default = "default_hours_back")]
    pub default_hours_back: u32,
    #[serde(default = "default_max_results_default")]
    pub max_results_default: u32,
    #[serde(default = "default_max_results_limit")]
    pub max_results_limit: u32,
    /// Wall-clock budget for a search's accumulation loop. On expiry the
    /// search returns the results gathered so far with `timed_out`/`partial`
    /// set — never an error. Must stay below `[telegram.timeouts] search_secs`
    /// (default 120) to be reachable.
    #[serde(default = "default_search_deadline_seconds")]
    pub deadline_seconds: u64,
}

impl SearchConfig {
    /// Reject a zero deadline (would end every search before its first page) or a
    /// deadline above `MAX_SEARCH_DEADLINE_SECONDS` (would overflow the `Instant`
    /// arithmetic in `SearchBudget::new`).
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.deadline_seconds == 0 {
            anyhow::bail!("search.deadline_seconds must be > 0");
        }
        if self.deadline_seconds > MAX_SEARCH_DEADLINE_SECONDS {
            anyhow::bail!(
                "search.deadline_seconds must be between 1 and {MAX_SEARCH_DEADLINE_SECONDS}, \
                 got {}",
                self.deadline_seconds
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_refill_rate")]
    pub refill_rate: f64,
    /// Tokens charged per get_message_media call (searches cost 1).
    #[serde(default = "default_media_download_cost")]
    pub media_download_cost: u32,
    /// Tokens charged per transcribe_voice_message call (Telegram's weekly
    /// transcription quota makes these calls precious; searches cost 1).
    #[serde(default = "default_transcription_cost")]
    pub transcription_cost: u32,
}

impl RateLimitConfig {
    /// Reject a per-call cost the bucket can never satisfy. A cost above
    /// `max_tokens` means every call of that kind fails on a full bucket —
    /// a configuration that is not merely tight but unsatisfiable. It also
    /// keeps per-call costs proportionate to the bucket's capacity.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.media_download_cost > self.max_tokens {
            anyhow::bail!(
                "rate_limiting.media_download_cost ({}) exceeds max_tokens ({}), \
                 so every media call would fail even on a full bucket",
                self.media_download_cost,
                self.max_tokens
            );
        }
        if self.transcription_cost > self.max_tokens {
            anyhow::bail!(
                "rate_limiting.transcription_cost ({}) exceeds max_tokens ({}), \
                 so every transcription call would fail even on a full bucket",
                self.transcription_cost,
                self.max_tokens
            );
        }
        Ok(())
    }
}

/// Bounds for the `transcribe_voice_message` `timeout_seconds` request param.
///
/// Sibling of the `transcription_cost` rate-limit knob; previously these were
/// hardcoded constants in the transcription tool (AD-6).
#[derive(Debug, Clone, Deserialize)]
pub struct TranscriptionConfig {
    /// Wait applied when a request omits `timeout_seconds`.
    #[serde(default = "default_transcription_default_timeout")]
    pub default_timeout_seconds: u32,
    /// Upper bound the request's `timeout_seconds` is clamped to.
    #[serde(default = "default_transcription_max_timeout")]
    pub max_timeout_seconds: u32,
}

/// Response-size limits applied at the MCP layer (`[limits]`, work-order B4).
#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    /// Cap (bytes) on a serialized message-stream response. When exceeded,
    /// trailing messages are dropped and `has_more`/`next_cursor` are set.
    /// At least one message is always returned, even over-budget.
    #[serde(default = "default_response_byte_budget")]
    pub response_byte_budget: u64,

    /// Cap (bytes of base64 payload, as sent to the client) on the total image
    /// payload of one `get_messages_media_batch` call. Base64 is 4/3 the size
    /// of the underlying JPEG, and base64 is what consumes client context, so
    /// the budget is counted in base64 bytes.
    #[serde(default = "default_media_batch_max_total_bytes")]
    pub media_batch_max_total_bytes: u64,
}

impl LimitsConfig {
    /// Reject a zero budget — it would make every response over-cap. Also
    /// reject a cap below `MIN_IMAGE_BASE64_BYTES`: `Base64Budget::allowance`
    /// returns `None` once the remaining budget drops under that floor, so a
    /// cap set below it makes every image in every batch call report
    /// `payload_cap_reached` after the downloads already happened — a silent,
    /// costly no-op at runtime. Failing fast at startup is cheaper.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.response_byte_budget == 0 {
            anyhow::bail!("limits.response_byte_budget must be > 0");
        }
        if self.media_batch_max_total_bytes == 0 {
            anyhow::bail!("limits.media_batch_max_total_bytes must be > 0");
        }
        if self.media_batch_max_total_bytes < MIN_IMAGE_BASE64_BYTES as u64 {
            anyhow::bail!(
                "limits.media_batch_max_total_bytes must be >= {MIN_IMAGE_BASE64_BYTES} bytes \
                 (below that floor every image reports payload_cap_reached), got {}",
                self.media_batch_max_total_bytes
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_format")]
    pub format: String,
    /// Enable file logging (default: true)
    #[serde(default = "default_file_enabled")]
    pub file_enabled: bool,
    /// Directory for log files (default: ~/.config/telegram-connector/logs/)
    #[serde(default = "default_log_path")]
    pub file_path: PathBuf,
    /// Number of days to retain log files (default: 7)
    #[serde(default = "default_max_log_days")]
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
    #[serde(default = "default_slow_write_threshold_ms")]
    pub slow_write_threshold_ms: u64,

    /// Ring buffer capacity for `get_last_responses` (0 disables buffering).
    #[serde(default = "default_response_buffer_size")]
    pub response_buffer_size: usize,

    /// Responses larger than this many bytes are recorded in the ring buffer
    /// with a stub payload instead of the full body (get_message_media responses
    /// are ~1.5 MB of base64; replaying them via get_last_responses is useless).
    #[serde(default = "default_max_buffered_payload_bytes")]
    pub max_buffered_payload_bytes: usize,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            slow_write_threshold_ms: default_slow_write_threshold_ms(),
            response_buffer_size: default_response_buffer_size(),
            max_buffered_payload_bytes: default_max_buffered_payload_bytes(),
        }
    }
}

impl Config {
    /// Load configuration from file
    ///
    /// If `config_path` is Some, uses that path. Otherwise, resolves the default path.
    pub fn load() -> anyhow::Result<Self> {
        Self::load_from(None)
    }

    /// Load configuration from a specific path or default location
    ///
    /// If `config_path` is Some, uses that path. Otherwise, resolves the default path.
    /// Environment variables using `${VAR}` syntax are expanded before parsing.
    ///
    /// Note: This does NOT validate credentials. Call `validate_for_setup()` if credentials
    /// are required (e.g., when running with --setup flag).
    pub fn load_from(config_path: Option<&std::path::Path>) -> anyhow::Result<Self> {
        use anyhow::Context;

        let path = match config_path {
            Some(p) => p.to_path_buf(),
            None => Self::resolve_config_path()?,
        };

        let content = std::fs::read_to_string(&path)
            .context(format!("Failed to read config: {}", path.display()))?;

        // Expand environment variables BEFORE parsing TOML
        // This allows ${VAR} syntax in any field, including numeric fields like api_id
        let expanded_content = expand_env_vars(&content)?;

        let config: Config =
            toml::from_str(&expanded_content).context("Failed to parse config.toml")?;

        // Validate sub-configs that have non-credential invariants. Credential
        // validation is deferred to `validate_for_setup()` because the daemon path
        // does not require api_hash/phone_number at startup.
        config
            .telegram
            .timeouts
            .validate()
            .context("invalid telegram.timeouts configuration")?;

        config
            .limits
            .validate()
            .context("invalid limits configuration")?;

        config
            .search
            .validate()
            .context("invalid search configuration")?;

        config
            .rate_limiting
            .validate()
            .context("invalid rate_limiting configuration")?;

        Ok(config)
    }

    /// Validate that authentication credentials are present (required for --setup mode)
    /// Note: api_id is always required and validated during config parsing
    pub fn validate_for_setup(&self) -> anyhow::Result<()> {
        if !self.telegram.has_auth_credentials() {
            anyhow::bail!(
                "Authentication credentials required for setup mode.\n\
                Please ensure these are set in your config.toml:\n\
                - telegram.api_hash\n\
                - telegram.phone_number\n\n\
                You can use environment variables: api_hash = \"${{YOUR_ENV_VAR}}\"\n\
                Get your API credentials from: https://my.telegram.org"
            );
        }
        Ok(())
    }

    /// Apply CLI overrides to the configuration
    pub fn apply_cli_overrides(&mut self, session_file: Option<std::path::PathBuf>) {
        if let Some(path) = session_file {
            self.telegram.session_file = path;
        }
    }

    fn resolve_config_path() -> anyhow::Result<PathBuf> {
        // 1. Check environment variable
        if let Ok(path) = std::env::var("TELEGRAM_MCP_CONFIG") {
            return Ok(PathBuf::from(path));
        }

        // 2. Use XDG config directory
        let dirs = directories::ProjectDirs::from("", "", "telegram-connector")
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

        Ok(dirs.config_dir().join("config.toml"))
    }
}

// Tests are in a separate file for better organization
#[cfg(test)]
#[path = "config/tests.rs"]
mod tests;
