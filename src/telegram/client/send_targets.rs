//! Validate exact reply/forum targets before any upload; never change destination/thread.

use super::TelegramClient;
use super::resolve_recipient::{ResolvedRecipient, read_failure};
use super::send_receipt::peer_identity;
use crate::telegram::sending::{SendDeadline, SendStage};
use crate::telegram::types::sending::{SendFailure, SendFailureKind, SendOptions};
use grammers_client::tl;
use std::future::Future;

#[derive(Debug)]
pub(super) struct ValidatedTargets {
    pub reply_to: Option<tl::enums::InputReplyTo>,
}

fn invalid_target() -> SendFailure {
    SendFailure::not_sent(SendFailureKind::InvalidReplyTarget, None)
}

fn check_topic(
    topics: &[tl::enums::ForumTopic],
    id: i32,
    recipient: &ResolvedRecipient,
) -> Result<(), SendFailure> {
    let mut found = false;
    for topic in topics {
        match topic {
            tl::enums::ForumTopic::Topic(topic) if topic.id == id => {
                if found
                    || topic.short
                    || topic.closed
                    || topic.hidden
                    || peer_identity(&topic.peer) != Some(recipient.identity)
                {
                    return Err(invalid_target());
                }
                found = true;
            }
            tl::enums::ForumTopic::Deleted(topic) if topic.id == id => return Err(invalid_target()),
            _ => {}
        }
    }
    if !found {
        return Err(invalid_target());
    }
    Ok(())
}

fn check_reply(
    message: Option<&tl::enums::Message>,
    id: i32,
    topic: Option<i32>,
    recipient: &ResolvedRecipient,
) -> Result<(), SendFailure> {
    let (message_id, peer, reply) = match message {
        Some(tl::enums::Message::Message(m)) => (m.id, &m.peer_id, m.reply_to.as_ref()),
        Some(tl::enums::Message::Service(m)) => (m.id, &m.peer_id, m.reply_to.as_ref()),
        _ => return Err(invalid_target()),
    };
    if message_id != id || peer_identity(peer) != Some(recipient.identity) {
        return Err(invalid_target());
    }
    if let Some(topic) = topic {
        // General (1) has no message root. Non-General topics are threads of
        // their creation service message; a direct root reply belongs to it.
        let actual = match reply {
            Some(tl::enums::MessageReplyHeader::Header(header)) if header.forum_topic => {
                if header
                    .reply_to_peer_id
                    .as_ref()
                    .is_some_and(|peer| peer_identity(peer) != Some(recipient.identity))
                {
                    return Err(invalid_target());
                }
                header
                    .reply_to_top_id
                    .or(header.reply_to_msg_id)
                    .ok_or_else(invalid_target)?
            }
            _ if id == topic && topic != 1 => topic,
            _ => 1,
        };
        if actual != topic {
            return Err(invalid_target());
        }
    }
    Ok(())
}

pub(super) async fn validate_targets_with<T, TF, R, RF>(
    recipient: &ResolvedRecipient,
    options: &SendOptions,
    deadline: SendDeadline,
    topic_lookup: T,
    reply_lookup: R,
) -> Result<ValidatedTargets, SendFailure>
where
    T: FnOnce(i32) -> TF,
    TF: Future<Output = Result<Vec<tl::enums::ForumTopic>, SendFailure>>,
    R: FnOnce(i32) -> RF,
    RF: Future<Output = Result<Option<tl::enums::Message>, SendFailure>>,
{
    options.validate()?;
    if recipient.forum != options.topic_id.is_some() {
        return Err(invalid_target());
    }
    let topic = options.topic_id.and_then(|id| id.as_i32());
    let reply = options.reply_to_message_id.and_then(|id| id.as_i32());
    if let Some(id) = topic {
        let topics = deadline
            .run(SendStage::ValidateTargets, async { topic_lookup(id).await })
            .await?;
        check_topic(&topics, id, recipient)?;
    }
    if let Some(id) = reply {
        let message = deadline
            .run(SendStage::ValidateTargets, async { reply_lookup(id).await })
            .await?;
        check_reply(message.as_ref(), id, topic, recipient)?;
    }
    // Telegram's General-topic semantics deliberately leave top_msg_id unset.
    // https://core.telegram.org/api/forum#forum-topics
    let reply_to = reply.or(topic.filter(|id| *id != 1)).map(|reply_id| {
        tl::types::InputReplyToMessage {
            reply_to_msg_id: reply_id,
            top_msg_id: topic.filter(|id| *id != 1 && *id != reply_id),
            reply_to_peer_id: None,
            quote_text: None,
            quote_entities: None,
            quote_offset: None,
            monoforum_peer_id: None,
            todo_item_id: None,
            poll_option: None,
            quote_rich: None,
        }
        .into()
    });
    Ok(ValidatedTargets { reply_to })
}

impl TelegramClient {
    pub(super) async fn validate_send_targets(
        &self,
        recipient: &ResolvedRecipient,
        options: &SendOptions,
        deadline: SendDeadline,
    ) -> Result<ValidatedTargets, SendFailure> {
        validate_targets_with(
            recipient,
            options,
            deadline,
            |id| async move {
                deadline
                    .run_capped(
                        SendStage::ValidateTargets,
                        self.timeouts.resolve_secs,
                        async {
                            let tl::enums::messages::ForumTopics::Topics(topics) = self
                                .client
                                .invoke(&tl::functions::messages::GetForumTopicsById {
                                    peer: recipient.reference.into(),
                                    topics: vec![id],
                                })
                                .await
                                .map_err(|e| {
                                    read_failure(e, SendFailureKind::InvalidReplyTarget)
                                })?;
                            Ok(topics.topics)
                        },
                    )
                    .await
            },
            |id| async move {
                deadline
                    .run_capped(
                        SendStage::ValidateTargets,
                        self.timeouts.history_secs,
                        async {
                            let (mut messages, _) = super::raw_fetch::fetch_messages_by_id(
                                &self.client,
                                recipient.reference,
                                &[id],
                            )
                            .await
                            .map_err(|e| read_failure(e, SendFailureKind::InvalidReplyTarget))?;
                            Ok(messages.remove(&id))
                        },
                    )
                    .await
            },
        )
        .await
    }
}
