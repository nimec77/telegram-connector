//! Clock, identity and grant boundaries shared by hosted operations.

use super::accounts::{ActiveAccount, Hosted, positive_channel_id};
use super::registry::AccountLease;
use super::{AccountGeneration, ApiError, ServiceState};
use crate::contracts::{ErrorCode, GrantSnapshot, ReadGrant, Scope};
use crate::identity::{CredentialKind, VerifiedIdentity, unix_now};
use axum::http::{HeaderMap, HeaderName, header};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use telegram_core::operations::OperationContext;
use tokio::time::Instant;

#[derive(Clone)]
pub(super) struct ReadAccess {
    state: Arc<ServiceState>,
    pub(super) hosted: Arc<Hosted>,
    pub(super) identity: VerifiedIdentity,
    pub(super) generation: AccountGeneration,
    pub(super) grant: GrantSnapshot,
    deadline: Instant,
    credential_deadline: Instant,
    browser_headers: Option<HeaderMap>,
}

impl ReadAccess {
    pub(super) async fn authorize(
        state: &Arc<ServiceState>,
        headers: &HeaderMap,
    ) -> Result<Self, ApiError> {
        Self::for_scope(state, headers, Scope::Read).await
    }

    pub(super) async fn for_scope(
        state: &Arc<ServiceState>,
        headers: &HeaderMap,
        scope: Scope,
    ) -> Result<Self, ApiError> {
        let hosted = state.hosted()?;
        let started = Instant::now();
        let service_deadline = (started + Duration::from_secs(hosted.config.request_seconds))
            .min(super::request_deadline());
        let authorization = async {
            let (identity, browser_headers, expiry) = if headers.contains_key(header::AUTHORIZATION)
            {
                let identity = super::bearer_identity(state, headers).await?;
                let expiry = identity.expires_at();
                (identity, None, expiry)
            } else {
                let (_, owner) = state.browser.owner_read(headers)?;
                let expiry = owner.info.expires_at_unix;
                (owner.identity, Some(headers.clone()), expiry)
            };
            if !identity.scopes().contains(&scope)
                || !hosted.authorized_client(identity.client_id())
            {
                return Err(ApiError::new(ErrorCode::Forbidden));
            }
            let principal = identity.principal();
            let generation = hosted
                .store
                .active(principal.issuer(), principal.subject())
                .await?
                .ok_or(ApiError::new(ErrorCode::UnavailableResource))?;
            let grant = hosted
                .store
                .grant(&generation, identity.client_id())
                .await?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| ApiError::new(ErrorCode::Unavailable))?;
            let remaining = Duration::from_secs(expiry)
                .checked_sub(now)
                .filter(|remaining| !remaining.is_zero())
                .ok_or(ApiError::new(ErrorCode::Unauthenticated))?;
            let instant = Instant::now();
            let credential_deadline = instant
                .checked_add(remaining)
                .ok_or(ApiError::new(ErrorCode::Unauthenticated))?;
            let mut deadline = service_deadline.min(credential_deadline);
            if identity.kind() == CredentialKind::Delegated {
                let name = HeaderName::from_static("x-telegram-deadline-unix-ms");
                if headers.contains_key(&name) {
                    let epoch = super::single_header(headers, name)
                        .and_then(|value| value.parse::<u64>().ok())
                        .ok_or(ApiError::new(ErrorCode::InvalidRequest))?;
                    let remaining = Duration::from_millis(epoch)
                        .checked_sub(now)
                        .and_then(|remaining| remaining.checked_sub(Duration::from_millis(100)))
                        .ok_or(ApiError::new(ErrorCode::Timeout))?;
                    deadline = deadline.min(instant.checked_add(remaining).unwrap_or(deadline));
                }
            }
            if deadline <= instant {
                return Err(ApiError::new(ErrorCode::Timeout));
            }
            Ok(Self {
                state: Arc::clone(state),
                hosted: Arc::clone(hosted),
                identity,
                generation,
                grant,
                deadline,
                credential_deadline,
                browser_headers,
            })
        };
        tokio::select! {
            biased;
            _ = hosted.cancellation.cancelled() => Err(ApiError::new(ErrorCode::Unavailable)),
            _ = tokio::time::sleep_until(service_deadline) => Err(ApiError::new(ErrorCode::Timeout)),
            result = authorization => result,
        }
    }

    pub(super) fn require_channel(&self, value: &str) -> Result<(), ApiError> {
        if positive_channel_id(value).is_none() {
            return Err(ApiError::new(ErrorCode::InvalidRequest));
        }
        if self.allows_channel(value) {
            Ok(())
        } else {
            Err(ApiError::new(ErrorCode::UnavailableResource))
        }
    }

    pub(super) fn allows_channel(&self, value: &str) -> bool {
        self.hosted.config.eligible_channels.contains(value)
            && match &self.grant.read {
                ReadGrant::None => false,
                ReadGrant::AllJoined => self.identity.kind() == CredentialKind::Direct,
                ReadGrant::Channels(channels) => channels
                    .binary_search_by(|id| id.as_str().cmp(value))
                    .is_ok(),
            }
    }

    pub(super) async fn lease(&self) -> Result<AccountLease<ActiveAccount>, ApiError> {
        let context = self.opening_context();
        let lease = self
            .hosted
            .lease(&self.identity, &self.generation, &context)
            .await?;
        self.revalidate().await?;
        Ok(lease)
    }

    pub(super) fn cap_deadline(&mut self, deadline: Instant) {
        self.deadline = self.deadline.min(deadline);
    }

    pub(super) fn opening_context(&self) -> OperationContext {
        OperationContext::new(self.deadline, self.hosted.cancellation.child_token())
    }

    pub(super) fn context<C>(&self, lease: &AccountLease<C>) -> OperationContext {
        OperationContext::new(self.deadline, lease.cancellation().child_token())
    }

    pub(super) fn credential_deadline(&self) -> Instant {
        self.credential_deadline
    }

    /// Hold the grant/retirement transition through local handle publication or access.
    pub(super) async fn current(&self) -> Result<tokio::sync::MutexGuard<'_, ()>, ApiError> {
        tokio::select! {
            biased;
            _ = self.hosted.cancellation.cancelled() => Err(ApiError::new(ErrorCode::Unavailable)),
            _ = tokio::time::sleep_until(self.deadline) => Err(ApiError::new(ErrorCode::Timeout)),
            result = async {
                let transition = self.hosted.transition.lock().await;
                self.revalidate().await?;
                Ok(transition)
            } => result,
        }
    }

    pub(super) async fn revalidate(&self) -> Result<(), ApiError> {
        if self.credential_deadline <= Instant::now() || self.identity.expires_at() <= unix_now()? {
            return Err(ApiError::new(ErrorCode::Unauthenticated));
        }
        if self.hosted.cancellation.is_cancelled() {
            return Err(ApiError::new(ErrorCode::Unavailable));
        }
        if let Some(headers) = &self.browser_headers {
            self.state.browser.owner_read(headers)?;
        }
        let grant = self
            .hosted
            .store
            .grant(&self.generation, self.identity.client_id())
            .await?;
        if grant.revision != self.grant.revision {
            return Err(ApiError::new(ErrorCode::UnavailableResource));
        }
        Ok(())
    }

    pub(super) fn any_reads(&self) -> bool {
        self.hosted
            .config
            .eligible_channels
            .iter()
            .any(|id| self.allows_channel(id))
    }

    pub(super) fn can_discover(&self) -> bool {
        self.hosted
            .config
            .discovery_clients
            .contains(self.identity.client_id())
            && self.any_reads()
    }

    pub(super) fn can_send(&self) -> bool {
        self.hosted.config.sending_enabled
            && self.identity.scopes().contains(&Scope::Send)
            && self.grant.send_enabled
    }
}

