use super::*;
use crate::config::defaults::{
    default_limits_config, default_max_download_bytes, default_media_download_cost,
    default_observability_config, default_timeout_config, default_transcription_config,
    default_transcription_cost,
};
use secrecy::SecretString;
use std::path::PathBuf;
/// Re-exec a single ignored entrypoint. Command configures a new process environment;
/// neither this test process nor the child ever mutates its own environment.
fn environment_child(case: &str) -> std::process::Command {
    let mut command = std::process::Command::new(std::env::current_exe().expect("test executable"));
    command
        .args([
            "--exact",
            "config::tests::env_tests::environment_child",
            "--ignored",
            "--test-threads=1",
        ])
        .env("CONFIG_TEST_CASE", case)
        .env_remove("TELEGRAM_MCP_CONFIG")
        .env_remove("STAGE4_DEFINITELY_UNSET");
    command
}

fn assert_child_success(command: &mut std::process::Command) {
    let output = command.output().expect("config subprocess");
    assert!(
        output.status.success(),
        "config child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "child entrypoint did not run"
    );
}

fn create_test_config(api_id: i32, api_hash: Option<&str>, phone_number: Option<&str>) -> Config {
    Config {
        sending: Default::default(),
        media_artifacts: Default::default(),
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
            deadline_seconds: 20,
        },
        rate_limiting: RateLimitConfig {
            send_message_cost: 1,
            send_media_cost: 3,
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
            ..ServerConfig::default()
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

#[test]
fn server_resource_limits_reject_invalid_values_at_load() {
    for setting in [
        "max_request_bytes = 0",
        "max_request_bytes = 16777217",
        "max_in_flight_requests = 0",
        "max_in_flight_requests = 1025",
        "shutdown_timeout_seconds = 3601",
    ] {
        let file = tempfile::NamedTempFile::new().expect("config fixture");
        std::fs::write(
            file.path(),
            format!("[telegram]\napi_id = 1\n[server]\n{setting}\n"),
        )
        .expect("write config");
        assert!(
            Config::load_from(Some(file.path())).is_err(),
            "accepted {setting}"
        );
    }
}

#[test]
fn media_artifact_configuration_defaults_and_bounds() {
    let config: Config = toml::from_str("[telegram]\napi_id = 1\n").expect("configuration");
    assert!(!config.media_artifacts.enabled);
    assert!(!config.media_artifacts.file_export_enabled);
    assert!(config.media_artifacts.root.is_none());
    assert_eq!(config.media_artifacts.ttl_seconds, 600);
    assert_eq!(config.media_artifacts.max_entries, 100);
    assert_eq!(config.media_artifacts.max_total_bytes, 268_435_456);
    for setting in [
        "ttl_seconds = 0",
        "ttl_seconds = 3601",
        "max_entries = 0",
        "max_entries = 1001",
        "max_total_bytes = 0",
        "max_total_bytes = 1073741825",
        "root = 'relative'",
        "root = ''",
        "file_export_enabled = true",
    ] {
        let file = tempfile::NamedTempFile::new().expect("config fixture");
        std::fs::write(
            file.path(),
            format!("[telegram]\napi_id = 1\n[media_artifacts]\n{setting}\n"),
        )
        .expect("write");
        assert!(
            Config::load_from(Some(file.path())).is_err(),
            "accepted {setting}"
        );
    }
}
