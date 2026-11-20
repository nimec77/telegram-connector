use super::{
    ChannelEvent, ChannelHead, ChannelHeadLookup, ChannelPost, InterruptionReason, StreamBudget,
    StreamLimits, SubscribeError, test_support,
};
use crate::UtcTimestamp;
use crate::config::RateLimitConfig;
use crate::error::Error;
use crate::operations::{CancellationToken, OperationContext};
use crate::rate_limiter::RateLimiter;
use crate::telegram::types::{ChannelId, MessageId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, Semaphore};
use tokio::time::Instant;

struct HeldHeads {
    started: Notify,
    release: Semaphore,
    deny: bool,
}

impl HeldHeads {
    fn new(deny: bool) -> Self {
        Self {
            started: Notify::new(),
            release: Semaphore::new(0),
            deny,
        }
    }
}

#[async_trait::async_trait]
impl ChannelHeadLookup for HeldHeads {
    async fn channel_heads(&self, channels: &[ChannelId]) -> Result<Vec<ChannelHead>, Error> {
        self.started.notify_one();
        self.release
            .acquire()
            .await
            .expect("open fixture gate")
            .forget();
        if self.deny {
            return Err(Error::ChannelNotFound("42".into()));
        }
        Ok(channels
            .iter()
            .map(|&channel_id| ChannelHead {
                channel_id,
                head_message_id: (channel_id.get() == 42).then_some(message(103)),
            })
            .collect())
    }
}

fn channel(id: i64) -> ChannelId {
    ChannelId::new(id).expect("positive channel ID")
}

fn message(id: i64) -> MessageId {
    MessageId::new(id).expect("positive message ID")
}

fn post(channel_id: i64, message_id: i64, text: &str) -> ChannelPost {
    ChannelPost {
        channel_id: channel(channel_id),
        message_id: message(message_id),
        published_at: UtcTimestamp::from_unix_seconds(1_795_078_800).expect("fixture date"),
        text: text.into(),
        media: None,
        album_id: None,
    }
}

fn context(seconds: u64) -> OperationContext {
    OperationContext::new(
        Instant::now() + Duration::from_secs(seconds),
        CancellationToken::new(),
    )
}

fn limiter() -> Arc<RateLimiter> {
    Arc::new(RateLimiter::new(&RateLimitConfig {
        max_tokens: 10,
        refill_rate: 0.0,
        ..RateLimitConfig::default()
    }))
}

#[tokio::test]
async fn ready_precedes_pending_and_live_posts() {
    let heads = Arc::new(HeldHeads::new(false));
    let limiter = limiter();
    let budget = Arc::new(StreamBudget::new(8192));
    let (streams, input) = test_support::account_streams(
        heads.clone(),
        limiter.clone(),
        StreamLimits::default(),
        budget.clone(),
    );
    let setup = tokio::spawn(async move {
        streams
            .subscribe_channel_posts(&[channel(42), channel(73), channel(42)], &context(10))
            .await
            .map(|subscription| (streams, subscription))
    });
    heads.started.notified().await;
    input.publish(post(42, 103, "already visible"));
    input.publish(post(99, 301, "other channel"));
    let mut first = post(42, 104, "album first");
    first.album_id = Some(908);
    input.publish(first.clone());
    input.publish(post(73, 1, "other channel first"));
    input.publish(first);
    heads.release.add_permits(1);
    let (_streams, mut subscription) = setup.await.expect("setup join").expect("ready");
    let ready = subscription.next_queued().await.expect("ready");
    assert_eq!(
        ready.event(),
        &ChannelEvent::Ready(vec![
            ChannelHead {
                channel_id: channel(42),
                head_message_id: Some(message(103))
            },
            ChannelHead {
                channel_id: channel(73),
                head_message_id: None
            },
        ])
    );
    assert_eq!(
        ready.json(),
        r#"{"channels":[{"channel_id":"42","head_message_id":103},{"channel_id":"73","head_message_id":null}]}"#
    );
    assert_eq!(limiter.available_tokens(), 8.0);
    let mut second = post(42, 105, "");
    second.album_id = Some(908);
    input.publish(second);
    for (expected_channel, expected_message) in [(42, 104), (73, 1), (42, 105)] {
        let Some(ChannelEvent::Post(next)) = subscription.next().await else {
            panic!("expected post");
        };
        assert_eq!(next.channel_id, channel(expected_channel));
        assert_eq!(next.message_id, message(expected_message));
        assert_eq!(next.album_id, (expected_channel == 42).then_some(908));
    }
    drop(subscription);
    assert!(budget.used_bytes() > 0, "ready remains in flight");
    drop(ready);
    assert_eq!(budget.used_bytes(), 0);
}

