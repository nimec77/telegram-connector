//! Charged preview generation and caller-bound JPEG handles.

use super::accounts::positive_channel_id;
use super::authorization::ReadAccess;
use super::http_error::from_telegram;
use super::{AccountGeneration, ApiError, ServiceState, json_error};
use crate::contracts::ErrorCode;
use crate::contracts::hosted_tools::Previews;
use axum::Json;
use axum::body::Bytes;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use telegram_core::error::Error;
use telegram_core::operations::{MAX_PREVIEW_JPEG_BYTES, MediaPreviewRequest};
use telegram_core::telegram::types::{ChannelId, MediaType, MessageId};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

const TTL: Duration = Duration::from_secs(300);
const MAX_PREVIEW_CALLS: usize = 8;

pub(super) struct MediaStore {
    state: Mutex<StateCache>,
    global: Quota,
    active_previews: Semaphore,
}

impl Default for MediaStore {
    fn default() -> Self {
        Self {
            state: Mutex::new(StateCache::default()),
            global: Quota::new(256, 64 * 1024 * 1024),
            active_previews: Semaphore::new(MAX_PREVIEW_CALLS),
        }
    }
}

#[derive(Default)]
struct StateCache {
    objects: HashMap<String, Stored>,
    clients: HashMap<ClientKey, Weak<Quota>>,
}

#[derive(Clone, Eq, PartialEq, Hash)]
struct ClientKey {
    generation: AccountGeneration,
    client: String,
}

#[derive(Clone)]
struct MediaOwner {
    generation: AccountGeneration,
    client: String,
    revision: u64,
}

impl MediaOwner {
    fn current(access: &ReadAccess) -> Self {
        Self {
            generation: access.generation.clone(),
            client: access.identity.client_id().into(),
            revision: access.grant.revision,
        }
    }
}

struct Stored {
    owner: MediaOwner,
    channel: ChannelId,
    expires_at: Instant,
    jpeg: Bytes,
}

struct PreviewObject {
    channel: ChannelId,
    jpeg: Bytes,
}

struct Quota {
    objects: Arc<Semaphore>,
    bytes: Arc<Semaphore>,
}

impl Quota {
    fn new(objects: usize, bytes: usize) -> Self {
        Self {
            objects: Arc::new(Semaphore::new(objects)),
            bytes: Arc::new(Semaphore::new(bytes)),
        }
    }

    fn reserve(&self, bytes: usize) -> Result<[OwnedSemaphorePermit; 2], ApiError> {
        let bytes = u32::try_from(bytes).map_err(|_| ApiError::new(ErrorCode::PayloadTooLarge))?;
        let object = Arc::clone(&self.objects)
            .try_acquire_owned()
            .map_err(|_| ApiError::new(ErrorCode::RateLimited))?;
        let bytes = Arc::clone(&self.bytes)
            .try_acquire_many_owned(bytes)
            .map_err(|_| ApiError::new(ErrorCode::RateLimited))?;
        Ok([object, bytes])
    }
}

struct RetainedJpeg {
    jpeg: Vec<u8>,
    _global: [OwnedSemaphorePermit; 2],
    _client: [OwnedSemaphorePermit; 2],
    _client_quota: Arc<Quota>,
}

impl AsRef<[u8]> for RetainedJpeg {
    fn as_ref(&self) -> &[u8] {
        &self.jpeg
    }
}