#[derive(serde::Serialize)]
pub(super) struct Capabilities {
    tools: Vec<&'static str>,
}

pub(super) async fn capabilities(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<ServiceState>>,
    headers: HeaderMap,
) -> Result<axum::Json<Capabilities>, ApiError> {
    let mut tools = Vec::new();
    let identity = super::bearer_identity(&state, &headers).await?;
    if identity.kind() != CredentialKind::Delegated {
        return Err(ApiError::new(ErrorCode::Forbidden));
    }
    let hosted = state.hosted()?;
    if !hosted.authorized_client(identity.client_id()) {
        return Err(ApiError::new(ErrorCode::Forbidden));
    }
    let principal = identity.principal();
    if hosted
        .store
        .active(principal.issuer(), principal.subject())
        .await?
        .is_none()
    {
        return Ok(axum::Json(Capabilities { tools }));
    }
    if identity.scopes().contains(&Scope::Read) {
        let access = ReadAccess::authorize(&state, &headers).await?;
        access.revalidate().await?;
        if access.any_reads() {
            tools.extend([
                "search_messages",
                "get_recent_messages",
                "get_messages_batch",
                "get_subscribed_channels",
                "get_channel_info",
                "resolve_channels",
                "get_channel_stats",
                "generate_message_link",
                "transcribe_voice_message",
                "get_message_media",
                "get_messages_media_batch",
            ]);
            if access.can_discover() {
                tools.push("search_public_channels");
            }
        }
    }
    if identity.scopes().contains(&Scope::Send) {
        let access = ReadAccess::for_scope(&state, &headers, Scope::Send).await?;
        access.revalidate().await?;
        if access.can_send() {
            tools.extend(["send_message", "send_media"]);
        }
    }
    Ok(axum::Json(Capabilities { tools }))
}
