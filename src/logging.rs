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

    // Combine layers and initialize.
    //
    // `try_init` has exactly two failure modes, and both mean "already
    // initialized": `set_global_default` (a global subscriber exists) and
    // `LogTracer::init` (the `log` crate's logger is set). Neither is worth
    // failing startup over — repeat calls and the test harness hit both — and
    // `TryInitError` boxes its cause privately (its `source()` forwards to the
    // cause's *own* source), so the two cannot be told apart by type anyway.
    //
    // Errors that are not double-init never reach here: `build_file_layer`
    // propagates through `?` above.
    if let Err(error) = tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init()
    {
        // Goes to the subscriber that is already installed, which is the point.
        tracing::debug!(%error, "tracing subscriber already initialized; keeping the existing one");
    }

    Ok(())
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

/// Characters a partial redaction must hide to be worth doing. Below this,
/// the "redacted" form would echo nearly the whole secret — a 7-character
/// phone once rendered as `+123***456`, i.e. every character it had.
const MIN_HIDDEN_CHARS: usize = 3;

/// Show the first `visible_start` and last `visible_end` characters, hiding
/// the middle. Falls back to `[REDACTED]` when that would hide fewer than
/// [`MIN_HIDDEN_CHARS`] characters.
///
/// Char-aware: byte slicing panics mid-codepoint, and neither a
/// config-supplied phone number nor an API hash is guaranteed to be ASCII.
fn redact(value: &str, visible_start: usize, visible_end: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < visible_start + visible_end + MIN_HIDDEN_CHARS {
        return "[REDACTED]".to_string();
    }

    let start: String = chars[..visible_start].iter().collect();
    let end: String = chars[chars.len() - visible_end..].iter().collect();
    format!("{start}***{end}")
}

/// Redact phone number for safe logging
/// Shows first 4 chars + last 3 chars, hides middle
/// Returns "[REDACTED]" for strings under 10 characters
pub fn redact_phone(phone: &str) -> String {
    redact(phone, 4, 3)
}

/// Redact API hash for safe logging
/// Shows first 4 chars + last 1 char, hides middle
/// Returns "[REDACTED]" for strings under 8 characters
pub fn redact_hash(hash: &str) -> String {
    redact(hash, 4, 1)
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