impl MediaStore {
    fn insert(
        &self,
        owner: &MediaOwner,
        channel: ChannelId,
        jpeg: Vec<u8>,
        expires_at: Instant,
    ) -> Result<String, ApiError> {
        if jpeg.len() > MAX_PREVIEW_JPEG_BYTES {
            return Err(ApiError::new(ErrorCode::PayloadTooLarge));
        }
        if !jpeg.starts_with(&[0xff, 0xd8]) {
            return Err(ApiError::new(ErrorCode::Unavailable));
        }
        let mut random = [0_u8; 24];
        getrandom::fill(&mut random).map_err(|_| ApiError::new(ErrorCode::Unavailable))?;
        let handle = URL_SAFE_NO_PAD.encode(random);
        let now = Instant::now();
        if expires_at <= now {
            return Err(ApiError::new(ErrorCode::Unauthenticated));
        }

        let mut state = self
            .state
            .lock()
            .map_err(|_| ApiError::new(ErrorCode::Unavailable))?;
        state.purge_expired(now);
        if state.objects.contains_key(&handle) {
            return Err(ApiError::new(ErrorCode::Unavailable));
        }
        let key = ClientKey {
            generation: owner.generation.clone(),
            client: owner.client.clone(),
        };
        let client_quota = state
            .clients
            .get(&key)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| Arc::new(Quota::new(32, 12 * 1024 * 1024)));
        let global = self.global.reserve(jpeg.capacity())?;
        let client = client_quota.reserve(jpeg.capacity())?;
        state.clients.insert(key, Arc::downgrade(&client_quota));
        state.objects.insert(
            handle.clone(),
            Stored {
                owner: owner.clone(),
                channel,
                expires_at,
                jpeg: Bytes::from_owner(RetainedJpeg {
                    jpeg,
                    _global: global,
                    _client: client,
                    _client_quota: client_quota,
                }),
            },
        );
        Ok(handle)
    }

    fn lookup(&self, owner: &MediaOwner, handle: &str) -> Result<PreviewObject, ApiError> {
        if handle.len() != 32
            || !handle
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(ApiError::new(ErrorCode::UnavailableResource));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| ApiError::new(ErrorCode::Unavailable))?;
        state.purge_expired(Instant::now());
        let found = state
            .objects
            .get(handle)
            .filter(|entry| {
                entry.owner.generation == owner.generation
                    && entry.owner.client == owner.client
                    && entry.owner.revision == owner.revision
            })
            .ok_or(ApiError::new(ErrorCode::UnavailableResource))?;
        Ok(PreviewObject {
            channel: found.channel,
            jpeg: found.jpeg.clone(),
        })
    }

    pub(super) fn purge_expired(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.purge_expired(Instant::now());
        }
    }

    pub(super) fn invalidate(&self, generation: &AccountGeneration, client: Option<&str>) {
        if let Ok(mut state) = self.state.lock() {
            state.objects.retain(|_, entry| {
                &entry.owner.generation != generation
                    || client.is_some_and(|client| entry.owner.client != client)
            });
            state.clients.retain(|_, quota| quota.strong_count() != 0);
        }
    }
}

impl StateCache {
    fn purge_expired(&mut self, now: Instant) {
        self.objects.retain(|_, entry| entry.expires_at > now);
        self.clients.retain(|_, quota| quota.strong_count() != 0);
    }
}

