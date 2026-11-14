use super::send_receipt::{ExpectedSend, map_ack};
use super::send_transport::{
    PreparedFinalSend, classify_final_error, random_id, submit_final_with,
};
use crate::telegram::sending::SendDeadline;
use crate::telegram::types::PeerNamespace;
use crate::telegram::types::sending::{
    ContentKind, FailureDeliveryState, QualifiedRecipient, SendFailureKind,
};
use grammers_client::tl::{self, Deserializable as _, Identifiable as _, Serializable as _};
use grammers_mtsender::{InvocationError, RpcError};
use std::sync::atomic::{AtomicUsize, Ordering};

fn expected() -> ExpectedSend {
    ExpectedSend {
        random_id: 739,
        recipient: QualifiedRecipient::try_new(PeerNamespace::Channel, 91).expect("recipient"),
        saved_messages: false,
        content_kind: ContentKind::Text,
        reply_to_message_id: None,
        topic_id: None,
        message_link: None,
        media: None,
    }
}

fn short(id: i32, date: i32) -> tl::enums::Updates {
    tl::types::UpdateShortSentMessage {
        out: true,
        id,
        date,
        pts: 1,
        pts_count: 1,
        media: None,
        entities: None,
        ttl_period: None,
    }
    .into()
}

fn message(id: i32, date: i32, namespace: PeerNamespace) -> tl::enums::Message {
    // Hand-assembled minimal real TL message: out flag, no optional fields.
    let mut bytes = Vec::new();
    tl::types::Message::CONSTRUCTOR_ID.serialize(&mut bytes);
    2_u32.serialize(&mut bytes);
    0_u32.serialize(&mut bytes);
    id.serialize(&mut bytes);
    let peer: tl::enums::Peer = match namespace {
        PeerNamespace::Channel => tl::types::PeerChannel { channel_id: 91 }.into(),
        PeerNamespace::Chat => tl::types::PeerChat { chat_id: 91 }.into(),
        PeerNamespace::User => tl::types::PeerUser { user_id: 91 }.into(),
    };
    peer.serialize(&mut bytes);
    date.serialize(&mut bytes);
    "private content must not be echoed"
        .to_string()
        .serialize(&mut bytes);
    tl::enums::Message::from_bytes(&bytes).expect("valid hand-built TL message")
}

fn mapping(random_id: i64, id: i32) -> tl::enums::Update {
    tl::types::UpdateMessageId { random_id, id }.into()
}

fn full(updates: Vec<tl::enums::Update>, combined: bool) -> tl::enums::Updates {
    if combined {
        tl::types::UpdatesCombined {
            updates,
            users: vec![],
            chats: vec![],
            date: 1_700_000_000,
            seq_start: 1,
            seq: 2,
        }
        .into()
    } else {
        tl::types::Updates {
            updates,
            users: vec![],
            chats: vec![],
            date: 1_700_000_000,
            seq: 1,
        }
        .into()
    }
}

fn new_message(id: i32, date: i32, namespace: PeerNamespace) -> tl::enums::Update {
    tl::types::UpdateNewChannelMessage {
        message: message(id, date, namespace),
        pts: 1,
        pts_count: 1,
    }
    .into()
}

#[test]
fn sending_transport_short_requires_real_positive_identity_and_date() {
    let receipt = map_ack(short(42, 1_700_000_000), &expected()).expect("sent");
    assert_eq!(receipt.message_id.get(), 42);
    assert_eq!(receipt.date.timestamp(), 1_700_000_000);
    assert_eq!(receipt.recipient.namespace(), PeerNamespace::Channel);
    for (id, date) in [(0, 1), (-1, 1), (42, 0), (42, -1)] {
        let error = map_ack(short(id, date), &expected()).expect_err("not an acknowledgement");
        assert_eq!(error.delivery_state(), FailureDeliveryState::Unknown);
        assert!(!error.retryable());
    }
}

#[test]
fn sending_transport_full_and_combined_correlate_random_id_and_namespace() {
    for combined in [false, true] {
        let receipt = map_ack(
            full(
                vec![
                    mapping(900, 19),
                    new_message(19, 30, PeerNamespace::Channel),
                    mapping(739, 42),
                    new_message(42, 1_700_000_000, PeerNamespace::Channel),
                ],
                combined,
            ),
            &expected(),
        )
        .expect("only correlated update");
        assert_eq!(receipt.message_id.get(), 42);
        for updates in [
            vec![mapping(900, 42), new_message(42, 1, PeerNamespace::Channel)],
            vec![mapping(739, 42), new_message(42, 1, PeerNamespace::User)],
            vec![mapping(739, 42), new_message(43, 1, PeerNamespace::Channel)],
            vec![mapping(739, 42), new_message(42, 0, PeerNamespace::Channel)],
            vec![mapping(739, 42)],
            vec![
                mapping(739, 42),
                mapping(739, 43),
                new_message(42, 1, PeerNamespace::Channel),
            ],
            vec![
                mapping(739, 42),
                new_message(42, 1, PeerNamespace::Channel),
                new_message(42, 2, PeerNamespace::Channel),
            ],
            vec![
                mapping(739, 42),
                tl::types::UpdateNewMessage {
                    message: tl::types::MessageEmpty {
                        id: 42,
                        peer_id: None,
                    }
                    .into(),
                    pts: 1,
                    pts_count: 1,
                }
                .into(),
            ],
        ] {
            let error = map_ack(full(updates, combined), &expected()).expect_err("unmatched");
            assert_eq!(error.kind(), SendFailureKind::DeliveryUnknown);
        }
    }
}

