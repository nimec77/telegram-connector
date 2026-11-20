//! Private, size-limited and caller-bound HTTP upload handles.

use super::authorization::ReadAccess;
use super::{AccountGeneration, ApiError, ServiceState};
use crate::contracts::{ErrorCode, Scope};
use crate::identity::Principal;
use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use base64::Engine as _;
use futures::StreamExt as _;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use telegram_core::sending::{StagedUpload, StagingConfig, StagingSource, StagingStore};
use telegram_core::telegram::sending::SendDeadline;
use telegram_core::telegram::types::sending::SendMediaKind;
use tokio::io::AsyncWriteExt as _;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

const MAX_UPLOAD_BYTES: u32 = 104_857_600;
const MAX_IMAGE_BYTES: u32 = 10_485_760;
const TOTAL_BYTES: usize = 209_715_200;
const MAX_HANDLES: usize = 128;
const MAX_CLIENT_HANDLES: usize = 2;
const UPLOAD_TTL: Duration = Duration::from_secs(300);

pub(super) struct Uploads {
    staged: Arc<StagingStore>,
    saved: Mutex<HashMap<String, Arc<Stored>>>,
    capacity: Arc<Semaphore>,
    _root: tempfile::TempDir,
    _snapshots: tempfile::TempDir,
}

#[derive(PartialEq, Eq)]
struct Owner {
    principal: Principal,
    client: String,
    generation: AccountGeneration,
    revision: u64,
}

struct Stored {
    file: tempfile::TempPath,
    owner: Owner,
    expires: Instant,
    mime: &'static str,
    _bytes: OwnedSemaphorePermit,
}

#[derive(Serialize)]
pub(super) struct UploadReply {
    upload_handle: String,
    mime_type: &'static str,
    size_bytes: u32,
    expires_in_seconds: u64,
}

impl Uploads {
    pub(super) async fn new(root: &Path) -> Result<Self, ApiError> {
        // Only this service opens its locked private data root. These designated
        // subtrees contain ephemeral uploads from the previous process.
        for name in ["incoming", "snapshots"] {
            let path = root.join(name);
            match std::fs::remove_dir_all(&path) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(_) => return Err(unavailable()),
            }
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt as _;
                builder.mode(0o700);
            }
            builder.create(path).map_err(|_| unavailable())?;
        }
        let mut builder = tempfile::Builder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        let incoming = builder
            .tempdir_in(root.join("incoming"))
            .map_err(|_| unavailable())?;
        let snapshots = builder
            .tempdir_in(root.join("snapshots"))
            .map_err(|_| unavailable())?;
        let config = StagingConfig {
            staging_root: Some(snapshots.path().to_owned()),
            upload_roots: BTreeMap::from([("hosted".into(), incoming.path().to_owned())]),
            ..StagingConfig::default()
        };
        let staged = StagingStore::try_new(&config, &[])
            .await
            .map_err(|_| unavailable())?;
        Ok(Self {
            _root: incoming,
            _snapshots: snapshots,
            staged,
            capacity: Arc::new(Semaphore::new(TOTAL_BYTES)),
            saved: Mutex::new(HashMap::new()),
        })
    }

    pub(super) fn reap(&self) {
        if let Ok(mut saved) = self.saved.lock() {
            let now = Instant::now();
            saved.retain(|_, object| object.expires > now);
        }
    }

    pub(super) fn invalidate(&self, generation: &AccountGeneration, client: Option<&str>) {
        if let Ok(mut saved) = self.saved.lock() {
            saved.retain(|_, object| {
                &object.owner.generation != generation
                    || client.is_some_and(|client| object.owner.client != client)
            });
        }
    }

    pub(super) async fn stage(
        &self,
        access: &ReadAccess,
        id: &str,
        kind: &SendMediaKind,
        deadline: SendDeadline,
    ) -> Result<Arc<StagedUpload>, ApiError> {
        let owner = owner(access);
        let object = self
            .saved
            .lock()
            .map_err(|_| unavailable())?
            .get(id)
            .filter(|object| object.owner == owner && object.expires > Instant::now())
            .cloned()
            .ok_or(ApiError::new(ErrorCode::UnavailableResource))?;
        let filename = object.file.file_name().ok_or_else(unavailable)?;
        let input =
            StagingSource::try_new("hosted", Path::new(filename)).map_err(|_| unavailable())?;
        let snapshot = self
            .staged
            .stage(&input, kind, deadline)
            .await
            .map_err(send_stage_error)?;
        if snapshot.mime_type() != object.mime {
            return Err(ApiError::new(ErrorCode::InvalidRequest));
        }
        Ok(snapshot)
    }

    fn insert(&self, object: Stored, bytes: u32) -> Result<UploadReply, ApiError> {
        self.reap();
        let mut saved = self.saved.lock().map_err(|_| unavailable())?;
        if saved.len() >= MAX_HANDLES
            || saved
                .values()
                .filter(|existing| {
                    existing.owner.principal == object.owner.principal
                        && existing.owner.client == object.owner.client
                })
                .count()
                >= MAX_CLIENT_HANDLES
        {
            return Err(ApiError::new(ErrorCode::RateLimited));
        }
        let mut entropy = [0_u8; 24];
        getrandom::fill(&mut entropy).map_err(|_| unavailable())?;
        let handle = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(entropy);
        let mime_type = object.mime;
        let entry = saved.entry(handle.clone());
        if let std::collections::hash_map::Entry::Vacant(slot) = entry {
            slot.insert(Arc::new(object));
        } else {
            return Err(unavailable());
        }
        Ok(UploadReply {
            upload_handle: handle,
            mime_type,
            size_bytes: bytes,
            expires_in_seconds: UPLOAD_TTL.as_secs(),
        })
    }
}

