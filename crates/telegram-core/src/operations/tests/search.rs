use crate::error::Error;
use crate::operations::{
    AccountLimits, AccountOperations, OperationContext, SearchOperation, SearchScope,
};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::ChannelId;
use crate::test_helpers::{create_test_message, create_test_search_result};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

fn request(scope: SearchScope) -> SearchOperation {
    SearchOperation {
        scope,
        query: "release".into(),
        hours_back: 48,
        limit: 20,
        media_filter: None,
        from_date: None,
        to_date: None,
        collapse_albums: true,
        before_id: None,
        after_id: None,
    }
}

fn deadline(seconds: u64) -> OperationContext {
    OperationContext::new(
        Instant::now() + Duration::from_secs(seconds),
        CancellationToken::new(),
    )
}

#[tokio::test]
async fn search_fanout_retains_failed_channel_charge_and_typed_outcome() {
    let mut limiter = MockRateLimiterTrait::new();
    limiter
        .expect_acquire()
        .with(mockall::predicate::eq(2))
        .once()
        .returning(|_| Ok(()));
    limiter.expect_refund().never();
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_search_messages()
        .withf(|params| params.channel_id == ChannelId::new(42).ok())
        .once()
        .returning(|_| {
            Ok(create_test_search_result(
                vec![create_test_message(7, "release", 42)],
                "release",
                1,
            ))
        });
    client
        .expect_search_messages()
        .withf(|params| params.channel_id == ChannelId::new(43).ok())
        .once()
        .returning(|_| Err(Error::ChannelNotFound("43".into())));
    let operations = AccountOperations::new(
        Arc::new(client),
        Arc::new(limiter),
        AccountLimits::default(),
    );
    let result = operations
        .search(
            request(SearchScope::Channels(vec![
                "42".into(),
                "+00042".into(),
                "43".into(),
            ])),
            &deadline(30),
        )
        .await
        .expect("usable channel");
    assert_eq!(result.result.messages[0].id.get(), 7);
    assert_eq!(result.result.query_metadata.channels_scanned, Some(2));
    assert_eq!(result.failures[0].0, "43");
    assert!(matches!(result.failures[0].1, Error::ChannelNotFound(_)));
}

#[tokio::test]
async fn invalid_or_cancelled_search_makes_no_charge_or_rpc() {
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().never();
    let mut client = MockTelegramClientTrait::new();
    client.expect_search_messages().never();
    client.expect_resolve_channel_identity().never();
    let operations = AccountOperations::new(
        Arc::new(client),
        Arc::new(limiter),
        AccountLimits::default(),
    );
    assert!(matches!(
        operations
            .search(
                request(SearchScope::Channels(vec!["42".into(), " ".into()])),
                &deadline(30)
            )
            .await,
        Err(Error::InvalidInput(_))
    ));
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let context = OperationContext::new(Instant::now() + Duration::from_secs(30), cancellation);
    assert!(matches!(
        operations
            .search(request(SearchScope::Global), &context)
            .await,
        Err(Error::Cancelled)
    ));
}

#[tokio::test(start_paused = true)]
async fn search_deadline_applies_to_username_resolution() {
    let (client, _runner, _directory) = crate::telegram::client::stalled_client().await;
    let mut limiter = MockRateLimiterTrait::new();
    limiter
        .expect_acquire()
        .with(mockall::predicate::eq(1))
        .once()
        .returning(|_| Ok(()));
    let operations = AccountOperations::new(
        Arc::new(client),
        Arc::new(limiter),
        AccountLimits::default(),
    );
    assert!(matches!(
        operations
            .search(
                request(SearchScope::Channel("@valid_name".into())),
                &deadline(1)
            )
            .await,
        Err(Error::Timeout {
            operation,
            secs: 1
        }) if operation == "search_messages"
    ));
}
