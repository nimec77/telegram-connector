use clap::Parser;
use std::path::PathBuf;

/// Telegram MCP Connector - A Model Context Protocol service for Telegram
///
/// This service enables Claude to search Russian-language Telegram channels
/// and messages in real-time.
#[derive(Parser, Debug)]
#[command(name = "telegram-mcp")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Run interactive setup to authenticate with Telegram
    ///
    /// Use this flag on first run to authenticate with your Telegram account.
    /// This will prompt for phone number, verification code, and 2FA password if enabled.
    /// After successful authentication, the session is saved and the program exits.
    #[arg(long, short = 's')]
    pub setup: bool,

    /// Path to session file (overrides config.toml setting)
    ///
    /// If not specified, uses the path from config.toml or the default location.
    #[arg(long, value_name = "FILE")]
    pub session_file: Option<PathBuf>,

    /// Path to configuration file
    ///
    /// Default: ~/.config/telegram-connector/config.toml
    #[arg(long, short = 'c', value_name = "FILE")]
    pub config: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_default_values() {
        let cli = Cli::parse_from(["telegram-mcp"]);
        assert!(!cli.setup);
        assert!(cli.session_file.is_none());
        assert!(cli.config.is_none());
    }

    #[test]
    fn cli_setup_flag() {
        let cli = Cli::parse_from(["telegram-mcp", "--setup"]);
        assert!(cli.setup);
    }

    #[test]
    fn cli_setup_short_flag() {
        let cli = Cli::parse_from(["telegram-mcp", "-s"]);
        assert!(cli.setup);
    }

    #[test]
    fn cli_session_file_override() {
        let cli = Cli::parse_from(["telegram-mcp", "--session-file", "/custom/session.bin"]);
        assert_eq!(cli.session_file, Some(PathBuf::from("/custom/session.bin")));
    }

    #[test]
    fn cli_config_override() {
        let cli = Cli::parse_from(["telegram-mcp", "--config", "/custom/config.toml"]);
        assert_eq!(cli.config, Some(PathBuf::from("/custom/config.toml")));
    }

    #[test]
    fn cli_config_short_flag() {
        let cli = Cli::parse_from(["telegram-mcp", "-c", "/custom/config.toml"]);
        assert_eq!(cli.config, Some(PathBuf::from("/custom/config.toml")));
    }

    #[test]
    fn cli_all_options() {
        let cli = Cli::parse_from([
            "telegram-mcp",
            "--setup",
            "--session-file",
            "/path/session.bin",
            "--config",
            "/path/config.toml",
        ]);
        assert!(cli.setup);
        assert_eq!(cli.session_file, Some(PathBuf::from("/path/session.bin")));
        assert_eq!(cli.config, Some(PathBuf::from("/path/config.toml")));
    }
}
