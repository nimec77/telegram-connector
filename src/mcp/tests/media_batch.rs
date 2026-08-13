//! Tests for get_messages_media_batch (work-order C).

use crate::error::Error;
use crate::mcp::server::McpServer;
use crate::mcp::tools::{GetMessageMediaResponse, GetMessagesMediaBatchRequest, MediaBatchSummary};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{MediaDownload, MediaFetchError, MediaFetchOutcome, MediaType};
use crate::test_helpers::create_test_jpeg;
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ContentBlock, NumberOrString};
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
        largest_width: None,
        largest_height: None,
    }
}

fn ok_outcome(message_id: i32, width: u32, height: u32) -> MediaFetchOutcome {
    MediaFetchOutcome {
        message_id,
        result: Ok(photo_download(width, height)),
    }
}

fn err_outcome(message_id: i32, error: MediaFetchError) -> MediaFetchOutcome {
    MediaFetchOutcome {
        message_id,
        result: Err(error),
    }
}

fn no_media(message_id: i32) -> MediaFetchOutcome {
    err_outcome(
        message_id,
        MediaFetchError::NoVisualMedia {
            media_type: "document".to_string(),
        },
    )
}

fn not_found(message_id: i32) -> MediaFetchOutcome {
    err_outcome(message_id, MediaFetchError::NotFound)
}

fn request(channel: &str, ids: Vec<i64>) -> GetMessagesMediaBatchRequest {
    GetMessagesMediaBatchRequest {
        channel_id: channel.to_string(),
        message_ids: ids,
        max_dimension: None,
    }
}

/// A limiter that accepts anything — charging is Task 7's subject.
fn permissive_limiter() -> MockRateLimiterTrait {
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().returning(|_| Ok(()));
    limiter.expect_refund().return_const(());
    limiter
}

fn summary_of(content: &[ContentBlock]) -> MediaBatchSummary {
    let ContentBlock::Text(text) = content.last().expect("summary block") else {
        panic!("last content block must be the summary text block");
    };
    serde_json::from_str(&text.text).expect("summary must be valid JSON")
}

#[tokio::test]
async fn mixed_batch_returns_images_and_reports_failures() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .withf(|channel, ids, max_dim| {
            channel == "news" && ids == [10, 11, 12, 13] && *max_dim == 1280
        })
        .return_once(|_, _, _| {
            Ok(vec![
                ok_outcome(10, 200, 100),
                no_media(11),
                ok_outcome(12, 160, 160),
                not_found(13),
            ])
        });

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11, 12, 13])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("a batch with per-id failures must still succeed");

    // Two image/metadata pairs, then the summary.
    assert_eq!(result.content.len(), 5);
    assert!(matches!(result.content[0], ContentBlock::Image(_)));
    assert!(matches!(result.content[1], ContentBlock::Text(_)));
    assert!(matches!(result.content[2], ContentBlock::Image(_)));
    assert!(matches!(result.content[3], ContentBlock::Text(_)));

    let summary = summary_of(&result.content);
    assert_eq!(summary.requested, 4);
    assert_eq!(summary.returned, 2);
    assert_eq!(summary.failed.len(), 2);
    assert_eq!(summary.failed[0].id, 11);
    assert_eq!(summary.failed[0].reason, "no_visual_media");
    assert_eq!(summary.failed[1].id, 13);
    assert_eq!(summary.failed[1].reason, "not_found");
}

#[tokio::test]
async fn metadata_blocks_are_adjacent_to_their_images_in_request_order() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .return_once(|_, _, _| Ok(vec![ok_outcome(10, 200, 100), ok_outcome(11, 160, 160)]));

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("tool should succeed");

    let ContentBlock::Text(first) = &result.content[1] else {
        panic!("block 1 must be metadata");
    };
    let first: GetMessageMediaResponse =
        serde_json::from_str(&first.text).expect("metadata must be valid JSON");
    assert_eq!(first.message_id, 10);

    let ContentBlock::Text(second) = &result.content[3] else {
        panic!("block 3 must be metadata");
    };
    let second: GetMessageMediaResponse =
        serde_json::from_str(&second.text).expect("metadata must be valid JSON");
    assert_eq!(second.message_id, 11);
}

