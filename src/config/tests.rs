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
