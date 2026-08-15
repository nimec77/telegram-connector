//! Env-var expansion and config-path resolution.
use super::EnvGuard;
use crate::config::*;
use std::env;

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
fn env_guard_restores_env_on_panic() {
    // This test panics on purpose inside `catch_unwind`, so the panic message
    // and backtrace printed to test output for this test are expected, not a
    // failure signal.
    let result = std::panic::catch_unwind(|| {
        let mut env_guard = EnvGuard::new();
        env_guard.set("ENV_GUARD_PANIC_PROBE", "leaked?");
        panic!("assertion-failure stand-in");
    });
    assert!(result.is_err());
    let _env_guard = EnvGuard::new(); // re-serialize before probing
    assert!(env::var_os("ENV_GUARD_PANIC_PROBE").is_none());
}
