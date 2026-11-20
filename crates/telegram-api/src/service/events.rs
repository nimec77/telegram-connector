//! Authenticated live-only channel streams over SSE.

use super::authorization::ReadAccess;
use super::event_writer::WriteAuthorization;
use super::registry::AccountLease;
use super::{ApiError, ServiceState};
use crate::contracts::ErrorCode;
use axum::body::{Body, Bytes};
use axum::extract::{Extension, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse as _, Response};
use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use telegram_core::operations::CancellationToken;
use telegram_core::streams::{
    ChannelEvent, ChannelSubscription, QueuedEvent, StreamLimits, SubscribeError,
};
use telegram_core::telegram::types::ChannelId;
use tokio::sync::{OwnedSemaphorePermit, broadcast};
use tokio::time::Instant;

const KEEPALIVE: &[u8] = b": heartbeat\n\n";

/// Tunable channel stream and transport admission limits.
#[derive(Clone, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EventConfig {
    pub max_channels: usize,
    pub max_account_subscriptions: usize,
    pub max_total_subscriptions: usize,
    pub max_queue_items: usize,
    pub max_queue_bytes: usize,
    pub max_event_bytes: usize,
    pub max_total_queue_bytes: usize,
    pub setup_seconds: u64,
    pub heartbeat_seconds: u64,
    pub stalled_write_seconds: u64,
}

impl Default for EventConfig {
    fn default() -> Self {
        Self {
            max_channels: 20,
            max_account_subscriptions: 4,
            max_total_subscriptions: 256,
            max_queue_items: 128,
            max_queue_bytes: 1024 * 1024,
            max_event_bytes: 64 * 1024,
            max_total_queue_bytes: 64 * 1024 * 1024,
            setup_seconds: 30,
            heartbeat_seconds: 15,
            stalled_write_seconds: 10,
        }
    }
}

impl EventConfig {
    pub(super) fn valid(&self) -> bool {
        (1..=100).contains(&self.max_channels)
            && (1..=32).contains(&self.max_account_subscriptions)
            && (1..=1024).contains(&self.max_total_subscriptions)
            && (1..=1024).contains(&self.max_queue_items)
            && (80..=1024 * 1024).contains(&self.max_event_bytes)
            && (self.max_event_bytes..=8 * 1024 * 1024).contains(&self.max_queue_bytes)
            && (self.max_queue_bytes..=256 * 1024 * 1024).contains(&self.max_total_queue_bytes)
            && (1..=120).contains(&self.setup_seconds)
            && (1..=60).contains(&self.heartbeat_seconds)
            && (1..=60).contains(&self.stalled_write_seconds)
    }

    pub(super) fn stream_limits(&self) -> StreamLimits {
        StreamLimits {
            max_channels: self.max_channels,
            max_subscriptions: self.max_account_subscriptions,
            max_queue_items: self.max_queue_items,
            max_queue_bytes: self.max_queue_bytes,
            max_event_bytes: self.max_event_bytes,
            setup_timeout: Duration::from_secs(self.setup_seconds),
        }
    }
}