#[tokio::test]
async fn channel_level_failure_fails_the_call() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .return_once(|_, _, _| Err(Error::InvalidInput("Channel not found: nope".to_string())));

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
    let result = server
        .get_messages_media_batch(
            Parameters(request("nope", vec![10, 11])),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let err = result.expect_err("an unresolvable channel is not a per-id failure");
    assert!(err.contains("Channel not found"));
}

#[tokio::test]
async fn channel_level_failure_refunds_all_but_one_token() {
    // A whole-call failure still performed a channel resolution and a fetch
    // RPC before it failed, so it is not free: exactly 1 token stays charged
    // (the same cost get_messages_batch charges for that resolve+fetch
    // shape), and everything else comes back.
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .return_once(|_, _, _| Err(Error::InvalidInput("Channel not found: nope".to_string())));

    let mut limiter = MockRateLimiterTrait::new();
    // 4 requested x default cost 3 = 12 acquired up front.
    limiter
        .expect_acquire()
        .with(eq(12))
        .times(1)
        .returning(|_| Ok(()));
    // 12 - 1 = 11 refunded; 1 token stays charged for the attempted call.
    limiter
        .expect_refund()
        .with(eq(11))
        .times(1)
        .return_const(());

    let server = McpServer::new(Arc::new(client), Arc::new(limiter));
    let result = server
        .get_messages_media_batch(
            Parameters(request("nope", vec![10, 11, 12, 13])),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    assert!(
        result.is_err(),
        "a channel-level failure must still fail the call"
    );
}

#[tokio::test]
async fn empty_message_ids_is_rejected_without_a_network_call() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_download_messages_media().never();

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![])),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    assert!(result.expect_err("empty ids").contains("at least one id"));
}

#[tokio::test]
async fn more_than_ten_ids_is_rejected() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_download_messages_media().never();

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", (1..=11).collect())),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let err = result.expect_err("11 ids exceeds the cap");
    assert!(
        err.contains("at most 10"),
        "error must state the cap: {err}"
    );
}

#[tokio::test]
async fn duplicate_ids_are_deduped_preserving_first_seen_order() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .withf(|_, ids, _| ids == [12, 10])
        .return_once(|_, _, _| Ok(vec![ok_outcome(12, 80, 80), ok_outcome(10, 80, 80)]));

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![12, 10, 12])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("tool should succeed");

    assert_eq!(summary_of(&result.content).requested, 2);
}

