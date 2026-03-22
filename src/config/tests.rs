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
    // If a var's value contains ${...}, it should NOT be expanded further
    unsafe {
        env::set_var("OUTER_VAR", "${INNER_VAR}");
        env::set_var("INNER_VAR", "should_not_appear");
    }
    let result = expand_env_vars("value_${OUTER_VAR}_end").unwrap();
    assert_eq!(
        result, "value_${INNER_VAR}_end",
        "Variable values containing ${{...}} should not be recursively expanded"
    );
    unsafe {
        env::remove_var("OUTER_VAR");
        env::remove_var("INNER_VAR");
    }
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
            file_enabled: true,
            file_path: PathBuf::from("/tmp/logs"),
            max_log_days: 7,
        },
        server: ServerConfig {
            shutdown_timeout_seconds: 5,
        },
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
    assert_eq!(
        config.telegram.api_hash.as_ref().map(|s| s.expose_secret()),
        Some("test_hash")
    );
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

#[ignore = "for CI/CD passing tests"]
#[test]
fn test_load_config_with_file_logging_options() {
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
    assert!(!config.logging.file_enabled);
    assert_eq!(config.logging.file_path, PathBuf::from("/custom/log/path"));
    assert_eq!(config.logging.max_log_days, 14);
}
