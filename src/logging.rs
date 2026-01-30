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
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("telegram-connector")
        .filename_suffix("log")
        .build(&config.file_path)?;

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
#[path = "logging_tests.rs"]
mod tests;
