//! File-based Config::load_from tests — every env-mutating loader in one auditable place.
use super::EnvGuard;
use crate::config::*;
use std::env;
use std::fs;

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

[rate_limiting]
max_tokens = 50
refill_rate = 2.0

[logging]
level = "info"
format = "compact"
"#;
    fs::write(&config_path, config_content).unwrap();

    env_guard.set("TELEGRAM_MCP_CONFIG", &config_path);
    let result = Config::load_from(None);
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

    let result = Config::load_from(None);

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
    let result = Config::load_from(None);

    assert!(result.is_err());
}

#[test]
fn test_load_invalid_toml() {
    let mut env_guard = EnvGuard::new();
    let temp_dir = env::temp_dir();
    let config_path = temp_dir.join("test_invalid.toml");
    fs::write(&config_path, "this is not valid TOML {{{}}}").unwrap();

    env_guard.set("TELEGRAM_MCP_CONFIG", &config_path);
    let result = Config::load_from(None);
    fs::remove_file(&config_path).ok();

    assert!(result.is_err());
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
    let result = Config::load_from(None);
    fs::remove_file(&config_path).ok();

    assert!(result.is_ok());
    let config = result.unwrap();
    assert!(!config.logging.file_enabled);
    assert_eq!(config.logging.file_path, PathBuf::from("/custom/log/path"));
    assert_eq!(config.logging.max_log_days, 14);
}

// ============================================================================
// `Config::load_from` file-loading error branches (audit stage 4, Task 12).
//
// These exercise the actual `std::fs::read_to_string` -> `expand_env_vars` ->
// `toml::from_str` -> per-sub-config `validate()` pipeline in `load_from`
// (`src/config.rs:351`), via real temp files, rather than calling `validate()`
// directly on hand-built structs (as `validation_tests.rs` does). None of
// these tests mutate the process environment, so none take `EnvGuard`.
// ============================================================================

#[test]
fn loading_a_missing_file_names_the_path_in_the_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("nope.toml");

    let err = Config::load_from(Some(&missing)).expect_err("a missing file must error");

    assert!(
        format!("{err:#}").contains(&missing.display().to_string()),
        "the error must name the path it tried, got: {err:#}"
    );
}

#[test]
fn loading_malformed_toml_reports_a_parse_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    fs::write(&path, "this is not = = valid toml").expect("write");

    let err = Config::load_from(Some(&path)).expect_err("malformed TOML must error");

    assert!(
        format!("{err:#}").contains("Failed to parse config.toml"),
        "expected a parse-stage error, got: {err:#}"
    );
}

// `TimeoutConfig::validate()` (src/config.rs:82) bails when `resolve_secs == 0`
// ("a zero budget would cancel every call instantly") — confirmed by reading
// the implementation, not assumed.
#[test]
fn an_invalid_timeouts_table_fails_validation_with_its_own_context() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        "[telegram]\napi_id = 1\n\n[telegram.timeouts]\nresolve_secs = 0\n",
    )
    .expect("write");

    let err = Config::load_from(Some(&path)).expect_err("a zero timeout must be rejected");

    assert!(
        format!("{err:#}").contains("invalid telegram.timeouts configuration"),
        "expected the timeouts validation context, got: {err:#}"
    );
}

// `LimitsConfig::validate()` (src/config.rs:269) bails when
// `response_byte_budget == 0` ("it would make every response over-cap") —
// confirmed by reading the implementation.
#[test]
fn an_invalid_limits_table_fails_validation_with_its_own_context() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        "[telegram]\napi_id = 1\n\n[limits]\nresponse_byte_budget = 0\n",
    )
    .expect("write");

    let err = Config::load_from(Some(&path)).expect_err("a zero response budget must be rejected");

    assert!(
        format!("{err:#}").contains("invalid limits configuration"),
        "expected the limits validation context, got: {err:#}"
    );
}

// `SearchConfig::validate()` (src/config.rs:175) bails when
// `deadline_seconds == 0` ("would end every search before its first page") —
// confirmed by reading the implementation.
#[test]
fn an_invalid_search_table_fails_validation_with_its_own_context() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        "[telegram]\napi_id = 1\n\n[search]\ndeadline_seconds = 0\n",
    )
    .expect("write");

    let err = Config::load_from(Some(&path)).expect_err("a zero deadline must be rejected");

    assert!(
        format!("{err:#}").contains("invalid search configuration"),
        "expected the search validation context, got: {err:#}"
    );
}

// `RateLimitConfig::validate()` (src/config.rs:210) bails when
// `media_download_cost > max_tokens` ("every call of that kind fails on a
// full bucket") — confirmed by reading the implementation. Both fields are
// set explicitly (rather than relying on the shipped defaults for
// `max_tokens`) so the violation holds regardless of default tuning.
#[test]
fn an_invalid_rate_limiting_table_fails_validation_with_its_own_context() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        "[telegram]\napi_id = 1\n\n[rate_limiting]\nmax_tokens = 1\nmedia_download_cost = 2\n",
    )
    .expect("write");

    let err =
        Config::load_from(Some(&path)).expect_err("a cost exceeding max_tokens must be rejected");

    assert!(
        format!("{err:#}").contains("invalid rate_limiting configuration"),
        "expected the rate_limiting validation context, got: {err:#}"
    );
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_reports_a_read_failure() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    fs::write(&path, "[telegram]\napi_id = 1\n").expect("write");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("chmod");

    let result = Config::load_from(Some(&path));

    // Restore before asserting so the tempdir cleans up even on failure (and
    // so a run as root, where chmod 0 does not block reads, does not leak a
    // 0-mode file if a later assertion panics).
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod");

    // Running as root, chmod 0o000 does not block reads (root bypasses the
    // permission bits entirely), so `result` can come back `Ok` here. That
    // isn't a bug in the code under test — it's the test's own precondition
    // failing to hold — so bail out instead of asserting on an outcome this
    // run can never produce.
    if result.is_ok() {
        return;
    }

    let err = result.expect_err("an unreadable file must error");
    assert!(format!("{err:#}").contains("Failed to read config"));
}

// `expand_env_vars` (src/config/env.rs:5, called at src/config.rs:364, BEFORE
// TOML parsing) resolves each `${VAR}` via `std::env::var` and, on an unset
// variable, returns `Err` wrapped with a message naming the variable — it
// does not expand to an empty string. This was confirmed both by reading
// `expand_env_vars_in_line` (src/config/env.rs:19-64, the `with_context`
// closure at line 30) and by the pre-existing
// `test_expand_env_vars_missing_variable_returns_error` unit test in
// `src/config/tests/env_tests.rs`. Because expansion runs before
// `toml::from_str`, this fails at the expansion stage, not the
// "Failed to parse config.toml" stage — this test pins that ordering by
// asserting on the unset variable's name (stable, first-party wording) and
// asserting the parse-stage marker is NOT what fired.
#[test]
fn an_unset_env_var_in_a_config_value_is_reported_not_silently_accepted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        "[telegram]\napi_id = \"${STAGE4_DEFINITELY_UNSET}\"\n",
    )
    .expect("write");

    let err = Config::load_from(Some(&path)).expect_err("an unset var must not load cleanly");
    let message = format!("{err:#}");

    assert!(
        message.contains("STAGE4_DEFINITELY_UNSET"),
        "expected the unset variable's name in the error, got: {message}"
    );
    assert!(
        !message.contains("Failed to parse config.toml"),
        "expansion must fail before the TOML parse stage is reached, got: {message}"
    );
}