#[test]
fn sending_transport_only_allowlisted_provider_refusals_claim_not_sent() {
    for (code, name, value, kind, retry, delay) in [
        (
            403,
            "CHAT_WRITE_FORBIDDEN",
            None,
            SendFailureKind::SendForbidden,
            false,
            None,
        ),
        (
            400,
            "TOPIC_CLOSED",
            None,
            SendFailureKind::InvalidReplyTarget,
            false,
            None,
        ),
        (
            420,
            "FLOOD_WAIT",
            Some(27),
            SendFailureKind::RateLimited,
            true,
            Some(27),
        ),
        (
            420,
            "SLOWMODE_WAIT",
            Some(9),
            SendFailureKind::RateLimited,
            true,
            Some(9),
        ),
        (
            401,
            "AUTH_KEY_UNREGISTERED",
            None,
            SendFailureKind::Unauthenticated,
            false,
            None,
        ),
        (
            500,
            "CHAT_WRITE_FORBIDDEN",
            None,
            SendFailureKind::DeliveryUnknown,
            false,
            None,
        ),
        (
            400,
            "SECRET_PROVIDER_PAYLOAD",
            None,
            SendFailureKind::DeliveryUnknown,
            false,
            None,
        ),
    ] {
        let error = classify_final_error(InvocationError::Rpc(RpcError {
            code,
            name: name.into(),
            value,
            caused_by: None,
        }));
        assert_eq!(error.kind(), kind);
        assert_eq!(error.retryable(), retry);
        assert_eq!(error.retry_after_seconds(), delay);
        assert!(!format!("{error:?} {error}").contains(name));
        assert!(std::error::Error::source(&error).is_none());
    }
    assert_eq!(
        classify_final_error(InvocationError::Dropped("secret")).kind(),
        SendFailureKind::DeliveryUnknown
    );
}

#[tokio::test(start_paused = true)]
async fn sending_transport_single_submit() {
    let count = AtomicUsize::new(0);
    let receipt = submit_final_with(
        PreparedFinalSend {
            body: vec![7, 3, 9],
            expected: expected(),
        },
        SendDeadline::new(30).expect("deadline"),
        |body| {
            assert_eq!(body, [7, 3, 9]);
            count.fetch_add(1, Ordering::SeqCst);
            async { Ok(short(42, 1_700_000_000).to_bytes()) }
        },
    )
    .await
    .expect("sent");
    assert_eq!(receipt.message_id.get(), 42);
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn sending_transport_expired_before_dispatch() {
    let deadline = SendDeadline::new(30).expect("deadline");
    tokio::time::advance(std::time::Duration::from_secs(30)).await;
    let error = submit_final_with(
        PreparedFinalSend {
            body: vec![],
            expected: expected(),
        },
        deadline,
        |_| async { panic!("expired operation must not dispatch") },
    )
    .await
    .expect_err("expired");
    assert_eq!(error.delivery_state(), FailureDeliveryState::NotSent);
    assert!(error.retryable());
}

#[tokio::test(start_paused = true)]
async fn sending_transport_timeout_after_dispatch() {
    let error = submit_final_with(
        PreparedFinalSend {
            body: vec![1],
            expected: expected(),
        },
        SendDeadline::new(30).expect("deadline"),
        |_| std::future::pending(),
    )
    .await
    .expect_err("unknown");
    assert_eq!(error.delivery_state(), FailureDeliveryState::Unknown);
    assert!(!error.retryable());
}

#[tokio::test(start_paused = true)]
async fn sending_transport_public_api_compiles() {
    let (client, runner, _dir) = super::stalled_client().await;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        client.send_adapter.submit(2, vec![1, 2, 3, 4]),
    )
    .await;
    assert!(
        result.is_err(),
        "retained unpolled runner performs no network I/O"
    );
    drop(runner);
}

#[test]
fn sending_transport_generates_nonzero_random_ids() {
    for _ in 0..32 {
        assert_ne!(random_id().expect("OS entropy"), 0);
    }
}

