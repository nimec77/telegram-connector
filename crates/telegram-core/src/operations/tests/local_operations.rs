use crate::error::Error;
use crate::operations::{AccountLimits, AccountOperations, CancellationToken};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use std::sync::Arc;

#[tokio::test]
async fn local_channel_metadata_rejects_an_invalid_reference_before_admission() {
    let mut telegram = MockTelegramClientTrait::new();
    telegram.expect_get_channel_info().never();
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().never();
    let account = AccountOperations::new(
        Arc::new(telegram),
        Arc::new(limiter),
        AccountLimits::default(),
    );
    let context = account
        .history_context(CancellationToken::new())
        .expect("operation context");
    assert!(matches!(
        account
            .channel_info_by_reference("!", false, &context)
            .await,
        Err(Error::InvalidInput(_))
    ));
}

#[tokio::test]
async fn cancelled_local_metadata_skips_admission_and_telegram() {
    let mut telegram = MockTelegramClientTrait::new();
    telegram.expect_get_subscribed_channels().never();
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().never();
    let account = AccountOperations::new(
        Arc::new(telegram),
        Arc::new(limiter),
        AccountLimits::default(),
    );
    let token = CancellationToken::new();
    token.cancel();
    let context = account.history_context(token).expect("operation context");
    assert!(matches!(
        account.subscribed_channels(20, 0, &context).await,
        Err(Error::Cancelled)
    ));
}

#[cfg(feature = "image-preview")]
#[tokio::test]
async fn dropping_local_media_before_shaping_refunds_the_reservation_once() {
    use crate::telegram::types::{ChannelId, MediaBatch, PeerNamespace};

    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_download_messages_media_pipeline()
        .once()
        .withf(|channel, ids, dimension| channel == "42" && ids == [7, 9] && *dimension == 256)
        .returning(|_, _, _| {
            Ok(MediaBatch {
                channel_id: ChannelId::new(42).expect("channel"),
                peer_namespace: PeerNamespace::Channel,
                outcomes: Vec::new(),
                stream: None,
                deadline: None,
            })
        });
    let mut limiter = MockRateLimiterTrait::new();
    let mut stages = mockall::Sequence::new();
    limiter
        .expect_acquire()
        .with(mockall::predicate::eq(6))
        .once()
        .in_sequence(&mut stages)
        .returning(|_| Ok(()));
    limiter
        .expect_refund()
        .with(mockall::predicate::eq(6))
        .once()
        .in_sequence(&mut stages)
        .return_const(());
    let account = AccountOperations::new(
        Arc::new(telegram),
        Arc::new(limiter),
        AccountLimits::default(),
    );
    let context = account
        .history_context(CancellationToken::new())
        .expect("operation context");
    let (_batch, reservation) = account
        .download_media_batch_by_reference("42", &[7, 9], 256, 3, &context)
        .await
        .expect("media admitted");
    drop(reservation);
}