pub(super) async fn subscribe(
    State(state): State<Arc<ServiceState>>,
    RawQuery(query): RawQuery,
    writer: Option<Extension<axum::extract::ConnectInfo<WriteAuthorization>>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let started = Instant::now();
    let mut access = ReadAccess::authorize(&state, &headers).await?;
    let service = Arc::clone(&access.hosted);
    access.cap_deadline(started + Duration::from_secs(service.config.events.setup_seconds));
    let ids = parse_channels(query.as_deref(), &access)?;
    let permit = Arc::clone(&service.subscribers)
        .try_acquire_owned()
        .map_err(|_| ApiError::new(ErrorCode::RateLimited))?;
    let mut changes = service.grant_changes.subscribe();
    let (lease, mut subscription) = {
        let mut setup = std::pin::pin!(establish(&access, &ids));
        loop {
            tokio::select! {
                biased;
                _ = service.cancellation.cancelled() => return Err(ApiError::new(ErrorCode::Unavailable)),
                _ = tokio::time::sleep_until(context_deadline(&access)) => return Err(ApiError::new(ErrorCode::Timeout)),
                _ = changes.recv() => access.revalidate().await?,
                result = &mut setup => break result?,
            }
        }
    };
    let Some(ready) = subscription.next_queued().await else {
        return Err(ApiError::new(ErrorCode::Unavailable));
    };
    if !matches!(ready.event(), ChannelEvent::Ready(_)) {
        return Err(ApiError::new(ErrorCode::Unavailable));
    }
    let writer = writer.map_or_else(
        WriteAuthorization::default,
        |Extension(axum::extract::ConnectInfo(writer))| writer,
    );
    let revocation = lease.revocation().clone();
    {
        let _transition =
            tokio::time::timeout_at(context_deadline(&access), service.transition.lock())
                .await
                .map_err(|_| ApiError::new(ErrorCode::Timeout))?;
        access.revalidate().await?;
        writer.bind(
            access.credential_deadline(),
            subscription.membership_lost(),
            revocation.clone(),
        )?;
        service.writers.register(&access, &writer)?;
    }
    let watch = watch_authorization(&access, &subscription, &writer, revocation.clone());
    let state = Stream {
        ready: Some(ready),
        subscription,
        _lease: lease,
        revocation,
        access,
        changes,
        writer,
        _watch: watch,
        _permit: permit,
        finished: false,
        heartbeat: Duration::from_secs(service.config.events.heartbeat_seconds),
    };
    let body = futures::stream::unfold(state, |mut state| async move {
        state
            .next()
            .await
            .map(|bytes| (Ok::<_, Infallible>(bytes), state))
    });
    let mut response = Body::from_stream(body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    headers.insert(header::CONNECTION, HeaderValue::from_static("close"));
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}

fn context_deadline(access: &ReadAccess) -> Instant {
    access.opening_context().deadline()
}

async fn establish(
    access: &ReadAccess,
    ids: &[ChannelId],
) -> Result<
    (
        AccountLease<super::accounts::ActiveAccount>,
        ChannelSubscription,
    ),
    ApiError,
> {
    let lease = access.lease().await?;
    let subscription = lease
        .client()
        .streams
        .subscribe_channel_posts(ids, &access.context(&lease))
        .await
        .map_err(subscribe_error)?;
    Ok((lease, subscription))
}

struct Watch(tokio::task::JoinHandle<()>);

impl Drop for Watch {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn watch_authorization(
    access: &ReadAccess,
    subscription: &ChannelSubscription,
    writer: &WriteAuthorization,
    generation: CancellationToken,
) -> Watch {
    let access = access.clone();
    let membership = subscription.membership_lost();
    let writer = writer.clone();
    let mut changes = access.hosted.grant_changes.subscribe();
    Watch(tokio::spawn(async move {
        let mut periodic = tokio::time::interval(Duration::from_secs(1));
        loop {
            let stop = tokio::select! {
                biased;
                _ = access.hosted.cancellation.cancelled() => return,
                _ = generation.cancelled() => true,
                _ = membership.cancelled() => true,
                _ = tokio::time::sleep_until(access.credential_deadline()) => true,
                _ = changes.recv() => access.revalidate().await.is_err(),
                _ = periodic.tick() => access.revalidate().await.is_err(),
            };
            if stop {
                writer.revoke();
                return;
            }
        }
    }))
}

struct Stream {
    ready: Option<QueuedEvent>,
    subscription: ChannelSubscription,
    _lease: AccountLease<super::accounts::ActiveAccount>,
    revocation: CancellationToken,
    _watch: Watch,
    access: ReadAccess,
    changes: broadcast::Receiver<()>,
    writer: WriteAuthorization,
    _permit: OwnedSemaphorePermit,
    heartbeat: Duration,
    finished: bool,
}

impl Stream {
    async fn next(&mut self) -> Option<Bytes> {
        if self.finished {
            return None;
        }
        if self.access.hosted.cancellation.is_cancelled() {
            self.finished = true;
            return Some(Bytes::from_static(
                b"event: interrupted\ndata: {\"reason\":\"service_draining\"}\n\n",
            ));
        }
        if self.access.revalidate().await.is_err() {
            self.writer.revoke();
            return None;
        }
        if let Some(ready) = self.ready.take() {
            return Some(encode(ready));
        }
        let pulse = Instant::now() + self.heartbeat;
        loop {
            tokio::select! {
                biased;
                _ = tokio::time::sleep_until(self.access.credential_deadline()) => {
                    self.writer.revoke();
                    return None;
                }
                _ = self.access.hosted.cancellation.cancelled() => {
                    self.finished = true;
                    return Some(Bytes::from_static(b"event: interrupted\ndata: {\"reason\":\"service_draining\"}\n\n"));
                }
                _ = self.revocation.cancelled() => {
                    self.writer.revoke();
                    return None;
                }
                _ = self.changes.recv() => {
                    if self.access.revalidate().await.is_err() {
                        self.writer.revoke();
                        return None;
                    }
                }
                event = self.subscription.next_queued() => {
                    let event = event?;
                    if self.access.revalidate().await.is_err() {
                        self.writer.revoke();
                        return None;
                    }
                    self.finished = matches!(event.event(), ChannelEvent::Interrupted(_));
                    return Some(encode(event));
                }
                _ = tokio::time::sleep_until(pulse) => return Some(Bytes::from_static(KEEPALIVE)),
            }
        }
    }
}

struct EventBytes {
    frame: Vec<u8>,
    _queued: QueuedEvent,
}

impl AsRef<[u8]> for EventBytes {
    fn as_ref(&self) -> &[u8] {
        &self.frame
    }
}

fn encode(queued: QueuedEvent) -> Bytes {
    let frame = format!(
        "event: {}\ndata: {}\n\n",
        queued.event_name(),
        queued.json()
    )
    .into_bytes();
    Bytes::from_owner(EventBytes {
        frame,
        _queued: queued,
    })
}

fn parse_channels(query: Option<&str>, access: &ReadAccess) -> Result<Vec<ChannelId>, ApiError> {
    let query = query
        .filter(|value| value.len() <= 4096)
        .ok_or(ApiError::new(ErrorCode::InvalidRequest))?;
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if key != "channel_id" {
            return Err(ApiError::new(ErrorCode::InvalidRequest));
        }
        let id = super::accounts::positive_channel_id(&value)
            .and_then(|id| ChannelId::new(id).ok())
            .ok_or(ApiError::new(ErrorCode::InvalidRequest))?;
        access.require_channel(&value)?;
        if seen.insert(id) {
            ids.push(id);
        }
    }
    if ids.is_empty() || ids.len() > access.hosted.config.events.max_channels {
        return Err(ApiError::new(ErrorCode::InvalidRequest));
    }
    Ok(ids)
}

fn subscribe_error(error: SubscribeError) -> ApiError {
    ApiError::new(match error {
        SubscribeError::InvalidChannels => ErrorCode::InvalidRequest,
        SubscribeError::Ineligible => ErrorCode::UnavailableResource,
        SubscribeError::Capacity | SubscribeError::SetupOverflow => ErrorCode::RateLimited,
        SubscribeError::ReaderUnavailable => ErrorCode::Unavailable,
        SubscribeError::Operation(error) => return super::http_error::from_telegram(error),
    })
}

#[cfg(test)]
mod tests;
