//! Effective outgoing policy and private file staging (independent of media previews).

use crate::config::{Config, OutputFormat, SendingConfig};
use crate::telegram::types::sending::{SendFailure, SendFailureKind};
use std::path::{Path, PathBuf};

mod privacy;
mod probe;
mod reader;
pub(crate) mod staging;
pub use reader::StagedReader;
pub use staging::StagedUpload;

#[cfg(all(test, unix))]
#[path = "sending/tests/privacy_tests.rs"]
mod privacy_tests;
#[cfg(test)]
#[path = "sending/tests/probe_tests.rs"]
mod probe_tests;
#[cfg(all(test, unix))]
#[path = "sending/tests/staging_tests.rs"]
mod staging_tests;
#[cfg(all(test, unix))]
#[path = "sending/tests/fixtures.rs"]
pub(crate) mod test_support;

/// Validated settings and effective protected paths after CLI session overrides.
#[derive(Clone)]
pub struct SendingRuntimeConfig {
    pub(crate) settings: SendingConfig,
    pub(crate) output_format: OutputFormat,
    pub(crate) send_secs: u64,
    pub(crate) upload_send_secs: u64,
    pub(crate) send_message_cost: u32,
    pub(crate) send_media_cost: u32,
    pub(crate) protected_paths: Vec<PathBuf>,
}

impl SendingRuntimeConfig {
    /// Validate settings without creating directories or inspecting source files.
    /// Filesystem policy is enforced only when enabled media storage is initialized.
    ///
    /// # Errors
    /// Rejects incompatible output mode, invalid limits, timeouts or enabled costs.
    pub fn try_from_config(config: &Config, config_path: &Path) -> Result<Self, SendFailure> {
        let invalid = |_| SendFailure::not_sent(SendFailureKind::InvalidInput, None);
        config
            .sending
            .validate(config.server.output_format)
            .map_err(invalid)?;
        config.telegram.timeouts.validate().map_err(invalid)?;
        if config.sending.enabled {
            config
                .rate_limiting
                .validate_sending_costs()
                .map_err(invalid)?;
        }
        let mut protected_paths = vec![
            config_path.to_path_buf(),
            config.telegram.session_file.clone(),
        ];
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut sidecar = config.telegram.session_file.as_os_str().to_os_string();
            sidecar.push(suffix);
            protected_paths.push(sidecar.into());
        }
        // LoggingConfig.file_path names the rolling-log directory, not a file.
        protected_paths.push(config.logging.file_path.clone());
        if let Some(root) = &config.media_artifacts.root {
            protected_paths.push(root.clone());
        }
        Ok(Self {
            settings: config.sending.clone(),
            output_format: config.server.output_format,
            send_secs: config.telegram.timeouts.send_secs,
            upload_send_secs: config.telegram.timeouts.upload_send_secs,
            send_message_cost: config.rate_limiting.send_message_cost,
            send_media_cost: config.rate_limiting.send_media_cost,
            protected_paths,
        })
    }
}
