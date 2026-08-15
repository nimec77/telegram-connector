//! get_messages_media_batch: payload cap and rate-limit charging/refunds.

use super::media_batch_fixtures::{no_media, not_found, ok_outcome, request, summary_of};
use crate::error::Error;
use crate::mcp::server::McpServer;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::test_helpers::permissive_limiter;
use mockall::predicate::eq;
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

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

#[tokio::test]
async fn an_enormous_media_cost_refunds_without_overflowing() {
    // Every id fails, so the refund multiplies the cost by the full request
    // size. With an unchecked `*` this panics in debug builds.
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media()
        .return_once(|_, _, _| Ok(vec![not_found(10), not_found(11), not_found(12)]));

    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().times(1).returning(|_| Ok(()));
    limiter.expect_refund().times(1).return_const(());

    let server =
        McpServer::new(Arc::new(client), Arc::new(limiter)).with_media_download_cost(u32::MAX / 2);

    let result = server
        .get_messages_media_batch(
            Parameters(request("chan", vec![10, 11, 12])),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    assert!(
        result.is_ok(),
        "a huge configured cost must not panic the call"
    );
}
