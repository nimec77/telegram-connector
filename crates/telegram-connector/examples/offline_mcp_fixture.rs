//! Credential-free protocol fixture. Files require an explicit disposable artifact root.
#[path = "support/offline_client.rs"]
mod offline_client;

use rmcp::model::{
    Implementation, InitializeResult, ProgressNotificationParam, ProtocolVersion,
    ServerCapabilities,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;
use telegram_connector::config::ServerConfig;
use telegram_connector::mcp::observability::{ResponseBuffer, SessionMetrics};
use telegram_connector::mcp::runtime::{Runtime, SUPPORTED_VERSIONS};

#[derive(Clone)]
struct Fixture;
#[tool_router]
impl Fixture {
    #[tool(description = "Return a deterministic offline result")]
    async fn echo(&self) -> String {
        "offline fixture".into()
    }
    #[tool(description = "Wait until cancelled or until the fixture timeout")]
    async fn wait(&self, context: RequestContext<RoleServer>) -> String {
        if let Some(token) = context.meta.get_progress_token() {
            let _ = context
                .peer
                .notify_progress(
                    ProgressNotificationParam::new(token, 1.0).with_message("Fixture wait started"),
                )
                .await;
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
        "finished".into()
    }
}
#[tool_handler]
impl ServerHandler for Fixture {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_11_25)
            .with_server_info(Implementation::new("offline-fixture", "1"))
    }
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(SUPPORTED_VERSIONS)
    }
}
fn main() -> anyhow::Result<()> {
    telegram_connector::mcp::runtime::process::run(application())
}

async fn application() -> anyhow::Result<()> {
    let mode = match std::env::args().nth(1).as_deref() {
        Some("--typed-tools") => Some(telegram_connector::config::OutputFormat::JsonV1),
        Some("--legacy-typed-tools") => Some(telegram_connector::config::OutputFormat::Legacy),
        _ => None,
    };
    if let Some(mode) = mode {
        let mut config: telegram_connector::Config = toml::from_str("[telegram]\napi_id = 1")?;
        config.server.output_format = mode;
        let mut args = std::env::args().skip(2);
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("fixture option requires a value"))?;
            match flag.as_str() {
                "--artifact-root" => {
                    config.media_artifacts.enabled = true;
                    config.media_artifacts.file_export_enabled = true;
                    config.media_artifacts.root = Some(value.into());
                }
                "--artifact-ttl-seconds" => config.media_artifacts.ttl_seconds = value.parse()?,
                "--sending-enabled" => config.sending.enabled = value.parse()?,
                "--upload-root" => {
                    config
                        .sending
                        .upload_roots
                        .insert("outgoing".into(), value.into());
                }
                "--staging-root" => config.sending.staging_root = Some(value.into()),
                _ => anyhow::bail!("unknown fixture option"),
            }
        }
        anyhow::ensure!(
            config.media_artifacts.root.is_some() || !config.media_artifacts.enabled,
            "fixture artifacts require an explicit root"
        );
        let server = telegram_connector::mcp::McpServer::new(
            Arc::new(offline_client::OfflineClient::new(true)),
            Arc::new(telegram_connector::RateLimiter::new(&config.rate_limiting)),
        )
        .with_output_format(config.server.output_format)
        .with_media_artifacts(&config.media_artifacts)
        .await?
        .with_sending(
            telegram_connector::sending::SendingRuntimeConfig::try_from_config(
                &config,
                &std::env::temp_dir().join("offline-mcp-fixture.toml"),
            )?,
        )
        .await?;
        return server
            .run_io(
                (tokio::io::stdin(), tokio::io::stdout()),
                config.server,
                telegram_connector::mcp::runtime::process::shutdown_signal(),
            )
            .await;
    }

    let runtime = Runtime::try_new(
        ServerConfig {
            shutdown_timeout_seconds: 1,
            ..Default::default()
        },
        Arc::new(SessionMetrics::new()),
        Arc::new(ResponseBuffer::new(8, 4096)),
        Duration::from_secs(1),
    )?;
    runtime
        .serve(
            Fixture,
            (tokio::io::stdin(), tokio::io::stdout()),
            telegram_connector::mcp::runtime::process::shutdown_signal(),
        )
        .await?;
    Ok(())
}
