use super::*;

#[tokio::test(start_paused = true)]
async fn task8_media_deadline_includes_resolution_before_any_raw_download() {
    for batch in [false, true] {
        let (mut client, _runner, _directory) =
            super::super::client_fixture::stalled_client().await;
        client.timeouts.download_secs = 1;
        let started = tokio::time::Instant::now();
        let result = if batch {
            client
                .download_messages_media_impl("100", &[1, 2], 1280)
                .await
                .map(|_| ())
        } else {
            client
                .download_message_media_impl("100", 1, 1280)
                .await
                .map(|_| ())
        };
        assert!(
            matches!(result, Err(Error::Timeout { secs: 1, .. })),
            "{result:?}"
        );
        assert_eq!(started.elapsed(), std::time::Duration::from_secs(1));
    }
}

use futures::FutureExt as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn transfer_concatenates_chunks_and_accepts_the_exact_byte_cap() {
    let chunks = futures::stream::iter(vec![Ok(vec![1, 2]), Ok(vec![]), Ok(vec![3, 4])]);
    let bytes = collect_download(
        chunks,
        DownloadBounds {
            max_bytes: 4,
            timeout_secs: 5,
        },
    )
    .await
    .expect("exact cap");
    assert_eq!(bytes, vec![1, 2, 3, 4]);
    let empty = futures::stream::empty::<Result<Vec<u8>, Error>>();
    assert_eq!(
        collect_download(
            empty,
            DownloadBounds {
                max_bytes: 4,
                timeout_secs: 5
            }
        )
        .await
        .expect("empty transfer"),
        Vec::<u8>::new()
    );
}

#[tokio::test]
async fn transfer_rejects_streamed_oversize_without_reading_another_chunk() {
    let observed = Arc::new(AtomicUsize::new(0));
    let reads = Arc::clone(&observed);
    let chunks = futures::stream::iter(vec![Ok(vec![1, 2]), Ok(vec![3, 4, 5]), Ok(vec![6])])
        .inspect(move |_| {
            reads.fetch_add(1, Ordering::SeqCst);
        });
    let error = collect_download(
        chunks,
        DownloadBounds {
            max_bytes: 4,
            timeout_secs: 5,
        },
    )
    .await
    .expect_err("stream cap");
    assert!(matches!(
        error,
        Error::MediaTooLarge {
            size_bytes: 5,
            max_bytes: 4
        }
    ));
    assert_eq!(
        observed.load(Ordering::SeqCst),
        2,
        "read after cap violation"
    );
}

#[tokio::test]
async fn transfer_propagates_failure_without_retry_or_partial_success() {
    let observed = Arc::new(AtomicUsize::new(0));
    let reads = Arc::clone(&observed);
    let chunks = futures::stream::iter(vec![
        Ok(vec![1]),
        Err(Error::DownloadFailed("fixture disconnect".into())),
        Ok(vec![2]),
    ])
    .inspect(move |_| {
        reads.fetch_add(1, Ordering::SeqCst);
    });
    let error = collect_download(
        chunks,
        DownloadBounds {
            max_bytes: 4,
            timeout_secs: 5,
        },
    )
    .await
    .expect_err("failed stream");
    assert!(matches!(error, Error::DownloadFailed(_)));
    assert_eq!(
        observed.load(Ordering::SeqCst),
        2,
        "retried or consumed past failure"
    );
}

struct TransferDrop(Arc<AtomicUsize>);
impl Drop for TransferDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn stalled_stream(
    drop_count: Arc<AtomicUsize>,
) -> impl futures::Stream<Item = Result<Vec<u8>, Error>> {
    futures::stream::unfold(TransferDrop(drop_count), |probe| async move {
        std::future::pending::<()>().await;
        drop(probe);
        None
    })
}

#[tokio::test(start_paused = true)]
async fn transfer_timeout_drops_the_inflight_stream() {
    let dropped = Arc::new(AtomicUsize::new(0));
    let started = tokio::time::Instant::now();
    let result = collect_download(
        stalled_stream(Arc::clone(&dropped)),
        DownloadBounds {
            max_bytes: 4,
            timeout_secs: 2,
        },
    )
    .await;
    assert!(matches!(result,Err(Error::Timeout {operation,secs:2}) if operation=="download_media"));
    assert_eq!(started.elapsed(), std::time::Duration::from_secs(2));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn transfer_progress_does_not_restart_the_total_timeout() {
    let chunks = futures::stream::iter([1, 2, 3]).then(|byte| async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        Ok(vec![byte])
    });
    let started = tokio::time::Instant::now();
    let result = collect_download(
        chunks,
        DownloadBounds {
            max_bytes: 4,
            timeout_secs: 2,
        },
    )
    .await;
    assert!(matches!(result, Err(Error::Timeout { secs: 2, .. })));
    assert_eq!(started.elapsed(), std::time::Duration::from_secs(2));
}

