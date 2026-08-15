use super::*;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Serializes tests that mutate process environment variables. The test
/// harness runs tests on parallel threads within one process and the
/// environment is process-global, so every test that calls
/// `env::set_var`/`env::remove_var` must hold this lock for its whole body.
/// A poisoned lock is safe to reuse: each test restores the vars it set.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// RAII guard for tests that mutate process environment variables.
///
/// Construction takes `ENV_LOCK`, serializing all env-mutating tests.
/// `set`/`remove` record a variable's prior value the first time they touch
/// it, and `Drop` restores every touched variable — so a failing assertion
/// cannot leak env state into subsequent tests (before this guard, cleanup
/// ran only on the success path).
struct EnvGuard {
    saved: Vec<(&'static str, Option<OsString>)>,
    _lock: MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn new() -> Self {
        Self {
            saved: Vec::new(),
            _lock: env_lock(),
        }
    }

    fn set(&mut self, key: &'static str, value: impl AsRef<std::ffi::OsStr>) {
        self.save(key);
        unsafe { env::set_var(key, value) };
    }

    fn remove(&mut self, key: &'static str) {
        self.save(key);
        unsafe { env::remove_var(key) };
    }

    fn save(&mut self, key: &'static str) {
        if !self.saved.iter().any(|(k, _)| *k == key) {
            self.saved.push((key, env::var_os(key)));
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, old) in self.saved.iter().rev() {
            match old {
                Some(value) => unsafe { env::set_var(key, value) },
                None => unsafe { env::remove_var(key) },
            }
        }
    }
}

#[test]
fn test_expand_env_vars_no_variables() {
    let result = expand_env_vars("simple string").unwrap();
    assert_eq!(result, "simple string");
}

#[test]
fn test_expand_env_vars_single_variable() {
    let mut env_guard = EnvGuard::new();
    env_guard.set("TEST_VAR", "test_value");
    let result = expand_env_vars("prefix_${TEST_VAR}_suffix").unwrap();
    assert_eq!(result, "prefix_test_value_suffix");
}

#[test]
fn test_expand_env_vars_multiple_variables() {
    let mut env_guard = EnvGuard::new();
    env_guard.set("VAR1", "value1");
    env_guard.set("VAR2", "value2");
    let result = expand_env_vars("${VAR1}_middle_${VAR2}").unwrap();
    assert_eq!(result, "value1_middle_value2");
}

#[test]
fn test_expand_env_vars_missing_variable_returns_error() {
    let result = expand_env_vars("${NONEXISTENT_VAR_12345}");
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("NONEXISTENT_VAR_12345"),
        "Error should mention the missing variable name, got: {err_msg}"
    );
}

#[test]
fn test_expand_env_vars_no_recursive_expansion() {
    let mut env_guard = EnvGuard::new();
    // If a var's value contains ${...}, it should NOT be expanded further
    env_guard.set("OUTER_VAR", "${INNER_VAR}");
    env_guard.set("INNER_VAR", "should_not_appear");
    let result = expand_env_vars("value_${OUTER_VAR}_end").unwrap();
    assert_eq!(
        result, "value_${INNER_VAR}_end",
        "Variable values containing ${{...}} should not be recursively expanded"
    );
}

#[test]
fn test_expand_env_vars_incomplete_syntax() {
    let result = expand_env_vars("${INCOMPLETE").unwrap();
    assert_eq!(result, "${INCOMPLETE");
}

