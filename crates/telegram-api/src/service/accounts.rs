//! Owner account administration and hosted account-generation state.

use super::group_capture::Capture;
use super::group_changes::{GroupChangesConfig, GroupMonitoring};
use super::group_journal::GroupJournal;
use super::linking::{self, LinkReply, Linking};
use super::registry::{AccountLease, AccountRegistry};
use super::storage::{AccountStore, LinkState, RetirementOutcome};
use super::{ApiError, ServiceState};
use crate::contracts::groups::GroupRef;
use crate::contracts::{ErrorCode, GrantSnapshot, GrantUpdate, ReadGrant, Scope};
use crate::identity::{VerifiedIdentity, unix_now, valid_label};
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Html;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::{Arc, Weak};
use std::time::Duration;
use telegram_core::config::{RateLimitConfig, TelegramConfig, defaults};
use telegram_core::deadline::DrainDeadline;
use telegram_core::operations::{
    AccountLimits, AccountOperations, CancellationToken, OperationContext,
};
use telegram_core::{TelegramClient, TelegramClientTrait};

/// Persistent storage and bounded account service configuration.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedConfig {
    /// Private directory on encrypted persistent storage, owned by the API.
    pub data_root: PathBuf,
    /// Operator MTProto application configuration. Session paths are server-assigned.
    pub telegram: TelegramConfig,
    /// Registered owner-configurable client IDs.
    pub registered_clients: HashSet<String>,
    /// Channel IDs approved by the operator for hosted use.
    #[serde(default)]
    pub eligible_channels: HashSet<String>,
    /// Typed basic-group and supergroup references enabled for change capture.
    #[serde(default)]
    pub eligible_groups: HashSet<String>,
    /// Retention for the replayable group journal.
    #[serde(default)]
    pub group_changes: GroupChangesConfig,
    /// Explicit OAuth client IDs allowed to use public directory discovery.
    #[serde(default)]
    pub discovery_clients: HashSet<String>,
    /// Enable grant-gated hosted writes.
    #[serde(default)]
    pub sending_enabled: bool,
    /// Shared rate policy for hosted account generations.
    #[serde(default)]
    pub rate_limiting: RateLimitConfig,
    /// Maximum number of resident clients including startup and shutdown.
    #[serde(default = "max_accounts")]
    pub max_accounts: usize,
    /// Maximum handlers admitted concurrently across all accounts.
    #[serde(default = "max_in_flight")]
    pub max_in_flight: usize,
    /// Evict an unleased account after this many seconds.
    #[serde(default = "idle_seconds")]
    pub idle_seconds: u64,
    /// Limits for live post subscriptions.
    #[serde(default)]
    pub events: super::events::EventConfig,
    /// Maximum time for the lifetime of an HTTP operation.
    #[serde(default = "request_seconds")]
    pub request_seconds: u64,
}

fn max_accounts() -> usize {
    64
}
fn max_in_flight() -> usize {
    256
}
fn idle_seconds() -> u64 {
    300
}
fn request_seconds() -> u64 {
    30
}

pub(super) struct Hosted {
    pub(super) store: Arc<AccountStore>,
    pub(super) uploads: Option<super::uploads::Uploads>,
    pub(super) media: super::media::MediaStore,
    pub(super) config: HostedConfig,
    pub(super) groups: GroupMonitoring,
    pub(super) cancellation: CancellationToken,
    drain_deadline: DrainDeadline,
    drained: CancellationToken,
    registry: AccountRegistry<ActiveAccount>,
    pub(super) streams: Arc<telegram_core::streams::StreamBudget>,
    pub(super) subscribers: Arc<tokio::sync::Semaphore>,
    pub(super) writers: super::event_writer::ActiveWriters,
    pub(super) grant_changes: tokio::sync::broadcast::Sender<()>,
    linking: Linking,
    pub(super) transition: tokio::sync::Mutex<()>,
    #[cfg(any(test, feature = "test-support"))]
    pub(super) send_fixture: std::sync::OnceLock<super::sends::fixture::SendFixture>,
    #[cfg(any(test, feature = "test-support"))]
    pub(super) reads_fixture: std::sync::OnceLock<super::reads::fixture::ReadFixture>,
    maintenance: tokio::sync::Notify,
}

