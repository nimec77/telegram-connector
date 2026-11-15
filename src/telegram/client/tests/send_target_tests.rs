use super::resolve_recipient::from_peer;
use super::send_targets::validate_targets_with;
use crate::telegram::sending::SendDeadline;
use crate::telegram::types::MessageId;
use crate::telegram::types::sending::{RecipientSpec, SendFailureKind, SendOptions};
use crate::test_helpers::{raw_tl_channel, raw_tl_message};
use grammers_client::peer::Peer;
use grammers_client::tl;

fn options(topic: Option<i64>, reply: Option<i64>) -> SendOptions {
    SendOptions {
        recipient: RecipientSpec::SavedMessages,
        silent: false,
        topic_id: topic.map(|id| MessageId::new(id).expect("topic")),
        reply_to_message_id: reply.map(|id| MessageId::new(id).expect("reply")),
    }
}
fn topic(id: i32, closed: bool, hidden: bool) -> tl::enums::ForumTopic {
    tl::types::ForumTopic {
        my: false,
        closed,
        pinned: false,
        short: false,
        hidden,
        title_missing: false,
        id,
        date: 1,
        peer: tl::types::PeerChannel { channel_id: 91 }.into(),
        title: "private topic".into(),
        icon_color: 0,
        icon_emoji_id: None,
        top_message: 100,
        read_inbox_max_id: 0,
        read_outbox_max_id: 0,
        unread_count: 0,
        unread_mentions_count: 0,
        unread_reactions_count: 0,
        unread_poll_votes_count: 0,
        from_id: tl::types::PeerUser { user_id: 1 }.into(),
        notify_settings: tl::types::PeerNotifySettings {
            show_previews: None,
            silent: None,
            mute_until: None,
            ios_sound: None,
            android_sound: None,
            other_sound: None,
            stories_muted: None,
            stories_hide_sender: None,
            stories_ios_sound: None,
            stories_android_sound: None,
            stories_other_sound: None,
        }
        .into(),
        draft: None,
    }
    .into()
}
fn reply(id: i32, root: Option<i32>, channel: i64) -> tl::enums::Message {
    let mut message = raw_tl_message(id, 1_700_000_000, channel);
    if let tl::enums::Message::Service(m) = &mut message {
        m.reply_to = root.map(|root| {
            tl::types::MessageReplyHeader {
                reply_to_scheduled: false,
                forum_topic: true,
                quote: false,
                reply_to_ephemeral: false,
                reply_to_msg_id: Some(root),
                reply_to_peer_id: None,
                reply_from: None,
                reply_media: None,
                reply_to_top_id: Some(root),
                quote_text: None,
                quote_entities: None,
                quote_offset: None,
                todo_item_id: None,
                poll_option: None,
                quote_rich: None,
            }
            .into()
        });
    }
    message
}

#[tokio::test]
async fn sending_targets_general_uses_topic_state_without_fetching_message_one() {
    let (client, _runner, _dir) = super::stalled_client().await;
    let mut raw = raw_tl_channel(91, "private", None);
    raw.date = 1_700_000_000;
    raw.broadcast = false;
    raw.megagroup = true;
    raw.forum = true;
    let recipient = from_peer(&Peer::from_raw(client.client(), raw.into()))
        .await
        .expect("forum");
    let reply_called = std::cell::Cell::new(false);
    let targets = validate_targets_with(
        &recipient,
        &options(Some(1), None),
        SendDeadline::new(30).expect("budget"),
        &client.timeouts,
        |id| async move {
            assert_eq!(id, 1);
            Ok(vec![topic(1, false, false)])
        },
        |_| async {
            reply_called.set(true);
            Ok(None)
        },
    )
    .await
    .expect("General");
    assert!(!reply_called.get(), "General root is not a message lookup");
    assert!(targets.reply_to.is_none());
    for (closed, hidden) in [(true, false), (false, true)] {
        let error = validate_targets_with(
            &recipient,
            &options(Some(1), None),
            SendDeadline::new(30).expect("budget"),
            &client.timeouts,
            |_| async move { Ok(vec![topic(1, closed, hidden)]) },
            |_| async { Ok(None) },
        )
        .await
        .expect_err("unavailable General");
        assert_eq!(error.kind(), SendFailureKind::InvalidReplyTarget);
    }
}

