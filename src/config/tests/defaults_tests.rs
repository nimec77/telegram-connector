//! Defaults and in-memory TOML table parsing (no env, no files).
use crate::config::*;

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
// Config-Directory Fallback Tests
// ========================================================================

#[test]
fn config_dir_join_appends_to_the_config_directory_when_one_exists() {
    let joined = config_dir_join(
        Some(PathBuf::from("/home/u/.config/telegram-connector")),
        "session.bin",
    );
    assert_eq!(
        joined,
        PathBuf::from("/home/u/.config/telegram-connector/session.bin")
    );
}

#[test]
fn config_dir_join_falls_back_to_a_relative_path_when_there_is_no_config_directory() {
    // `ProjectDirs::from` returns `None` when no home directory can be
    // determined. These are `#[serde(default = "...")]` providers, which
    // cannot return an error — so they degrade to a relative path instead of
    // panicking mid-deserialization.
    assert_eq!(config_dir_join(None, "logs"), PathBuf::from("logs"));
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

// ========================================================================
// Search Deadline Config Tests (global-search-latency, Task 3)
// ========================================================================

#[test]
fn search_deadline_defaults_to_twenty_seconds() {
    let config: Config = toml::from_str("[telegram]\napi_id = 123\n").expect("parse");
    assert_eq!(config.search.deadline_seconds, 20);
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
