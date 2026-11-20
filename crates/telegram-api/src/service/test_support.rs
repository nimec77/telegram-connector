//! Hosted API wiring for loopback integration fixtures.

use super::accounts::Hosted;
use super::reads::fixture::{FixtureAccount, ReadFixture};
use super::{BrowserConfig, build_router};
use crate::contracts::{GrantUpdate, ReadGrant};
use crate::identity::{Audience, CredentialVerifier, VerifiedIdentity};
use secrecy::SecretString;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use telegram_core::config::RateLimitConfig;
use telegram_core::operations::CancellationToken;
use telegram_core::telegram::types::{ChannelId, Message, PeerNamespace};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Owns a disposable hosted API and its local listener task.
pub struct ReadApi {
    server: JoinHandle<()>,
    shutdown: CancellationToken,
    _root: tempfile::TempDir,
}

impl Drop for ReadApi {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.server.abort();
    }
}

/// Start the actual hosted read routes with one fixture channel and two account tokens.
///
/// # Errors
/// Returns a fixed diagnostic if the listener, fixture storage or hosted configuration fails.
pub async fn start_read_api(
    listener: TcpListener,
    verifier: CredentialVerifier,
    owner: &VerifiedIdentity,
) -> Result<ReadApi, &'static str> {
    if !listener
        .local_addr()
        .map_err(|_| "fixture address")?
        .ip()
        .is_loopback()
    {
        return Err("fixture requires loopback");
    }
    let root = tempfile::tempdir().map_err(|_| "fixture data root")?;
    let config = serde_json::from_value(json!({
        "data_root":root.path(),"telegram":{"api_id":1,"api_hash":"fixture"},
        "registered_clients":[owner.client_id(),"restricted"],"eligible_channels":["42"]
    }))
    .map_err(|_| "fixture settings")?;
    let hosted = Hosted::open(config)
        .await
        .map_err(|_| "fixture hosted state")?;
    let principal = owner.principal();
    let generation = hosted
        .store
        .start(principal.issuer(), principal.subject())
        .await
        .map_err(|_| "fixture generation")?;
    hosted
        .store
        .activate(&generation, 1)
        .await
        .map_err(|_| "fixture activation")?;
    hosted
        .store
        .update_grant(
            &generation,
            owner.client_id(),
            GrantUpdate {
                expected_revision: 0,
                read: ReadGrant::Channels(vec!["42".into()]),
                send_enabled: false,
            },
        )
        .await
        .map_err(|_| "fixture grant")?;

    let mut row: Message = serde_json::from_value(json!({
        "id":104,"channel_id":42,"channel_name":"Channel","text":"same account message",
        "timestamp":"2023-11-14T22:13:20Z","has_media":false,"media_type":"none","link":""
    }))
    .map_err(|_| "fixture message")?;
    row.peer_namespace = Some(PeerNamespace::Channel);
    let channel = ChannelId::new(42).map_err(|_| "fixture channel")?;
    let account = Arc::new(FixtureAccount {
        channels: [(channel, vec![row])].into(),
        fetches: 0.into(),
        history_timeout: false,
    });
    let fixture = ReadFixture::new(
        RateLimitConfig {
            max_tokens: 2,
            refill_rate: 0.001,
            ..RateLimitConfig::default()
        },
        [(generation.clone(), account)].into(),
    );
    fixture.warm(&generation).await;
    hosted
        .reads_fixture
        .set(fixture)
        .map_err(|_| "fixture already initialized")?;
    let shutdown = hosted.cancellation.clone();
    let cancel = shutdown.clone();
    let callback = url::Url::parse(verifier.audience(Audience::Api))
        .map_err(|_| "fixture audience")?
        .join("/auth/callback")
        .map_err(|_| "fixture callback")?;
    let app = build_router(
        verifier,
        BrowserConfig {
            client_id: "fixture-browser".into(),
            client_secret: SecretString::from("test-only-secret"),
            callback_url: callback,
        },
        Some(hosted),
    )
    .map_err(|_| "fixture API router")?;
    let server = tokio::spawn(async move {
        let _ = super::serve(listener, app, Duration::from_secs(1), async move {
            cancel.cancelled().await
        })
        .await;
    });
    Ok(ReadApi {
        server,
        shutdown,
        _root: root,
    })
}