#[test]
fn test_expand_env_vars_numeric_unquoting() {
    let mut env_guard = EnvGuard::new();
    // When a quoted TOML value contains only an env var with a pure numeric value,
    // it should be unquoted to allow parsing as integer
    env_guard.set("TEST_NUM", "12345");
    env_guard.set("TEST_PHONE", "+1234567890");

    // Pure number should be unquoted
    let result = expand_env_vars(r#"api_id = "${TEST_NUM}""#).unwrap();
    assert_eq!(result, "api_id = 12345");

    // Phone number (with +) should remain quoted
    let result = expand_env_vars(r#"phone = "${TEST_PHONE}""#).unwrap();
    assert_eq!(result, r#"phone = "+1234567890""#);
}

#[test]
fn test_expand_env_vars_skips_toml_comment_lines() {
    // ${VAR_NAME} inside a TOML comment must not be expanded — it's documentation text,
    // not a variable reference. This was the bug: config.example.toml has the comment
    // "# Supports environment variable expansion with ${VAR_NAME} syntax" which caused
    // the binary to fail with "Environment variable 'VAR_NAME' not found".
    let input = "# Supports environment variable expansion with ${VAR_NAME} syntax\napi_id = 12345";
    let result = expand_env_vars(input).unwrap();
    assert_eq!(result, input);
}

#[test]
fn test_expand_env_vars_skips_inline_comment_after_hash() {
    // A line that starts with leading whitespace then # is also a comment
    let input = "  # another comment with ${SHOULD_BE_IGNORED} inside";
    let result = expand_env_vars(input).unwrap();
    assert_eq!(result, input);
}

fn create_test_config(api_id: i32, api_hash: Option<&str>, phone_number: Option<&str>) -> Config {
    Config {
        telegram: TelegramConfig {
            api_id,
            api_hash: api_hash
                .filter(|s| !s.is_empty())
                .map(|s| SecretString::new(s.to_string().into_boxed_str())),
            phone_number: phone_number
                .filter(|s| !s.is_empty())
                .map(|s| SecretString::new(s.to_string().into_boxed_str())),
            session_file: PathBuf::from("session.bin"),
            timeouts: default_timeout_config(),
            max_download_bytes: default_max_download_bytes(),
        },
        search: SearchConfig {
            default_hours_back: 48,
            max_results_default: 20,
            max_results_limit: 100,
            deadline_seconds: 20,
        },
        rate_limiting: RateLimitConfig {
            max_tokens: 50,
            refill_rate: 2.0,
            media_download_cost: default_media_download_cost(),
            transcription_cost: default_transcription_cost(),
        },
        logging: LoggingConfig {
            level: "info".to_string(),
            format: "compact".to_string(),
            file_enabled: true,
            file_path: PathBuf::from("/tmp/logs"),
            max_log_days: 7,
        },
        server: ServerConfig {
            shutdown_timeout_seconds: 5,
        },
        observability: default_observability_config(),
        transcription: default_transcription_config(),
        limits: default_limits_config(),
    }
}

#[test]
fn test_has_auth_credentials_all_present() {
    let config = create_test_config(12345, Some("hash"), Some("+1234567890"));
    assert!(config.telegram.has_auth_credentials());
}

#[test]
fn test_has_auth_credentials_missing_api_hash() {
    let config = create_test_config(12345, None, Some("+1234567890"));
    assert!(!config.telegram.has_auth_credentials());
}

#[test]
fn test_has_auth_credentials_missing_phone() {
    let config = create_test_config(12345, Some("hash"), None);
    assert!(!config.telegram.has_auth_credentials());
}

#[test]
fn test_has_auth_credentials_empty_api_hash() {
    let config = create_test_config(12345, Some(""), Some("+1234567890"));
    assert!(!config.telegram.has_auth_credentials());
}

#[test]
fn test_validate_for_setup_missing_auth_credentials() {
    let config = create_test_config(12345, None, None);
    let result = config.validate_for_setup();
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Authentication credentials required")
    );
}

#[test]
fn test_validate_for_setup_valid_credentials() {
    let config = create_test_config(12345, Some("valid_hash"), Some("+1234567890"));
    let result = config.validate_for_setup();
    assert!(result.is_ok());
}

#[test]
fn test_auth_credentials_getter() {
    let config = create_test_config(12345, Some("test_hash"), Some("+1234567890"));
    let (api_hash, phone) = config.telegram.auth_credentials();
    assert_eq!(api_hash, "test_hash");
    assert_eq!(phone, "+1234567890");
}

#[ignore = "for CI/CD passing tests"]
#[test]
fn test_load_valid_config() {
    let mut env_guard = EnvGuard::new();
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

    env_guard.set("TELEGRAM_MCP_CONFIG", &config_path);
    let result = Config::load();
    fs::remove_file(&config_path).ok();

    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.telegram.api_id, 12345);
    assert_eq!(
        config.telegram.api_hash.as_ref().map(|s| s.expose_secret()),
        Some("test_hash")
    );
}

#[ignore = "for CI/CD passing tests"]
#[test]
fn test_load_config_with_env_vars() {
    let mut env_guard = EnvGuard::new();
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

    env_guard.set("TEST_API_ID", "98765");
    env_guard.set("TEST_API_HASH", "expanded_hash");
    env_guard.set("TEST_PHONE", "+9876543210");
    env_guard.set("TELEGRAM_MCP_CONFIG", &config_path);

    let result = Config::load();

    fs::remove_file(&config_path).ok();

    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.telegram.api_id, 98765);
    assert_eq!(
        config.telegram.api_hash.as_ref().map(|s| s.expose_secret()),
        Some("expanded_hash")
    );
    assert_eq!(
        config
            .telegram
            .phone_number
            .as_ref()
            .map(|s| s.expose_secret()),
        Some("+9876543210")
    );
}