#[tokio::test]
async fn max_dimension_is_clamped_to_the_supported_range() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .withf(|_, _, max_dim| *max_dim == 2048)
        .return_once(|_, _, _| Ok(vec![ok_outcome(10, 80, 80)]));

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()));
    let mut req = request("news", vec![10]);
    req.max_dimension = Some(99_999);
    let result = server
        .get_messages_media_batch(Parameters(req), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn batch_of_one_matches_the_single_tool_metadata() {
    use crate::mcp::tools::GetMessageMediaRequest;

    let mut batch_client = MockTelegramClientTrait::new();
    batch_client
        .expect_download_messages_media()
        .return_once(|_, _, _| Ok(vec![ok_outcome(42, 200, 100)]));
    let batch_server = McpServer::new(Arc::new(batch_client), Arc::new(permissive_limiter()));
    let batch = batch_server
        .get_messages_media_batch(
            Parameters(request("news", vec![42])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("batch should succeed");

    let mut single_client = MockTelegramClientTrait::new();
    single_client
        .expect_download_message_media()
        .return_once(|_, _, _| Ok(photo_download(200, 100)));
    let single_server = McpServer::new(Arc::new(single_client), Arc::new(permissive_limiter()));
    let single = single_server
        .get_message_media(
            Parameters(GetMessageMediaRequest {
                channel_id: "news".to_string(),
                message_id: 42,
                max_dimension: None,
            }),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("single should succeed");

    let (ContentBlock::Image(batch_img), ContentBlock::Image(single_img)) =
        (&batch.content[0], &single.content[0])
    else {
        panic!("both must lead with an image block");
    };
    assert_eq!(
        batch_img.data, single_img.data,
        "image payload must be identical"
    );

    let (ContentBlock::Text(batch_meta), ContentBlock::Text(single_meta)) =
        (&batch.content[1], &single.content[1])
    else {
        panic!("both must follow with a metadata block");
    };
    assert_eq!(
        batch_meta.text, single_meta.text,
        "batch-of-1 metadata must be byte-identical to the single tool's"
    );

    // The batch adds a summary; that is the only permitted difference.
    assert_eq!(batch.content.len(), 3);
    assert_eq!(single.content.len(), 2);
}

#[tokio::test]
async fn payload_cap_downscales_then_reports_cap_reached() {
    // Three sizeable photos against a cap that fits roughly one of them.
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .return_once(|_, _, _| {
            Ok(vec![
                ok_outcome(10, 1200, 1200),
                ok_outcome(11, 1200, 1200),
                ok_outcome(12, 1200, 1200),
            ])
        });

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()))
        .with_media_batch_max_total_bytes(400_000);
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11, 12])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("hitting the cap is not an error");

    let summary = summary_of(&result.content);
    assert!(
        summary.total_base64_bytes <= 400_000,
        "cap must hold: {} bytes returned",
        summary.total_base64_bytes
    );
    assert_eq!(summary.max_total_bytes, 400_000);
    assert!(summary.returned >= 1, "at least one image must come back");
    assert!(
        summary
            .failed
            .iter()
            .any(|f| f.reason == "payload_cap_reached"),
        "ids dropped at the cap must say so: {:?}",
        summary.failed
    );
    assert_eq!(
        summary.returned + summary.failed.len(),
        3,
        "every requested id must be accounted for"
    );
}

#[tokio::test]
async fn cap_reached_ids_are_reported_in_request_order() {
    // Six sizeable photos against a cap that only fits the first two — this
    // reliably drops four ids (>= 3, so the ordering assertion below cannot
    // be satisfied by a 0- or 1-element list trivially matching itself).
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .return_once(|_, _, _| {
            Ok(vec![
                ok_outcome(10, 1200, 1200),
                ok_outcome(11, 1200, 1200),
                ok_outcome(12, 1200, 1200),
                ok_outcome(13, 1200, 1200),
                ok_outcome(14, 1200, 1200),
                ok_outcome(15, 1200, 1200),
            ])
        });

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()))
        .with_media_batch_max_total_bytes(400_000);
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11, 12, 13, 14, 15])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("tool should succeed");

    let summary = summary_of(&result.content);
    let capped: Vec<i64> = summary
        .failed
        .iter()
        .filter(|f| f.reason == "payload_cap_reached")
        .map(|f| f.id)
        .collect();
    // Ids 10 and 11 fit under the cap; the budget is spent by the time 12
    // comes up, so 12-15 are dropped in the order they were requested.
    assert_eq!(
        capped,
        vec![12, 13, 14, 15],
        "cap failures must be exactly these ids, in request order"
    );
}

