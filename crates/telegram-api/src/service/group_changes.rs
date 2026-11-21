//! Authorization, long polling and hosted configuration for group changes.

use super::accounts::ActiveAccount;
use super::authorization::ReadAccess;
use super::group_journal::{GroupJournal, PageResult, Retention, valid_cursor};
use super::http_error::from_telegram;
use super::registry::AccountLease;
use super::storage::StorageError;
use super::{ApiError, ServiceState};
use crate::contracts::ErrorCode;
use crate::contracts::groups::{CursorExpired, GroupRef};
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use telegram_core::TelegramClient;
use telegram_core::error::Error;
use telegram_core::rate_limiter::RateLimiterTrait as _;
use tokio::sync::Notify;
use tokio::time::{Instant, sleep_until};

/// Limits for the private per-group replay journal.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GroupChangesConfig {
    /// Retain each entry for this many seconds at most.
    pub retention_seconds: u64,
    /// Maximum serialized change bytes retained per group.
    pub retention_bytes: usize,
}

impl Default for GroupChangesConfig {
    fn default() -> Self {
        Self {
            retention_seconds: 86_400,
            retention_bytes: 64 * 1024 * 1024,
        }
    }
}

impl GroupChangesConfig {
    pub(super) fn valid(&self) -> bool {
        (1..=31_536_000).contains(&self.retention_seconds)
            && (256 * 1024..=1024 * 1024 * 1024).contains(&self.retention_bytes)
    }

    pub(super) fn retention(&self) -> Retention {
        Retention {
            seconds: self.retention_seconds,
            bytes: self.retention_bytes,
        }
    }
}

