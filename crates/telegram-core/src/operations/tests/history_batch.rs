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

#[tokio::test]
async fn history_range_locally_empty_pages_skip_admission_and_the_backend() {
    use crate::error::Error;
    use crate::operations::HistoryIdRange;
    use crate::telegram::client::stalled_client;

    let (client, _runner, _dir) = stalled_client().await;
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().never();
    let operations = AccountOperations::new(
        Arc::new(client),
        Arc::new(limiter),
        AccountLimits::default(),
    );
    let id = |value| MessageId::new(value).expect("positive message ID");
    for (through, before) in [(9, None), (10, None), (20, Some(11))] {
        let page = operations
            .history_range(
                HistoryIdRange {
                    channel_id: ChannelId::new(42).expect("channel"),
                    after_id: Some(id(10)),
                    through_id: id(through),
                    before_id: before.map(id),
                    limit: 10,
                },
                &context(),
            )
            .await
            .expect("empty page");
        assert!(page.messages.is_empty());
        assert_eq!(page.next_before_id, None);
    }
    assert!(matches!(
        operations
            .history_range(
                HistoryIdRange {
                    channel_id: ChannelId::new(42).expect("channel"),
                    after_id: Some(id(10)),
                    through_id: id(i64::from(i32::MAX) + 1),
                    before_id: None,
                    limit: 10,
                },
                &context(),
            )
            .await,
        Err(Error::InvalidInput(_))
    ));
}

#[tokio::test]
async fn history_range_propagates_admission_refusal_before_contacting_telegram() {
    use crate::error::Error;
    use crate::operations::HistoryIdRange;
    use crate::telegram::client::stalled_client;

    let (client, _runner, _dir) = stalled_client().await;
    let mut limiter = MockRateLimiterTrait::new();
    limiter
        .expect_acquire()
        .with(mockall::predicate::eq(1))
        .once()
        .returning(|_| {
            Err(Error::RateLimit {
                retry_after_seconds: 7,
                detail: String::new(),
            })
        });
    let operations = AccountOperations::new(
        Arc::new(client),
        Arc::new(limiter),
        AccountLimits::default(),
    );

    let result = operations
        .history_range(
            HistoryIdRange {
                channel_id: ChannelId::new(42).expect("channel"),
                after_id: None,
                through_id: MessageId::new(10).expect("through"),
                before_id: None,
                limit: 10,
            },
            &context(),
        )
        .await;
    assert!(matches!(
        result,
        Err(Error::RateLimit {
            retry_after_seconds: 7,
            ..
        })
    ));
}

#[tokio::test]
async fn hosted_search_rejects_global_and_non_numeric_scopes_before_any_charge() {
    use crate::error::Error;
    use crate::operations::{SearchOperation, SearchScope};
    use crate::telegram::client::stalled_client;

    let (client, _runner, _dir) = stalled_client().await;
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().never();
    let operations = AccountOperations::new(
        Arc::new(client),
        Arc::new(limiter),
        AccountLimits::default(),
    );
    for scope in [
        SearchScope::Global,
        SearchScope::Channel("a_username".into()),
    ] {
        let result = operations
            .search_joined_channels(
                SearchOperation {
                    scope,
                    query: "post".into(),
                    hours_back: 48,
                    limit: 10,
                    media_filter: None,
                    from_date: None,
                    to_date: None,
                    collapse_albums: false,
                    before_id: None,
                    after_id: None,
                },
                &context(),
            )
            .await;
        assert!(matches!(result, Err(Error::InvalidInput(_))));
    }
}

#[tokio::test]
async fn hosted_discovery_filters_basic_chat_with_the_same_numeric_id() {
    use crate::test_helpers::create_test_channel;

    let mut client = MockTelegramClientTrait::new();
    client
        .expect_search_public_channels()
        .once()
        .with(
            mockall::predicate::eq("example"),
            mockall::predicate::eq(20),
        )
        .returning(|_, _| {
            let public_channel = create_test_channel(42, "eligible_channel");
            let mut basic_group = create_test_channel(42, "basic_group");
            basic_group.peer_namespace = Some(PeerNamespace::Chat);
            Ok(vec![basic_group, public_channel])
        });
    let operations = AccountOperations::new(
        Arc::new(client),
        Arc::new(limiter()),
        AccountLimits::default(),
    );

    let result = operations
        .discover_channels("example", 20, &context())
        .await
        .expect("one eligible discovery");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name.as_str(), "Channel eligible_channel");
}