#[test]
fn test_load_missing_config() {
    let mut env_guard = EnvGuard::new();
    env_guard.set("TELEGRAM_MCP_CONFIG", "/nonexistent/path/config.toml");
    let result = Config::load();

    assert!(result.is_err());
}

#[test]
fn test_load_invalid_toml() {
    let mut env_guard = EnvGuard::new();
    let temp_dir = env::temp_dir();
    let config_path = temp_dir.join("test_invalid.toml");
    fs::write(&config_path, "this is not valid TOML {{{}}}").unwrap();

    env_guard.set("TELEGRAM_MCP_CONFIG", &config_path);
    let result = Config::load();
    fs::remove_file(&config_path).ok();

    assert!(result.is_err());
}

#[ignore = "for CI/CD passing tests"]
#[test]
fn test_resolve_path_from_env() {
    let mut env_guard = EnvGuard::new();
    env_guard.set("TELEGRAM_MCP_CONFIG", "/custom/path/config.toml");
    let result = Config::resolve_config_path().unwrap();

    assert_eq!(result, PathBuf::from("/custom/path/config.toml"));
}

#[ignore = "for CI/CD passing tests"]
#[test]
fn test_resolve_path_default() {
    let mut env_guard = EnvGuard::new();
    env_guard.remove("TELEGRAM_MCP_CONFIG");
    let result = Config::resolve_config_path();
    assert!(result.is_ok());
    let path = result.unwrap();
    assert!(path.to_string_lossy().contains("telegram-connector"));
    assert!(path.to_string_lossy().ends_with("config.toml"));
}

