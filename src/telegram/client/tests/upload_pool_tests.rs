use super::*;
use crate::sending::staging::StagingStore;
use crate::sending::test_support::Fixture;
use crate::telegram::sending::SendDeadline;
use crate::telegram::types::sending::{SendFailureKind, SendMediaKind};
use std::io::{Seek as _, Write as _};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, ReadBuf};

#[derive(Default)]
struct Counts {
    bytes: AtomicU64,
    max: AtomicUsize,
}
struct Counting<R> {
    inner: R,
    counts: Arc<Counts>,
}

#[tokio::test]
async fn sending_media_stalled_setup_cannot_hold_capacity_after_caller_cancellation() {
    let mut fixture = Fixture::new();
    fixture.config.settings.max_concurrent_media = 1;
    let store = StagingStore::try_new(&fixture.config).await.expect("store");
    let snapshot = store
        .stage(
            &fixture.source("image.jpg"),
            &SendMediaKind::Image,
            SendDeadline::new(30).expect("budget"),
        )
        .await
        .expect("snapshot");
    let (original, _runner, _dir) = super::super::stalled_client().await;
    let pool = SenderPool::new(Arc::clone(&original.session), 1);
    let client = Client::new(pool.handle);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let driver = async move {
        let _ = started_tx.send(());
        let _ = stop_rx.await;
        drop(pool.runner);
    };
    let upload = UploadPool::supervise(
        client,
        snapshot,
        tokio::time::Instant::now() + Duration::from_secs(30),
        driver,
    )
    .expect("owned runtime");
    tokio::time::timeout(Duration::from_secs(2), started_rx)
        .await
        .expect("running setup")
        .expect("started");
    drop(upload);
    let reclaimed = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match store
                .stage(
                    &fixture.source("image.jpg"),
                    &SendMediaKind::Image,
                    SendDeadline::new(30).expect("budget"),
                )
                .await
            {
                Ok(snapshot) => break snapshot,
                Err(error) if error.kind() == SendFailureKind::SendCapacityReached => {
                    tokio::task::yield_now().await
                }
                Err(error) => panic!("unexpected admission error {error}"),
            }
        }
    })
    .await;
    let _ = stop_tx.send(());
    assert!(
        reclaimed.is_ok(),
        "stalled running setup must be torn down without processing queued Quit"
    );
    drop(reclaimed);
    store
        .close(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
        .expect("cleanup");
}

async fn reclaimed_snapshot(store: &Arc<StagingStore>, fixture: &Fixture) -> Arc<StagedUpload> {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match store
                .stage(
                    &fixture.source("image.jpg"),
                    &SendMediaKind::Image,
                    SendDeadline::new(30).expect("budget"),
                )
                .await
            {
                Ok(snapshot) => break snapshot,
                Err(error) if error.kind() == SendFailureKind::SendCapacityReached => {
                    tokio::task::yield_now().await
                }
                Err(error) => panic!("unexpected admission error {error}"),
            }
        }
    })
    .await
    .expect("bounded upload teardown restores admission")
}