pub(super) struct ActiveAccount {
    pub(super) client: Arc<TelegramClient>,
    pub(super) operations:
        AccountOperations<TelegramClient, telegram_core::rate_limiter::RateLimiter>,
    pub(super) streams: telegram_core::streams::AccountStreams,
    pub(super) capture: Option<Arc<Capture>>,
}

impl ActiveAccount {
    fn abort(&self) {
        self.streams.abort();
        self.client.abort_sender();
    }

    async fn close(&self) {
        self.streams.close().await;
        self.client.close().await;
    }
}

impl Hosted {
    pub(super) async fn open(config: HostedConfig) -> Result<Arc<Self>, ApiError> {
        Self::open_config(config, true).await
    }

    pub(super) async fn open_config(
        config: HostedConfig,
        supervise_capture: bool,
    ) -> Result<Arc<Self>, ApiError> {
        if config.telegram.api_id <= 0
            || config.telegram.api_hash.is_none()
            || !config.events.valid()
            || config.registered_clients.is_empty()
            || config.registered_clients.len() > 1024
            || !config.registered_clients.iter().all(|id| valid_label(id))
            || !(1..=256).contains(&config.max_accounts)
            || !(1..=1024).contains(&config.max_in_flight)
            || !(1..=3600).contains(&config.idle_seconds)
            || !(1..=120).contains(&config.request_seconds)
            || !config
                .discovery_clients
                .is_subset(&config.registered_clients)
            || config.eligible_channels.len() > 10_000
            || config.eligible_groups.len() > 20
            || !config.group_changes.valid()
            || !config
                .eligible_channels
                .iter()
                .all(|value| super::accounts::positive_channel_id(value).is_some())
            || config.rate_limiting.validate().is_err()
            || config.telegram.timeouts.validate().is_err()
            || config.telegram.validate_media_limits().is_err()
            || (config.sending_enabled && config.rate_limiting.validate_sending_costs().is_err())
        {
            return Err(ApiError::new(ErrorCode::InvalidRequest));
        }
        let eligible_groups = config
            .eligible_groups
            .iter()
            .map(|group| group.parse::<GroupRef>())
            .collect::<Result<HashSet<_>, _>>()
            .map_err(|()| ApiError::new(ErrorCode::InvalidRequest))?;
        let store = Arc::new(AccountStore::open(&config.data_root).await?);
        let journal = if !eligible_groups.is_empty() {
            Some(GroupJournal::open(store.root(), config.group_changes.retention()).await?)
        } else if GroupJournal::exists(store.root()) {
            GroupJournal::open(store.root(), config.group_changes.retention())
                .await
                .ok()
        } else {
            None
        };
        let groups = GroupMonitoring::new(journal, eligible_groups);
        store
            .replace_eligible_channels(&config.eligible_channels)
            .await?;
        let uploads = super::uploads::Uploads::open(store.root(), config.sending_enabled).await?;
        let hosted = Arc::new(Self {
            store,
            groups,
            uploads,
            media: super::media::MediaStore::default(),
            registry: AccountRegistry::new(
                config.max_accounts,
                Duration::from_secs(config.idle_seconds),
                config.rate_limiting.clone(),
            ),
            streams: Arc::new(telegram_core::streams::StreamBudget::new(
                config.events.max_total_queue_bytes,
            )),
            subscribers: Arc::new(tokio::sync::Semaphore::new(
                config.events.max_total_subscriptions,
            )),
            writers: super::event_writer::ActiveWriters::default(),
            grant_changes: tokio::sync::broadcast::channel(32).0,
            config,
            linking: Linking::default(),
            cancellation: CancellationToken::new(),
            drain_deadline: DrainDeadline::default(),
            drained: CancellationToken::new(),
            transition: tokio::sync::Mutex::new(()),
            #[cfg(any(test, feature = "test-support"))]
            reads_fixture: std::sync::OnceLock::new(),
            #[cfg(any(test, feature = "test-support"))]
            send_fixture: std::sync::OnceLock::new(),
            maintenance: tokio::sync::Notify::new(),
        });
        tokio::spawn(maintain(Arc::downgrade(&hosted)));
        if supervise_capture && !hosted.config.eligible_groups.is_empty() {
            tokio::spawn(supervise_groups(Arc::downgrade(&hosted)));
        }
        Ok(hosted)
    }

