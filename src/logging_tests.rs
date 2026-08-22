use super::*;

// ========================================================================
// Phone Number Redaction Tests
// ========================================================================

#[test]
fn redact_phone_normal_length() {
    // Standard international phone number
    let phone = "+1234567890";
    let redacted = redact_phone(phone);
    assert_eq!(redacted, "+123***890");
}

#[test]
fn redact_phone_longer_number() {
    // Longer phone number
    let phone = "+12345678901234";
    let redacted = redact_phone(phone);
    assert_eq!(redacted, "+123***234");
}

#[test]
fn redact_phone_hides_nothing_at_seven_chars_so_it_redacts_wholesale() {
    // 4 visible leading + 3 visible trailing == the whole 7-char string, so
    // the "redacted" form would have printed the phone verbatim.
    let phone = "+123456";
    let redacted = redact_phone(phone);
    assert_eq!(redacted, "[REDACTED]");
}

#[test]
fn redact_phone_redacts_wholesale_below_three_hidden_chars() {
    // 9 chars: only 2 would be hidden.
    let phone = "+12345678";
    let redacted = redact_phone(phone);
    assert_eq!(redacted, "[REDACTED]");
}

#[test]
fn redact_phone_shows_edges_once_three_chars_are_hidden() {
    // 10 chars is the shortest input that hides 3.
    let phone = "+123456789";
    let redacted = redact_phone(phone);
    assert_eq!(redacted, "+123***789");
}

#[test]
fn redact_phone_too_short() {
    // Phone too short to redact safely (≤6 chars)
    let phone = "+12345";
    let redacted = redact_phone(phone);
    assert_eq!(redacted, "[REDACTED]");
}

#[test]
fn redact_phone_empty_string() {
    let phone = "";
    let redacted = redact_phone(phone);
    assert_eq!(redacted, "[REDACTED]");
}

#[test]
fn redact_phone_multibyte_input_does_not_panic() {
    // 10 chars but 30 bytes: byte index 4 is not a char boundary, so the old
    // byte-slicing implementation panicked here.
    assert_eq!(redact_phone("€€€€€€€€€€"), "€€€€***€€€");
}

// ========================================================================
// Initialization Tests
// ========================================================================

#[test]
fn init_with_valid_config() {
    // Test that init succeeds with a valid configuration
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "compact".to_string(),
        file_enabled: false, // Disable file logging for tests
        file_path: std::path::PathBuf::from("/tmp/test_logs"),
        max_log_days: 7,
    };

    // Should not panic or return error
    let result = init(&config);
    assert!(result.is_ok());
}

#[test]
fn init_with_different_log_levels() {
    // Test various log levels
    let levels = vec!["trace", "debug", "info", "warn", "error"];

    for level in levels {
        let config = LoggingConfig {
            level: level.to_string(),
            format: "compact".to_string(),
            file_enabled: false,
            file_path: std::path::PathBuf::from("/tmp/test_logs"),
            max_log_days: 7,
        };

        let result = init(&config);
        assert!(result.is_ok(), "Failed to init with level: {}", level);
    }
}

#[test]
fn init_propagates_a_file_layer_failure() {
    // The only errors `init` swallows are `try_init`'s, which are both
    // "already initialized". A failure to build the file layer must still
    // reach the caller — here the log directory's parent is a regular file,
    // so `create_dir_all` cannot succeed.
    let blocker = tempfile::NamedTempFile::new().expect("temp file");
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "compact".to_string(),
        file_enabled: true,
        file_path: blocker.path().join("logs"),
        max_log_days: 7,
    };

    assert!(init(&config).is_err());
}

#[test]
fn init_with_different_formats() {
    // Test various formats
    let formats = vec!["compact", "pretty", "json"];

    for format in formats {
        let config = LoggingConfig {
            level: "info".to_string(),
            format: format.to_string(),
            file_enabled: false,
            file_path: std::path::PathBuf::from("/tmp/test_logs"),
            max_log_days: 7,
        };

        let result = init(&config);
        assert!(result.is_ok(), "Failed to init with format: {}", format);
    }
}

// ========================================================================
// File Layer Tests
// ========================================================================

#[test]
fn build_file_layer_creates_directory() {
    // Test that build_file_layer creates the log directory if it doesn't exist
    let temp_dir = tempfile::tempdir().unwrap();
    let log_dir = temp_dir.path().join("new_log_dir");

    // Directory should not exist yet
    assert!(!log_dir.exists());

    let config = LoggingConfig {
        level: "info".to_string(),
        format: "compact".to_string(),
        file_enabled: true,
        file_path: log_dir.clone(),
        max_log_days: 7,
    };

    // Build file layer should create the directory
    let result = build_file_layer::<tracing_subscriber::Registry>(&config);
    assert!(result.is_ok());

    // Directory should now exist
    assert!(log_dir.exists());
    assert!(log_dir.is_dir());
}

#[test]
fn build_file_layer_works_with_existing_directory() {
    // Test that build_file_layer works with an existing directory
    let temp_dir = tempfile::tempdir().unwrap();
    let log_dir = temp_dir.path().to_path_buf();

    // Directory already exists
    assert!(log_dir.exists());

    let config = LoggingConfig {
        level: "info".to_string(),
        format: "compact".to_string(),
        file_enabled: true,
        file_path: log_dir.clone(),
        max_log_days: 7,
    };

    // Build file layer should succeed
    let result = build_file_layer::<tracing_subscriber::Registry>(&config);
    assert!(result.is_ok());
}

