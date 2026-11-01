//! Process-wide raw-buffer admission and ordered, deadline-bound media delivery.
use crate::error::Error;
use crate::telegram::types::{MediaDownload, MediaFetchError, MediaFetchOutcome};
use futures::StreamExt as _;
use futures::stream::BoxStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

/// Maximum simultaneous retained raw inputs across all production calls.
pub const RAW_MEDIA_SLOTS: usize = 4;
/// Global raw-input reservation allowance (80 MiB); output/decoded memory is separate.
pub const RAW_MEDIA_MEMORY_BYTES: usize = 80 * 1024 * 1024;

/// One absolute media deadline, established before resolution or capacity waits.
#[derive(Debug, Clone, Copy)]
pub struct MediaDeadline {
    at: Instant,
    seconds: u64,
}
impl MediaDeadline {
    /// Create a finite operation budget.
    ///
    /// # Errors
    /// Returns invalid input for a zero or unrepresentable duration.
    pub fn try_new(seconds: u64) -> Result<Self, Error> {
        if seconds == 0 {
            return Err(Error::InvalidInput("media timeout must be positive".into()));
        }
        let at = Instant::now()
            .checked_add(Duration::from_secs(seconds))
            .ok_or_else(|| Error::InvalidInput("media timeout is too large".into()))?;
        Ok(Self { at, seconds })
    }
    /// Bound a future without restarting the operation budget.
    ///
    /// # Errors
    /// Returns a typed timeout before polling expired work or accepting a late result.
    pub async fn run<F: Future>(&self, future: F) -> Result<F::Output, Error> {
        let timeout = || Error::Timeout {
            operation: "download_media".into(),
            secs: self.seconds,
        };
        if Instant::now() >= self.at {
            return Err(timeout());
        }
        let result = tokio::select! {
            biased;
            _ = tokio::time::sleep_until(self.at) => return Err(timeout()),
            result = future => result,
        };
        if Instant::now() >= self.at {
            return Err(timeout());
        }
        Ok(result)
    }
}

struct RawMediaPool {
    slots: Arc<Semaphore>,
    bytes: Arc<Semaphore>,
    byte_limit: usize,
    retained: AtomicUsize,
    peak_retained: AtomicUsize,
}
impl RawMediaPool {
    fn new(slots: usize, bytes: usize) -> Arc<Self> {
        Arc::new(Self {
            slots: Arc::new(Semaphore::new(slots)),
            bytes: Arc::new(Semaphore::new(bytes)),
            byte_limit: bytes,
            retained: AtomicUsize::new(0),
            peak_retained: AtomicUsize::new(0),
        })
    }
    async fn acquire(
        self: &Arc<Self>,
        max_bytes: u64,
        deadline: MediaDeadline,
    ) -> Result<RawMediaLease, Error> {
        if max_bytes == 0 || max_bytes > self.byte_limit as u64 {
            return Err(Error::InvalidInput(
                "media download cap exceeds the global raw-memory allowance".into(),
            ));
        }
        let amount = u32::try_from(max_bytes)
            .map_err(|_| Error::InvalidInput("media download cap is too large".into()))?;
        deadline
            .run(async {
                // These private semaphores have no close path.
                let slot = Arc::clone(&self.slots)
                    .acquire_owned()
                    .await
                    .expect("private raw semaphore is never closed");
                let bytes = Arc::clone(&self.bytes)
                    .acquire_many_owned(amount)
                    .await
                    .expect("private raw byte semaphore is never closed");
                RawMediaLease {
                    inner: Arc::new(RawLeaseInner {
                        pool: Arc::clone(self),
                        _slot: slot,
                        _bytes: bytes,
                        retained: AtomicUsize::new(0),
                    }),
                    deadline,
                }
            })
            .await
    }
}
fn global_pool() -> &'static Arc<RawMediaPool> {
    static POOL: OnceLock<Arc<RawMediaPool>> = OnceLock::new();
    POOL.get_or_init(|| RawMediaPool::new(RAW_MEDIA_SLOTS, RAW_MEDIA_MEMORY_BYTES))
}