    pub(super) async fn lease(
        &self,
        identity: &VerifiedIdentity,
        generation: &super::AccountGeneration,
        context: &OperationContext,
    ) -> Result<AccountLease<ActiveAccount>, ApiError> {
        let principal = identity.principal();
        self.open_account(principal.issuer(), principal.subject(), generation, context)
            .await
    }

    async fn open_account(
        &self,
        issuer: &str,
        subject: &str,
        generation: &super::AccountGeneration,
        context: &OperationContext,
    ) -> Result<AccountLease<ActiveAccount>, ApiError> {
        let transition = tokio::select! {
            biased;
            _ = context.cancellation().cancelled() => return Err(ApiError::new(ErrorCode::UnavailableResource)),
            _ = tokio::time::sleep_until(context.deadline()) => return Err(ApiError::new(ErrorCode::Timeout)),
            guard = self.transition.lock() => guard,
        };
        if self.store.active(issuer, subject).await?.as_ref() != Some(generation) {
            return Err(ApiError::new(ErrorCode::UnavailableResource));
        }
        let reservation = self.registry.reserve(generation.clone(), context)?;
        drop(transition);
        let settings = linking::session_config(&self.config.telegram, generation);
        let global_queue = Arc::clone(&self.streams);
        let stream_limits = self.config.events.stream_limits();
        self.registry
            .open(reservation, context, async |limiter| {
                let client = Arc::new(
                    TelegramClient::new_with_streams(&settings, &defaults::default_search_config())
                        .await
                        .map_err(|_| super::storage::StorageError::Unavailable)?,
                );
                let capture = if self.groups.configured.is_empty() {
                    None
                } else {
                    let joined = self
                        .groups
                        .memberships(&client, Duration::from_secs(2))
                        .await;
                    Some(Arc::new(Capture::new(
                        Arc::clone(self.groups.journal()?),
                        generation.id().to_owned(),
                        self.groups.configured.clone(),
                        joined,
                        Arc::clone(&self.groups.changed),
                        self.groups.pending_gap(generation.id())?,
                    )))
                };
                let streams = match &capture {
                    Some(observer) => telegram_core::streams::AccountStreams::with_observer(
                        Arc::clone(&client),
                        limiter.clone(),
                        stream_limits,
                        global_queue,
                        observer.clone(),
                    ),
                    None => telegram_core::streams::AccountStreams::new(
                        Arc::clone(&client),
                        limiter.clone(),
                        stream_limits,
                        global_queue,
                    ),
                }
                .map_err(|_| super::storage::StorageError::Unavailable)?;
                let operations = AccountOperations::new(
                    Arc::clone(&client),
                    limiter,
                    AccountLimits {
                        search_timeout: Duration::from_secs(settings.timeouts.search_secs),
                        history_timeout: Duration::from_secs(settings.timeouts.history_secs),
                    },
                );
                Ok(ActiveAccount {
                    client,
                    operations,
                    streams,
                    capture,
                })
            })
            .await
            .map_err(ApiError::from)
    }

    #[cfg(test)]
    pub(super) async fn seed_account(
        &self,
        key: super::AccountGeneration,
        account: impl FnOnce(Arc<telegram_core::rate_limiter::RateLimiter>) -> ActiveAccount,
    ) -> Result<(), super::registry::LeaseError> {
        let context = OperationContext::new(
            tokio::time::Instant::now() + Duration::from_secs(5),
            self.cancellation.child_token(),
        );
        self.registry
            .lease(key, &context, async |limiter| Ok(account(limiter)))
            .await?;
        Ok(())
    }

    pub(super) fn begin_draining(&self) -> tokio::time::Instant {
        let deadline = self
            .drain_deadline
            .start(tokio::time::Instant::now() + super::shutdown::DRAIN_LIMIT);
        self.cancellation.cancel();
        self.registry.stop(ActiveAccount::abort);
        self.linking.stop_idle();
        deadline
    }

    pub(super) async fn wait_drained(&self) {
        self.drained.cancelled().await;
    }

    pub(super) fn authorized_client(&self, id: &str) -> bool {
        self.config.registered_clients.contains(id)
    }
}