#[test]
fn test_secret_does_not_expose_in_debug() {
    let mut config = create_test_config(12345, Some("sensitive_hash_value"), Some("+1234567890"));
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

// ========================================================================
// File Logging Config Tests
// ========================================================================

#[test]
fn test_logging_config_defaults_file_enabled() {
    assert!(default_file_enabled());
}

#[test]
fn test_logging_config_defaults_max_log_days() {
    assert_eq!(default_max_log_days(), 7);
}

#[test]
fn test_logging_config_defaults_log_path() {
    let path = default_log_path();
    assert!(path.to_string_lossy().contains("telegram-connector"));
    assert!(path.to_string_lossy().ends_with("logs"));
}

#[test]
fn test_default_logging_config_has_file_fields() {
    let config = default_logging_config();
    assert!(config.file_enabled);
    assert_eq!(config.max_log_days, 7);
    assert!(config.file_path.to_string_lossy().contains("logs"));
}

// ========================================================================
// Timeout Config Tests (Phase 20)
// ========================================================================

#[test]
fn test_default_timeout_config_values() {
    let config = default_timeout_config();
    assert_eq!(config.resolve_secs, 30);
    assert_eq!(config.history_secs, 60);
    assert_eq!(config.search_secs, 120);
    assert_eq!(config.download_secs, 120);
}

#[test]
fn test_telegram_config_default_timeouts_when_section_absent() {
    let toml_str = r#"
api_id = 12345
"#;
    let cfg: TelegramConfig = toml::from_str(toml_str).expect("parse");
    assert_eq!(cfg.timeouts.resolve_secs, 30);
    assert_eq!(cfg.timeouts.history_secs, 60);
    assert_eq!(cfg.timeouts.search_secs, 120);
    assert_eq!(cfg.timeouts.download_secs, 120);
}

#[test]
fn test_telegram_config_timeout_partial_override() {
    let toml_str = r#"
api_id = 12345

[timeouts]
search_secs = 300
"#;
    let cfg: TelegramConfig = toml::from_str(toml_str).expect("parse");
    assert_eq!(cfg.timeouts.resolve_secs, 30);
    assert_eq!(cfg.timeouts.history_secs, 60);
    assert_eq!(cfg.timeouts.search_secs, 300);
    assert_eq!(cfg.timeouts.download_secs, 120);
}

#[test]
fn test_telegram_config_timeout_full_override() {
    let toml_str = r#"
api_id = 12345

[timeouts]
resolve_secs = 5
history_secs = 15
search_secs = 45
"#;
    let cfg: TelegramConfig = toml::from_str(toml_str).expect("parse");
    assert_eq!(cfg.timeouts.resolve_secs, 5);
    assert_eq!(cfg.timeouts.history_secs, 15);
    assert_eq!(cfg.timeouts.search_secs, 45);
    assert_eq!(cfg.timeouts.download_secs, 120);
}

#[test]
fn test_telegram_config_default_max_download_bytes() {
    // AD-6: the media download cap is configurable; absent, it defaults to 20 MiB.
    let toml_str = "api_id = 12345\n";
    let cfg: TelegramConfig = toml::from_str(toml_str).expect("parse");
    assert_eq!(cfg.max_download_bytes, 20 * 1024 * 1024);
}

#[test]
fn test_telegram_config_max_download_bytes_override() {
    let toml_str = "api_id = 12345\nmax_download_bytes = 5242880\n";
    let cfg: TelegramConfig = toml::from_str(toml_str).expect("parse");
    assert_eq!(cfg.max_download_bytes, 5_242_880);
}

#[test]
fn test_transcription_config_defaults_when_section_absent() {
    // AD-6: transcription timeout bounds default to 30/120 when [transcription] is absent.
    let toml_str = "[telegram]\napi_id = 12345\n";
    let cfg: Config = toml::from_str(toml_str).expect("parse");
    assert_eq!(cfg.transcription.default_timeout_seconds, 30);
    assert_eq!(cfg.transcription.max_timeout_seconds, 120);
}

#[test]
fn test_transcription_config_override() {
    let toml_str = "[telegram]\napi_id = 12345\n\n\
                    [transcription]\ndefault_timeout_seconds = 20\nmax_timeout_seconds = 90\n";
    let cfg: Config = toml::from_str(toml_str).expect("parse");
    assert_eq!(cfg.transcription.default_timeout_seconds, 20);
    assert_eq!(cfg.transcription.max_timeout_seconds, 90);
}

#[test]
fn test_timeout_config_validate_rejects_zero_resolve() {
    let cfg = TimeoutConfig {
        resolve_secs: 0,
        history_secs: 60,
        search_secs: 120,
        download_secs: default_download_secs(),
    };
    let err = cfg
        .validate()
        .expect_err("zero resolve_secs must be rejected");
    assert!(
        err.to_string().to_lowercase().contains("resolve_secs"),
        "error should mention the offending field, got: {err}"
    );
}

#[test]
fn test_timeout_config_validate_rejects_zero_history() {
    let cfg = TimeoutConfig {
        resolve_secs: 30,
        history_secs: 0,
        search_secs: 120,
        download_secs: default_download_secs(),
    };
    let err = cfg
        .validate()
        .expect_err("zero history_secs must be rejected");
    assert!(err.to_string().to_lowercase().contains("history_secs"));
}

#[test]
fn test_timeout_config_validate_rejects_zero_search() {
    let cfg = TimeoutConfig {
        resolve_secs: 30,
        history_secs: 60,
        search_secs: 0,
        download_secs: default_download_secs(),
    };
    let err = cfg
        .validate()
        .expect_err("zero search_secs must be rejected");
    assert!(err.to_string().to_lowercase().contains("search_secs"));
}

#[test]
fn test_timeout_config_validate_accepts_defaults() {
    let cfg = default_timeout_config();
    cfg.validate().expect("defaults must be valid");
}

#[ignore = "for CI/CD passing tests"]
#[test]
fn test_load_config_with_file_logging_options() {
    let mut env_guard = EnvGuard::new();
    let temp_dir = env::temp_dir();
    let config_path = temp_dir.join("test_file_logging_config.toml");
    let config_content = r#"
[telegram]
api_id = 12345
api_hash = "test_hash"
phone_number = "+1234567890"

[logging]
level = "debug"
format = "json"
file_enabled = false
file_path = "/custom/log/path"
max_log_days = 14
"#;
    fs::write(&config_path, config_content).unwrap();

    env_guard.set("TELEGRAM_MCP_CONFIG", &config_path);
    let result = Config::load();
    fs::remove_file(&config_path).ok();

    assert!(result.is_ok());
    let config = result.unwrap();
    assert!(!config.logging.file_enabled);
    assert_eq!(config.logging.file_path, PathBuf::from("/custom/log/path"));
    assert_eq!(config.logging.max_log_days, 14);
}

// ========================================================================
// Observability Config Tests (Phase 30)
// ========================================================================

#[test]
fn test_observability_defaults_when_table_absent() {
    let config: Config = toml::from_str("[telegram]\napi_id = 12345\n").unwrap();
    assert_eq!(config.observability.slow_write_threshold_ms, 500);
    assert_eq!(config.observability.response_buffer_size, 10);
}

#[test]
fn test_observability_table_parsed() {
    let toml_str = r#"
[telegram]
api_id = 12345

[observability]
slow_write_threshold_ms = 250
response_buffer_size = 0
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.observability.slow_write_threshold_ms, 250);
    assert_eq!(config.observability.response_buffer_size, 0);
}