pub(super) async fn upload(
    State(state): State<Arc<ServiceState>>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<UploadReply>, ApiError> {
    let access = ReadAccess::for_scope(&state, &headers, Scope::Send).await?;
    if !access.can_send() || headers.contains_key(header::COOKIE) {
        return Err(ApiError::new(ErrorCode::Forbidden));
    }
    let uploads = access
        .hosted
        .uploads
        .as_ref()
        .ok_or(ApiError::new(ErrorCode::Forbidden))?;
    let mime = super::single_header(&headers, header::CONTENT_TYPE)
        .ok_or(ApiError::new(ErrorCode::InvalidRequest))?;
    let (mime, kind, suffix, limit) =
        media_description(mime).ok_or(ApiError::new(ErrorCode::InvalidRequest))?;
    let supplied = if headers.contains_key(header::CONTENT_LENGTH) {
        Some(
            super::single_header(&headers, header::CONTENT_LENGTH)
                .and_then(|value| value.parse::<u32>().ok())
                .ok_or(ApiError::new(ErrorCode::InvalidRequest))?,
        )
    } else {
        None
    };
    if supplied == Some(0) {
        return Err(ApiError::new(ErrorCode::InvalidRequest));
    }
    if supplied.is_some_and(|size| size > limit) {
        return Err(ApiError::new(ErrorCode::PayloadTooLarge));
    }
    let mut reserved = Arc::clone(&uploads.capacity)
        .try_acquire_many_owned(supplied.unwrap_or(limit))
        .map_err(|_| ApiError::new(ErrorCode::RateLimited))?;
    let deadline = SendDeadline::at(access.opening_context().deadline());
    let temp = tempfile::Builder::new()
        .prefix("upload-")
        .suffix(suffix)
        .tempfile_in(uploads._root.path())
        .map_err(|_| unavailable())?;
    let (file, path) = temp.into_parts();
    let mut file = tokio::fs::File::from_std(file);
    let transfer = async {
        let mut stream = body.into_data_stream();
        let mut size = 0_u32;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| ApiError::new(ErrorCode::InvalidRequest))?;
            let amount = u32::try_from(chunk.len())
                .map_err(|_| ApiError::new(ErrorCode::PayloadTooLarge))?;
            size = size
                .checked_add(amount)
                .filter(|size| *size <= supplied.unwrap_or(limit))
                .ok_or(ApiError::new(ErrorCode::PayloadTooLarge))?;
            file.write_all(&chunk).await.map_err(|_| unavailable())?;
        }
        if size == 0 || supplied.is_some_and(|expected| size != expected) {
            return Err(ApiError::new(ErrorCode::InvalidRequest));
        }
        file.flush().await.map_err(|_| unavailable())?;
        let mut file = file.into_std().await;
        let (path, observed) = tokio::task::spawn_blocking(move || {
            let observed = telegram_core::sending::check_upload_type(
                &mut file,
                &kind,
                u64::from(size),
                deadline,
            );
            (path, observed)
        })
        .await
        .map_err(|_| unavailable())?;
        if observed.map_err(send_stage_error)? != mime {
            return Err(ApiError::new(ErrorCode::InvalidRequest));
        }
        Ok((path, size))
    };
    let (path, size) = tokio::select! {
        biased;
        _ = access.hosted.cancellation.cancelled() => return Err(unavailable()),
        _ = tokio::time::sleep_until(deadline.instant()) => return Err(ApiError::new(ErrorCode::Timeout)),
        result = transfer => result?,
    };
    access.revalidate().await?;
    // A reservation includes every byte in flight. Shrink it only after the
    // file and type are validated.
    drop(reserved.split(reserved.num_permits().saturating_sub(size as usize)));
    uploads
        .insert(
            Stored {
                file: path,
                owner: owner(&access),
                expires: Instant::now() + UPLOAD_TTL,
                mime,
                _bytes: reserved,
            },
            size,
        )
        .map(Json)
}