#[tokio::test]
async fn denied_membership_releases_pending_bytes() {
    let heads = Arc::new(HeldHeads::new(true));
    let budget = Arc::new(StreamBudget::new(1024));
    let (streams, input) = test_support::account_streams(
        heads.clone(),
        limiter(),
        StreamLimits::default(),
        budget.clone(),
    );
    let setup = tokio::spawn(async move {
        streams
            .subscribe_channel_posts(&[channel(42)], &context(10))
            .await
    });
    heads.started.notified().await;
    input.publish(post(42, 104, "pending"));
    assert!(budget.used_bytes() > 0);
    heads.release.add_permits(1);
    assert!(matches!(
        setup.await.expect("setup join"),
        Err(SubscribeError::Ineligible)
    ));
    assert_eq!(budget.used_bytes(), 0);
}

#[tokio::test]
async fn pending_overflow_returns_before_head_rpc_finishes() {
    for limits in [
        StreamLimits {
            max_queue_items: 1,
            ..StreamLimits::default()
        },
        StreamLimits {
            max_queue_bytes: 200,
            ..StreamLimits::default()
        },
        StreamLimits {
            max_event_bytes: 200,
            ..StreamLimits::default()
        },
    ] {
        let heads = Arc::new(HeldHeads::new(false));
        let budget = Arc::new(StreamBudget::new(8192));
        let (streams, input) =
            test_support::account_streams(heads.clone(), limiter(), limits, budget.clone());
        let setup = tokio::spawn(async move {
            streams
                .subscribe_channel_posts(&[channel(42)], &context(10))
                .await
        });
        heads.started.notified().await;
        input.publish(post(42, 104, "pending"));
        assert!(budget.used_bytes() > 0);
        input.publish(post(42, 105, &"x".repeat(250)));
        assert!(matches!(
            setup.await.expect("setup join"),
            Err(SubscribeError::SetupOverflow)
        ));
        assert_eq!(budget.used_bytes(), 0);
    }
}

#[tokio::test(start_paused = true)]
async fn timeout_releases_pending_registration() {
    let heads = Arc::new(HeldHeads::new(false));
    let budget = Arc::new(StreamBudget::new(8192));
    let (streams, input) = test_support::account_streams(
        heads.clone(),
        limiter(),
        StreamLimits {
            max_subscriptions: 1,
            ..StreamLimits::default()
        },
        budget.clone(),
    );
    let streams = Arc::new(streams);
    let started = streams.clone();
    let setup = tokio::spawn(async move {
        started
            .subscribe_channel_posts(&[channel(42)], &context(1))
            .await
    });
    heads.started.notified().await;
    input.publish(post(42, 104, "pending"));
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(matches!(
        setup.await.expect("setup join"),
        Err(SubscribeError::Operation(Error::Timeout { .. }))
    ));
    assert_eq!(budget.used_bytes(), 0);
    heads.release.add_permits(1);
    let subscription = streams
        .subscribe_channel_posts(&[channel(42)], &context(10))
        .await;
    assert!(subscription.is_ok());
}

#[tokio::test]
async fn a_reader_generation_closes_pending_setup() {
    let heads = Arc::new(HeldHeads::new(false));
    let budget = Arc::new(StreamBudget::new(1024));
    let (streams, input) = test_support::account_streams(
        heads.clone(),
        limiter(),
        StreamLimits::default(),
        budget.clone(),
    );
    let setup = tokio::spawn(async move {
        streams
            .subscribe_channel_posts(&[channel(42)], &context(10))
            .await
    });
    heads.started.notified().await;
    input.publish(post(42, 104, "pending"));
    input.interrupt(InterruptionReason::UpstreamDisconnected);
    assert!(matches!(
        setup.await.expect("setup join"),
        Err(SubscribeError::ReaderUnavailable)
    ));
    assert_eq!(budget.used_bytes(), 0);
}

