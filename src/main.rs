use anyhow::{Context, Result};
use secrecy::ExposeSecret;
use std::sync::Arc;
use telegram_connector::{
    Cli, Config, RateLimiter, TelegramClientTrait, logging,
    mcp::server::McpServer,
    telegram::{auth::interactive_auth, client::TelegramClient},
};
use tokio::signal;
use tokio::sync::oneshot;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let cli = Cli::parse_args();

    // Load configuration
    let mut config =
        Config::load_from(cli.config.as_deref()).context("Failed to load configuration")?;

    // Apply CLI overrides
    config.apply_cli_overrides(cli.session_file);

    // Initialize logging
    logging::init(&config.logging).context("Failed to initialize logging")?;

    tracing::info!("Telegram MCP Connector starting...");

    // Create Telegram client
    let telegram_client = TelegramClient::new(&config.telegram)
        .await
        .context("Failed to create Telegram client")?;

    // Check if we need to authenticate
    let is_authorized = telegram_client.is_connected().await;

    if cli.setup {
        // Setup mode: interactive authentication
        run_setup_mode(&telegram_client, &config).await?;
        return Ok(());
    }

    // Normal mode: require existing authentication
    if !is_authorized {
        anyhow::bail!(
            "Not authenticated. Run with --setup flag to authenticate first:\n\
            telegram-mcp -- --setup"
        );
    }

    tracing::info!("Authenticated with Telegram");

    // Run MCP server
    run_mcp_server(telegram_client, config).await?;
    std::process::exit(0);
}

/// Run interactive setup mode for authentication
async fn run_setup_mode(client: &TelegramClient, config: &Config) -> Result<()> {
    println!("=== Telegram MCP Connector Setup ===\n");

    if client.is_connected().await {
        println!("Already authenticated!");
        println!("Session file: {}", client.session_path().display());
        return Ok(());
    }

    println!("Authenticating with Telegram...\n");
    println!("A login code will be sent to your Telegram app for the phone number in your config.");

    let phone = config.telegram.phone_number.expose_secret();
    let api_hash = config.telegram.api_hash.expose_secret();

    interactive_auth(client, phone, api_hash)
        .await
        .context("Authentication failed")?;

    println!("\nAuthentication successful!");
    println!("Session saved to: {}", client.session_path().display());
    println!("\nYou can now run the MCP server without --setup flag.");

    Ok(())
}

/// Run the MCP server with graceful shutdown handling
async fn run_mcp_server(telegram_client: TelegramClient, config: Config) -> Result<()> {
    // Create rate limiter
    let rate_limiter = RateLimiter::new(&config.rate_limiting);

    // Create MCP server
    let server = McpServer::new(Arc::new(telegram_client), Arc::new(rate_limiter));

    tracing::info!("Starting MCP server on stdio...");

    // Create shutdown channel
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Spawn shutdown signal handler
    let shutdown_timeout = config.server.shutdown_timeout_seconds;
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        tracing::info!("Shutdown signal received");
        let _ = shutdown_tx.send(());
    });

    // Run MCP server with shutdown handling
    tokio::select! {
        result = server.run_stdio() => {
            result.context("MCP server error")?;
        }
        _ = shutdown_rx => {
            tracing::info!("Initiating graceful shutdown (timeout: {}s)...", shutdown_timeout);
        }
    }

    // Graceful shutdown: give ongoing operations time to complete
    tracing::info!("Waiting for operations to complete...");
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    tracing::info!("Telegram MCP Connector stopped");
    Ok(())
}

/// Wait for shutdown signal (SIGTERM or SIGINT)
async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::debug!("Received SIGINT (Ctrl+C)");
        }
        _ = terminate => {
            tracing::debug!("Received SIGTERM");
        }
    }
}
