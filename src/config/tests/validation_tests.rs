//! Validation rules, credential predicates, and secret redaction.
use super::create_test_config;
use crate::config::*;

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
    let (api_hash, phone) = config
        .telegram
        .auth_credentials()
        .expect("both credentials are present");
    assert_eq!(api_hash, "test_hash");
    assert_eq!(phone, "+1234567890");
}

#[test]
fn auth_credentials_returns_none_when_api_hash_is_missing() {
    let config = create_test_config(12345, None, Some("+1234567890"));
    assert!(config.telegram.auth_credentials().is_none());
}

#[test]
fn auth_credentials_returns_none_when_phone_number_is_missing() {
    let config = create_test_config(12345, Some("hash"), None);
    assert!(config.telegram.auth_credentials().is_none());
}

#[test]
fn auth_credentials_returns_none_for_an_empty_credential() {
    // `has_auth_credentials` treats an empty string as absent; the getter that
    // backs it must agree, or the two could disagree. `create_test_config`
    // filters empty strings to `None`, so set the empty secret directly.
    let mut config = create_test_config(12345, Some("hash"), Some("+1234567890"));
    config.telegram.api_hash = Some(SecretString::new(String::new().into_boxed_str()));
    assert!(config.telegram.auth_credentials().is_none());
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

#[test]
fn test_download_secs_zero_fails_validation() {
    let toml_str = "[telegram]\napi_id = 12345\n[telegram.timeouts]\ndownload_secs = 0\n";
    let config: Config = toml::from_str(toml_str).unwrap();
    let result = config.telegram.timeouts.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("download_secs"));
}

#[test]
fn retired_search_keys_are_ignored_not_rejected() {
    // `default_hours_back` / `max_results_default` / `max_results_limit` were
    // deserialized but never read — the behaviour lives in the
    // `SearchParams`/`HistoryParams` constants. A config that still carries
    // them must keep loading, with the keys ignored.
    let config: Config = toml::from_str(
        "[telegram]\napi_id = 123\n\n[search]\ndefault_hours_back = 200\n\
         max_results_default = 5\nmax_results_limit = 7\n",
    )
    .expect("retired keys must be ignored, not rejected");
    assert_eq!(
        config.search.deadline_seconds,
        default_search_deadline_seconds()
    );
}

#[test]
fn limits_config_rejects_zero_budget() {
    let config: Config =
        toml::from_str("[telegram]\napi_id = 123\n\n[limits]\nresponse_byte_budget = 0\n")
            .expect("parse");
    assert!(config.limits.validate().is_err());
}

#[test]
fn search_config_rejects_zero_deadline() {
    let config: Config =
        toml::from_str("[telegram]\napi_id = 123\n\n[search]\ndeadline_seconds = 0\n")
            .expect("parse");
    assert!(config.search.validate().is_err());
}

#[test]
fn search_config_rejects_deadline_over_one_hour() {
    let config: Config =
        toml::from_str("[telegram]\napi_id = 123\n\n[search]\ndeadline_seconds = 3601\n")
            .expect("parse");
    assert!(config.search.validate().is_err());
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
fn a_non_positive_refill_rate_is_rejected() {
    // refill_rate = 0 never refills, so the first rejection's retry hint is
    // ceil(deficit / 0) = inf, saturating to u64::MAX seconds; a negative rate
    // drains the bucket over time; NaN slips past a plain `<= 0.0` check.
    // Each is a permanent lockout, not a tight limit.
    for rate in [0.0, -1.0, f64::NAN] {
        let config = RateLimitConfig {
            max_tokens: 10,
            refill_rate: rate,
            media_download_cost: 3,
            transcription_cost: 5,
        };
        let err = config
            .validate()
            .expect_err("a refill_rate that can never refill must be rejected");
        assert!(
            err.to_string().contains("refill_rate"),
            "the error must name the offending key, got: {err}"
        );
    }
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