#[tokio::test]
async fn sending_targets_non_general_reply_preserves_top_thread_and_refuses_cross_topic() {
    let (client, _runner, _dir) = super::stalled_client().await;
    let mut raw = raw_tl_channel(91, "private", None);
    raw.date = 1_700_000_000;
    raw.broadcast = false;
    raw.megagroup = true;
    raw.forum = true;
    let recipient = from_peer(&Peer::from_raw(client.client(), raw.into()))
        .await
        .expect("forum");
    for (reply_root, channel, valid) in [(50, 91, true), (51, 91, false), (50, 92, false)] {
        let result = validate_targets_with(
            &recipient,
            &options(Some(50), Some(60)),
            SendDeadline::new(30).expect("budget"),
            &client.timeouts,
            |_| async { Ok(vec![topic(50, false, false)]) },
            |_| async move { Ok(Some(reply(60, Some(reply_root), channel))) },
        )
        .await;
        if valid {
            let tl::enums::InputReplyTo::Message(reply) =
                result.expect("same topic").reply_to.expect("reply")
            else {
                panic!("message routing")
            };
            assert_eq!(reply.reply_to_msg_id, 60);
            assert_eq!(reply.top_msg_id, Some(50));
        } else {
            assert_eq!(
                result.expect_err("wrong destination/thread").kind(),
                SendFailureKind::InvalidReplyTarget
            );
        }
    }
}

#[tokio::test]
async fn sending_targets_forum_shape_and_missing_targets_never_fall_back() {
    let (client, _runner, _dir) = super::stalled_client().await;
    for forum in [false, true] {
        let mut raw = raw_tl_channel(91, "private", None);
        raw.date = 1_700_000_000;
        raw.forum = forum;
        let recipient = from_peer(&Peer::from_raw(client.client(), raw.into()))
            .await
            .expect("recipient");
        let options = options(if forum { None } else { Some(50) }, None);
        let error = validate_targets_with(
            &recipient,
            &options,
            SendDeadline::new(30).expect("budget"),
            &client.timeouts,
            |_| async {
                Err(grammers_mtsender::InvocationError::Dropped(
                    "unexpected lookup",
                ))
            },
            |_| async {
                Err(grammers_mtsender::InvocationError::Dropped(
                    "unexpected lookup",
                ))
            },
        )
        .await
        .expect_err("shape before RPC");
        assert_eq!(error.kind(), SendFailureKind::InvalidReplyTarget);
    }
}

#[tokio::test]
async fn sending_targets_missing_deleted_short_or_wrong_peer_topic_prevents_reply_lookup() {
    let (client, _runner, _dir) = super::stalled_client().await;
    let mut raw = raw_tl_channel(91, "private", None);
    raw.date = 1_700_000_000;
    raw.forum = true;
    let recipient = from_peer(&Peer::from_raw(client.client(), raw.into()))
        .await
        .expect("forum");
    let mut short = topic(50, false, false);
    if let tl::enums::ForumTopic::Topic(t) = &mut short {
        t.short = true;
    }
    let mut other_peer = topic(50, false, false);
    if let tl::enums::ForumTopic::Topic(t) = &mut other_peer {
        t.peer = tl::types::PeerChannel { channel_id: 92 }.into();
    }
    for topics in [
        vec![],
        vec![tl::types::ForumTopicDeleted { id: 50 }.into()],
        vec![short],
        vec![other_peer],
    ] {
        let called = std::cell::Cell::new(false);
        let error = validate_targets_with(
            &recipient,
            &options(Some(50), Some(60)),
            SendDeadline::new(30).expect("budget"),
            &client.timeouts,
            |_| async move { Ok(topics) },
            |_| async {
                called.set(true);
                Ok(None)
            },
        )
        .await
        .expect_err("invalid topic");
        assert_eq!(error.kind(), SendFailureKind::InvalidReplyTarget);
        assert!(!called.get());
    }
}

#[tokio::test]
async fn sending_targets_topic_root_and_general_reply_use_exact_telegram_routing() {
    let (client, _runner, _dir) = super::stalled_client().await;
    let mut raw = raw_tl_channel(91, "private", None);
    raw.date = 1_700_000_000;
    raw.forum = true;
    let recipient = from_peer(&Peer::from_raw(client.client(), raw.into()))
        .await
        .expect("forum");
    for (topic_id, reply_id, want_reply) in [(50, None, 50), (50, Some(50), 50), (1, Some(60), 60)]
    {
        let result = validate_targets_with(
            &recipient,
            &options(Some(topic_id), reply_id),
            SendDeadline::new(30).expect("budget"),
            &client.timeouts,
            |id| async move { Ok(vec![topic(id, false, false)]) },
            |id| async move { Ok(Some(reply(id, None, 91))) },
        )
        .await
        .expect("exact routing");
        let tl::enums::InputReplyTo::Message(r) = result.reply_to.expect("message routing") else {
            panic!("message")
        };
        assert_eq!(r.reply_to_msg_id, want_reply);
        assert_eq!(r.top_msg_id, None);
    }
}

#[tokio::test(start_paused = true)]
async fn sending_targets_real_reads_obey_stage_and_operation_caps() {
    let (client, _runner, _dir) = super::stalled_client().await;
    for forum in [false, true] {
        let mut raw = raw_tl_channel(91, "private", None);
        raw.date = 1_700_000_000;
        raw.forum = forum;
        let recipient = from_peer(&Peer::from_raw(client.client(), raw.into()))
            .await
            .expect("recipient");
        let cap = if forum {
            client.timeouts.resolve_secs
        } else {
            client.timeouts.history_secs
        };
        for total in [1, 30] {
            let start = tokio::time::Instant::now();
            let failure = client
                .validate_send_targets(
                    &recipient,
                    &options(forum.then_some(50), (!forum).then_some(60)),
                    SendDeadline::new(total).expect("budget"),
                )
                .await
                .expect_err("stalled read");
            assert_eq!(failure.kind(), SendFailureKind::Timeout);
            assert_eq!(
                (tokio::time::Instant::now() - start).as_secs(),
                total.min(cap)
            );
        }
    }
}

