//! Owned private snapshots, nonwaiting reservations and lease-retaining cleanup.
use super::privacy::FileIdentity;
use super::{SendingRuntimeConfig, privacy, probe};
use crate::mcp::tools::types::sending::UploadSource;
use crate::telegram::sending::{SendDeadline, SendStage};
use crate::telegram::types::sending::{SendFailure, SendFailureKind, SendMediaKind};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Seek as _, Write as _};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::time::Instant;

const COPY_BYTES: usize = 524_288;

fn copy_stream(
    input: &mut impl std::io::Read,
    output: &mut impl std::io::Write,
    declared: u64,
    cap: u64,
    mut check: impl FnMut() -> Result<(), SendFailure>,
) -> Result<u64, SendFailure> {
    let mut buffer = vec![0u8; COPY_BYTES];
    let mut copied = 0u64;
    loop {
        check()?;
        let size = input.read(&mut buffer).map_err(|_| privacy::invalid())?;
        if size == 0 {
            break;
        }
        copied = copied
            .checked_add(size as u64)
            .ok_or_else(privacy::invalid)?;
        if copied > cap {
            return Err(SendFailure::not_sent(SendFailureKind::MediaTooLarge, None));
        }
        if copied > declared {
            return Err(privacy::invalid());
        }
        output
            .write_all(&buffer[..size])
            .map_err(|_| unavailable())?;
    }
    if copied != declared {
        return Err(privacy::invalid());
    }
    Ok(copied)
}
fn unavailable() -> SendFailure {
    SendFailure::not_sent(SendFailureKind::Unavailable, None)
}
fn capacity() -> SendFailure {
    SendFailure::not_sent(SendFailureKind::SendCapacityReached, None)
}

struct QuotaState {
    active: usize,
    bytes: u64,
    closed: bool,
}
struct Quota {
    state: Mutex<QuotaState>,
    changed: tokio::sync::Notify,
    max_active: usize,
    max_bytes: u64,
}
struct Lease {
    quota: Arc<Quota>,
    bytes: u64,
}
impl Lease {
    fn bytes(&mut self, bytes: u64) -> Result<(), SendFailure> {
        let mut state = self.quota.state.lock().map_err(|_| unavailable())?;
        let total = state
            .bytes
            .checked_add(bytes)
            .filter(|n| *n <= self.quota.max_bytes)
            .ok_or_else(capacity)?;
        state.bytes = total;
        self.bytes = bytes;
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.quota.state.lock() {
            if let (Some(active), Some(bytes)) = (
                state.active.checked_sub(1),
                state.bytes.checked_sub(self.bytes),
            ) {
                state.active = active;
                state.bytes = bytes;
            } else {
                state.closed = true;
            }
        }
        self.quota.changed.notify_waiters();
    }
}
struct Root {
    path: PathBuf,
    identity: FileIdentity,
}
struct Owner {
    runtime_deadline: Mutex<Option<crate::mcp::runtime::DrainDeadline>>,
    root: PathBuf,
    root_identity: FileIdentity,
    uid: u32,
    roots: BTreeMap<String, Root>,
    quota: Arc<Quota>,
    failed: Mutex<Vec<Cleanup>>,
    lock: Mutex<Option<(File, FileIdentity)>>,
    drain: Mutex<Option<Instant>>,
    max_image: u64,
    max_media: u64,
}
impl Owner {
    fn retain(&self, entry: Cleanup) {
        if let Ok(mut failed) = self.failed.lock() {
            failed.push(entry);
        } else {
            std::mem::forget(entry);
        } // Poison cannot release occupied file quota.
        self.quota.changed.notify_waiters();
    }
    fn cleanup_allowed(&self) -> bool {
        if !self.runtime_deadline.lock().ok().is_some_and(|source| {
            source
                .as_ref()
                .and_then(|s| s.get())
                .is_none_or(|at| Instant::now() < at)
        }) {
            return false;
        }
        self.drain
            .lock()
            .ok()
            .is_some_and(|d| d.is_none_or(|at| Instant::now() < at))
    }
}
struct Cleanup {
    path: PathBuf,
    identity: Option<FileIdentity>,
    file: Option<File>,
    lease: Option<Lease>,
}
impl Cleanup {
    fn remove(&mut self, owner: &Owner) -> Result<(), SendFailure> {
        if !owner.cleanup_allowed() {
            return Err(unavailable());
        }
        let root =
            privacy::identity(&std::fs::symlink_metadata(&owner.root).map_err(|_| unavailable())?)?;
        if (root.device, root.inode) != (owner.root_identity.device, owner.root_identity.inode) {
            return Err(unavailable());
        }
        let actual = if let Some(file) = &self.file {
            privacy::identity(&file.metadata().map_err(|_| unavailable())?)?
        } else {
            self.identity.ok_or_else(unavailable)?
        };
        if self.identity.is_some_and(|expected| {
            (actual.device, actual.inode) != (expected.device, expected.inode)
        }) || actual.uid != owner.uid
            || actual.links != 1
        {
            return Err(unavailable());
        }
        match std::fs::symlink_metadata(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => {
                privacy::same_file(&self.path, actual)?;
                std::fs::remove_file(&self.path).map_err(|_| unavailable())?;
            }
        }
        self.file.take();
        self.lease.take();
        Ok(())
    }
}

