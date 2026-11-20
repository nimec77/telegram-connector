//! Start the hosted API behind a trusted HTTPS edge.

use secrecy::SecretString;
use serde::Deserialize;
use std::fs::File;
use std::io::Read as _;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::process::ExitCode;
use telegram_api::identity::{AuthorityConfig, CredentialVerifier};
use telegram_api::service::{BrowserConfig, HostedConfig, serve_hosted};
use tokio::net::TcpListener;
use url::Url;

const MAX_CONFIG_BYTES: u64 = 64 * 1024;
const USAGE: &str = "Usage: telegram-api <config.json>\nSet TELEGRAM_API_OIDC_SECRET to the confidential browser client secret.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default = "default_listen")]
    listen: SocketAddr,
    authority: AuthorityConfig,
    browser_client_id: String,
    browser_callback: Url,
    hosted: HostedConfig,
}

fn default_listen() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, 3000))
}

fn load_config(path: &Path) -> Result<Config, &'static str> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| "Could not read the hosted API configuration")?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err("Hosted API configuration exceeds its size limit");
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid hosted API configuration")
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), &'static str> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or(USAGE)?;
    if args.next().is_some() {
        return Err(USAGE);
    }
    if path == "--help" || path == "-h" {
        println!("{USAGE}");
        return Ok(());
    }
    let config = load_config(Path::new(&path))?;
    if !config.listen.ip().is_loopback() {
        return Err("The hosted API must bind to loopback behind an HTTPS proxy");
    }
    let client_secret = SecretString::from(
        std::env::var("TELEGRAM_API_OIDC_SECRET")
            .map_err(|_| "Set TELEGRAM_API_OIDC_SECRET for the hosted API")?,
    );
    let verifier = CredentialVerifier::discover(config.authority)
        .await
        .map_err(|_| "Could not initialize hosted API identity")?;
    let listener = TcpListener::bind(config.listen)
        .await
        .map_err(|_| "Could not bind the hosted API listener")?;
    serve_hosted(
        listener,
        verifier,
        BrowserConfig {
            client_id: config.browser_client_id,
            client_secret,
            callback_url: config.browser_callback,
        },
        config.hosted,
        shutdown_signal(),
    )
    .await
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {
                _ = terminate.recv() => (),
                _ = tokio::signal::ctrl_c() => (),
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