#[tokio::test]
async fn sending_targets_raw_reply_and_topic_read_failures_preserve_unavailable() {
    use crate::telegram::types::sending::FailureDeliveryState;
    let (client, _runner, _dir) = super::stalled_client().await;
    for is_topic in [false, true] {
        let mut raw = raw_tl_channel(91, "private", None);
        raw.date = 1_700_000_000;
        raw.forum = is_topic;
        let recipient = from_peer(&Peer::from_raw(client.client(), raw.into()))
            .await
            .expect("recipient");
        for error in super::send_transport_tests::uncertain_read_errors() {
            let (topic_error, reply_error) = if is_topic {
                (Some(error), None)
            } else {
                (None, Some(error))
            };
            let failure = validate_targets_with(
                &recipient,
                &options(is_topic.then_some(50), (!is_topic).then_some(60)),
                SendDeadline::new(30).expect("budget"),
                &client.timeouts,
                |_| {
                    let error = topic_error;
                    async move { Err(error.expect("topic error")) }
                },
                |_| {
                    let error = reply_error;
                    async move { Err(error.expect("reply error")) }
                },
            )
            .await
            .expect_err("read failure");
            assert_eq!(failure.kind(), SendFailureKind::Unavailable);
            assert_eq!(failure.delivery_state(), FailureDeliveryState::NotSent);
            assert!(!failure.retryable());
            assert!(!format!("{failure:?} {failure}").contains("secret"));
            assert!(std::error::Error::source(&failure).is_none());
        }
    }
}

#[tokio::test(start_paused = true)]
async fn sending_targets_unused_and_expired_reads_are_never_polled() {
    use crate::telegram::types::sending::FailureDeliveryState;
    let (client, _runner, _dir) = super::stalled_client().await;
    for forum in [false, true] {
        let mut raw = raw_tl_channel(91, "private", None);
        raw.date = 1_700_000_000;
        raw.forum = forum;
        let recipient = from_peer(&Peer::from_raw(client.client(), raw.into()))
            .await
            .expect("recipient");
        let result = validate_targets_with(
            &recipient,
            &options(forum.then_some(50), None),
            SendDeadline::new(30).expect("budget"),
            &client.timeouts,
            |_| async {
                assert!(forum, "unused topic lookup");
                Ok(vec![topic(50, false, false)])
            },
            |_| async { panic!("unused reply lookup") },
        )
        .await
        .expect("unused reads stay unpolled");
        assert_eq!(result.reply_to.is_some(), forum);
    }
    let mut raw = raw_tl_channel(91, "private", None);
    raw.date = 1_700_000_000;
    raw.forum = true;
    let recipient = from_peer(&Peer::from_raw(client.client(), raw.into()))
        .await
        .expect("forum");
    for already_expired in [false, true] {
        let deadline = SendDeadline::new(1).expect("budget");
        if already_expired {
            tokio::time::advance(std::time::Duration::from_secs(1)).await;
        }
        let error = validate_targets_with(
            &recipient,
            &options(Some(50), Some(60)),
            deadline,
            &client.timeouts,
            |_| async {
                assert!(
                    !already_expired,
                    "expired operation must not poll topic lookup"
                );
                tokio::time::advance(std::time::Duration::from_secs(1)).await;
                Ok(vec![topic(50, false, false)])
            },
            |_| async { panic!("expired operation must not poll subsequent reply lookup") },
        )
        .await
        .expect_err("shared operation deadline");
        assert_eq!(error.kind(), SendFailureKind::Timeout);
        assert_eq!(error.delivery_state(), FailureDeliveryState::NotSent);
        assert!(error.retryable());
    }
}

#[tokio::test]
async fn sending_targets_successful_absence_remains_invalid() {
    let (client, _runner, _dir) = super::stalled_client().await;
    for forum in [false, true] {
        let mut raw = raw_tl_channel(91, "private", None);
        raw.date = 1_700_000_000;
        raw.forum = forum;
        let recipient = from_peer(&Peer::from_raw(client.client(), raw.into()))
            .await
            .expect("recipient");
        let failure = validate_targets_with(
            &recipient,
            &options(forum.then_some(50), (!forum).then_some(60)),
            SendDeadline::new(30).expect("budget"),
            &client.timeouts,
            |_| async { Ok(vec![]) },
            |_| async { Ok(None) },
        )
        .await
        .expect_err("authoritative absence");
        assert_eq!(failure.kind(), SendFailureKind::InvalidReplyTarget);
        assert!(!failure.retryable());
    }
}
