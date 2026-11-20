//! Serve hosted MCP requests behind a trusted HTTPS edge.

use secrecy::SecretString;
use serde::Deserialize;
use std::fs::File;
use std::io::Read as _;
use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;
use telegram_api::identity::{AuthorityConfig, CredentialVerifier};
use tokio::net::TcpListener;
use url::Url;

const USAGE: &str = "Usage: telegram-mcp-http <config.json>\nSet TELEGRAM_MCP_EXCHANGE_SECRET to the gateway's registered OAuth client secret.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    listen: SocketAddr,
    authority: AuthorityConfig,
    api_origin: Url,
    #[serde(default = "default_timeout")]
    timeout_seconds: u64,
}
fn default_timeout() -> u64 {
    30
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(reason) => {
            eprintln!("{reason}");
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
    let mut bytes = Vec::new();
    File::open(Path::new(&path))
        .and_then(|file| file.take(65537).read_to_end(&mut bytes))
        .map_err(|_| "Could not read gateway configuration")?;
    if bytes.len() > 65536 {
        return Err("Gateway configuration is too large");
    }
    let config: Config =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid gateway configuration")?;
    if !config.listen.ip().is_loopback() {
        return Err("Bind the gateway to loopback behind a trusted HTTPS proxy");
    }
    let secret =
        SecretString::from(std::env::var("TELEGRAM_MCP_EXCHANGE_SECRET").map_err(|_| USAGE)?);
    let verifier = CredentialVerifier::discover(config.authority)
        .await
        .map_err(|_| "Could not initialize issuer discovery")?;
    let gateway = telegram_mcp_http::router(
        verifier,
        config.api_origin,
        secret,
        Duration::from_secs(config.timeout_seconds),
    )?;
    let listener = TcpListener::bind(config.listen)
        .await
        .map_err(|_| "Could not bind gateway listener")?;
    axum::serve(listener, gateway)
        .with_graceful_shutdown(shutdown())
        .await
        .map_err(|_| "Gateway listener failed")
}

async fn shutdown() {
    #[cfg(unix)]
    if let Ok(mut signal) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        tokio::select! {_=signal.recv()=>(),_=tokio::signal::ctrl_c()=>()}
        return;
    }
    let _ = tokio::signal::ctrl_c().await;
}