#[tokio::test]
async fn transfer_cancellation_drops_waiting_work() {
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut transfer = Box::pin(collect_download(
        stalled_stream(Arc::clone(&dropped)),
        DownloadBounds {
            max_bytes: 4,
            timeout_secs: 60,
        },
    ));
    assert!(transfer.as_mut().now_or_never().is_none());
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    drop(transfer);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

use super::media_fixtures::{cached_size, photo, source, video};
use crate::telegram::types::{PeerNamespace, VideoKind};
use grammers_client::media::Media as UpstreamMedia;
use grammers_client::tl;
use grammers_session::types::PeerId;

#[tokio::test]
async fn visual_download_selects_the_smallest_adequate_cached_photo_and_keeps_metadata() {
    let (client, _runner, _directory) = super::super::client_fixture::stalled_client().await;
    let fixture = source(Some(photo(vec![
        cached_size("s", 100, 100, &[1]),
        cached_size("m", 1280, 720, &[2, 3]),
        cached_size("x", 1920, 1080, &[4, 5, 6]),
    ])));
    let result = download_visual_media(&client.client, 5, 20, fixture, "fixture", 42, 1280)
        .await
        .expect("real cached downloader");
    assert_eq!(result.bytes, vec![2, 3]);
    assert_eq!(result.media_type, MediaType::Photo);
    assert!(!result.is_thumbnail);
    assert_eq!(
        (result.width, result.height, result.source_size_bytes),
        (Some(1280), Some(720), 2)
    );
    assert_eq!(
        (result.largest_width, result.largest_height),
        (Some(1920), Some(1080))
    );
    assert_eq!(result.caption.as_deref(), Some("Фото 🦀"));
    assert_eq!(result.channel_id.get(), 100);
    assert_eq!(result.peer_namespace, PeerNamespace::Channel);
    assert!(result.lease.is_none());
}

#[tokio::test]
async fn visual_download_falls_back_to_largest_available_size_and_preserves_chat_identity() {
    let (client, _runner, _directory) = super::super::client_fixture::stalled_client().await;
    let mut fixture = source(Some(photo(vec![
        cached_size("s", 100, 100, &[1]),
        cached_size("m", 640, 360, &[2, 3]),
    ])));
    fixture.peer_id = PeerId::chat_unchecked(77);
    fixture.caption = None;
    let result = download_visual_media(&client.client, 5, 20, fixture, "fixture", 42, 1280)
        .await
        .expect("largest fallback");
    assert_eq!(result.bytes, vec![2, 3]);
    assert_eq!((result.width, result.height), (Some(640), Some(360)));
    assert_eq!(result.channel_id.get(), 77);
    assert_eq!(result.peer_namespace, PeerNamespace::Chat);
    assert!(result.caption.is_none());
}

#[tokio::test]
async fn visual_download_uses_cached_thumbnails_for_all_video_like_kinds() {
    let (client, _runner, _directory) = super::super::client_fixture::stalled_client().await;
    for (kind, media_type) in [
        (VideoKind::Video, MediaType::Video),
        (VideoKind::VideoNote, MediaType::VideoNote),
        (VideoKind::Animation, MediaType::Animation),
    ] {
        let fixture = source(Some(video(
            kind,
            Some(vec![cached_size("m", 640, 360, &[7, 8])]),
        )));
        let result = download_visual_media(&client.client, 5, 20, fixture, "fixture", 42, 1280)
            .await
            .expect("cached thumbnail");
        assert_eq!(result.bytes, vec![7, 8]);
        assert_eq!(result.media_type, media_type);
        assert!(result.is_thumbnail);
        let info = result.video_info.expect("source video metadata");
        assert_eq!(info.kind, kind);
        assert_eq!(info.duration_seconds, 7);
        assert_eq!(
            (info.width, info.height, info.file_size_bytes),
            (1920, 1080, 123_456)
        );
    }
}

#[tokio::test(start_paused = true)]
async fn visual_download_rejects_reported_oversize_before_a_remote_transfer() {
    let (client, _runner, _directory) = super::super::client_fixture::stalled_client().await;
    let remote = tl::types::PhotoSize {
        r#type: "x".into(),
        w: 1280,
        h: 720,
        size: 10,
    }
    .into();
    let fixture = source(Some(photo(vec![remote])));
    let started = tokio::time::Instant::now();
    let result = download_visual_media(&client.client, 1, 9, fixture, "fixture", 42, 1280).await;
    assert!(matches!(
        result,
        Err(Error::MediaTooLarge {
            size_bytes: 10,
            max_bytes: 9
        })
    ));
    assert_eq!(
        started.elapsed(),
        std::time::Duration::ZERO,
        "started remote work before rejecting the reported cap"
    );
}

#[tokio::test(start_paused = true)]
async fn visual_download_bounds_the_real_remote_iterator_without_polling_its_sender() {
    let (client, _runner, _directory) = super::super::client_fixture::stalled_client().await;
    let remote = tl::types::PhotoSize {
        r#type: "x".into(),
        w: 1280,
        h: 720,
        size: 10,
    }
    .into();
    let fixture = source(Some(photo(vec![remote])));
    let started = tokio::time::Instant::now();
    let result = download_visual_media(&client.client, 1, 20, fixture, "fixture", 42, 1280).await;
    assert!(matches!(result,Err(Error::Timeout {operation,secs:1}) if operation=="download_media"));
    assert_eq!(started.elapsed(), std::time::Duration::from_secs(1));
}

#[tokio::test]
async fn visual_download_distinguishes_absent_media_from_missing_variants() {
    let (client, _runner, _directory) = super::super::client_fixture::stalled_client().await;
    let absent =
        download_visual_media(&client.client, 1, 20, source(None), "fixture", 42, 1280).await;
    assert!(matches!(absent,Err(Error::NoVisualMedia {media_type}) if media_type=="none"));
    let empty: tl::enums::PhotoSize = tl::types::PhotoSizeEmpty { r#type: "i".into() }.into();
    for media in [
        photo(vec![]),
        photo(vec![empty]),
        video(VideoKind::Video, None),
    ] {
        let result = download_visual_media(
            &client.client,
            1,
            20,
            source(Some(media)),
            "fixture",
            42,
            1280,
        )
        .await;
        assert!(
            matches!(result, Err(Error::DownloadFailed(_))),
            "{result:?}"
        );
    }
}

#[tokio::test]
async fn visual_download_rejects_nonvisual_documents_and_unsupported_user_peers() {
    let (client, _runner, _directory) = super::super::client_fixture::stalled_client().await;
    let mut document = video(
        VideoKind::Video,
        Some(vec![cached_size("m", 640, 360, &[1])]),
    );
    if let UpstreamMedia::Document(ref mut document) = document
        && let Some(tl::enums::Document::Document(raw)) = &mut document.raw.document
    {
        raw.attributes.clear();
    }
    let result = download_visual_media(
        &client.client,
        1,
        20,
        source(Some(document)),
        "fixture",
        42,
        1280,
    )
    .await;
    assert!(matches!(result,Err(Error::NoVisualMedia {media_type}) if media_type=="document"));
    let mut fixture = source(Some(photo(vec![cached_size("m", 640, 360, &[1])])));
    fixture.peer_id = PeerId::user_unchecked(7);
    let result = download_visual_media(&client.client, 1, 20, fixture, "fixture", 42, 1280).await;
    assert!(matches!(result, Err(Error::UnsupportedPeer)));
}

#[tokio::test]
#[ignore = "explicit local cached-transfer latency measurement"]
async fn media_cached_transfer_latency_measurement() {
    let (client, _unpolled_runner, _directory) =
        super::super::client_fixture::stalled_client().await;
    let payload = crate::test_helpers::create_test_jpeg(1280, 720);
    let mut nanos = Vec::new();
    for _ in 0..100 {
        let fixture = source(Some(photo(vec![cached_size("m", 1280, 720, &payload)])));
        let started = std::time::Instant::now();
        let result = download_visual_media(
            &client.client,
            5,
            payload.len() as u64,
            fixture,
            "fixture",
            42,
            1280,
        )
        .await
        .expect("cached transfer");
        nanos.push(started.elapsed().as_nanos());
        assert_eq!(result.bytes.len(), payload.len());
    }
    nanos.sort_unstable();
    eprintln!(
        "cached_transfer_samples=100 payload_bytes={} p50_ns={} p95_ns={} max_ns={}",
        payload.len(),
        nanos[49],
        nanos[94],
        nanos[99]
    );
}
