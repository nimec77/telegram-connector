//! File-loading tests use immutable parent environments and isolated child overrides.
use crate::config::*;
use secrecy::ExposeSecret;
use std::fs;
use std::path::PathBuf;

fn run_config_child(case: &str, content: &str) {
    let dir = tempfile::tempdir().expect("config dir");
    let path = dir.path().join("config.toml");
    fs::write(&path, content).expect("config fixture");
    let mut command = super::environment_child(case);
    command
        .env("TELEGRAM_MCP_CONFIG", &path)
        .env("TEST_API_ID", "98765")
        .env("TEST_API_HASH", "expanded_hash")
        .env("TEST_PHONE", "+9876543210")
        .env("CONFIG_PARSE_TEST_HASH", "FAKE_CONFIG_PARSE_HASH_7c3a")
        .env("CONFIG_PARSE_TEST_PHONE", "+15550001234");
    super::assert_child_success(&mut command);
}

#[test]
fn test_load_valid_config() {
    run_config_child(
        "valid",
        "[telegram]\napi_id = 12345\napi_hash = \"test_hash\"\nphone_number = \"+1234567890\"\n",
    );
}
#[test]
fn test_load_config_with_env_vars() {
    run_config_child(
        "expanded",
        "[telegram]\napi_id = \"${TEST_API_ID}\"\napi_hash = \"${TEST_API_HASH}\"\nphone_number = \"${TEST_PHONE}\"\n",
    );
}
#[test]
fn test_load_missing_config() {
    let dir = tempfile::tempdir().expect("config dir");
    let mut command = super::environment_child("missing");
    command.env("TELEGRAM_MCP_CONFIG", dir.path().join("missing.toml"));
    super::assert_child_success(&mut command);
}
#[test]
fn test_load_invalid_toml() {
    run_config_child("invalid", "this is not valid TOML {{{}}}");
}
#[test]
fn test_load_config_with_file_logging_options() {
    run_config_child(
        "logging",
        "[telegram]\napi_id = 12345\n[logging]\nlevel = \"debug\"\nformat = \"json\"\nfile_enabled = false\nfile_path = \"/custom/log/path\"\nmax_log_days = 14\n",
    );
}

pub(super) fn run_child_case(case: &str) {
    if case == "parse_secrets" {
        config_parse_secrets_case();
        return;
    }
    let result = Config::load_from(None);
    match case {
        "valid" => {
            let config = result.expect("valid config");
            assert_eq!(config.telegram.api_id, 12345);
            assert_eq!(
                config.telegram.api_hash.as_ref().map(|s| s.expose_secret()),
                Some("test_hash")
            );
        }
        "expanded" => {
            let config = result.expect("expanded config");
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
        "logging" => {
            let config = result.expect("logging config");
            assert!(!config.logging.file_enabled);
            assert_eq!(config.logging.file_path, PathBuf::from("/custom/log/path"));
            assert_eq!(config.logging.max_log_days, 14);
        }
        "missing" | "invalid" => assert!(result.is_err()),
        "unset" => {
            let error = result.expect_err("unset environment must fail");
            let message = format!("{error:#}");
            assert!(message.contains("STAGE4_DEFINITELY_UNSET"));
            assert!(!message.contains("Failed to parse config.toml"));
        }
        _ => panic!("unknown child case"),
    }
}

// ============================================================================
// `Config::load_from` file-loading error branches (audit stage 4, Task 12).
//
// These exercise the actual `std::fs::read_to_string` -> `expand_env_vars` ->
// `toml::from_str` -> per-sub-config `validate()` pipeline in `load_from`
// (`src/config.rs:351`), via real temp files, rather than calling `validate()`
// directly on hand-built structs (as `validation_tests.rs` does). None of
// these tests mutate the process environment.
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

#[test]
fn an_unset_env_var_in_a_config_value_is_reported_not_silently_accepted() {
    run_config_child(
        "unset",
        "[telegram]\napi_id = \"${STAGE4_DEFINITELY_UNSET}\"\n",
    );
}

#[test]
fn config_parse_error_redacts_literal_and_expanded_secrets() {
    run_config_child("parse_secrets", "[telegram]\napi_id = 1\n");
}

fn config_parse_secrets_case() {
    let hash = "FAKE_CONFIG_PARSE_HASH_7c3a";
    let phone = "+15550001234";
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");

    // Both source excerpts and parser messages (including invalid values and keys)
    // can contain secrets. Exercise real loading, expansion, and deserialization.
    let cases = [
        ("hash syntax", "api_id = 1\napi_hash = \"HASH\";\n"),
        ("phone syntax", "api_id = 1\nphone_number = \"PHONE\";\n"),
        ("hash type", "api_id = \"HASH\"\n"),
        ("phone type", "api_id = \"PHONE\"\n"),
        ("hash key", "api_id = 1\nHASH = 1\nHASH = 2\n"),
    ];
    let mut failures = Vec::new();
    for expanded in [false, true] {
        let (hash_input, phone_input) = if expanded {
            ("${CONFIG_PARSE_TEST_HASH}", "${CONFIG_PARSE_TEST_PHONE}")
        } else {
            (hash, phone)
        };
        for (case, body) in cases {
            let content = format!("[telegram]\n{body}")
                .replace("HASH", hash_input)
                .replace("PHONE", phone_input);
            fs::write(&path, content).expect("write config");
            let err = Config::load_from(Some(&path))
                .expect_err("invalid config must fail")
                .context("configuration startup failed");
            let mut diagnostics = vec![
                err.to_string(),
                format!("{err:#}"),
                format!("{err:?}"),
                format!("{err:#?}"),
            ];
            for cause in err.chain() {
                diagnostics.push(cause.to_string());
                diagnostics.push(format!("{cause:?}"));
            }
            if diagnostics
                .iter()
                .any(|text| text.contains(hash) || text.contains(phone))
            {
                failures.push(format!("{case} (expanded={expanded}) leaks a secret"));
            }
            let chain = format!("{err:#}");
            if !chain.contains("line ") || !chain.contains("column ") {
                failures.push(format!("{case} (expanded={expanded}) lacks a location"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