async fn maintain(hosted: Weak<Hosted>) {
    let mut reported = (0, telegram_core::streams::StreamDiagnostics::default());
    let mut journal_purge_after = String::new();
    loop {
        let Some(service) = hosted.upgrade() else {
            break;
        };
        let maintenance = async {
            tokio::select! {
                _ = service.maintenance.notified() => (),
                _ = tokio::time::sleep(maintenance_interval()) => (),
            }
            close_idle(&service).await;
            if let Some(uploads) = &service.uploads {
                uploads.reap();
            }
            service.media.purge_expired();
            prune_group_journal(&service, &mut journal_purge_after).await;
            let _ = service.linking.reap(&service.store).await;
            cleanup_retired(&service).await;
        };
        tokio::select! {
            biased;
            _ = service.cancellation.cancelled() => {
                let deadline = service.begin_draining();
                let _ = tokio::time::timeout_at(deadline, service.registry.join_stopped(ActiveAccount::close)).await;
                service.registry.release_stopped();
                report_streams(&service, &mut reported);
                service.drained.cancel();
                break;
            }
            () = maintenance => report_streams(&service, &mut reported),
        }
        if Arc::strong_count(&service) == 1 {
            break;
        }
    }
}

fn report_streams(
    service: &Hosted,
    reported: &mut (usize, telegram_core::streams::StreamDiagnostics),
) {
    let subscribers =
        service.config.events.max_total_subscriptions - service.subscribers.available_permits();
    let totals = service.streams.diagnostics();
    if (subscribers, totals) != *reported {
        *reported = (subscribers, totals);
        use std::io::Write as _;
        let _ = writeln!(
            std::io::stderr(),
            "Channel streams active_readers={} subscriptions={} queued_bytes={} rejected_setups={} slow_consumers={} upstream_disconnects={} membership_losses={} service_drains={}",
            totals.active_readers,
            subscribers,
            totals.queued_bytes,
            totals.rejected_setups,
            totals.slow_consumers,
            totals.upstream_disconnects,
            totals.membership_losses,
            totals.service_drains
        );
    }
}

fn maintenance_interval() -> Duration {
    let jitter = getrandom::u32().map_or(0, |value| value % 2_000);
    Duration::from_millis(4_000 + u64::from(jitter))
}

async fn supervise_groups(hosted: Weak<Hosted>) {
    use futures::StreamExt as _;
    let mut after = String::new();
    while let Some(service) = hosted.upgrade() {
        let step = async {
            if let Ok(accounts) = service.store.active_batch(&after).await {
                after = accounts
                    .last()
                    .map_or_else(String::new, |(generation, _, _)| generation.id().to_owned());
                futures::stream::iter(accounts)
                    .for_each_concurrent(4, |(generation, issuer, subject)| {
                        let service = &service;
                        async move {
                            let context = OperationContext::new(
                                tokio::time::Instant::now() + Duration::from_secs(60),
                                service.cancellation.child_token(),
                            );
                            let Ok(mut lease) = service
                                .open_account(&issuer, &subject, &generation, &context)
                                .await
                            else {
                                return;
                            };
                            lease.background();
                            let account = lease.client();
                            if let Some(capture) = &account.capture {
                                tokio::select! {
                                    biased;
                                    _ = lease.cancellation().cancelled() => (),
                                    _ = service.cancellation.cancelled() => (),
                                    _ = tokio::time::sleep_until(context.deadline()) => (),
                                    _ = capture.refresh_memberships(
                                        service.groups.memberships(
                                            &account.client, Duration::from_secs(18)
                                        )
                                    ) => (),
                                }
                            }
                        }
                    })
                    .await;
            } else {
                after.clear();
            }
        };
        tokio::select! {
            biased;
            _ = service.cancellation.cancelled() => break,
            () = step => (),
        }
        let stop = service.cancellation.clone();
        drop(service);
        if after.is_empty() {
            tokio::select! {
                _ = stop.cancelled() => break,
                _ = tokio::time::sleep(maintenance_interval()) => (),
            }
        } else {
            tokio::task::yield_now().await;
        }
    }
}

async fn close_idle(service: &Hosted) {
    let _ = service
        .registry
        .invalidate_failed(|account| account.streams.is_closed());
    if let Ok(clients) = service.registry.evict_with(|account| {
        account
            .capture
            .as_ref()
            .is_some_and(|capture| capture.is_monitoring())
    }) {
        for (key, account) in clients {
            account.close().await;
            drop(account);
            let _ = service.registry.finish_close(&key);
        }
    }
}