/// An immutable private upload snapshot. Only validated staging can construct one.
pub struct StagedUpload {
    owner: Arc<Owner>,
    path: PathBuf,
    identity: FileIdentity,
    file: Mutex<Option<File>>,
    lease: Option<Lease>,
    mime: &'static str,
    name: String,
    size: u64,
    deadline: SendDeadline,
}
impl std::fmt::Debug for StagedUpload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StagedUpload")
            .field("size_bytes", &self.size)
            .finish_non_exhaustive()
    }
}
impl StagedUpload {
    /// Number of original submitted snapshot bytes.
    pub fn size_bytes(&self) -> u64 {
        self.size
    }
    /// Controlled MIME type established by bounded header inspection.
    pub fn mime_type(&self) -> &str {
        self.mime
    }
    /// Bounded sanitized basename with a verified-kind extension, never a path.
    pub fn upload_name(&self) -> &str {
        &self.name
    }
    /// Obtain the single verified reader; started reads retain this snapshot's lease.
    ///
    /// # Errors
    /// Rejects repeated readers, changed/unavailable snapshot identity and worker failure.
    pub async fn reader(self: &Arc<Self>) -> Result<super::StagedReader, SendFailure> {
        let owner = Arc::clone(self);
        tokio::runtime::Handle::try_current()
            .map_err(|_| unavailable())?
            .spawn_blocking(move || {
                let file = owner.reader_file()?;
                Ok(super::StagedReader::new(file, owner))
            })
            .await
            .map_err(|_| unavailable())?
    }
    fn reader_file(&self) -> Result<File, SendFailure> {
        let file = self
            .file
            .lock()
            .map_err(|_| unavailable())?
            .take()
            .ok_or_else(unavailable)?;
        if privacy::identity(&file.metadata().map_err(|_| unavailable())?)? != self.identity {
            return Err(unavailable());
        }
        privacy::same_file(&self.path, self.identity)?;
        Ok(file)
    }
    pub(super) fn deadline(&self) -> SendDeadline {
        self.deadline
    }
}
impl Drop for StagedUpload {
    fn drop(&mut self) {
        let entry = Cleanup {
            path: self.path.clone(),
            identity: Some(self.identity),
            file: self.file.get_mut().ok().and_then(Option::take),
            lease: self.lease.take(),
        };
        let owner = Arc::clone(&self.owner);
        if !owner.cleanup_allowed() {
            owner.retain(entry);
            return;
        }
        // At most max_concurrent_media cleanup jobs exist: each still owns its slot.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn_blocking(move || {
                let mut entry = entry;
                if entry.remove(&owner).is_err() {
                    owner.retain(entry);
                }
            });
        } else {
            owner.retain(entry);
        }
    }
}