fn owner(access: &ReadAccess) -> Owner {
    Owner {
        principal: access.identity.principal().clone(),
        client: access.identity.client_id().to_owned(),
        generation: access.generation.clone(),
        revision: access.grant.revision,
    }
}

fn unavailable() -> ApiError {
    ApiError::new(ErrorCode::Unavailable)
}

fn send_stage_error(error: telegram_core::telegram::types::sending::SendFailure) -> ApiError {
    use telegram_core::telegram::types::sending::SendFailureKind;
    ApiError::new(match error.kind() {
        SendFailureKind::InvalidInput
        | SendFailureKind::UnsupportedMedia
        | SendFailureKind::UploadSourceInvalid => ErrorCode::InvalidRequest,
        SendFailureKind::MediaTooLarge => ErrorCode::PayloadTooLarge,
        SendFailureKind::Timeout => ErrorCode::Timeout,
        SendFailureKind::SendCapacityReached | SendFailureKind::RateLimited => {
            ErrorCode::RateLimited
        }
        _ => ErrorCode::Unavailable,
    })
}

fn media_description(mime: &str) -> Option<(&'static str, SendMediaKind, &'static str, u32)> {
    let audio = || SendMediaKind::Audio {
        duration_seconds: 1,
        title: None,
        performer: None,
    };
    Some(match mime {
        "image/jpeg" => ("image/jpeg", SendMediaKind::Image, ".jpg", MAX_IMAGE_BYTES),
        "image/png" => ("image/png", SendMediaKind::Image, ".png", MAX_IMAGE_BYTES),
        "audio/mpeg" => ("audio/mpeg", audio(), ".mp3", MAX_UPLOAD_BYTES),
        "audio/mp4" => ("audio/mp4", audio(), ".m4a", MAX_UPLOAD_BYTES),
        "audio/aac" => ("audio/aac", audio(), ".aac", MAX_UPLOAD_BYTES),
        "audio/ogg" => (
            "audio/ogg",
            SendMediaKind::Voice {
                duration_seconds: 1,
            },
            ".ogg",
            MAX_UPLOAD_BYTES,
        ),
        "video/mp4" => (
            "video/mp4",
            SendMediaKind::Video {
                duration_seconds: 1.0,
                width: 1,
                height: 1,
            },
            ".mp4",
            MAX_UPLOAD_BYTES,
        ),
        _ => return None,
    })
}