async fn prune_group_journal(service: &Hosted, after: &mut String) {
    let Ok(journal) = service.groups.journal() else {
        return;
    };
    let _ = journal.reap().await;
    if let Ok(ids) = journal.generations_after(after).await {
        *after = ids.last().cloned().unwrap_or_default();
        for id in ids {
            // A cancelled reader may still be finishing an already submitted
            // write. Retry the purge after the registry has closed that account.
            if let Ok(generation) = service.store.generation(id)
                && !service.registry.busy(&generation)
                && matches!(
                    service.store.is_active_generation(&generation).await,
                    Ok(false)
                )
            {
                let _ = service.groups.retire(generation.id()).await;
            }
        }
    }
}

async fn cleanup_retired(service: &Hosted) {
    use futures::StreamExt as _;

    let Ok(now) = unix_now() else { return };
    let mut cursor = None;
    loop {
        if service.cancellation.is_cancelled() {
            return;
        }
        let Ok(pending) = service.store.retirements_after(cursor.as_ref()).await else {
            return;
        };
        if pending.is_empty() {
            return;
        }
        cursor = pending.last().cloned();
        futures::stream::iter(pending)
            .for_each_concurrent(8, |retired| cleanup_one(service, retired, now))
            .await;
    }
}

async fn cleanup_one(service: &Hosted, retired: super::RetiredGeneration, now: u64) {
    if service.registry.busy(retired.generation()) {
        return;
    }
    let outcome = if retired.expired_at(now) {
        RetirementOutcome::ManualRevocation
    } else {
        let clean = tokio::select! {
            biased;
            _ = service.cancellation.cancelled() => return,
            result = tokio::time::timeout(Duration::from_secs(5), async {
                if !retired.session_file().is_file() {
                    return false;
                }
                let settings = linking::session_config(&service.config.telegram, retired.generation());
                let Ok(client) = TelegramClient::new_with_streams(&settings, &defaults::default_search_config()).await else {
                    return false;
                };
                let clean = client.client().sign_out().await.is_ok();
                client.close().await;
                clean
            }) => matches!(result, Ok(true)),
        };
        if !clean {
            return;
        }
        RetirementOutcome::LoggedOut
    };
    let _ = service.store.finish_retirement(&retired, outcome).await;
}

#[derive(Serialize)]
pub(super) struct AccountInfo {
    state: LinkState,
    cached_status: Option<CachedStatus>,
}

#[derive(Serialize)]
struct CachedStatus {
    authorized: bool,
    premium: Option<bool>,
    authorization_checked_at: Option<telegram_core::UtcTimestamp>,
    premium_checked_at: Option<telegram_core::UtcTimestamp>,
}