pub(crate) struct StagingStore {
    owner: Arc<Owner>,
}
impl std::fmt::Debug for StagingStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StagingStore").finish_non_exhaustive()
    }
}
struct Cancel(Arc<AtomicBool>);
impl Drop for Cancel {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl StagingStore {
    pub(crate) async fn try_new(config: &SendingRuntimeConfig) -> Result<Arc<Self>, SendFailure> {
        if !config.settings.enabled {
            return Err(SendFailure::not_sent(
                SendFailureKind::SendingDisabled,
                None,
            ));
        }
        config
            .settings
            .validate(config.output_format)
            .map_err(|_| privacy::invalid())?;
        let config = config.clone();
        let cancelled = Cancel(Arc::new(AtomicBool::new(false)));
        let flag = Arc::clone(&cancelled.0);
        let job = tokio::task::spawn_blocking(move || Self::initialize(config, &flag));
        tokio::time::timeout(std::time::Duration::from_secs(30), job)
            .await
            .map_err(|_| unavailable())?
            .map_err(|_| unavailable())?
    }
    fn initialize(
        config: SendingRuntimeConfig,
        cancelled: &AtomicBool,
    ) -> Result<Arc<Self>, SendFailure> {
        let uid = privacy::uid()?;
        let requested = config
            .settings
            .staging_root
            .as_ref()
            .ok_or_else(privacy::invalid)?;
        let root = privacy::directory(requested, uid, true)?;
        let root_identity =
            privacy::identity(&std::fs::metadata(&root).map_err(|_| privacy::invalid())?)?;
        let protected = config
            .protected_paths
            .iter()
            .map(|p| privacy::canonical_missing(p))
            .collect::<Result<Vec<_>, _>>()?;
        let mut roots = BTreeMap::new();
        for (alias, path) in &config.settings.upload_roots {
            if cancelled.load(Ordering::Acquire) {
                return Err(unavailable());
            }
            let path = privacy::directory(path, uid, false)?;
            if path.starts_with(&root)
                || root.starts_with(&path)
                || protected
                    .iter()
                    .any(|p| p.starts_with(&path) || path.starts_with(p))
            {
                return Err(privacy::invalid());
            }
            let identity =
                privacy::identity(&std::fs::metadata(&path).map_err(|_| privacy::invalid())?)?;
            roots.insert(alias.clone(), Root { path, identity });
        }
        if roots.is_empty()
            || std::fs::read_dir(&root)
                .map_err(|_| privacy::invalid())?
                .next()
                .is_some()
            || cancelled.load(Ordering::Acquire)
        {
            return Err(privacy::invalid());
        }
        let path = root.join(".lock");
        let file = privacy::create_private(&path)?;
        let identity = privacy::identity(&file.metadata().map_err(|_| privacy::invalid())?)?;
        if let Err(error) = privacy::secure_private(&path, &file, uid) {
            let _ = privacy::same_file(&path, identity)
                .and_then(|_| std::fs::remove_file(&path).map_err(|_| privacy::invalid()));
            return Err(error);
        }
        let identity = privacy::identity(&file.metadata().map_err(|_| privacy::invalid())?)?;
        let quota = Arc::new(Quota {
            state: Mutex::new(QuotaState {
                active: 0,
                bytes: 0,
                closed: false,
            }),
            changed: tokio::sync::Notify::new(),
            max_active: config.settings.max_concurrent_media,
            max_bytes: config.settings.max_staging_bytes,
        });
        Ok(Arc::new(Self {
            owner: Arc::new(Owner {
                runtime_deadline: Mutex::new(None),
                root,
                root_identity,
                uid,
                roots,
                quota,
                failed: Mutex::new(Vec::new()),
                lock: Mutex::new(Some((file, identity))),
                drain: Mutex::new(None),
                max_image: config.settings.max_image_bytes,
                max_media: config.settings.max_media_bytes,
            }),
        }))
    }
    pub(crate) async fn stage(
        self: &Arc<Self>,
        source: &UploadSource,
        media: &SendMediaKind,
        deadline: SendDeadline,
    ) -> Result<Arc<StagedUpload>, SendFailure> {
        self.stage_using(source, media, deadline, CopyJob::run)
            .await
    }
    async fn stage_using<F>(
        self: &Arc<Self>,
        source: &UploadSource,
        media: &SendMediaKind,
        deadline: SendDeadline,
        worker: F,
    ) -> Result<Arc<StagedUpload>, SendFailure>
    where
        F: FnOnce(CopyJob) -> Result<Arc<StagedUpload>, SendFailure> + Send + 'static,
    {
        source.validate()?;
        media.validate()?;
        if deadline.expired() {
            return Err(SendFailure::not_sent(SendFailureKind::Timeout, None));
        }
        if !self.owner.roots.contains_key(&source.root) {
            return Err(privacy::invalid());
        }
        let lease = {
            let mut q = self.owner.quota.state.lock().map_err(|_| unavailable())?;
            if q.closed {
                return Err(unavailable());
            }
            if q.active >= self.owner.quota.max_active {
                return Err(capacity());
            }
            q.active += 1;
            Lease {
                quota: Arc::clone(&self.owner.quota),
                bytes: 0,
            }
        };
        let cancelled = Cancel(Arc::new(AtomicBool::new(false)));
        let job = CopyJob {
            owner: Arc::clone(&self.owner),
            source: UploadSource {
                root: source.root.clone(),
                relative_path: source.relative_path.clone(),
            },
            media: media.clone(),
            deadline,
            cancelled: Arc::clone(&cancelled.0),
            lease,
        };
        deadline
            .run(SendStage::LocalPreparation, async {
                tokio::task::spawn_blocking(move || worker(job))
                    .await
                    .map_err(|_| unavailable())?
            })
            .await
    }
    pub(crate) async fn close(&self, deadline: Instant) -> Result<(), SendFailure> {
        let deadline = self.stop_admission(deadline)?;
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        let owner = Arc::clone(&self.owner);
        let entries = std::mem::take(&mut *owner.failed.lock().map_err(|_| unavailable())?);
        if !entries.is_empty() {
            let copy = Arc::clone(&owner);
            tokio::time::timeout_at(
                deadline,
                tokio::task::spawn_blocking(move || {
                    for mut entry in entries {
                        if entry.remove(&copy).is_err() {
                            copy.retain(entry);
                        }
                    }
                }),
            )
            .await
            .map_err(|_| unavailable())?
            .map_err(|_| unavailable())?;
        }
        loop {
            let changed = self.owner.quota.changed.notified();
            if !self
                .owner
                .failed
                .lock()
                .map_err(|_| unavailable())?
                .is_empty()
            {
                return Err(unavailable());
            }
            if self
                .owner
                .quota
                .state
                .lock()
                .map_err(|_| unavailable())?
                .active
                == 0
            {
                break;
            }
            tokio::time::timeout_at(deadline, changed)
                .await
                .map_err(|_| unavailable())?;
        }
        tokio::time::timeout_at(
            deadline,
            tokio::task::spawn_blocking(move || {
                let lock = owner.lock.lock().map_err(|_| unavailable())?.take();
                if let Some((file, identity)) = lock {
                    let mut entry = Cleanup {
                        path: owner.root.join(".lock"),
                        identity: Some(identity),
                        file: Some(file),
                        lease: None,
                    };
                    entry.remove(&owner)?;
                }
                Ok(())
            }),
        )
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())?
    }
    pub(crate) fn bind_runtime_deadline(
        &self,
        deadline: crate::mcp::runtime::DrainDeadline,
    ) -> Result<(), SendFailure> {
        let mut current = self
            .owner
            .runtime_deadline
            .lock()
            .map_err(|_| unavailable())?;
        if current.is_none() {
            *current = Some(deadline);
        }
        Ok(())
    }
    pub(crate) fn stop_admission(&self, deadline: Instant) -> Result<Instant, SendFailure> {
        self.owner
            .quota
            .state
            .lock()
            .map_err(|_| unavailable())?
            .closed = true;
        let mut current = self.owner.drain.lock().map_err(|_| unavailable())?;
        let deadline = current.map_or(deadline, |at| at.min(deadline));
        *current = Some(deadline);
        Ok(deadline)
    }
}