#[test]
fn build_stderr_layer_compact() {
    // Test compact format stderr layer
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "compact".to_string(),
        file_enabled: false,
        file_path: std::path::PathBuf::from("/tmp/test_logs"),
        max_log_days: 7,
    };

    // Should not panic
    let _layer = build_stderr_layer::<tracing_subscriber::Registry>(&config);
}

#[test]
fn build_stderr_layer_json() {
    // Test json format stderr layer
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "json".to_string(),
        file_enabled: false,
        file_path: std::path::PathBuf::from("/tmp/test_logs"),
        max_log_days: 7,
    };

    // Should not panic
    let _layer = build_stderr_layer::<tracing_subscriber::Registry>(&config);
}

#[test]
fn build_stderr_layer_pretty() {
    // Test pretty format stderr layer
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "pretty".to_string(),
        file_enabled: false,
        file_path: std::path::PathBuf::from("/tmp/test_logs"),
        max_log_days: 7,
    };

    // Should not panic
    let _layer = build_stderr_layer::<tracing_subscriber::Registry>(&config);
}

#[test]
fn build_stderr_layer_unknown_defaults_to_compact() {
    // Test that unknown formats default to compact
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "unknown_format".to_string(),
        file_enabled: false,
        file_path: std::path::PathBuf::from("/tmp/test_logs"),
        max_log_days: 7,
    };

    // Should not panic - defaults to compact
    let _layer = build_stderr_layer::<tracing_subscriber::Registry>(&config);
}

// ========================================================================
// Log Cleanup Tests
// ========================================================================

#[test]
fn cleanup_skipped_when_file_disabled() {
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "compact".to_string(),
        file_enabled: false, // Disabled
        file_path: std::path::PathBuf::from("/tmp/nonexistent"),
        max_log_days: 7,
    };

    let result = cleanup_old_logs(&config);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}

#[test]
fn cleanup_skipped_when_max_days_zero() {
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "compact".to_string(),
        file_enabled: true,
        file_path: std::path::PathBuf::from("/tmp/nonexistent"),
        max_log_days: 0, // Zero = keep forever
    };

    let result = cleanup_old_logs(&config);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}

#[test]
fn cleanup_handles_missing_directory() {
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "compact".to_string(),
        file_enabled: true,
        file_path: std::path::PathBuf::from("/tmp/definitely_nonexistent_dir_12345"),
        max_log_days: 7,
    };

    let result = cleanup_old_logs(&config);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}

#[test]
fn cleanup_removes_old_log_files() {
    use filetime::{FileTime, set_file_mtime};
    use std::fs::File;
    use std::io::Write;

    let temp_dir = tempfile::tempdir().unwrap();
    let log_dir = temp_dir.path();

    // Create an "old" log file (modified 10 days ago)
    let old_log = log_dir.join("telegram-connector.2025-01-01.log");
    File::create(&old_log)
        .unwrap()
        .write_all(b"old log")
        .unwrap();
    let ten_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 86400);
    set_file_mtime(&old_log, FileTime::from_system_time(ten_days_ago)).unwrap();

    // Create a "recent" log file (now)
    let recent_log = log_dir.join("telegram-connector.2025-01-09.log");
    File::create(&recent_log)
        .unwrap()
        .write_all(b"recent log")
        .unwrap();

    let config = LoggingConfig {
        level: "info".to_string(),
        format: "compact".to_string(),
        file_enabled: true,
        file_path: log_dir.to_path_buf(),
        max_log_days: 7,
    };

    let result = cleanup_old_logs(&config);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 1); // 1 file removed

    // Old file should be gone, recent should remain
    assert!(!old_log.exists());
    assert!(recent_log.exists());
}

#[test]
fn cleanup_ignores_non_log_files() {
    use filetime::{FileTime, set_file_mtime};
    use std::fs::File;
    use std::io::Write;

    let temp_dir = tempfile::tempdir().unwrap();
    let log_dir = temp_dir.path();

    // Create an old non-log file
    let old_txt = log_dir.join("notes.txt");
    File::create(&old_txt).unwrap().write_all(b"notes").unwrap();
    let ten_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 86400);
    set_file_mtime(&old_txt, FileTime::from_system_time(ten_days_ago)).unwrap();

    let config = LoggingConfig {
        level: "info".to_string(),
        format: "compact".to_string(),
        file_enabled: true,
        file_path: log_dir.to_path_buf(),
        max_log_days: 7,
    };

    let result = cleanup_old_logs(&config);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0); // No files removed

    // Non-log file should still exist
    assert!(old_txt.exists());
}

#[test]
fn cleanup_handles_empty_directory() {
    let temp_dir = tempfile::tempdir().unwrap();

    let config = LoggingConfig {
        level: "info".to_string(),
        format: "compact".to_string(),
        file_enabled: true,
        file_path: temp_dir.path().to_path_buf(),
        max_log_days: 7,
    };

    let result = cleanup_old_logs(&config);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}