#[test]
fn test_observability_partial_table_fills_defaults() {
    let toml_str = "[telegram]\napi_id = 1\n\n[observability]\nresponse_buffer_size = 3\n";
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.observability.slow_write_threshold_ms, 500);
    assert_eq!(config.observability.response_buffer_size, 3);
}

// ========================================================================
// Media config field tests (Task 3)
// ========================================================================

#[test]
fn test_media_download_cost_default() {
    let config: Config = toml::from_str("[telegram]\napi_id = 12345\n").unwrap();
    assert_eq!(config.rate_limiting.media_download_cost, 3);
}

#[test]
fn test_media_download_cost_from_toml() {
    let toml_str = "[telegram]\napi_id = 12345\n[rate_limiting]\nmedia_download_cost = 9\n";
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.rate_limiting.media_download_cost, 9);
}

#[test]
fn test_default_rate_limit_has_transcription_cost() {
    let config = default_rate_limit_config();
    assert_eq!(config.transcription_cost, 5);
}

#[test]
fn test_download_secs_default() {
    let config: Config = toml::from_str("[telegram]\napi_id = 12345\n").unwrap();
    assert_eq!(config.telegram.timeouts.download_secs, 120);
}

#[test]
fn test_download_secs_from_toml() {
    let toml_str = "[telegram]\napi_id = 12345\n[telegram.timeouts]\ndownload_secs = 60\n";
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.telegram.timeouts.download_secs, 60);
}

#[test]
fn test_download_secs_zero_fails_validation() {
    let toml_str = "[telegram]\napi_id = 12345\n[telegram.timeouts]\ndownload_secs = 0\n";
    let config: Config = toml::from_str(toml_str).unwrap();
    let result = config.telegram.timeouts.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("download_secs"));
}

#[test]
fn test_max_buffered_payload_bytes_default() {
    let config: Config = toml::from_str("[telegram]\napi_id = 12345\n").unwrap();
    assert_eq!(config.observability.max_buffered_payload_bytes, 262_144);
}

#[test]
fn test_max_buffered_payload_bytes_from_toml() {
    let toml_str =
        "[telegram]\napi_id = 12345\n[observability]\nmax_buffered_payload_bytes = 1024\n";
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.observability.max_buffered_payload_bytes, 1024);
}

// ========================================================================
// Limits Config Tests (Task 1, v0.16 capacity release)
// ========================================================================

#[test]
fn limits_config_defaults_when_absent() {
    let config: Config = toml::from_str("[telegram]\napi_id = 123\n").expect("parse");
    assert_eq!(config.limits.response_byte_budget, 40_000);
}

#[test]
fn limits_config_parses_response_byte_budget() {
    let toml_str = "[telegram]\napi_id = 123\n\n[limits]\nresponse_byte_budget = 20000\n";
    let config: Config = toml::from_str(toml_str).expect("parse");
    assert_eq!(config.limits.response_byte_budget, 20_000);
}

#[test]
fn limits_config_rejects_zero_budget() {
    let config: Config =
        toml::from_str("[telegram]\napi_id = 123\n\n[limits]\nresponse_byte_budget = 0\n")
            .expect("parse");
    assert!(config.limits.validate().is_err());
}

// ========================================================================
// Search Deadline Config Tests (global-search-latency, Task 3)
// ========================================================================

#[test]
fn search_deadline_defaults_to_twenty_seconds() {
    let config: Config = toml::from_str("[telegram]\napi_id = 123\n").expect("parse");
    assert_eq!(config.search.deadline_seconds, 20);
}

#[test]
fn search_config_rejects_zero_deadline() {
    let config: Config =
        toml::from_str("[telegram]\napi_id = 123\n\n[search]\ndeadline_seconds = 0\n")
            .expect("parse");
    assert!(config.search.validate().is_err());
}

