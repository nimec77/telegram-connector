use crate::config::LoggingConfig;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Initialize tracing subscriber with configured format and output
/// Supports dual output: stderr (configurable format) and file (JSON, daily rotation)
pub fn init(config: &LoggingConfig) -> anyhow::Result<()> {
    // Build filter from config level or environment variable
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.level));

    // Build stderr layer with configured format
    let stderr_layer = build_stderr_layer(config);

    // Build file layer if enabled
    let file_layer = if config.file_enabled {
        Some(build_file_layer(config)?)
    } else {
        None
    };

    // Combine layers and initialize
    // Use try_init() to gracefully handle already-initialized subscriber (common in tests)
    let result = tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init();

    // Ignore error if subscriber is already initialized (common in tests)
    result.or(Ok(()))
}

/// Build stderr layer with configured format
fn build_stderr_layer<S>(config: &LoggingConfig) -> Box<dyn Layer<S> + Send + Sync>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    match config.format.as_str() {
        "json" => tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .json()
            .boxed(),
        "pretty" => tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .pretty()
            .boxed(),
        _ => {
            // Default to compact
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .compact()
                .boxed()
        }
    }
}

/// Build file layer with daily rotation and JSON format
fn build_file_layer<S>(config: &LoggingConfig) -> anyhow::Result<Box<dyn Layer<S> + Send + Sync>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    // Create log directory if it doesn't exist
    std::fs::create_dir_all(&config.file_path)?;

    // Create rolling file appender with daily rotation
    let file_appender =
        RollingFileAppender::new(Rotation::DAILY, &config.file_path, "telegram-connector.log");

    // File layer always uses JSON format for structured logging
    Ok(tracing_subscriber::fmt::layer()
        .with_writer(file_appender)
        .json()
        .boxed())
}

/// Redact phone number for safe logging
/// Shows first 4 chars + last 3 chars, hides middle
/// Returns "[REDACTED]" for strings ≤6 characters
pub fn redact_phone(phone: &str) -> String {
    if phone.len() <= 6 {
        return "[REDACTED]".to_string();
    }

    let visible_start = 4;
    let visible_end = 3;

    format!(
        "{}***{}",
        &phone[..visible_start],
        &phone[phone.len() - visible_end..]
    )
}

/// Redact API hash for safe logging
/// Shows first 4 chars + last 1 char, hides middle
/// Returns "[REDACTED]" for strings ≤6 characters
pub fn redact_hash(hash: &str) -> String {
    if hash.len() <= 6 {
        return "[REDACTED]".to_string();
    }

    let visible_start = 4;
    let visible_end = 1;

    format!(
        "{}***{}",
        &hash[..visible_start],
        &hash[hash.len() - visible_end..]
    )
}

/// Clean up old log files based on max_log_days configuration.
/// Returns the number of files removed.
///
/// Skips cleanup if:
/// - file_enabled is false
/// - max_log_days is 0 (keep logs forever)
/// - log directory doesn't exist
pub fn cleanup_old_logs(config: &LoggingConfig) -> anyhow::Result<usize> {
    // Skip if file logging disabled or max_days is 0 (infinite retention)
    if !config.file_enabled || config.max_log_days == 0 {
        return Ok(0);
    }

    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(u64::from(config.max_log_days) * 86400);

    // Gracefully handle missing directory
    let entries = match std::fs::read_dir(&config.file_path) {
        Ok(e) => e,
        Err(_) => return Ok(0),
    };

    let mut removed = 0;

    for entry in entries.flatten() {
        let path = entry.path();

        // Only process log files (pattern: telegram-connector.log.YYYY-MM-DD)
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !file_name.contains(".log") {
            continue;
        }

        if let Ok(metadata) = entry.metadata()
            && let Ok(modified) = metadata.modified()
            && modified < cutoff
            && std::fs::remove_file(&path).is_ok()
        {
            removed += 1;
        }
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
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
    fn redact_phone_exactly_minimum_length() {
        // Phone with 7 characters (minimum: 4 visible start + 3 visible end)
        let phone = "+123456";
        let redacted = redact_phone(phone);
        assert_eq!(redacted, "+123***456");
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

    // ========================================================================
    // API Hash Redaction Tests
    // ========================================================================

    #[test]
    fn redact_hash_normal_length() {
        // Standard API hash
        let hash = "abc123def456";
        let redacted = redact_hash(hash);
        assert_eq!(redacted, "abc1***6");
    }

    #[test]
    fn redact_hash_long_string() {
        // Longer hash
        let hash = "abcdefghijklmnopqrstuvwxyz";
        let redacted = redact_hash(hash);
        assert_eq!(redacted, "abcd***z");
    }

    #[test]
    fn redact_hash_exactly_minimum_length() {
        // Hash with 7 characters (minimum: 4 visible start + 1 visible end)
        let hash = "abcdefg";
        let redacted = redact_hash(hash);
        assert_eq!(redacted, "abcd***g");
    }

    #[test]
    fn redact_hash_too_short() {
        // Hash too short to redact safely (≤6 chars)
        let hash = "abc123";
        let redacted = redact_hash(hash);
        assert_eq!(redacted, "[REDACTED]");
    }

    #[test]
    fn redact_hash_empty_string() {
        let hash = "";
        let redacted = redact_hash(hash);
        assert_eq!(redacted, "[REDACTED]");
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
        let old_log = log_dir.join("telegram-connector.log.2025-01-01");
        File::create(&old_log)
            .unwrap()
            .write_all(b"old log")
            .unwrap();
        let ten_days_ago =
            std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 86400);
        set_file_mtime(&old_log, FileTime::from_system_time(ten_days_ago)).unwrap();

        // Create a "recent" log file (now)
        let recent_log = log_dir.join("telegram-connector.log.2025-01-09");
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
        let ten_days_ago =
            std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 86400);
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
}