#[derive(Serialize)]
struct PreviewResponse {
    channel_id: String,
    items: Vec<PreviewItem>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum PreviewItem {
    Ready {
        message_id: i32,
        handle: String,
        mime_type: &'static str,
        width: u32,
        height: u32,
        size_bytes: usize,
        media_type: MediaType,
        is_thumbnail: bool,
        source_size_bytes: u64,
    },
    Failed {
        message_id: i32,
        error: PreviewError,
    },
}

#[derive(Serialize)]
struct PreviewError {
    code: &'static str,
}

pub(super) async fn previews(
    State(state): State<Arc<ServiceState>>,
    headers: HeaderMap,
    request: Result<Json<Previews>, JsonRejection>,
) -> Result<Response, ApiError> {
    let access = ReadAccess::authorize(&state, &headers).await?;
    let Json(request) = request.map_err(json_error)?;
    access.require_channel(&request.channel_id)?;
    let channel = positive_channel_id(&request.channel_id)
        .and_then(|value| ChannelId::new(value).ok())
        .ok_or(ApiError::new(ErrorCode::InvalidRequest))?;
    let ids = request
        .message_ids
        .into_iter()
        .map(|id| MessageId::new(i64::from(id)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(from_telegram)?;
    let operation =
        MediaPreviewRequest::try_new(channel, ids, request.max_dimension).map_err(from_telegram)?;
    let store = &access.hosted.media;
    let _admitted = store
        .active_previews
        .try_acquire()
        .map_err(|_| ApiError::new(ErrorCode::RateLimited))?;
    let lease = access.lease().await?;
    let context = access.context(&lease);
    let config = &access.hosted.config;
    let mut batch = lease
        .client()
        .operations
        .media_previews(
            operation,
            config.rate_limiting.media_download_cost,
            Duration::from_secs(config.telegram.timeouts.download_secs),
            &context,
        )
        .await
        .map_err(from_telegram)?;

    let _transition = access.current().await?;
    let owner = MediaOwner::current(&access);
    let expires_at = access.credential_deadline().min(Instant::now() + TTL);
    let mut delivered = 0;
    let items = std::mem::take(&mut batch.items)
        .into_iter()
        .map(|row| {
            let id = row.message_id;
            let preview = match row.result {
                Ok(preview) => preview,
                Err(error) => return failed_item(id, preview_failure(error)),
            };
            let size_bytes = preview.jpeg.len();
            match store.insert(&owner, channel, preview.jpeg, expires_at) {
                Ok(handle) => {
                    delivered += 1;
                    PreviewItem::Ready {
                        message_id: id,
                        handle,
                        mime_type: "image/jpeg",
                        width: preview.width,
                        height: preview.height,
                        size_bytes,
                        media_type: preview.media_type,
                        is_thumbnail: preview.is_thumbnail,
                        source_size_bytes: preview.source_size_bytes,
                    }
                }
                Err(error) if error.0 == ErrorCode::RateLimited => failed_item(id, "rate_limited"),
                Err(error) if error.0 == ErrorCode::PayloadTooLarge => {
                    failed_item(id, "media_too_large")
                }
                Err(_) => failed_item(id, "unavailable"),
            }
        })
        .collect();
    batch.deliver(delivered);
    Ok((
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(PreviewResponse {
            channel_id: request.channel_id,
            items,
        }),
    )
        .into_response())
}

pub(super) async fn download(
    State(state): State<Arc<ServiceState>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
) -> Result<Response, ApiError> {
    let access = ReadAccess::authorize(&state, &headers).await?;
    let Path(handle) = path.map_err(|_| ApiError::new(ErrorCode::UnavailableResource))?;
    let owner = MediaOwner::current(&access);
    let store = &access.hosted.media;
    let candidate = store.lookup(&owner, &handle)?;
    let channel = candidate.channel;
    drop(candidate);
    access.require_channel(&channel.to_string())?;
    let lease = access.lease().await?;
    let context = access.context(&lease);
    lease
        .client()
        .operations
        .authorize_media_channel(channel, &context)
        .await
        .map_err(from_telegram)?;

    let _transition = access.current().await?;
    let preview = store.lookup(&owner, &handle)?;
    Ok((
        [
            (header::CACHE_CONTROL, "private, no-store"),
            (header::CONTENT_TYPE, "image/jpeg"),
        ],
        preview.jpeg,
    )
        .into_response())
}

fn failed_item(message_id: i32, code: &'static str) -> PreviewItem {
    PreviewItem::Failed {
        message_id,
        error: PreviewError { code },
    }
}

fn preview_failure(error: Error) -> &'static str {
    match error {
        Error::MessageNotFound { .. } => "message_not_found",
        Error::NoVisualMedia { .. } => "no_visual_media",
        Error::MediaTooLarge { .. }
        | Error::ImageLimitExceeded
        | Error::PayloadCapExceeded { .. } => "media_too_large",
        Error::DownloadFailed(_) => "download_failed",
        Error::Timeout { .. } => "timeout",
        Error::RateLimit { .. } => "rate_limited",
        _ => "unavailable",
    }
}

#[cfg(test)]
#[path = "media/tests.rs"]
mod tests;
