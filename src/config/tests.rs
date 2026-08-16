use super::*;
use std::env;
use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Serializes tests that mutate process environment variables. The test
/// harness runs tests on parallel threads within one process and the
/// environment is process-global. Tests never take this lock directly —
/// construct an `EnvGuard`, which holds the lock and restores every touched
/// variable on drop, even on panic.
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
///
/// Note: never construct two guards in one scope — `new()` takes the
/// non-reentrant `ENV_LOCK`, so nesting self-deadlocks.
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

#[path = "tests/defaults_tests.rs"]
mod defaults_tests;
#[path = "tests/env_tests.rs"]
mod env_tests;
#[path = "tests/load_tests.rs"]
mod load_tests;
#[path = "tests/validation_tests.rs"]
mod validation_tests;

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
    std::fs::write(&path, "this is not = = valid toml").expect("write");

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
    std::fs::write(
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
    std::fs::write(
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
    std::fs::write(
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
    std::fs::write(
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
    std::fs::write(&path, "[telegram]\napi_id = 1\n").expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");

    let result = Config::load_from(Some(&path));

    // Restore before asserting so the tempdir cleans up even on failure (and
    // so a run as root, where chmod 0 does not block reads, does not leak a
    // 0-mode file if a later assertion panics).
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
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
    std::fs::write(
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