struct RawLeaseInner {
    pool: Arc<RawMediaPool>,
    _slot: OwnedSemaphorePermit,
    _bytes: OwnedSemaphorePermit,
    retained: AtomicUsize,
}
impl Drop for RawLeaseInner {
    fn drop(&mut self) {
        self.pool
            .retained
            .fetch_sub(*self.retained.get_mut(), Ordering::Relaxed);
    }
}

/// Ownership of one raw-buffer slot and its byte reservation. Clones share admission.
/// Keep it with the raw bytes, including inside started blocking work.
#[derive(Clone)]
pub struct RawMediaLease {
    inner: Arc<RawLeaseInner>,
    deadline: MediaDeadline,
}
impl std::fmt::Debug for RawMediaLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawMediaLease").finish_non_exhaustive()
    }
}
impl PartialEq for RawMediaLease {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}
impl RawMediaLease {
    /// Original operation deadline, shared with download and processing waits.
    pub fn deadline(&self) -> MediaDeadline {
        self.deadline
    }
    pub(crate) fn record_retained(&self, bytes: usize) {
        let previous = self.inner.retained.swap(bytes, Ordering::Relaxed);
        let current = if bytes >= previous {
            self.inner
                .pool
                .retained
                .fetch_add(bytes - previous, Ordering::Relaxed)
                + bytes
                - previous
        } else {
            self.inner
                .pool
                .retained
                .fetch_sub(previous - bytes, Ordering::Relaxed)
                - (previous - bytes)
        };
        self.inner
            .pool
            .peak_retained
            .fetch_max(current, Ordering::Relaxed);
    }
}

pub(crate) async fn acquire_raw(
    max_bytes: u64,
    deadline: MediaDeadline,
) -> Result<RawMediaLease, Error> {
    global_pool().acquire(max_bytes, deadline).await
}

pub(crate) fn media_stream<J, D, F>(
    jobs: Vec<(i32, J)>,
    deadline: MediaDeadline,
    max_bytes: u64,
    download: D,
) -> BoxStream<'static, MediaFetchOutcome>
where
    J: Send + 'static,
    D: Fn(J) -> F + Send + Sync + 'static,
    F: Future<Output = Result<MediaDownload, MediaFetchError>> + Send + 'static,
{
    media_stream_with_pool(
        Arc::clone(global_pool()),
        jobs,
        deadline,
        max_bytes,
        download,
    )
}
fn media_stream_with_pool<J, D, F>(
    pool: Arc<RawMediaPool>,
    jobs: Vec<(i32, J)>,
    deadline: MediaDeadline,
    max_bytes: u64,
    download: D,
) -> BoxStream<'static, MediaFetchOutcome>
where
    J: Send + 'static,
    D: Fn(J) -> F + Send + Sync + 'static,
    F: Future<Output = Result<MediaDownload, MediaFetchError>> + Send + 'static,
{
    let download = Arc::new(download);
    futures::stream::iter(jobs.into_iter().map(move |(message_id, job)| {
        let pool = Arc::clone(&pool);
        let download = Arc::clone(&download);
        async move {
            let result = async {
                let lease = pool
                    .acquire(max_bytes, deadline)
                    .await
                    .map_err(MediaFetchError::Failed)?;
                let mut image = deadline
                    .run(download(job))
                    .await
                    .map_err(MediaFetchError::Failed)??;
                if image.bytes.capacity() as u64 > max_bytes {
                    return Err(MediaFetchError::Failed(Error::MediaTooLarge {
                        size_bytes: image.bytes.capacity() as u64,
                        max_bytes,
                    }));
                }
                lease.record_retained(image.bytes.capacity());
                image.lease = Some(lease);
                Ok(image)
            }
            .await;
            MediaFetchOutcome { message_id, result }
        }
    }))
    .buffered(RAW_MEDIA_SLOTS)
    .boxed()
}

#[cfg(test)]
#[path = "tests/media_pipeline_tests.rs"]
mod tests;
