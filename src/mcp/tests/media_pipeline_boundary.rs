use crate::error::Error;
use crate::telegram::media_pipeline::{
    MediaDeadline, RAW_MEDIA_MEMORY_BYTES, RAW_MEDIA_SLOTS, RawMediaPool, media_stream_with_pool,
};
use crate::telegram::types::{ChannelId, MediaDownload, MediaType, PeerNamespace};
use futures::StreamExt as _;
use std::sync::Arc;
use std::time::Duration;

fn raw(id: u8, bytes: usize) -> MediaDownload {
    MediaDownload {
        lease: None,
        channel_id: ChannelId::new(100).expect("id"),
        peer_namespace: PeerNamespace::Channel,
        bytes: vec![id; bytes],
        media_type: MediaType::Photo,
        is_thumbnail: false,
        caption: None,
        width: None,
        height: None,
        source_size_bytes: bytes as u64,
        video_info: None,
        largest_width: None,
        largest_height: None,
    }
}

#[tokio::test(start_paused = true)]
async fn task8_cancelled_image_awaiters_keep_raw_capacity_until_the_worker_exits() {
    use std::sync::{Condvar, Mutex};
    let pool = RawMediaPool::new(1, 1024);
    let lease = pool
        .acquire(1024, MediaDeadline::try_new(30).expect("deadline"))
        .await
        .expect("lease");
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let worker_release = Arc::clone(&release);
    let (started, ready) = tokio::sync::oneshot::channel();
    let (finished, done) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(crate::mcp::runtime::image_job(move || {
        started.send(()).expect("started");
        let (lock, cv) = &*worker_release;
        let mut released = lock.lock().expect("fixture lock");
        while !*released {
            released = cv.wait(released).expect("fixture wait");
        }
        drop(lease);
        finished.send(()).expect("finished");
    }));
    ready.await.expect("worker started");
    task.abort();
    let replacement = Arc::clone(&pool);
    let deadline = MediaDeadline::try_new(1).expect("deadline");
    let replacement = tokio::spawn(async move { replacement.acquire(1024, deadline).await });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    let blocked = replacement.await.expect("replacement waiter");
    let (lock, cv) = &*release;
    *lock.lock().expect("release") = true;
    cv.notify_all();
    done.await.expect("worker released capacity");
    assert!(matches!(blocked, Err(Error::Timeout { secs: 1, .. })));
    assert!(
        pool.acquire(1024, MediaDeadline::try_new(1).expect("deadline"))
            .await
            .is_ok()
    );
    assert!(task.await.expect_err("cancelled waiter").is_cancelled());
}

#[tokio::test]
#[ignore = "representative memory/RSS measurement; run explicitly with time -l"]
async fn task8_media_memory_measurement() {
    let pool = RawMediaPool::new(RAW_MEDIA_SLOTS, RAW_MEDIA_MEMORY_BYTES);
    let source = Arc::new(crate::test_helpers::create_test_jpeg(4096, 2048));
    let run = |base| {
        let pool = Arc::clone(&pool);
        let source = Arc::clone(&source);
        async move {
            let mut stream = media_stream_with_pool(
                pool,
                (base..base + 10).map(|id| (id, id)).collect(),
                MediaDeadline::try_new(120).expect("deadline"),
                20 * 1024 * 1024,
                move |id| {
                    let source = Arc::clone(&source);
                    async move {
                        let mut image = raw(id as u8, 0);
                        image.bytes = (*source).clone();
                        image.bytes.resize(20 * 1024 * 1024, 0);
                        image.bytes.shrink_to_fit();
                        Ok(image)
                    }
                },
            );
            let mut ids = Vec::new();
            let mut encoded = Vec::new();
            while let Some(outcome) = stream.next().await {
                let mut download = outcome.result.expect("download");
                let bytes = std::mem::take(&mut download.bytes);
                let lease = download.lease.take();
                let image = crate::mcp::runtime::image_job(move || {
                    let _lease = lease;
                    crate::mcp::tools::image::process_image(&bytes, 1280)
                })
                .await
                .expect("worker")
                .expect("valid image");
                ids.push(outcome.message_id);
                encoded.push(image.base64_jpeg);
            }
            (ids, encoded.iter().map(String::len).sum::<usize>())
        }
    };
    let started = std::time::Instant::now();
    let (a, b) = tokio::join!(run(0), run(100));
    assert_eq!(a.0, (0..10).collect::<Vec<_>>());
    assert_eq!(b.0, (100..110).collect::<Vec<_>>());
    let peak = pool.usage().1;
    assert!(peak <= 80 * 1024 * 1024);
    assert_eq!(pool.usage().0, 0);
    eprintln!(
        "raw_peak_bytes={peak} encoded_output_bytes={} elapsed_ms={}",
        a.1 + b.1,
        started.elapsed().as_millis()
    );
}