#[tokio::test]
async fn sending_media_owned_runtime_deadline_stops_stalled_driver_without_caller_drop() {
    let mut fixture = Fixture::new();
    fixture.config.settings.max_concurrent_media = 1;
    let store = StagingStore::try_new(&fixture.config).await.expect("store");
    let snapshot = store
        .stage(
            &fixture.source("image.jpg"),
            &SendMediaKind::Image,
            SendDeadline::new(30).expect("budget"),
        )
        .await
        .expect("snapshot");
    let (original, _original_runner, _dir) = super::super::stalled_client().await;
    let pool = SenderPool::new(Arc::clone(&original.session), 1);
    let client = Client::new(pool.handle);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let driver = async move {
        let _ = started_tx.send(());
        std::future::pending::<()>().await;
        drop(pool.runner);
    };
    let upload = UploadPool::supervise(
        client,
        snapshot,
        tokio::time::Instant::now() + Duration::from_secs(1),
        driver,
    )
    .expect("owned runtime");
    started_rx.await.expect("stalled running setup");
    let reclaimed = reclaimed_snapshot(&store, &fixture).await;
    drop(upload);
    drop(reclaimed);
    store
        .close(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
        .expect("cleanup");
}

struct ChildBuffer {
    _bytes: Vec<u8>,
    snapshot: std::sync::Weak<StagedUpload>,
    started: Option<tokio::sync::oneshot::Sender<()>>,
    release: std::sync::mpsc::Receiver<()>,
    violation: Arc<std::sync::atomic::AtomicBool>,
    finished: Arc<std::sync::atomic::AtomicBool>,
}
impl Drop for ChildBuffer {
    fn drop(&mut self) {
        if let Some(started) = self.started.take() {
            let _ = started.send(());
        }
        let _ = self.release.recv_timeout(Duration::from_secs(5));
        if self.snapshot.upgrade().is_none() {
            self.violation.store(true, Ordering::SeqCst);
        }
        self.finished.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn sending_media_owned_runtime_destroys_child_buffers_before_releasing_snapshot_admission() {
    let mut fixture = Fixture::new();
    fixture.config.settings.max_concurrent_media = 1;
    let store = StagingStore::try_new(&fixture.config).await.expect("store");
    let snapshot = store
        .stage(
            &fixture.source("image.jpg"),
            &SendMediaKind::Image,
            SendDeadline::new(30).expect("budget"),
        )
        .await
        .expect("snapshot");
    let weak = Arc::downgrade(&snapshot);
    let violation = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (child_tx, child_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let buffer = ChildBuffer {
        _bytes: vec![1; 524_288],
        snapshot: weak.clone(),
        started: Some(child_tx),
        release: release_rx,
        violation: Arc::clone(&violation),
        finished: Arc::clone(&finished),
    };
    let (original, _original_runner, _dir) = super::super::stalled_client().await;
    let pool = SenderPool::new(Arc::clone(&original.session), 1);
    let client = Client::new(pool.handle);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let driver = async move {
        tokio::spawn(async move {
            let _buffer = buffer;
            std::future::pending::<()>().await;
        });
        let _ = started_tx.send(());
        std::future::pending::<()>().await;
        drop(pool.runner);
    };
    let upload = UploadPool::supervise(
        client,
        snapshot,
        tokio::time::Instant::now() + Duration::from_secs(30),
        driver,
    )
    .expect("owned runtime");
    started_rx.await.expect("driver running");
    drop(upload);
    tokio::time::timeout(Duration::from_secs(2), child_rx)
        .await
        .expect("child teardown begins")
        .expect("child");
    assert!(
        weak.upgrade().is_some(),
        "runtime teardown still owns the lease"
    );
    assert_eq!(
        store
            .stage(
                &fixture.source("image.jpg"),
                &SendMediaKind::Image,
                SendDeadline::new(30).expect("budget")
            )
            .await
            .expect_err("child buffer holds admission")
            .kind(),
        SendFailureKind::SendCapacityReached
    );
    release_tx.send(()).expect("finish child destructor");
    let reclaimed = reclaimed_snapshot(&store, &fixture).await;
    assert!(finished.load(Ordering::SeqCst));
    assert!(!violation.load(Ordering::SeqCst));
    drop(reclaimed);
    store
        .close(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
        .expect("cleanup");
}
impl<R: AsyncRead + Unpin> AsyncRead for Counting<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        if matches!(result, Poll::Ready(Ok(()))) {
            let n = buf.filled().len() - before;
            self.counts.bytes.fetch_add(n as u64, Ordering::SeqCst);
            self.counts.max.fetch_max(n, Ordering::SeqCst);
        }
        result
    }
}

#[tokio::test]
async fn sending_media_two_near_cap_real_uploaders_keep_queued_parts_admitted_after_cancel() {
    let fixture = Fixture::new();
    let path = fixture.source.join("large.mp4");
    let mut file = std::fs::File::create(&path).expect("disposable sparse file");
    file.write_all(b"\0\0\0\x10ftypmp42\0\0\0\0")
        .expect("MP4 header");
    file.set_len(104_857_600).expect("100 MiB");
    file.rewind().expect("position");
    drop(file);
    let store = StagingStore::try_new(&fixture.config).await.expect("store");
    let (client, _original_runner, _dir) = super::super::stalled_client().await;
    let kind = SendMediaKind::Video {
        duration_seconds: 1.0,
        width: 16,
        height: 16,
    };
    let mut tasks = Vec::new();
    let mut stops = Vec::new();
    let mut counters = Vec::new();
    let mut weak = Vec::new();
    for _ in 0..2 {
        let snapshot = store
            .stage(
                &fixture.source("large.mp4"),
                &kind,
                SendDeadline::new(120).expect("budget"),
            )
            .await
            .expect("near-cap snapshot");
        weak.push(Arc::downgrade(&snapshot));
        let pool = SenderPool::new(Arc::clone(&client.session), 1);
        let upload_client = Client::new(pool.handle);
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        stops.push(stop_tx);
        // Own the actual public runner and its request queue without ever polling
        // run(): real upload_stream queues parts, but no network I/O can execute.
        let driver = async move {
            let _ = stop_rx.await;
            drop(pool.runner);
        };
        let pool = UploadPool::supervise(
            upload_client,
            Arc::clone(&snapshot),
            tokio::time::Instant::now() + Duration::from_secs(120),
            driver,
        )
        .expect("owned upload runtime");
        let counts = Arc::new(Counts::default());
        counters.push(Arc::clone(&counts));
        tasks.push(tokio::spawn(async move {
            let reader = snapshot.reader().await.expect("reader");
            let mut reader = Counting {
                inner: reader,
                counts,
            };
            pool.client
                .upload_stream(&mut reader, 104_857_600, snapshot.upload_name().into())
                .await
        }));
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        while counters
            .iter()
            .any(|c| c.bytes.load(Ordering::SeqCst) < 2_097_152)
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("four 512 KiB parts per actual uploader");
    for counts in &counters {
        assert_eq!(counts.bytes.load(Ordering::SeqCst), 2_097_152);
        assert!(counts.max.load(Ordering::SeqCst) <= 524_288);
    }
    for task in tasks {
        task.abort();
        assert!(task.await.expect_err("cancelled").is_cancelled());
    }
    for stop in stops {
        let _ = stop.send(());
    }
    store
        .close(tokio::time::Instant::now() + Duration::from_secs(10))
        .await
        .expect("queued bodies and snapshots released");
}
