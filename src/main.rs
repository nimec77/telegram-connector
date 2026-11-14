use anyhow::{Context, Result};
use clap::Parser;
use std::sync::Arc;
use telegram_connector::mcp::runtime::process;
use telegram_connector::mcp::server::McpServer;
use telegram_connector::telegram::auth::interactive_auth;
use telegram_connector::telegram::client::TelegramClient;
use telegram_connector::{Cli, Config, RateLimiter, TelegramClientTrait, logging};

fn main() -> Result<()> {
    process::run(application())
}

async fn application() -> Result<()> {
    // Parse CLI arguments
    let cli = Cli::parse();

    // Load configuration (does not validate auth credentials)
    let config_path =
        Config::selected_path(cli.config.as_deref()).context("Failed to select configuration")?;
    let mut config =
        Config::load_from(Some(&config_path)).context("Failed to load configuration")?;

    // Apply CLI overrides
    config.apply_cli_overrides(cli.session_file);
    let sending =
        telegram_connector::sending::SendingRuntimeConfig::try_from_config(&config, &config_path)?;

    // Initialize logging
    logging::init(&config.logging).context("Failed to initialize logging")?;

    // Clean up old log files
    if let Ok(removed) = logging::cleanup_old_logs(&config.logging)
        && removed > 0
    {
        tracing::info!(removed = removed, "Cleaned up old log files");
    }

    tracing::info!("Telegram MCP Connector starting...");

    if cli.setup {
        // Setup mode: requires auth credentials
        config
            .validate_for_setup()
            .context("Setup mode requires authentication credentials")?;

        // Create Telegram client for setup
        let telegram_client = TelegramClient::new(&config.telegram, &config.search)
            .await
            .context("Failed to create Telegram client")?;

        run_setup_mode(&telegram_client, &config).await?;
        return Ok(());
    }

    // Normal mode: create client and check session
    let telegram_client = TelegramClient::new(&config.telegram, &config.search)
        .await
        .context("Failed to create Telegram client")?;

    // Verify we have a valid session
    if !telegram_client
        .refresh_account_status()
        .await
        .context("Failed to check Telegram authorization")?
        .authorized
    {
        anyhow::bail!(
            "Not authenticated. Session file may be missing or invalid.\n\
            Run with --setup flag to authenticate first:\n\
            \n  telegram-mcp --setup --config <config-file>\n\
            \nMake sure TELEGRAM_API_HASH and TELEGRAM_PHONE_NUMBER are set for setup."
        );
    }

    tracing::info!("Authenticated with Telegram");

    // Run MCP server
    run_mcp_server(telegram_client, config, sending).await
}

/// Run interactive setup mode for authentication
async fn run_setup_mode(client: &TelegramClient, config: &Config) -> Result<()> {
    println!("=== Telegram MCP Connector Setup ===\n");

    if client
        .refresh_account_status()
        .await
        .context("Failed to check Telegram authorization")?
        .authorized
    {
        println!("Already authenticated!");
        println!("Session file: {}", client.session_path().display());
        return Ok(());
    }

    println!("Authenticating with Telegram...\n");
    println!("A login code will be sent to your Telegram app for the phone number in your config.");

    let (api_hash, phone) = config.telegram.auth_credentials().context(
        "Authentication credentials missing: set telegram.api_hash and telegram.phone_number",
    )?;

    interactive_auth(client, phone, api_hash)
        .await
        .context("Authentication failed")?;

    println!("\nAuthentication successful!");
    println!("Session saved to: {}", client.session_path().display());
    println!("\nYou can now run the MCP server without --setup flag.");

    Ok(())
}

/// Run the MCP server with graceful shutdown handling
async fn run_mcp_server(
    telegram_client: TelegramClient,
    config: Config,
    sending: telegram_connector::sending::SendingRuntimeConfig,
) -> Result<()> {
    // Create rate limiter
    let rate_limiter = RateLimiter::new(&config.rate_limiting);

    // Create MCP server
    let server = McpServer::new(Arc::new(telegram_client), Arc::new(rate_limiter))
        .with_output_format(config.server.output_format)
        .with_observability(&config.observability)
        .with_media_download_cost(config.rate_limiting.media_download_cost)
        .with_transcription_cost(config.rate_limiting.transcription_cost)
        .with_transcription_timeouts(
            config.transcription.default_timeout_seconds,
            config.transcription.max_timeout_seconds,
        )
        .with_response_byte_budget(config.limits.response_byte_budget)
        .with_media_batch_max_total_bytes(config.limits.media_batch_max_total_bytes)
        .with_media_artifacts(&config.media_artifacts)
        .await?
        .with_sending(sending)
        .await?;

    tracing::info!("Starting MCP server on stdio...");
    server
        .run_stdio_with_shutdown(config.server, process::shutdown_signal())
        .await
        .context("MCP server error")?;
    tracing::info!("Telegram MCP Connector stopped");
    Ok(())
}
