//! Hosted API wiring for loopback integration fixtures.

pub use super::sends::fixture::SendOutcome;

use super::accounts::Hosted;
use super::reads::fixture::{FixtureAccount, ReadFixture};
use super::sends::fixture::SendFixture;
use super::{AccountGeneration, BrowserConfig, build_router};
use crate::contracts::{GrantUpdate, ReadGrant};
use crate::identity::{Audience, CredentialVerifier, VerifiedIdentity};
use secrecy::SecretString;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use telegram_core::config::RateLimitConfig;
use telegram_core::telegram::types::{ChannelId, Message, PeerNamespace};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

struct HostedApi {
    server: JoinHandle<()>,
    hosted: Arc<Hosted>,
    _root: tempfile::TempDir,
}

impl HostedApi {
    fn start(
        listener: TcpListener,
        verifier: CredentialVerifier,
        hosted: Arc<Hosted>,
        root: tempfile::TempDir,
    ) -> Result<Self, &'static str> {
        let cancel = hosted.cancellation.clone();
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
            Some(Arc::clone(&hosted)),
        )
        .map_err(|_| "fixture API router")?;
        let server = tokio::spawn(async move {
            let _ = super::serve(listener, app, Duration::from_secs(1), async move {
                cancel.cancelled().await
            })
            .await;
        });
        Ok(Self {
            server,
            hosted,
            _root: root,
        })
    }
}

impl Drop for HostedApi {
    fn drop(&mut self) {
        self.hosted.cancellation.cancel();
        self.server.abort();
    }
}

/// Owns a disposable hosted API and its local listener task.
pub struct ReadApi(HostedApi);

impl ReadApi {
    /// Pause the next search after it holds an account lease and charges the account.
    ///
    /// The returned receiver yields the release sender when the operation reaches the pause.
    /// Closing that sender's receiving side signals that the pending operation ended.
    pub fn hold_next_search(&self) -> oneshot::Receiver<oneshot::Sender<()>> {
        self.0
            .hosted
            .reads_fixture
            .get()
            .expect("initialized read fixture")
            .hold_next_search()
    }

    /// Run normal registry eviction against the fixture's immediately idle account.
    ///
    /// # Errors
    /// Returns a fixed diagnostic if the fixture registry is unavailable.
    pub fn evict_idle_account(&self) -> Result<bool, &'static str> {
        self.0
            .hosted
            .reads_fixture
            .get()
            .expect("initialized read fixture")
            .evict_idle_account()
            .map_err(|_| "fixture account eviction")
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
    start_read_api_with_admission(listener, verifier, owner, super::MAX_IN_FLIGHT).await
}

/// Start the hosted read fixture with a custom service-wide admission limit.
///
/// # Errors
/// Returns a fixed diagnostic if the listener, fixture storage or hosted configuration fails.
pub async fn start_read_api_with_admission(
    listener: TcpListener,
    verifier: CredentialVerifier,
    owner: &VerifiedIdentity,
    max_in_flight: usize,
) -> Result<ReadApi, &'static str> {
    let root = loopback_root(&listener)?;
    let config = serde_json::from_value(json!({
        "data_root":root.path(),"telegram":{"api_id":1,"api_hash":"fixture"},
        "registered_clients":[owner.client_id(),"restricted"],"eligible_channels":["42"],
        "max_in_flight":max_in_flight
    }))
    .map_err(|_| "fixture settings")?;
    let hosted = Hosted::open(config)
        .await
        .map_err(|_| "fixture hosted state")?;
    let generation = activate_account(
        &hosted,
        owner,
        GrantUpdate {
            expected_revision: 0,
            read: ReadGrant::Channels(vec!["42".into()]),
            send_enabled: false,
        },
    )
    .await?;

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
    HostedApi::start(listener, verifier, hosted, root).map(ReadApi)
}

/// Owns the loopback hosted send API and its offline Telegram outcome fixture.
pub struct SendApi(HostedApi);

impl SendApi {
    /// Select the Telegram outcome for subsequent hosted sends.
    pub fn set_outcome(&self, outcome: SendOutcome) {
        self.fixture().set_outcome(outcome);
    }

    /// Return the number of submissions accepted by the real hosted final gate.
    pub fn dispatches(&self) -> usize {
        self.fixture().dispatches()
    }

    fn fixture(&self) -> &SendFixture {
        self.0
            .hosted
            .send_fixture
            .get()
            .expect("initialized send fixture")
    }
}

/// Start the real hosted send routes with a send-only grant and an offline Telegram outcome.
///
/// # Errors
/// Returns a fixed diagnostic if the listener, fixture storage or hosted configuration fails.
pub async fn start_send_api(
    listener: TcpListener,
    verifier: CredentialVerifier,
    owner: &VerifiedIdentity,
    sending_enabled: bool,
) -> Result<SendApi, &'static str> {
    let root = loopback_root(&listener)?;
    let config = serde_json::from_value(json!({
        "data_root":root.path(),"telegram":{"api_id":1,"api_hash":"fixture"},
        "registered_clients":[owner.client_id()],"sending_enabled":sending_enabled
    }))
    .map_err(|_| "fixture settings")?;
    let hosted = Hosted::open(config)
        .await
        .map_err(|_| "fixture hosted state")?;
    activate_account(
        &hosted,
        owner,
        GrantUpdate {
            expected_revision: 0,
            read: ReadGrant::None,
            send_enabled: true,
        },
    )
    .await?;
    hosted
        .send_fixture
        .set(SendFixture::default())
        .map_err(|_| "fixture already initialized")?;
    HostedApi::start(listener, verifier, hosted, root).map(SendApi)
}

fn loopback_root(listener: &TcpListener) -> Result<tempfile::TempDir, &'static str> {
    if !listener
        .local_addr()
        .map_err(|_| "fixture address")?
        .ip()
        .is_loopback()
    {
        return Err("fixture requires loopback");
    }
    tempfile::tempdir().map_err(|_| "fixture data root")
}

async fn activate_account(
    hosted: &Hosted,
    owner: &VerifiedIdentity,
    grant: GrantUpdate,
) -> Result<AccountGeneration, &'static str> {
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
        .update_grant(&generation, owner.client_id(), grant)
        .await
        .map_err(|_| "fixture grant")?;
    Ok(generation)
}
