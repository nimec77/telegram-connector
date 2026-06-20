//! Tests for get_message_media tool

use crate::error::Error;
use crate::mcp::server::McpServer;
use crate::mcp::tools::{GetMessageMediaRequest, GetMessageMediaResponse};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{MediaDownload, MediaType};
use crate::test_helpers::create_test_jpeg;
use base64::Engine as _;
use mockall::predicate::eq;
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{NumberOrString, RawContent};
use std::sync::Arc;

fn photo_download(width: u32, height: u32) -> MediaDownload {
    let bytes = create_test_jpeg(width, height);
    let source_size_bytes = bytes.len() as u64;
    MediaDownload {
        bytes,
        media_type: MediaType::Photo,
        is_thumbnail: false,
        caption: Some("benchmark chart".to_string()),
        width: Some(width),
        height: Some(height),
        source_size_bytes,
        video_info: None,
    }
}

fn request(channel: &str, message_id: i64, max_dimension: Option<u32>) -> GetMessageMediaRequest {
    GetMessageMediaRequest {
        channel_id: channel.to_string(),
        message_id,
        max_dimension,
    }
}

#[tokio::test]
async fn photo_returns_image_and_metadata_blocks() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_download_message_media()
        .withf(|channel, msg_id, max_dim| channel == "news" && *msg_id == 42 && *max_dim == 1280)
        .return_once(|_, _, _| Ok(photo_download(200, 100)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter
        .expect_acquire()
        .with(eq(5))
        .returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 42, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let call_result = result.expect("tool should succeed");
    assert_eq!(call_result.content.len(), 2);

    let RawContent::Image(img) = &call_result.content[0].raw else {
        panic!("first content block must be an image");
    };
    assert_eq!(img.mime_type, "image/jpeg");
    let jpeg = base64::engine::general_purpose::STANDARD
        .decode(&img.data)
        .expect("image data must be valid base64");
    let decoded = image::load_from_memory(&jpeg).expect("must be a decodable JPEG");
    assert_eq!(decoded.width(), 200); // source smaller than max_dimension: no upscale

    let RawContent::Text(text) = &call_result.content[1].raw else {
        panic!("second content block must be text");
    };
    let metadata: GetMessageMediaResponse = serde_json::from_str(&text.text).unwrap();
    assert_eq!(metadata.media_type, MediaType::Photo);
    assert!(!metadata.is_thumbnail);
    assert_eq!(metadata.caption.as_deref(), Some("benchmark chart"));
    assert_eq!(metadata.mime_type, "image/jpeg");
    assert_eq!(metadata.returned_width, 200);
    assert_eq!(metadata.returned_height, 100);
    assert_eq!(metadata.returned_size_bytes, jpeg.len());
}

#[tokio::test]
async fn video_thumbnail_sets_is_thumbnail() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_download_message_media()
        .return_once(|_, _, _| {
            let bytes = create_test_jpeg(320, 180);
            let source_size_bytes = bytes.len() as u64;
            Ok(MediaDownload {
                bytes,
                media_type: MediaType::Video,
                is_thumbnail: true,
                caption: None,
                width: Some(320),
                height: Some(180),
                source_size_bytes,
                video_info: None,
            })
        });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 7, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let call_result = result.expect("tool should succeed");
    let RawContent::Text(text) = &call_result.content[1].raw else {
        panic!("second content block must be text");
    };
    let metadata: GetMessageMediaResponse = serde_json::from_str(&text.text).unwrap();
    assert_eq!(metadata.media_type, MediaType::Video);
    assert!(metadata.is_thumbnail);
    assert!(metadata.caption.is_none());
}

#[tokio::test]
async fn video_metadata_included_in_response() {
    use crate::telegram::types::{VideoInfo, VideoKind};

    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_download_message_media()
        .return_once(|_, _, _| {
            let bytes = create_test_jpeg(320, 180);
            let source_size_bytes = bytes.len() as u64;
            Ok(MediaDownload {
                bytes,
                media_type: MediaType::Video,
                is_thumbnail: true,
                caption: None,
                width: Some(320),
                height: Some(180),
                source_size_bytes,
                video_info: Some(VideoInfo {
                    duration_seconds: 30,
                    width: 1920,
                    height: 1080,
                    file_size_bytes: 5_000_000,
                    kind: VideoKind::Video,
                    has_thumbnail: true,
                    mime_type: Some("video/mp4".to_string()),
                }),
            })
        });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 7, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let call_result = result.expect("tool should succeed");
    let RawContent::Text(text) = &call_result.content[1].raw else {
        panic!("second content block must be text");
    };
    let metadata: GetMessageMediaResponse = serde_json::from_str(&text.text).unwrap();
    let vi = metadata.video_info.expect("video_info present in metadata");
    assert_eq!(vi.kind, VideoKind::Video);
    assert_eq!(vi.duration_seconds, 30);
    assert_eq!(vi.width, 1920);
    assert!(vi.has_thumbnail);
}

#[tokio::test]
async fn no_visual_media_returns_structured_error() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_download_message_media()
        .return_once(|_, _, _| {
            Err(Error::NoVisualMedia {
                media_type: "poll".to_string(),
            })
        });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 8, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let error = result.expect_err("must be an error");
    assert!(error.contains("no visual media"));
    assert!(error.contains("poll"));
}

#[tokio::test]
async fn oversize_media_is_rejected() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_download_message_media()
        .return_once(|_, _, _| {
            Err(Error::MediaTooLarge {
                size_bytes: 25_000_000,
                max_bytes: 20_971_520,
            })
        });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 9, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let error = result.expect_err("must be an error");
    assert!(error.contains("media too large"));
    assert!(error.contains("25000000"));
}

#[tokio::test]
async fn max_dimension_is_clamped_to_2048() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_download_message_media()
        .withf(|_, _, max_dim| *max_dim == 2048)
        .return_once(|_, _, _| Ok(photo_download(64, 64)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 10, Some(5000))),
            RequestId(NumberOrString::Number(1)),
        )
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn max_dimension_is_clamped_up_to_64() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_download_message_media()
        .withf(|_, _, max_dim| *max_dim == 64)
        .return_once(|_, _, _| Ok(photo_download(64, 64)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 14, Some(10))),
            RequestId(NumberOrString::Number(1)),
        )
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn configured_media_download_cost_is_charged() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_download_message_media()
        .return_once(|_, _, _| Ok(photo_download(64, 64)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter
        .expect_acquire()
        .with(eq(9))
        .returning(|_| Ok(()));

    let server =
        McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter)).with_media_download_cost(9);
    let result = server
        .get_message_media(
            Parameters(request("news", 11, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn rate_limited_request_never_reaches_telegram() {
    // No expectation on the client mock: a call would panic the test.
    let mock_client = MockTelegramClientTrait::new();

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| {
        Err(Error::RateLimit {
            retry_after_seconds: 3,
        })
    });

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 12, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let error = result.expect_err("must be rate limited");
    assert!(error.contains("rate limit exceeded"));
}

#[tokio::test]
async fn corrupt_image_bytes_return_decode_error() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_download_message_media()
        .return_once(|_, _, _| {
            Ok(MediaDownload {
                bytes: vec![0x00; 32],
                media_type: MediaType::Photo,
                is_thumbnail: false,
                caption: None,
                width: None,
                height: None,
                source_size_bytes: 32,
                video_info: None,
            })
        });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 13, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let error = result.expect_err("must fail to decode");
    assert!(error.contains("decode"));
}