#[tokio::test]
async fn one_full_subscriber_closes_while_the_other_continues() {
    let heads = Arc::new(HeldHeads::new(false));
    heads.release.add_permits(2);
    let budget = Arc::new(StreamBudget::new(8192));
    let (streams, input) = test_support::account_streams(
        heads,
        limiter(),
        StreamLimits {
            max_queue_items: 1,
            ..StreamLimits::default()
        },
        budget.clone(),
    );
    let mut held = streams
        .subscribe_channel_posts(&[channel(42)], &context(10))
        .await
        .expect("ready");
    let mut fast = streams
        .subscribe_channel_posts(&[channel(42)], &context(10))
        .await
        .expect("ready");
    let first_in_flight = held.next_queued().await.expect("ready");
    assert!(matches!(fast.next().await, Some(ChannelEvent::Ready(_))));
    input.publish(post(42, 104, "live"));
    assert_eq!(
        held.next().await,
        Some(ChannelEvent::Interrupted(InterruptionReason::SlowConsumer))
    );
    assert!(held.next().await.is_none());
    assert!(matches!(fast.next().await, Some(ChannelEvent::Post(_))));
    assert!(budget.used_bytes() > 0);
    drop(first_in_flight);
    assert_eq!(budget.used_bytes(), 0);
}

#[test]
fn native_batches_include_own_text_and_captionless_media() {
    use super::reader::{adapt_short_channel_send, decode_post};
    use crate::test_helpers::raw_tl_message_grouped;
    use grammers_client::tl::{self, Deserializable as _, Serializable as _};
    use grammers_session::updates::{MessageBoxes, UpdatesLike};

    let mut state = MessageBoxes::new();
    let request = tl::functions::messages::SendMessage {
        peer: tl::enums::InputPeer::Channel(tl::types::InputPeerChannel {
            channel_id: 42,
            access_hash: 73,
        }),
        message: "my text".into(),
        random_id: 881,
        no_webpage: false,
        silent: false,
        background: false,
        clear_draft: false,
        noforwards: false,
        update_stickersets_order: false,
        invert_media: false,
        allow_paid_floodskip: false,
        reply_to: None,
        reply_markup: None,
        entities: None,
        schedule_date: None,
        schedule_repeat_period: None,
        send_as: None,
        quick_reply_shortcut: None,
        effect: None,
        allow_paid_stars: None,
        suggested_post: None,
        rich_message: None,
    };
    let short = tl::types::UpdateShortSentMessage {
        out: true,
        id: 104,
        pts: 1,
        pts_count: 1,
        date: 1_795_078_800,
        media: None,
        entities: None,
        ttl_period: None,
    };
    let as_received = adapt_short_channel_send(UpdatesLike::ShortSentMessage {
        request,
        update: short,
    });
    let (updates, _, _) = state
        .process_updates(as_received)
        .expect("channel message is sequenced");
    let text = updates
        .into_iter()
        .find_map(|(update, _)| decode_post(update))
        .expect("own text post");
    assert_eq!(text, post(42, 104, "my text"));

    let tl::enums::Message::Message(mut media) =
        raw_tl_message_grouped(105, 1_795_078_800, 42, 908)
    else {
        panic!("ordinary fixture");
    };
    media.out = true;
    media.media = Some(tl::enums::MessageMedia::Dice(tl::types::MessageMediaDice {
        value: 4,
        emoticon: "🎲".into(),
        game_outcome: None,
    }));
    let wire = tl::enums::Updates::UpdateShort(tl::types::UpdateShort {
        date: 1_795_078_800,
        update: tl::types::UpdateNewChannelMessage {
            message: media.into(),
            pts: 2,
            pts_count: 1,
        }
        .into(),
    })
    .to_bytes();
    let batch = UpdatesLike::Updates(
        tl::enums::Updates::from_bytes(&wire).expect("serialized own RPC update"),
    );
    let (updates, _, _) = state.process_updates(batch).expect("second channel event");
    let media = updates
        .into_iter()
        .find_map(|(update, _)| decode_post(update))
        .expect("own media post");
    assert_eq!(media.message_id, message(105));
    assert_eq!(media.album_id, Some(908));
    assert!(media.text.is_empty());
    assert_eq!(
        media.media.expect("captionless media remains present").kind,
        crate::telegram::types::MediaType::Dice
    );
}

#[test]
fn native_adapter_excludes_other_update_classes() {
    use super::reader::decode_post;
    use crate::test_helpers::{raw_tl_message, raw_tl_message_grouped};
    use grammers_client::tl;

    let ordinary = raw_tl_message_grouped(104, 1_795_078_800, 42, 908);
    let tl::enums::Message::Message(mut group) = ordinary.clone() else {
        panic!("ordinary fixture");
    };
    group.post = false;
    let tl::enums::Message::Message(mut direct) = ordinary.clone() else {
        panic!("ordinary fixture");
    };
    direct.peer_id = tl::types::PeerUser { user_id: 42 }.into();
    for update in [
        tl::types::UpdateNewChannelMessage {
            message: group.into(),
            pts: 1,
            pts_count: 1,
        }
        .into(),
        tl::types::UpdateNewMessage {
            message: direct.into(),
            pts: 1,
            pts_count: 1,
        }
        .into(),
        tl::types::UpdateNewChannelMessage {
            message: raw_tl_message(104, 1_795_078_800, 42),
            pts: 1,
            pts_count: 1,
        }
        .into(),
        tl::types::UpdateEditChannelMessage {
            message: ordinary,
            pts: 1,
            pts_count: 1,
        }
        .into(),
        tl::types::UpdateDeleteChannelMessages {
            channel_id: 42,
            messages: vec![104],
            pts: 1,
            pts_count: 1,
        }
        .into(),
    ] {
        assert_eq!(decode_post(update), None);
    }
}

