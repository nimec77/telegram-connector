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
        let handle = pool.handle.thin.clone();
        let upload_client = Client::new(pool.handle);
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        stops.push(stop_tx);
        // Own the actual public runner and its request queue without ever polling
        // run(): real upload_stream queues parts, but no network I/O can execute.
        let runner = tokio::spawn(async move {
            let _ = stop_rx.await;
            drop(pool.runner);
        });
        let pool = UploadPool::track(upload_client, handle, runner, Arc::clone(&snapshot));
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
    assert!(
        weak.iter().all(|w| w.upgrade().is_some()),
        "queued part buffers still own media admission"
    );
    assert_eq!(
        store
            .stage(
                &fixture.source("large.mp4"),
                &kind,
                SendDeadline::new(120).expect("budget")
            )
            .await
            .expect_err("queued buffers occupy both slots")
            .kind(),
        SendFailureKind::SendCapacityReached
    );
    for stop in stops {
        let _ = stop.send(());
    }
    store
        .close(tokio::time::Instant::now() + Duration::from_secs(10))
        .await
        .expect("queued bodies and snapshots released");
}
