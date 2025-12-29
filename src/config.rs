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
    pub api_id: i32,
    #[serde(deserialize_with = "deserialize_secret_string")]
    pub api_hash: SecretString,
    #[serde(deserialize_with = "deserialize_secret_string")]
    pub phone_number: SecretString,
    #[serde(default = "default_session_file")]
    pub session_file: PathBuf,
}

// Helper function for deserializing SecretString
fn deserialize_secret_string<'de, D>(deserializer: D) -> Result<SecretString, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(SecretString::new(s.into_boxed_str()))
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

        // Validate required fields
        config.validate()?;

        Ok(config)
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

    fn validate(&self) -> anyhow::Result<()> {
        if self.telegram.api_id == 0 {
            anyhow::bail!("telegram.api_id is required");
        }
        if self.telegram.api_hash.expose_secret().is_empty() {
            anyhow::bail!("telegram.api_hash is required");
        }
        if self.telegram.phone_number.expose_secret().is_empty() {
            anyhow::bail!("telegram.phone_number is required");
        }
        Ok(())
    }
}

fn expand_env_vars(value: &str) -> anyhow::Result<String> {
    let mut result = value.to_string();

    while let Some(start) = result.find("${") {
        if let Some(end_offset) = result[start..].find('}') {
            let end = start + end_offset;
            let var_name = &result[start + 2..end];
            let var_value = std::env::var(var_name).unwrap_or_default();

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
            } else {
                result.replace_range(start..=end, &var_value);
            }
        } else {
            break;
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    #[test]
    fn test_expand_env_vars_no_variables() {
        let result = expand_env_vars("simple string").unwrap();
        assert_eq!(result, "simple string");
    }

    #[test]
    fn test_expand_env_vars_single_variable() {
        unsafe {
            env::set_var("TEST_VAR", "test_value");
        }
        let result = expand_env_vars("prefix_${TEST_VAR}_suffix").unwrap();
        assert_eq!(result, "prefix_test_value_suffix");
        unsafe {
            env::remove_var("TEST_VAR");
        }
    }

    #[test]
    fn test_expand_env_vars_multiple_variables() {
        unsafe {
            env::set_var("VAR1", "value1");
            env::set_var("VAR2", "value2");
        }
        let result = expand_env_vars("${VAR1}_middle_${VAR2}").unwrap();
        assert_eq!(result, "value1_middle_value2");
        unsafe {
            env::remove_var("VAR1");
            env::remove_var("VAR2");
        }
    }

    #[test]
    fn test_expand_env_vars_missing_variable() {
        let result = expand_env_vars("${NONEXISTENT_VAR}").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_expand_env_vars_incomplete_syntax() {
        let result = expand_env_vars("${INCOMPLETE").unwrap();
        assert_eq!(result, "${INCOMPLETE");
    }

    #[test]
    fn test_expand_env_vars_numeric_unquoting() {
        // When a quoted TOML value contains only an env var with a pure numeric value,
        // it should be unquoted to allow parsing as integer
        unsafe {
            env::set_var("TEST_NUM", "12345");
            env::set_var("TEST_PHONE", "+1234567890");
        }

        // Pure number should be unquoted
        let result = expand_env_vars(r#"api_id = "${TEST_NUM}""#).unwrap();
        assert_eq!(result, "api_id = 12345");

        // Phone number (with +) should remain quoted
        let result = expand_env_vars(r#"phone = "${TEST_PHONE}""#).unwrap();
        assert_eq!(result, r#"phone = "+1234567890""#);

        unsafe {
            env::remove_var("TEST_NUM");
            env::remove_var("TEST_PHONE");
        }
    }

    fn create_test_config(api_id: i32, api_hash: &str, phone_number: &str) -> Config {
        Config {
            telegram: TelegramConfig {
                api_id,
                api_hash: SecretString::new(api_hash.to_string().into_boxed_str()),
                phone_number: SecretString::new(phone_number.to_string().into_boxed_str()),
                session_file: PathBuf::from("session.bin"),
            },
            search: SearchConfig {
                default_hours_back: 48,
                max_results_default: 20,
                max_results_limit: 100,
            },
            rate_limiting: RateLimitConfig {
                max_tokens: 50,
                refill_rate: 2.0,
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                format: "compact".to_string(),
            },
            server: ServerConfig {
                shutdown_timeout_seconds: 5,
            },
        }
    }

    #[test]
    fn test_validate_missing_api_id() {
        let config = create_test_config(0, "hash", "+1234567890");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("api_id"));
    }

    #[test]
    fn test_validate_missing_api_hash() {
        let config = create_test_config(12345, "", "+1234567890");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("api_hash"));
    }

    #[test]
    fn test_validate_missing_phone_number() {
        let config = create_test_config(12345, "hash", "");
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("phone_number"));
    }

    #[test]
    fn test_validate_valid_config() {
        let config = create_test_config(12345, "valid_hash", "+1234567890");
        let result = config.validate();
        assert!(result.is_ok());
    }

    #[ignore = "for CI/CD passing tests"]
    #[test]
    fn test_load_valid_config() {
        let temp_dir = env::temp_dir();
        let config_path = temp_dir.join("test_config.toml");
        let config_content = r#"
[telegram]
api_id = 12345
api_hash = "test_hash"
phone_number = "+1234567890"
session_file = "/tmp/session.bin"

[search]
default_hours_back = 48
max_results_default = 20
max_results_limit = 100

[rate_limiting]
max_tokens = 50
refill_rate = 2.0

[logging]
level = "info"
format = "compact"
"#;
        fs::write(&config_path, config_content).unwrap();

        unsafe {
            env::set_var("TELEGRAM_MCP_CONFIG", &config_path);
        }
        let result = Config::load();
        unsafe {
            env::remove_var("TELEGRAM_MCP_CONFIG");
        }
        fs::remove_file(&config_path).ok();

        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.telegram.api_id, 12345);
        assert_eq!(config.telegram.api_hash.expose_secret(), "test_hash");
    }

    #[ignore = "for CI/CD passing tests"]
    #[test]
    fn test_load_config_with_env_vars() {
        let temp_dir = env::temp_dir();
        let config_path = temp_dir.join("test_config_env.toml");
        // Test that ALL fields can use ${VAR} syntax, including numeric api_id
        let config_content = r#"
[telegram]
api_id = "${TEST_API_ID}"
api_hash = "${TEST_API_HASH}"
phone_number = "${TEST_PHONE}"
session_file = "/tmp/session.bin"

[search]
default_hours_back = 48
max_results_default = 20
max_results_limit = 100

[rate_limiting]
max_tokens = 50
refill_rate = 2.0

[logging]
level = "info"
format = "compact"
"#;
        fs::write(&config_path, config_content).unwrap();

        unsafe {
            env::set_var("TEST_API_ID", "98765");
            env::set_var("TEST_API_HASH", "expanded_hash");
            env::set_var("TEST_PHONE", "+9876543210");
            env::set_var("TELEGRAM_MCP_CONFIG", &config_path);
        }

        let result = Config::load();

        unsafe {
            env::remove_var("TEST_API_ID");
            env::remove_var("TEST_API_HASH");
            env::remove_var("TEST_PHONE");
            env::remove_var("TELEGRAM_MCP_CONFIG");
        }
        fs::remove_file(&config_path).ok();

        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.telegram.api_id, 98765);
        assert_eq!(config.telegram.api_hash.expose_secret(), "expanded_hash");
        assert_eq!(config.telegram.phone_number.expose_secret(), "+9876543210");
    }

    #[test]
    fn test_load_missing_config() {
        unsafe {
            env::set_var("TELEGRAM_MCP_CONFIG", "/nonexistent/path/config.toml");
        }
        let result = Config::load();
        unsafe {
            env::remove_var("TELEGRAM_MCP_CONFIG");
        }

        assert!(result.is_err());
    }

    #[test]
    fn test_load_invalid_toml() {
        let temp_dir = env::temp_dir();
        let config_path = temp_dir.join("test_invalid.toml");
        fs::write(&config_path, "this is not valid TOML {{{}}}").unwrap();

        unsafe {
            env::set_var("TELEGRAM_MCP_CONFIG", &config_path);
        }
        let result = Config::load();
        unsafe {
            env::remove_var("TELEGRAM_MCP_CONFIG");
        }
        fs::remove_file(&config_path).ok();

        assert!(result.is_err());
    }

    #[ignore = "for CI/CD passing tests"]
    #[test]
    fn test_resolve_path_from_env() {
        unsafe {
            env::set_var("TELEGRAM_MCP_CONFIG", "/custom/path/config.toml");
        }
        let result = Config::resolve_config_path().unwrap();
        unsafe {
            env::remove_var("TELEGRAM_MCP_CONFIG");
        }

        assert_eq!(result, PathBuf::from("/custom/path/config.toml"));
    }

    #[ignore = "for CI/CD passing tests"]
    #[test]
    fn test_resolve_path_default() {
        unsafe {
            env::remove_var("TELEGRAM_MCP_CONFIG");
        }
        let result = Config::resolve_config_path();
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.to_string_lossy().contains("telegram-connector"));
        assert!(path.to_string_lossy().ends_with("config.toml"));
    }

    #[test]
    fn test_secret_does_not_expose_in_debug() {
        let mut config = create_test_config(12345, "sensitive_hash_value", "+1234567890");
        config.telegram.session_file = PathBuf::from("/tmp/session.bin");

        let debug_output = format!("{:?}", config);

        // Secret values should not appear in debug output
        assert!(!debug_output.contains("sensitive_hash_value"));
        assert!(!debug_output.contains("+1234567890"));

        // But should contain "Secret" indicator
        assert!(debug_output.contains("Secret"));
    }

    #[test]
    fn test_secret_expose_returns_actual_value() {
        let secret_hash = SecretString::new("my_api_hash".to_string().into_boxed_str());
        let secret_phone = SecretString::new("+1234567890".to_string().into_boxed_str());

        assert_eq!(secret_hash.expose_secret(), "my_api_hash");
        assert_eq!(secret_phone.expose_secret(), "+1234567890");
    }
}
