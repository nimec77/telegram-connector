use crate::operations::{
    AccountLimits, AccountOperations, BatchOperation, CancellationToken, HistoryOperation,
    HistoryScope, OperationContext,
};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{ChannelId, MessageBatch, MessageId, PeerNamespace};
use crate::test_helpers::{create_test_message, create_test_search_result};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

fn context() -> OperationContext {
    OperationContext::new(
        Instant::now() + Duration::from_secs(30),
        CancellationToken::new(),
    )
}

fn limiter() -> MockRateLimiterTrait {
    let mut limiter = MockRateLimiterTrait::new();
    limiter
        .expect_acquire()
        .with(mockall::predicate::eq(1))
        .once()
        .returning(|_| Ok(()));
    limiter.expect_refund().never();
    limiter
}

#[tokio::test]
async fn recent_preserves_exclusive_cursor_and_album_policy() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_get_recent_messages()
        .once()
        .withf(|arg| {
            arg.channel_id == ChannelId::new(42).ok()
                && arg.before_id == MessageId::new(10).ok()
                && arg.after_id == MessageId::new(3).ok()
                && !arg.collapse_albums
        })
        .returning(|_| {
            Ok(create_test_search_result(
                vec![create_test_message(9, "recent", 42)],
                "",
                1,
            ))
        });
    let operations = AccountOperations::new(
        Arc::new(client),
        Arc::new(limiter()),
        AccountLimits::default(),
    );
    let page = operations
        .recent(
            HistoryOperation {
                scope: HistoryScope::Channel("42".into()),
                hours_back: 48,
                limit: 10,
                media_filter: None,
                from_date: None,
                to_date: None,
                collapse_albums: false,
                before_id: Some(MessageId::new(10).expect("upper bound")),
                after_id: Some(MessageId::new(3).expect("lower bound")),
            },
            &context(),
        )
        .await
        .expect("one recent post");
    assert_eq!(page.result.messages[0].id.get(), 9);
}

#[tokio::test]
async fn batch_deduplicates_before_limit_and_retains_missing_ids() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_get_messages_batch()
        .once()
        .withf(|reference, ids| reference == "42" && ids == [7, 5])
        .returning(|_, _| {
            Ok(MessageBatch {
                channel_id: ChannelId::new(42).expect("channel"),
                peer_namespace: PeerNamespace::Channel,
                messages: vec![create_test_message(7, "found", 42)],
                missing_ids: vec![5],
            })
        });
    let operations = AccountOperations::new(
        Arc::new(client),
        Arc::new(limiter()),
        AccountLimits::default(),
    );
    let mut message_ids = vec![MessageId::new(7).expect("message"); 60];
    message_ids.push(MessageId::new(5).expect("missing message"));
    let page = operations
        .batch(
            BatchOperation {
                channel: "42".into(),
                message_ids,
            },
            &context(),
        )
        .await
        .expect("valid batch");
    assert_eq!(page.messages[0].id.get(), 7);
    assert_eq!(page.missing_ids, vec![5]);
}