pub(super) async fn status(
    State(state): State<Arc<ServiceState>>,
    headers: HeaderMap,
) -> Result<Json<AccountInfo>, ApiError> {
    let identity = if headers.contains_key(axum::http::header::AUTHORIZATION) {
        super::bearer_identity(&state, &headers).await?
    } else {
        state.browser.owner(&headers)?.1.identity
    };
    require_owner(&identity)?;
    let principal = identity.principal();
    let hosted = state.hosted()?;
    let status = hosted
        .store
        .status(principal.issuer(), principal.subject())
        .await?;
    let generation = hosted
        .store
        .active(principal.issuer(), principal.subject())
        .await?;
    let account = generation.as_ref().and_then(|generation| {
        hosted
            .registry
            .inspect(generation, |account| Arc::clone(&account.client))
    });
    let cached_status = if let Some(account) = account {
        let cached = account.cached_account_status().await;
        cached.authorization_checked_at.map(|_| CachedStatus {
            authorized: cached.authorized,
            premium: cached.premium,
            authorization_checked_at: cached.authorization_checked_at,
            premium_checked_at: cached.premium_checked_at,
        })
    } else {
        None
    };
    Ok(Json(AccountInfo {
        state: status,
        cached_status,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StartLink {
    phone_number: SecretString,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Code {
    code: SecretString,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Password {
    password: SecretString,
}

pub(super) async fn start(
    State(state): State<Arc<ServiceState>>,
    headers: HeaderMap,
    body: Result<Json<StartLink>, JsonRejection>,
) -> Result<Json<LinkReply>, ApiError> {
    let (_, owner) = state.browser.owner_mutation(&headers)?;
    let Json(body) = body.map_err(super::json_error)?;
    let hosted = state.hosted()?;
    // The trusted edge replaces this header with the real address. Requests from
    // local development without the edge share one conservative source bucket.
    let source = super::single_header(&headers, axum::http::HeaderName::from_static("x-real-ip"))
        .and_then(|value| value.parse::<IpAddr>().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    link_step(
        hosted,
        hosted.linking.start(
            &hosted.store,
            owner.identity.principal(),
            source,
            &hosted.config.telegram,
            body.phone_number,
        ),
    )
    .await
    .map(Json)
}

pub(super) async fn code(
    State(state): State<Arc<ServiceState>>,
    Path(challenge): Path<String>,
    headers: HeaderMap,
    body: Result<Json<Code>, JsonRejection>,
) -> Result<Json<LinkReply>, ApiError> {
    let (_, owner) = state.browser.owner_mutation(&headers)?;
    let Json(body) = body.map_err(super::json_error)?;
    let hosted = state.hosted()?;
    link_step(
        hosted,
        hosted.linking.code(
            &hosted.store,
            owner.identity.principal(),
            &challenge,
            body.code,
        ),
    )
    .await
    .map(Json)
}

pub(super) async fn password(
    State(state): State<Arc<ServiceState>>,
    Path(challenge): Path<String>,
    headers: HeaderMap,
    body: Result<Json<Password>, JsonRejection>,
) -> Result<Json<LinkReply>, ApiError> {
    let (_, owner) = state.browser.owner_mutation(&headers)?;
    let Json(body) = body.map_err(super::json_error)?;
    let hosted = state.hosted()?;
    link_step(
        hosted,
        hosted.linking.password(
            &hosted.store,
            owner.identity.principal(),
            &challenge,
            body.password,
        ),
    )
    .await
    .map(Json)
}

async fn link_step<T>(
    hosted: &Hosted,
    step: impl Future<Output = Result<T, ApiError>>,
) -> Result<T, ApiError> {
    tokio::select! {
        biased;
        _ = hosted.cancellation.cancelled() => Err(ApiError::new(ErrorCode::Unavailable)),
        result = step => result,
    }
}

pub(super) async fn unlink(
    State(state): State<Arc<ServiceState>>,
    headers: HeaderMap,
) -> Result<Json<AccountInfo>, ApiError> {
    let (_, owner) = state.browser.owner_mutation(&headers)?;
    let hosted = state.hosted()?;
    let principal = owner.identity.principal();
    link_step(hosted, hosted.linking.cancel(&hosted.store, principal)).await?;
    let _transition = hosted.transition.lock().await;
    if let Some(generation) = hosted
        .store
        .retire(principal.issuer(), principal.subject(), unix_now()?)
        .await?
    {
        hosted.writers.revoke(&generation, None);
        hosted.registry.invalidate(&generation)?;
        if let Some(uploads) = &hosted.uploads {
            uploads.invalidate(&generation, None);
        }
        hosted.media.invalidate(&generation, None);
        hosted.groups.forget_gap(generation.id());
        hosted.maintenance.notify_one();
    }
    let status = hosted
        .store
        .status(principal.issuer(), principal.subject())
        .await?;
    Ok(Json(AccountInfo {
        state: status,
        cached_status: None,
    }))
}

#[derive(Serialize)]
pub(super) struct GrantView {
    generation: String,
    #[serde(flatten)]
    snapshot: GrantSnapshot,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GrantUpdateRequest {
    generation: String,
    expected_revision: u64,
    read: ReadGrant,
    send_enabled: bool,
    group_refs: Option<Vec<String>>,
}

pub(super) async fn grant(
    State(state): State<Arc<ServiceState>>,
    Path(client): Path<String>,
    headers: HeaderMap,
) -> Result<Json<GrantView>, ApiError> {
    let (_, owner) = state.browser.owner(&headers)?;
    require_owner(&owner.identity)?;
    let hosted = state.hosted()?;
    let generation = grant_generation(hosted, &owner.identity, &client).await?;
    let snapshot = hosted.store.grant(&generation, &client).await?;
    Ok(Json(GrantView {
        generation: generation.id().to_owned(),
        snapshot,
    }))
}

pub(super) async fn update_grant(
    State(state): State<Arc<ServiceState>>,
    Path(client): Path<String>,
    headers: HeaderMap,
    body: Result<Json<GrantUpdateRequest>, JsonRejection>,
) -> Result<Json<GrantView>, ApiError> {
    let (_, owner) = state.browser.owner_mutation(&headers)?;
    let Json(body) = body.map_err(super::json_error)?;
    let hosted = state.hosted()?;
    let _transition = hosted.transition.lock().await;
    let generation = grant_generation(hosted, &owner.identity, &client).await?;
    if generation.id() != body.generation {
        return Err(ApiError::new(ErrorCode::Conflict));
    }
    let snapshot = hosted
        .store
        .update_grant_with_groups(
            &generation,
            &client,
            GrantUpdate {
                expected_revision: body.expected_revision,
                read: body.read,
                send_enabled: body.send_enabled,
            },
            body.group_refs,
        )
        .await?;
    if let Some(uploads) = &hosted.uploads {
        uploads.invalidate(&generation, Some(&client));
    }
    hosted.media.invalidate(&generation, Some(&client));
    hosted.writers.revoke(&generation, Some(&client));
    let _ = hosted.grant_changes.send(());
    Ok(Json(GrantView {
        generation: body.generation,
        snapshot,
    }))
}

pub(super) async fn joined_channels(
    State(state): State<Arc<ServiceState>>,
    headers: HeaderMap,
    query: Result<
        axum::extract::Query<crate::contracts::hosted_tools::ListChannels>,
        axum::extract::rejection::QueryRejection,
    >,
) -> Result<Json<serde_json::Value>, ApiError> {
    let axum::extract::Query(query) =
        query.map_err(|_| ApiError::new(ErrorCode::InvalidRequest))?;
    let limit = query.limit.unwrap_or(100);
    if !(1..=100).contains(&limit) {
        return Err(ApiError::new(ErrorCode::InvalidRequest));
    }
    let (_, owner) = state.browser.owner_read(&headers)?;
    require_owner(&owner.identity)?;
    let hosted = state.hosted()?;
    let principal = owner.identity.principal();
    let generation = hosted
        .store
        .active(principal.issuer(), principal.subject())
        .await?
        .ok_or(ApiError::new(ErrorCode::UnavailableResource))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(hosted.config.request_seconds);
    let context = OperationContext::new(deadline, hosted.cancellation.child_token());
    let lease = hosted.lease(&owner.identity, &generation, &context).await?;
    let context = OperationContext::new(deadline, lease.cancellation().child_token());
    let page = lease
        .client()
        .operations
        .list_channels(
            limit,
            query.offset.unwrap_or(0),
            |channel| {
                channel.is_subscribed
                    && channel.peer_namespace
                        == Some(telegram_core::telegram::types::PeerNamespace::Channel)
            },
            &context,
        )
        .await
        .map_err(super::http_error::from_telegram)?;
    let _ = state.browser.owner_read(&headers)?;
    if lease.cancellation().is_cancelled() {
        return Err(ApiError::new(ErrorCode::UnavailableResource));
    }
    Ok(Json(serde_json::json!({
        "channels":page.channels.into_iter().map(super::channels::channel_json).collect::<Vec<_>>(),
        "total":page.total,
    })))
}

pub(super) async fn page() -> Html<&'static str> {
    Html(include_str!("account.html"))
}

async fn grant_generation(
    hosted: &Hosted,
    identity: &VerifiedIdentity,
    client: &str,
) -> Result<super::AccountGeneration, ApiError> {
    if !hosted.authorized_client(client) {
        return Err(ApiError::new(ErrorCode::UnavailableResource));
    }
    let principal = identity.principal();
    hosted
        .store
        .active(principal.issuer(), principal.subject())
        .await?
        .ok_or(ApiError::new(ErrorCode::UnavailableResource))
}

fn require_owner(identity: &VerifiedIdentity) -> Result<(), ApiError> {
    if identity.scopes().contains(&Scope::Link) {
        Ok(())
    } else {
        Err(ApiError::new(ErrorCode::Forbidden))
    }
}

pub(super) fn positive_channel_id(value: &str) -> Option<i64> {
    let id = value.parse::<i64>().ok()?;
    (id > 0 && id.to_string() == value).then_some(id)
}

#[cfg(test)]
mod tests;