pub(super) struct GroupMonitoring {
    journal: Option<Arc<GroupJournal>>,
    pub(super) configured: HashSet<GroupRef>,
    pub(super) changed: Arc<Notify>,
    pending: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl GroupMonitoring {
    pub(super) fn new(journal: Option<GroupJournal>, configured: HashSet<GroupRef>) -> Self {
        Self {
            journal: journal.map(Arc::new),
            configured,
            changed: Arc::new(Notify::new()),
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub(super) fn journal(&self) -> Result<&Arc<GroupJournal>, StorageError> {
        self.journal.as_ref().ok_or(StorageError::Unavailable)
    }

    pub(super) fn pending_gap(&self, generation: &str) -> Result<Arc<AtomicBool>, StorageError> {
        let mut pending = self.pending.lock().map_err(|_| StorageError::Unavailable)?;
        Ok(Arc::clone(
            pending.entry(generation.to_owned()).or_default(),
        ))
    }

    pub(super) async fn retire(&self, generation: &str) -> Result<(), StorageError> {
        self.journal()?.retire(generation).await?;
        self.forget_gap(generation);
        Ok(())
    }

    pub(super) fn forget_gap(&self, generation: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(generation);
        }
    }

    pub(super) async fn memberships(
        &self,
        client: &TelegramClient,
        budget: Duration,
    ) -> HashSet<GroupRef> {
        let mut groups: Vec<_> = self.configured.iter().copied().collect();
        if !groups.is_empty() {
            // A slow first wave should not starve later groups across sweeps.
            let start = getrandom::u32().map_or(0, |value| value as usize % groups.len());
            groups.rotate_left(start);
        }
        let source = groups.into_iter().map(|group| async move {
            matches!(joined_group(client, group).await, Ok(true)).then_some(group)
        });
        let checks = futures::stream::iter(source).buffer_unordered(4);
        tokio::pin!(checks);
        let deadline = Instant::now() + budget;
        let mut joined = HashSet::new();
        loop {
            tokio::select! {
                biased;
                _ = sleep_until(deadline) => break,
                next = checks.next() => match next {
                    Some(Some(group)) => { joined.insert(group); }
                    Some(None) => {}
                    None => break,
                }
            }
        }
        joined
    }
}

async fn joined_group(client: &TelegramClient, group: GroupRef) -> Result<bool, Error> {
    match group {
        GroupRef::Chat(id) => client.joined_basic_group(id).await,
        GroupRef::Supergroup(id) => client.joined_supergroup(id).await,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Start {
    Earliest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Parameters {
    start: Option<Start>,
    after: Option<String>,
    #[serde(default = "page_limit")]
    limit: usize,
    #[serde(default)]
    wait_ms: u64,
}

fn page_limit() -> usize {
    100
}

impl Parameters {
    fn valid(&self) -> bool {
        matches!(
            (&self.start, &self.after),
            (Some(Start::Earliest), None) | (None, Some(_))
        ) && self.after.as_deref().is_none_or(valid_cursor)
            && (1..=100).contains(&self.limit)
            && self.wait_ms <= 15_000
    }
}

pub(super) async fn read(
    State(state): State<Arc<ServiceState>>,
    Path(group_ref): Path<String>,
    query: Result<Query<Parameters>, QueryRejection>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::new(ErrorCode::InvalidRequest))?;
    let group = group_ref
        .parse()
        .map_err(|()| ApiError::new(ErrorCode::InvalidRequest))?;
    if !query.valid() {
        return Err(ApiError::new(ErrorCode::InvalidRequest));
    }
    if !headers.contains_key(header::AUTHORIZATION) {
        return Err(ApiError::new(ErrorCode::Unauthenticated));
    }
    let access = ReadAccess::authorize(&state, &headers).await?;
    if !access.hosted.groups.configured.contains(&group)
        || access.grant.group_refs.binary_search(&group_ref).is_err()
    {
        return Err(ApiError::new(ErrorCode::UnavailableResource));
    }
    let deadline = access.opening_context().deadline();
    let work = async {
        let mut grant_changes = access.hosted.grant_changes.subscribe();
        let lease = access.lease().await?;
        require_membership(&lease, group).await?;
        let principal = access.identity.principal();
        let binding = serde_json::to_string(&(principal.issuer(), principal.subject()))
            .map_err(|_| ApiError::new(ErrorCode::Unavailable))?;
        let monitoring = &access.hosted.groups;
        let wait_started = Instant::now();
        // Leave time for the membership check needed to publish a waited page.
        let remaining = deadline.saturating_duration_since(wait_started);
        let recheck_budget = (remaining / 2).min(Duration::from_secs(1));
        let wait_until =
            (wait_started + Duration::from_millis(query.wait_ms)).min(deadline - recheck_budget);
        let mut waited = false;
        let page = loop {
            let changed = monitoring.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let result = monitoring
                .journal()?
                .page(
                    access.generation.id(),
                    &binding,
                    &group_ref,
                    query.after.as_deref(),
                    query.limit,
                )
                .await
                .map_err(|error| match error {
                    StorageError::Missing => ApiError::new(ErrorCode::Unavailable),
                    other => ApiError::from(other),
                })?;
            if !matches!(&result, PageResult::Page(page) if page.entries.is_empty())
                || Instant::now() >= wait_until
            {
                break result;
            }

            waited = true;
            tokio::select! {
                biased;
                _ = lease.revocation().cancelled() => {
                    return Err(ApiError::new(ErrorCode::UnavailableResource));
                }
                _ = grant_changes.recv() => access.revalidate().await?,
                _ = sleep_until(wait_until) => (),
                _ = &mut changed => (),
            }
        };
        if waited {
            access.revalidate().await?;
            require_membership(&lease, group).await?;
        }
        let response = render(page)?;
        let _guard = access.current().await?;
        Ok(response)
    };
    tokio::select! {
        biased;
        _ = sleep_until(access.credential_deadline()) => Err(ApiError::new(ErrorCode::Unauthenticated)),
        _ = sleep_until(deadline) => Err(ApiError::new(ErrorCode::Timeout)),
        _ = access.hosted.cancellation.cancelled() => Err(ApiError::new(ErrorCode::Unavailable)),
        result = work => result,
    }
}

async fn require_membership(
    lease: &AccountLease<ActiveAccount>,
    group: GroupRef,
) -> Result<(), ApiError> {
    lease.limiter().acquire(1).await.map_err(from_telegram)?;
    if joined_group(&lease.client().client, group)
        .await
        .map_err(from_telegram)?
    {
        Ok(())
    } else {
        Err(ApiError::new(ErrorCode::UnavailableResource))
    }
}

#[derive(Serialize)]
struct Expired {
    error: ExpiredCode,
    #[serde(flatten)]
    boundary: CursorExpired,
}

#[derive(Serialize)]
struct ExpiredCode {
    code: &'static str,
}

fn render(result: PageResult) -> Result<Response, ApiError> {
    match result {
        PageResult::Page(page) => Ok(Json(page).into_response()),
        PageResult::Expired(boundary) => Ok((
            StatusCode::GONE,
            Json(Expired {
                error: ExpiredCode {
                    code: "cursor_expired",
                },
                boundary,
            }),
        )
            .into_response()),
        PageResult::InvalidCursor => Err(ApiError::new(ErrorCode::InvalidRequest)),
    }
}

#[cfg(test)]
mod tests;
