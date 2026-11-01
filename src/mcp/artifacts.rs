//! Private process-owned processed JPEG artifacts.
//!
//! Call synchronous operations only inside `runtime::image_job`. Started blocking writes retain
//! their slot through cancellation and may finish as bounded entries without a delivered handle.
//! Expiry is monotonic and never refreshed; physical deletion may lag by up to the cleanup interval
//! (at most 60 seconds, plus worker admission). Final drop schedules bounded, identity-checked
//! cleanup. Shutdown should await `close` inside an image job before destroying its runtime;
//! a deadline/error/abrupt teardown may leave files and a lock, which require explicit operator
//! cleanup or a fresh empty private root. No recovery or foreign-file deletion is attempted.
mod files;
mod privacy;
use files::OwnedFile;
use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::sync::{Mutex, MutexGuard};
use tokio::time::Instant;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub(crate) struct StoreConfig {
    pub(crate) root: PathBuf,
    pub(crate) ttl: Duration,
    pub(crate) max_entries: usize,
    pub(crate) max_bytes: u64,
}
#[derive(Debug)]
pub(crate) struct StoredArtifact {
    pub(crate) uri: String,
    pub(crate) path: PathBuf,
    pub(crate) size_bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) expires_at: chrono::DateTime<chrono::Utc>,
}
#[derive(Debug)]
pub(crate) struct ArtifactRead {
    pub(crate) bytes: Vec<u8>,
    pub(crate) expires_at: tokio::time::Instant,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StoreSnapshot {
    pub(crate) entries: usize,
    pub(crate) bytes: u64,
}
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum StoreError {
    #[error("Invalid artifact URI")]
    InvalidUri,
    #[error("Artifact is missing or expired")]
    Missing,
    #[error("Artifact exceeds the byte limit")]
    TooLarge,
    #[error("Artifact storage is unavailable")]
    Unavailable,
}
const PREFIX: &str = "telegram-media://artifact/";
const MAX_JPEG_BYTES: usize = 20 * 1024 * 1024;
struct Entry {
    uri: String,
    file: OwnedFile,
    expires: Instant,
}
struct State {
    entries: VecDeque<Entry>,
    bytes: u64,
    pending: Option<OwnedFile>,
    lock: OwnedFile,
    failed: bool,
    closed: bool,
}
pub(crate) struct ArtifactStore {
    config: StoreConfig,
    uid: u32,
    state: Mutex<Option<State>>,
    runtime: tokio::runtime::Handle,
}
impl ArtifactStore {
    pub(crate) async fn try_new(mut config: StoreConfig) -> Result<Arc<Self>, StoreError> {
        if !config.root.is_absolute()
            || config.root.as_os_str().is_empty()
            || !(Duration::from_secs(1)..=Duration::from_secs(3600)).contains(&config.ttl)
            || !(1..=1000).contains(&config.max_entries)
            || !(1..=1_073_741_824).contains(&config.max_bytes)
        {
            return Err(StoreError::Unavailable);
        }
        let uid = privacy::uid().await?;
        let requested = config.root.clone();
        let (root, ancestors) =
            crate::mcp::runtime::image_job(move || files::prepare(&requested, uid))
                .await
                .map_err(|_| StoreError::Unavailable)??;
        for path in ancestors {
            privacy::directory_acl(&path).await?;
        }
        config.root = root;
        let runtime = tokio::runtime::Handle::current();
        let store = crate::mcp::runtime::image_job(move || {
            if std::fs::read_dir(&config.root)
                .map_err(|_| StoreError::Unavailable)?
                .next()
                .is_some()
            {
                return Err(StoreError::Unavailable);
            }
            let (lock, _) = OwnedFile::create(config.root.join(".lock"), uid)?;
            Ok(Arc::new(Self {
                config,
                uid,
                state: Mutex::new(Some(State {
                    entries: VecDeque::new(),
                    bytes: 0,
                    pending: None,
                    lock,
                    failed: false,
                    closed: false,
                })),
                runtime,
            }))
        })
        .await
        .map_err(|_| StoreError::Unavailable)??;
        Self::schedule_cleanup(&store);
        Ok(store)
    }
    fn schedule_cleanup(store: &Arc<Self>) {
        let weak = Arc::downgrade(store);
        let interval = store.config.ttl.min(Duration::from_secs(60));
        store.runtime.spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                let queued = weak.clone();
                // Admission can wait behind long image work. A queued cleanup owns no store.
                let result = crate::mcp::runtime::image_job(move || {
                    queued.upgrade().map(|store| store.cleanup())
                })
                .await;
                if !matches!(result, Ok(Some(Ok(())))) {
                    break;
                }
            }
        });
    }
    fn state(&self) -> Result<MutexGuard<'_, Option<State>>, StoreError> {
        let state = self.state.lock().map_err(|_| StoreError::Unavailable)?;
        if state.as_ref().is_none_or(|s| s.failed || s.closed) {
            return Err(StoreError::Unavailable);
        }
        Ok(state)
    }
    pub(crate) fn insert(
        &self,
        bytes: &[u8],
        width: u32,
        height: u32,
    ) -> Result<StoredArtifact, StoreError> {
        let mut guard = self.state()?;
        let state = guard.as_mut().ok_or(StoreError::Unavailable)?;
        if bytes.len() > MAX_JPEG_BYTES || bytes.len() as u64 > self.config.max_bytes {
            return Err(StoreError::TooLarge);
        }
        if bytes.is_empty() || width == 0 || height == 0 {
            return Err(StoreError::Unavailable);
        }
        self.verify_contents(state)?;
        self.cleanup_expired(state)?;
        while state.entries.len() >= self.config.max_entries
            || state.bytes + bytes.len() as u64 > self.config.max_bytes
        {
            remove_entry(state, 0, self.uid)?;
        }
        let id = random_id()?;
        let uri = format!("{PREFIX}{id}");
        let path = self.config.root.join(format!("{id}.jpg"));
        // Reserve before touching disk; a failed create may still have created an empty inode.
        state.bytes += bytes.len() as u64;
        let (mut file, mut handle) =
            match OwnedFile::create(self.config.root.join(format!("{id}.tmp")), self.uid) {
                Ok(created) => created,
                Err(_) => {
                    state.failed = true;
                    return Err(StoreError::Unavailable);
                }
            };
        state.pending = Some(file.clone());
        let written = (|| {
            handle
                .write_all(bytes)
                .map_err(|_| StoreError::Unavailable)?;
            handle.sync_all().map_err(|_| StoreError::Unavailable)?;
            match std::fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(StoreError::Unavailable),
            }
            std::fs::rename(&file.path, &path).map_err(|_| StoreError::Unavailable)
        })();
        if written.is_err() {
            if let Ok(metadata) = handle.metadata()
                && let Some(pending) = state.pending.as_mut()
            {
                pending.size = metadata.len();
            }
            state.failed = true;
            return Err(StoreError::Unavailable);
        }
        file.path = path.clone();
        file.size = bytes.len() as u64;
        state.pending = None;
        let expires = Instant::now() + self.config.ttl;
        state.entries.push_back(Entry {
            uri: uri.clone(),
            file,
            expires,
        });
        Ok(StoredArtifact {
            uri,
            path,
            size_bytes: bytes.len(),
            width,
            height,
            expires_at: chrono::Utc::now()
                + chrono::Duration::from_std(self.config.ttl)
                    .map_err(|_| StoreError::Unavailable)?,
        })
    }
    pub(crate) fn read(&self, uri: &str, max_bytes: usize) -> Result<ArtifactRead, StoreError> {
        validate_uri(uri)?;
        let mut guard = self.state()?;
        let state = guard.as_mut().ok_or(StoreError::Unavailable)?;
        self.verify_contents(state)?;
        self.cleanup_expired(state)?;
        let entry = state
            .entries
            .iter()
            .find(|entry| entry.uri == uri)
            .ok_or(StoreError::Missing)?;
        if entry.file.size > max_bytes as u64 {
            return Err(StoreError::TooLarge);
        }
        let mut file = entry.file.open(self.uid)?;
        let mut bytes = Vec::with_capacity(entry.file.size as usize);
        (&mut file)
            .take(entry.file.size + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| StoreError::Unavailable)?;
        if bytes.len() as u64 != entry.file.size {
            return Err(StoreError::Unavailable);
        }
        Ok(ArtifactRead {
            bytes,
            expires_at: entry.expires,
        })
    }
    pub(crate) fn remove(&self, uri: &str) -> Result<(), StoreError> {
        validate_uri(uri)?;
        let mut guard = self.state()?;
        let state = guard.as_mut().ok_or(StoreError::Unavailable)?;
        self.verify_contents(state)?;
        self.cleanup_expired(state)?;
        let index = state
            .entries
            .iter()
            .position(|entry| entry.uri == uri)
            .ok_or(StoreError::Missing)?;
        remove_entry(state, index, self.uid)
    }
    pub(crate) fn cleanup(&self) -> Result<(), StoreError> {
        let mut guard = self.state()?;
        let state = guard.as_mut().ok_or(StoreError::Unavailable)?;
        self.verify_contents(state)?;
        self.cleanup_expired(state)
    }
    fn cleanup_expired(&self, state: &mut State) -> Result<(), StoreError> {
        while state
            .entries
            .front()
            .is_some_and(|entry| entry.expires <= Instant::now())
        {
            remove_entry(state, 0, self.uid)?;
        }
        Ok(())
    }
    fn verify_contents(&self, state: &mut State) -> Result<(), StoreError> {
        let result = (|| {
            state
                .lock
                .validate(self.uid)
                .map_err(|_| StoreError::Unavailable)?;
            for item in std::fs::read_dir(&self.config.root).map_err(|_| StoreError::Unavailable)? {
                let path = item.map_err(|_| StoreError::Unavailable)?.path();
                let known = if state.lock.path == path {
                    Some(&state.lock)
                } else {
                    state
                        .entries
                        .iter()
                        .find(|entry| entry.file.path == path)
                        .map(|entry| &entry.file)
                };
                known.ok_or(StoreError::Unavailable)?.validate(self.uid)?;
            }
            Ok(())
        })();
        if result.is_err() {
            state.failed = true;
        }
        result
    }
    /// Stop all access and finish identity-checked owned-file cleanup in the current image job.
    /// A failed close leaves the store closed; another close may retry bounded cleanup. Poisoned
    /// state is refused. Unknown files survive, and live Arc clones cannot recreate storage.
    pub(crate) fn close(&self) -> Result<(), StoreError> {
        let mut guard = self.state.lock().map_err(|_| StoreError::Unavailable)?;
        let Some(state) = guard.as_mut() else {
            return Ok(());
        };
        state.closed = true;
        let mut failed = false;
        let mut index = 0;
        while index < state.entries.len() {
            if remove_entry(state, index, self.uid).is_err() {
                failed = true;
                index += 1;
            }
        }
        if let Some(pending) = state.pending.as_ref() {
            if pending.remove(self.uid).is_ok() {
                state.pending = None;
            } else {
                failed = true;
            }
        }
        if state.lock.remove(self.uid).is_err() {
            failed = true;
        }
        if failed {
            state.failed = true;
            return Err(StoreError::Unavailable);
        }
        // Successful finalization disarms Drop and makes repeated close a no-op, including
        // after a different store acquires the now-empty root with its own fresh lock identity.
        *guard = None;
        Ok(())
    }
    pub(crate) fn snapshot(&self) -> Result<StoreSnapshot, StoreError> {
        let guard = self.state()?;
        let state = guard.as_ref().ok_or(StoreError::Unavailable)?;
        Ok(StoreSnapshot {
            entries: state.entries.len() + usize::from(state.pending.is_some()),
            bytes: state.bytes,
        })
    }
}
fn random_id() -> Result<String, StoreError> {
    use std::fmt::Write as _;
    let mut random = [0; 24];
    getrandom::fill(&mut random).map_err(|_| StoreError::Unavailable)?;
    let mut id = String::with_capacity(48);
    for byte in random {
        write!(&mut id, "{byte:02x}").map_err(|_| StoreError::Unavailable)?;
    }
    Ok(id)
}
fn validate_uri(uri: &str) -> Result<(), StoreError> {
    let id = uri.strip_prefix(PREFIX).ok_or(StoreError::InvalidUri)?;
    if id.len() != 48
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(StoreError::InvalidUri);
    }
    Ok(())
}
fn remove_entry(state: &mut State, index: usize, uid: u32) -> Result<(), StoreError> {
    if state.entries[index].file.remove(uid).is_err() {
        state.failed = true;
        return Err(StoreError::Unavailable);
    }
    let entry = state.entries.remove(index).ok_or(StoreError::Unavailable)?;
    state.bytes -= entry.file.size;
    Ok(())
}
impl Drop for ArtifactStore {
    fn drop(&mut self) {
        let Ok(state) = self.state.get_mut() else {
            return;
        };
        let Some(state) = state.take() else {
            return;
        };
        let uid = self.uid;
        self.runtime.spawn(async move {
            let _ = crate::mcp::runtime::image_job(move || {
                for entry in state.entries {
                    let _ = entry.file.remove(uid);
                }
                if let Some(file) = state.pending {
                    let _ = file.remove(uid);
                }
                let _ = state.lock.remove(uid);
            })
            .await;
        });
    }
}
#[cfg(all(test, unix))]
#[path = "artifacts/tests.rs"]
mod tests;

#[cfg(all(test, not(unix)))]
mod platform_tests {
    #[tokio::test]
    async fn unverified_platform_refuses_artifact_initialization() {
        let config = super::StoreConfig {
            root: std::env::temp_dir().join("telegram-artifacts-unverified-platform"),
            ttl: std::time::Duration::from_secs(600),
            max_entries: 100,
            max_bytes: 1024,
        };
        assert!(matches!(
            super::ArtifactStore::try_new(config).await,
            Err(super::StoreError::Unavailable)
        ));
    }
}