#[tokio::test]
async fn payload_cap_drops_refund_their_cost() {
    // Same deterministic setup as cap_reached_ids_are_reported_in_request_order:
    // ids 10 and 11 fit under a 400_000-byte cap, 12-15 are dropped by it.
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .return_once(|_, _, _| {
            Ok(vec![
                ok_outcome(10, 1200, 1200),
                ok_outcome(11, 1200, 1200),
                ok_outcome(12, 1200, 1200),
                ok_outcome(13, 1200, 1200),
                ok_outcome(14, 1200, 1200),
                ok_outcome(15, 1200, 1200),
            ])
        });

    let mut limiter = MockRateLimiterTrait::new();
    // 6 requested x default cost 3 = 18 acquired up front.
    limiter
        .expect_acquire()
        .with(eq(18))
        .times(1)
        .returning(|_| Ok(()));
    // 4 ids dropped by the payload cap (12-15) x 3 = 12 refunded.
    limiter
        .expect_refund()
        .with(eq(12))
        .times(1)
        .return_const(());

    let server = McpServer::new(Arc::new(client), Arc::new(limiter))
        .with_media_batch_max_total_bytes(400_000);
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11, 12, 13, 14, 15])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("hitting the cap is not an error");

    assert_eq!(summary_of(&result.content).returned, 2);
}

#[tokio::test]
async fn a_generous_cap_returns_every_image() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .return_once(|_, _, _| Ok(vec![ok_outcome(10, 200, 100), ok_outcome(11, 200, 100)]));

    let server = McpServer::new(Arc::new(client), Arc::new(permissive_limiter()))
        .with_media_batch_max_total_bytes(8 * 1024 * 1024);
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("tool should succeed");

    let summary = summary_of(&result.content);
    assert_eq!(summary.returned, 2);
    assert!(summary.failed.is_empty());
}

use mockall::predicate::eq;

#[tokio::test]
async fn charges_for_every_requested_id_then_refunds_the_failures() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .return_once(|_, _, _| {
            Ok(vec![
                ok_outcome(10, 80, 80),
                no_media(11),
                ok_outcome(12, 80, 80),
                not_found(13),
                ok_outcome(14, 80, 80),
            ])
        });

    let mut limiter = MockRateLimiterTrait::new();
    // 5 requested x default cost 3 = 15 acquired up front.
    limiter
        .expect_acquire()
        .with(eq(15))
        .times(1)
        .returning(|_| Ok(()));
    // 2 produced nothing x 3 = 6 refunded.
    limiter
        .expect_refund()
        .with(eq(6))
        .times(1)
        .return_const(());

    let server = McpServer::new(Arc::new(client), Arc::new(limiter));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11, 12, 13, 14])),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("tool should succeed");

    assert_eq!(summary_of(&result.content).returned, 3);
}

#[tokio::test]
async fn a_fully_successful_batch_refunds_nothing() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .return_once(|_, _, _| Ok(vec![ok_outcome(10, 80, 80), ok_outcome(11, 80, 80)]));

    let mut limiter = MockRateLimiterTrait::new();
    limiter
        .expect_acquire()
        .with(eq(6))
        .times(1)
        .returning(|_| Ok(()));
    limiter
        .expect_refund()
        .with(eq(0))
        .times(1)
        .return_const(());

    let server = McpServer::new(Arc::new(client), Arc::new(limiter));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", vec![10, 11])),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn a_rejected_acquire_performs_no_download() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_download_messages_media().never();

    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().returning(|_| {
        Err(Error::RateLimit {
            retry_after_seconds: 4,
            detail: ": requested 30 tokens, 12.00 available".to_string(),
        })
    });
    limiter.expect_refund().never();

    let server = McpServer::new(Arc::new(client), Arc::new(limiter));
    let result = server
        .get_messages_media_batch(
            Parameters(request("news", (1..=10).collect())),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let err = result.expect_err("an unaffordable batch must be refused before any work");
    assert!(
        err.contains("retry after 4 seconds"),
        "the rate-limit error must carry the wait hint: {err}"
    );
}

#[test]
fn rate_limit_errors_carry_a_retry_hint() {
    // Pre-existing behaviour (src/error.rs). Pinned here because batch callers
    // are the ones most likely to hit the limiter and need a precise wait.
    let error = Error::RateLimit {
        retry_after_seconds: 7,
        detail: ": requested 30 tokens, 9.00 available".to_string(),
    };
    assert_eq!(
        error.to_string(),
        "rate limit exceeded: requested 30 tokens, 9.00 available, retry after 7 seconds"
    );
}