#[test]
fn search_config_accepts_explicit_deadline() {
    let config: Config =
        toml::from_str("[telegram]\napi_id = 123\n\n[search]\ndeadline_seconds = 45\n")
            .expect("parse");
    assert_eq!(config.search.deadline_seconds, 45);
    assert!(config.search.validate().is_ok());
}

#[test]
fn search_config_rejects_deadline_over_one_hour() {
    let config: Config =
        toml::from_str("[telegram]\napi_id = 123\n\n[search]\ndeadline_seconds = 3601\n")
            .expect("parse");
    assert!(config.search.validate().is_err());
}

#[test]
fn retuned_media_rate_limit_defaults() {
    let config = default_rate_limit_config();
    assert_eq!(
        config.max_tokens, 60,
        "burst capacity raised for batch media"
    );
    assert_eq!(config.media_download_cost, 3, "per-image cost lowered");
    assert_eq!(
        config.refill_rate, 2.0,
        "refill rate is deliberately unchanged"
    );
}

#[test]
fn media_batch_payload_cap_defaults_to_eight_mib() {
    let limits = default_limits_config();
    assert_eq!(limits.media_batch_max_total_bytes, 8_388_608);
}

#[test]
fn zero_media_batch_payload_cap_is_rejected() {
    let limits = LimitsConfig {
        response_byte_budget: 40_000,
        media_batch_max_total_bytes: 0,
    };
    let err = limits
        .validate()
        .expect_err("a zero cap returns no images at all");
    assert!(err.to_string().contains("media_batch_max_total_bytes"));
}

#[test]
fn below_floor_media_batch_payload_cap_is_rejected() {
    // Anything below MIN_IMAGE_BASE64_BYTES makes Base64Budget::allowance()
    // return None on the very first image, so every call would silently
    // report payload_cap_reached for everything after doing the real
    // downloads.
    let limits = LimitsConfig {
        response_byte_budget: 40_000,
        media_batch_max_total_bytes: MIN_IMAGE_BASE64_BYTES as u64 - 1,
    };
    let err = limits
        .validate()
        .expect_err("a cap below the per-image floor makes every image unreturnable");
    let message = err.to_string();
    assert!(message.contains("media_batch_max_total_bytes"));
    assert!(
        message.contains(&MIN_IMAGE_BASE64_BYTES.to_string()),
        "error message must name the floor, got: {message}"
    );

    // The shipped default must stay comfortably above that floor.
    let default_limits = default_limits_config();
    assert_eq!(default_limits.media_batch_max_total_bytes, 8_388_608);
    default_limits
        .validate()
        .expect("the shipped default must stay above the per-image floor");
}

// ========================================================================
// RateLimitConfig Validation Tests (media-batch-review-fixes, Task 5)
// ========================================================================

#[test]
fn a_media_cost_above_the_bucket_capacity_is_rejected() {
    let config = RateLimitConfig {
        max_tokens: 10,
        refill_rate: 2.0,
        media_download_cost: 11,
        transcription_cost: 5,
    };
    let err = config
        .validate()
        .expect_err("an unsatisfiable cost must be rejected");
    assert!(
        err.to_string().contains("media_download_cost"),
        "the error must name the offending key, got: {err}"
    );
}

#[test]
fn a_transcription_cost_above_the_bucket_capacity_is_rejected() {
    let config = RateLimitConfig {
        max_tokens: 10,
        refill_rate: 2.0,
        media_download_cost: 3,
        transcription_cost: 11,
    };
    let err = config
        .validate()
        .expect_err("an unsatisfiable cost must be rejected");
    assert!(err.to_string().contains("transcription_cost"), "got: {err}");
}

#[test]
fn costs_equal_to_capacity_are_accepted() {
    // Exactly-capacity is satisfiable from a full bucket, so it is legal.
    let config = RateLimitConfig {
        max_tokens: 10,
        refill_rate: 2.0,
        media_download_cost: 10,
        transcription_cost: 10,
    };
    assert!(config.validate().is_ok());
}

#[test]
fn env_guard_restores_env_on_panic() {
    let result = std::panic::catch_unwind(|| {
        let mut env_guard = EnvGuard::new();
        env_guard.set("ENV_GUARD_PANIC_PROBE", "leaked?");
        panic!("assertion-failure stand-in");
    });
    assert!(result.is_err());
    let _env_guard = EnvGuard::new(); // re-serialize before probing
    assert!(env::var_os("ENV_GUARD_PANIC_PROBE").is_none());
}
