//! Operational bounds and setup credential validation.

use super::{Config, LimitsConfig, ServerConfig};

impl ServerConfig {
    /// Reject unbounded input/admission and unreasonable drain budgets.
    ///
    /// # Errors
    /// Returns an error when request bytes or in-flight admission are outside their
    /// supported ranges, or the shutdown budget exceeds one hour.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=16_777_216).contains(&self.max_request_bytes),
            "server.max_request_bytes must be 1..=16777216"
        );
        anyhow::ensure!(
            (1..=1024).contains(&self.max_in_flight_requests),
            "server.max_in_flight_requests must be 1..=1024"
        );
        anyhow::ensure!(
            self.shutdown_timeout_seconds <= 3600,
            "server.shutdown_timeout_seconds must be <= 3600"
        );
        Ok(())
    }
}

impl super::SendingConfig {
    /// Check local settings; filesystem ownership/ACL checks occur at media startup.
    ///
    /// # Errors
    /// Rejects legacy-enabled writes, unsupported caps, aliases or media configuration.
    pub fn validate(&self, format: super::OutputFormat) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.enabled || format == super::OutputFormat::JsonV1,
            "sending requires canonical json_v1 output"
        );
        self.staging_config().validate()
    }
}

/// Floor below which an image would be downscaled past usefulness. Once a
/// batch's remaining budget drops under this, `Base64Budget::allowance`
/// returns `None` rather than emitting an unreadable thumbnail — which is why
/// `limits.media_batch_max_total_bytes` is validated against it here.
pub(crate) const MIN_IMAGE_BASE64_BYTES: usize = 32_768;

impl LimitsConfig {
    /// Reject a zero budget — it would make every response over-cap. Also
    /// reject a cap below `MIN_IMAGE_BASE64_BYTES`: `Base64Budget::allowance`
    /// returns `None` once the remaining budget drops under that floor, so a
    /// cap set below it makes every image in every batch call report
    /// `payload_cap_reached` after the downloads already happened — a silent,
    /// costly no-op at runtime. Failing fast at startup is cheaper.
    ///
    /// # Errors
    /// Returns an error for a zero response budget or a media batch budget below
    /// the minimum useful image allowance.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.response_byte_budget == 0 {
            anyhow::bail!("limits.response_byte_budget must be > 0");
        }
        if self.media_batch_max_total_bytes == 0 {
            anyhow::bail!("limits.media_batch_max_total_bytes must be > 0");
        }
        if self.media_batch_max_total_bytes < MIN_IMAGE_BASE64_BYTES as u64 {
            anyhow::bail!(
                "limits.media_batch_max_total_bytes must be >= {MIN_IMAGE_BASE64_BYTES} bytes \
                 (below that floor every image reports payload_cap_reached), got {}",
                self.media_batch_max_total_bytes
            );
        }
        Ok(())
    }
}

impl Config {
    /// Validate that authentication credentials are present (required for --setup mode)
    /// Only checks the presence of nonempty api_hash and phone_number; it does
    /// not authenticate, validate API ID ranges, or contact Telegram.
    ///
    /// # Errors
    /// Returns a secret-free description of required fields when either is absent.
    pub fn validate_for_setup(&self) -> anyhow::Result<()> {
        if !self.telegram.has_auth_credentials() {
            anyhow::bail!(
                "Authentication credentials required for setup mode.\n\
                Please ensure these are set in your config.toml:\n\
                - telegram.api_hash\n\
                - telegram.phone_number\n\n\
                You can use environment variables: api_hash = \"${{YOUR_ENV_VAR}}\"\n\
                Get your API credentials from: https://my.telegram.org"
            );
        }
        Ok(())
    }
}

impl super::MediaArtifactsConfig {
    /// Validate cache bounds and platform support without filesystem work.
    ///
    /// # Errors
    /// Returns an error for unavailable exports, unsafe roots, unsupported platforms or bounds.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.file_export_enabled || self.enabled,
            "media_artifacts file export requires enabled"
        );
        anyhow::ensure!(
            (1..=3600).contains(&self.ttl_seconds),
            "media_artifacts.ttl_seconds must be 1..=3600"
        );
        anyhow::ensure!(
            (1..=1000).contains(&self.max_entries),
            "media_artifacts.max_entries must be 1..=1000"
        );
        anyhow::ensure!(
            (1..=1_073_741_824).contains(&self.max_total_bytes),
            "media_artifacts.max_total_bytes must be 1..=1073741824"
        );
        if let Some(root) = &self.root {
            anyhow::ensure!(
                root.is_absolute(),
                "media_artifacts.root must be a nonempty absolute path"
            );
        }
        anyhow::ensure!(
            !self.enabled || cfg!(unix),
            "media_artifacts requires Unix ownership checks"
        );
        Ok(())
    }
}