#[tokio::test]
async fn membership_loss_cancels_an_in_flight_writer_and_discards_queued_posts() {
    let heads = Arc::new(HeldHeads::new(false));
    heads.release.add_permits(2);
    let budget = Arc::new(StreamBudget::new(8192));
    let (streams, input) =
        test_support::account_streams(heads, limiter(), StreamLimits::default(), budget.clone());
    let mut affected = streams
        .subscribe_channel_posts(&[channel(42)], &context(10))
        .await
        .expect("ready");
    let mut other = streams
        .subscribe_channel_posts(&[channel(73)], &context(10))
        .await
        .expect("ready");
    let affected_writer = affected.membership_lost();
    let other_writer = other.membership_lost();
    assert!(matches!(
        affected.next().await,
        Some(ChannelEvent::Ready(_))
    ));
    assert!(matches!(other.next().await, Some(ChannelEvent::Ready(_))));
    input.publish(post(42, 104, "already in flight"));
    let in_flight = affected.next_queued().await.expect("in-flight post");
    input.publish(post(42, 105, "queued"));
    input.membership_lost(channel(42));
    assert!(affected_writer.is_cancelled());
    assert!(!other_writer.is_cancelled());
    drop(in_flight);
    assert_eq!(budget.used_bytes(), 0);
    assert_eq!(
        affected.next().await,
        Some(ChannelEvent::Interrupted(
            InterruptionReason::MembershipLost
        )),
    );
    assert!(affected.next().await.is_none());
    input.publish(post(73, 1, "still eligible"));
    assert!(matches!(other.next().await, Some(ChannelEvent::Post(_))));
}

#[tokio::test]
async fn shared_byte_budget_covers_events_in_flight_across_accounts() {
    let heads = Arc::new(HeldHeads::new(false));
    heads.release.add_permits(3);
    let budget = Arc::new(StreamBudget::new(128));
    let (first, _) = test_support::account_streams(
        heads.clone(),
        limiter(),
        StreamLimits::default(),
        budget.clone(),
    );
    let (second, _) =
        test_support::account_streams(heads, limiter(), StreamLimits::default(), budget.clone());
    let mut first_sub = first
        .subscribe_channel_posts(&[channel(42)], &context(10))
        .await
        .expect("first ready");
    let ready = first_sub.next_queued().await.expect("in-flight ready");
    drop(first_sub);
    assert!(matches!(
        second
            .subscribe_channel_posts(&[channel(42)], &context(10))
            .await,
        Err(SubscribeError::SetupOverflow)
    ));
    drop(ready);
    assert!(
        second
            .subscribe_channel_posts(&[channel(42)], &context(10))
            .await
            .is_ok()
    );
    assert_eq!(budget.used_bytes(), 0);
}

#[tokio::test]
async fn invalid_and_saturated_admission_spend_no_additional_tokens() {
    let heads = Arc::new(HeldHeads::new(false));
    heads.release.add_permits(1);
    let limiter = limiter();
    let (streams, _) = test_support::account_streams(
        heads.clone(),
        limiter.clone(),
        StreamLimits {
            max_subscriptions: 1,
            max_channels: 1,
            ..StreamLimits::default()
        },
        Arc::new(StreamBudget::default()),
    );
    for invalid in [&[][..], &[channel(42), channel(73)][..]] {
        assert!(matches!(
            streams.subscribe_channel_posts(invalid, &context(10)).await,
            Err(SubscribeError::InvalidChannels)
        ));
    }
    assert_eq!(limiter.available_tokens(), 10.0);
    let _subscription = streams
        .subscribe_channel_posts(&[channel(42)], &context(10))
        .await
        .expect("slot");
    assert!(matches!(
        streams
            .subscribe_channel_posts(&[channel(73)], &context(10))
            .await,
        Err(SubscribeError::Capacity)
    ));
    assert_eq!(limiter.available_tokens(), 9.0);
}
