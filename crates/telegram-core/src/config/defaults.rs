//! Shared connection and search defaults.

use super::{SearchConfig, TimeoutConfig};
use std::path::PathBuf;

/// The user's config directory, or `None` when no home directory can be
/// determined. Split out from [`config_dir_join`] so the join is pure.
pub fn project_config_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "telegram-connector")
        .map(|dirs| dirs.config_dir().to_path_buf())
}

/// Append `relative` to the config directory, degrading to a bare relative
/// path when there is none.
///
/// Serde `default = "..."` providers cannot return an error, so a missing
/// home directory must not abort deserialization — `Config::load_from` and
/// `logging::init` surface the resulting path in their own errors instead.
pub fn config_dir_join(config_dir: Option<PathBuf>, relative: &str) -> PathBuf {
    match config_dir {
        Some(dir) => dir.join(relative),
        None => PathBuf::from(relative),
    }
}

pub fn default_session_file() -> PathBuf {
    config_dir_join(project_config_dir(), "session.bin")
}

pub fn default_search_deadline_seconds() -> u64 {
    20
}

pub fn default_resolve_secs() -> u64 {
    30
}

pub fn default_history_secs() -> u64 {
    60
}

pub fn default_search_secs() -> u64 {
    120
}

pub fn default_download_secs() -> u64 {
    120
}

pub fn default_max_download_bytes() -> u64 {
    20 * 1024 * 1024
}

pub fn default_timeout_config() -> TimeoutConfig {
    TimeoutConfig {
        send_secs: default_send_secs(),
        upload_send_secs: default_upload_send_secs(),
        resolve_secs: default_resolve_secs(),
        history_secs: default_history_secs(),
        search_secs: default_search_secs(),
        download_secs: default_download_secs(),
    }
}

pub fn default_search_config() -> SearchConfig {
    SearchConfig {
        deadline_seconds: default_search_deadline_seconds(),
    }
}

pub fn default_send_secs() -> u64 {
    30
}

pub fn default_upload_send_secs() -> u64 {
    120
}
