use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::path::PathBuf;

fn default_session_file() -> PathBuf {
    let dirs = directories::ProjectDirs::from("", "", "telegram-connector")
        .expect("Could not determine config directory");
    dirs.config_dir().join("session.bin")
}

fn default_hours_back() -> u32 {
    48
}

fn default_max_results_default() -> u32 {
    20
}

fn default_max_results_limit() -> u32 {
    100
}

fn default_max_tokens() -> u32 {
    50
}

fn default_refill_rate() -> f64 {
    2.0
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "compact".to_string()
}

fn default_file_enabled() -> bool {
    true
}

fn default_log_path() -> PathBuf {
    let dirs = directories::ProjectDirs::from("", "", "telegram-connector")
        .expect("Could not determine config directory");
    dirs.config_dir().join("logs")
}

fn default_max_log_days() -> u32 {
    7
}

fn default_shutdown_timeout() -> u64 {
    5
}

fn default_server_config() -> ServerConfig {
    ServerConfig {
        shutdown_timeout_seconds: default_shutdown_timeout(),
    }
}

fn default_search_config() -> SearchConfig {
    SearchConfig {
        default_hours_back: default_hours_back(),
        max_results_default: default_max_results_default(),
        max_results_limit: default_max_results_limit(),
    }
}

fn default_rate_limit_config() -> RateLimitConfig {
    RateLimitConfig {
        max_tokens: default_max_tokens(),
        refill_rate: default_refill_rate(),
    }
}

fn default_logging_config() -> LoggingConfig {
    LoggingConfig {
        level: default_log_level(),
        format: default_log_format(),
        file_enabled: default_file_enabled(),
        file_path: default_log_path(),
        max_log_days: default_max_log_days(),
    }
}

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

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    #[serde(default = "default_hours_back")]
    pub default_hours_back: u32,
    #[serde(default = "default_max_results_default")]
    pub max_results_default: u32,
    #[serde(default = "default_max_results_limit")]
    pub max_results_limit: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_refill_rate")]
    pub refill_rate: f64,
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

        let mut config: Config =
            toml::from_str(&expanded_content).context("Failed to parse config.toml")?;

        // Apply defaults (currently no-op, but kept for future use)
        config.apply_defaults();

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

    fn apply_defaults(&mut self) {
        // Defaults are handled by serde with #[serde(default)] attributes
        // This method is kept for potential future use
    }
}

fn expand_env_vars(value: &str) -> anyhow::Result<String> {
    let lines: Vec<String> = value
        .split('\n')
        .map(|line| {
            if line.trim_start().starts_with('#') {
                Ok(line.to_string())
            } else {
                expand_env_vars_in_line(line)
            }
        })
        .collect::<anyhow::Result<_>>()?;
    Ok(lines.join("\n"))
}

fn expand_env_vars_in_line(value: &str) -> anyhow::Result<String> {
    use anyhow::Context;

    let mut result = value.to_string();
    let mut search_from = 0;

    while let Some(rel_start) = result[search_from..].find("${") {
        let start = search_from + rel_start;
        if let Some(end_offset) = result[start..].find('}') {
            let end = start + end_offset;
            let var_name = &result[start + 2..end];
            let var_value = std::env::var(var_name).with_context(|| {
                format!(
                    "Environment variable '{}' not found. \
                     Referenced in config as '${{{}}}'. \
                     Ensure it is set in the process environment.",
                    var_name, var_name
                )
            })?;

            // Check if this is a quoted value that's ONLY an env var: "= \"${VAR}\""
            // If so and the value is purely numeric (digits only), unquote for TOML parsing
            let is_quoted_only_env_var = start >= 1
                && result.as_bytes().get(start - 1) == Some(&b'"')
                && result.as_bytes().get(end + 1) == Some(&b'"');

            // Only unquote if value is purely digits (no +/- signs, no decimals)
            // This ensures phone numbers like "+1234567890" stay as strings
            let is_pure_integer =
                !var_value.is_empty() && var_value.chars().all(|c| c.is_ascii_digit());

            if is_quoted_only_env_var && is_pure_integer {
                // Replace including surrounding quotes: "12345" -> 12345
                result.replace_range((start - 1)..=(end + 1), &var_value);
                search_from = start - 1 + var_value.len();
            } else {
                result.replace_range(start..=end, &var_value);
                search_from = start + var_value.len();
            }
        } else {
            break;
        }
    }

    Ok(result)
}

// Tests are in a separate file for better organization
#[cfg(test)]
#[path = "config/tests.rs"]
mod tests;
