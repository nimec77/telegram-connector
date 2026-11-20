//! File/environment loading, path selection and in-memory CLI overrides.

use super::Config;
use super::env::parse_config_with;
use std::path::PathBuf;

impl Config {
    /// Load configuration from a specific path or default location
    ///
    /// If `config_path` is Some, uses that path. Otherwise, resolves the default path.
    /// Environment variables using `${VAR}` syntax are expanded in parsed TOML values.
    ///
    /// Selects the explicit path, otherwise `TELEGRAM_MCP_CONFIG`, otherwise the
    /// platform config directory. An unreadable selected path returns an error.
    /// The file is read but never written; expansion does not mutate
    /// the process environment and replacement values are not expanded recursively.
    /// Validates timeouts, search, rate, server and output limits. This does not
    /// authenticate or open a Telegram session. Call [`Self::validate_for_setup`]
    /// separately when setup credentials are needed.
    ///
    /// # Errors
    /// Returns contextual `anyhow` failures for path/read errors, missing or
    /// non-Unicode values in referenced variables, malformed TOML and invalid limits. TOML
    /// parser text and non-Unicode values are discarded so credentials do not
    /// survive in Display, Debug or source chains; safe line/column context remains.
    /// Paths and referenced environment variable names can appear in diagnostics.
    pub fn load_from(config_path: Option<&std::path::Path>) -> anyhow::Result<Self> {
        use anyhow::Context;

        let path = Self::selected_path(config_path)?;

        let content = std::fs::read_to_string(&path)
            .context(format!("Failed to read config: {}", path.display()))?;

        let config = parse_config_with(&content, |name| std::env::var(name))?;

        // Validate sub-configs that have non-credential invariants. Credential
        // validation is deferred to `validate_for_setup()` because the daemon path
        // does not require api_hash/phone_number at startup.
        config
            .telegram
            .validate_media_limits()
            .context("invalid telegram media configuration")?;
        config
            .telegram
            .timeouts
            .validate()
            .context("invalid telegram.timeouts configuration")?;

        config
            .limits
            .validate()
            .context("invalid limits configuration")?;

        config
            .search
            .validate()
            .context("invalid search configuration")?;

        config
            .rate_limiting
            .validate()
            .context("invalid rate_limiting configuration")?;

        config
            .server
            .validate()
            .context("invalid server configuration")?;
        config
            .media_artifacts
            .validate()
            .context("invalid media_artifacts configuration")?;
        config
            .sending
            .validate(config.server.output_format)
            .context("invalid sending configuration")?;
        if config.sending.enabled {
            config
                .rate_limiting
                .validate_sending_costs()
                .context("invalid sending costs")?;
        }
        Ok(config)
    }

    /// Resolve the exact configuration file selection without opening it.
    ///
    /// # Errors
    /// Returns an error if platform configuration directories are unavailable.
    pub fn selected_path(explicit: Option<&std::path::Path>) -> anyhow::Result<PathBuf> {
        explicit
            .map(std::path::Path::to_path_buf)
            .map(Ok)
            .unwrap_or_else(Self::resolve_config_path)
    }

    /// Replace only the in-memory session path when an override is supplied.
    /// Does not create/move a session or write configuration.
    pub fn apply_cli_overrides(&mut self, session_file: Option<std::path::PathBuf>) {
        if let Some(path) = session_file {
            self.telegram.session_file = path;
        }
    }

    fn resolve_config_path() -> anyhow::Result<PathBuf> {
        // 1. Check environment variable
        if let Ok(path) = std::env::var("TELEGRAM_MCP_CONFIG") {
            return Ok(PathBuf::from(path));
        }

        // 2. Use XDG config directory
        let dirs = directories::ProjectDirs::from("", "", "telegram-connector")
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

        Ok(dirs.config_dir().join("config.toml"))
    }
}

#[cfg(test)]
#[path = "tests/loading_tests.rs"]
mod tests;
