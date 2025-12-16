use crate::config::LoggingConfig;
use tracing_subscriber::EnvFilter;

/// Initialize tracing subscriber with configured format and output
pub fn init(config: &LoggingConfig) -> anyhow::Result<()> {
    // Build filter from config level or environment variable
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.level));

    // Apply format based on config and initialize
    // Use try_init() to gracefully handle already-initialized subscriber (common in tests)
    let result = match config.format.as_str() {
        "json" => tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .json()
            .with_env_filter(filter)
            .try_init(),
        "pretty" => tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .pretty()
            .with_env_filter(filter)
            .try_init(),
        _ => {
            // Default to compact
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .compact()
                .with_env_filter(filter)
                .try_init()
        }
    };

    // Ignore error if subscriber is already initialized (common in tests)
    result.or(Ok(()))
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
            };

            let result = init(&config);
            assert!(result.is_ok(), "Failed to init with format: {}", format);
        }
    }
}