#[test]
fn sending_protocol_full_ack_must_match_requested_reply_and_forum_thread() {
    use crate::telegram::types::MessageId;
    for (wanted_topic, wanted_reply, actual_topic, actual_reply, valid) in [
        (Some(50), Some(60), Some(50), Some(60), true),
        (Some(50), Some(60), Some(51), Some(60), false),
        (Some(50), Some(60), Some(50), Some(61), false),
        (Some(1), Some(60), None, Some(60), true),
        (Some(1), Some(60), Some(50), Some(60), false),
        (Some(50), None, Some(50), Some(50), true),
        (None, None, Some(50), Some(50), false),
        (None, None, None, Some(60), false),
        (Some(1), None, None, Some(60), false),
        (Some(50), None, Some(50), Some(60), false),
    ] {
        let mut expected = expected();
        expected.topic_id = wanted_topic.map(|id| MessageId::new(id).expect("topic"));
        expected.reply_to_message_id = wanted_reply.map(|id| MessageId::new(id).expect("reply"));
        let mut raw = message(42, 1_700_000_000, PeerNamespace::Channel);
        if let tl::enums::Message::Message(m) = &mut raw {
            m.reply_to = Some(
                tl::types::MessageReplyHeader {
                    reply_to_scheduled: false,
                    forum_topic: actual_topic.is_some(),
                    quote: false,
                    reply_to_ephemeral: false,
                    reply_to_msg_id: actual_reply,
                    reply_to_peer_id: None,
                    reply_from: None,
                    reply_media: None,
                    reply_to_top_id: actual_topic,
                    quote_text: None,
                    quote_entities: None,
                    quote_offset: None,
                    todo_item_id: None,
                    poll_option: None,
                    quote_rich: None,
                }
                .into(),
            );
        }
        let result = map_ack(
            full(
                vec![
                    mapping(739, 42),
                    tl::types::UpdateNewChannelMessage {
                        message: raw,
                        pts: 1,
                        pts_count: 1,
                    }
                    .into(),
                ],
                false,
            ),
            &expected,
        );
        assert_eq!(
            result.is_ok(),
            valid,
            "requested {wanted_topic:?}/{wanted_reply:?}, returned {actual_topic:?}/{actual_reply:?}"
        );
        if !valid {
            assert_eq!(
                result.expect_err("mismatch").kind(),
                SendFailureKind::DeliveryUnknown
            );
        }
    }
}

#[test]
fn sending_protocol_incomplete_forum_header_cannot_be_inferred_as_general() {
    use crate::telegram::types::MessageId;
    let mut expected = expected();
    expected.topic_id = Some(MessageId::new(1).expect("General"));
    let mut raw = message(42, 1_700_000_000, PeerNamespace::Channel);
    if let tl::enums::Message::Message(m) = &mut raw {
        m.reply_to = Some(
            tl::types::MessageReplyHeader {
                reply_to_scheduled: false,
                forum_topic: true,
                quote: false,
                reply_to_ephemeral: false,
                reply_to_msg_id: None,
                reply_to_peer_id: None,
                reply_from: None,
                reply_media: None,
                reply_to_top_id: None,
                quote_text: None,
                quote_entities: None,
                quote_offset: None,
                todo_item_id: None,
                poll_option: None,
                quote_rich: None,
            }
            .into(),
        );
    }
    assert_eq!(
        map_ack(
            full(
                vec![
                    mapping(739, 42),
                    tl::types::UpdateNewChannelMessage {
                        message: raw,
                        pts: 1,
                        pts_count: 1
                    }
                    .into()
                ],
                false
            ),
            &expected
        )
        .expect_err("thread not established")
        .kind(),
        SendFailureKind::DeliveryUnknown
    );
    assert_eq!(
        map_ack(short(42, 1_700_000_000), &expected)
            .expect("direct short ack is allowed by contract")
            .message_id
            .get(),
        42
    );
}

#[tokio::test(start_paused = true)]
async fn sending_transport_expiry_prevents_every_predispatch_continuation() {
    use crate::telegram::sending::SendStage;
    let deadline = SendDeadline::new(1).expect("deadline");
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    for stage in [
        SendStage::LocalPreparation,
        SendStage::Resolve,
        SendStage::ValidateTargets,
        SendStage::Upload,
    ] {
        let polled = std::cell::Cell::new(false);
        let error = deadline
            .run(stage, async {
                polled.set(true);
                Ok(())
            })
            .await
            .expect_err("expired");
        assert!(!polled.get());
        assert_eq!(error.kind(), SendFailureKind::Timeout);
        assert_eq!(error.delivery_state(), FailureDeliveryState::NotSent);
    }
}