struct CopyJob {
    owner: Arc<Owner>,
    source: UploadSource,
    media: SendMediaKind,
    deadline: SendDeadline,
    cancelled: Arc<AtomicBool>,
    lease: Lease,
}
impl CopyJob {
    fn check(&self) -> Result<(), SendFailure> {
        if self.deadline.expired() || self.cancelled.load(Ordering::Acquire) {
            Err(SendFailure::not_sent(SendFailureKind::Timeout, None))
        } else {
            Ok(())
        }
    }
    fn run(mut self) -> Result<Arc<StagedUpload>, SendFailure> {
        self.check()?;
        let root = self
            .owner
            .roots
            .get(&self.source.root)
            .ok_or_else(privacy::invalid)?;
        let (mut input, identity, source_path) = privacy::open_source(
            &root.path,
            root.identity,
            &self.source.relative_path,
            self.owner.uid,
        )?;
        let cap = if matches!(self.media, SendMediaKind::Image) {
            self.owner.max_image
        } else {
            self.owner.max_media
        };
        if identity.len > cap {
            return Err(SendFailure::not_sent(SendFailureKind::MediaTooLarge, None));
        }
        if identity.len == 0 {
            return Err(privacy::invalid());
        }
        self.lease.bytes(identity.len)?;
        self.check()?;
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(|_| unavailable())?;
        let name = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let path = self.owner.root.join(name);
        let file = privacy::create_private(&path)?;
        let first = match file
            .metadata()
            .map_err(|_| unavailable())
            .and_then(|m| privacy::identity(&m))
        {
            Ok(info) => info,
            Err(error) => {
                self.owner.retain(Cleanup {
                    path,
                    identity: None,
                    file: Some(file),
                    lease: Some(self.lease),
                });
                return Err(error);
            }
        };
        let mut snapshot = StagedUpload {
            owner: Arc::clone(&self.owner),
            path,
            identity: first,
            file: Mutex::new(Some(file)),
            lease: Some(self.lease),
            mime: "",
            name: String::new(),
            size: identity.len,
            deadline: self.deadline,
        };
        let output = snapshot
            .file
            .get_mut()
            .map_err(|_| unavailable())?
            .as_mut()
            .ok_or_else(unavailable)?;
        privacy::secure_private(&snapshot.path, output, self.owner.uid)?;
        copy_stream(&mut input, output, identity.len, cap, || {
            if self.deadline.expired() || self.cancelled.load(Ordering::Acquire) {
                Err(SendFailure::not_sent(SendFailureKind::Timeout, None))
            } else {
                Ok(())
            }
        })?;
        if privacy::identity(&input.metadata().map_err(|_| privacy::invalid())?)? != identity {
            return Err(privacy::invalid());
        }
        privacy::same_file(&source_path, identity)?;
        output.flush().map_err(|_| unavailable())?;
        let inspected = probe::probe(output, &self.media)?;
        output.rewind().map_err(|_| unavailable())?;
        snapshot.identity = privacy::identity(&output.metadata().map_err(|_| unavailable())?)?;
        let readonly = File::open(&snapshot.path).map_err(|_| unavailable())?;
        if privacy::identity(&readonly.metadata().map_err(|_| unavailable())?)? != snapshot.identity
        {
            return Err(unavailable());
        }
        *snapshot.file.get_mut().map_err(|_| unavailable())? = Some(readonly);
        let stem = self
            .source
            .relative_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("attachment");
        let stem = stem
            .chars()
            .take(48)
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();
        snapshot.name = format!(
            "{}.{}",
            if stem.is_empty() { "attachment" } else { &stem },
            inspected.extension
        );
        snapshot.mime = inspected.mime_type;
        Ok(Arc::new(snapshot))
    }
}

#[cfg(all(test, unix))]
#[path = "tests/resource_tests.rs"]
mod resource_tests;
