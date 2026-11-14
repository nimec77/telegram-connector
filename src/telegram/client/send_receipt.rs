//! Correlate only this invocation's real message identity; never retain raw Updates.

use crate::telegram::types::sending::{
    ContentKind, QualifiedRecipient, SendFailure, SentMessage, SubmittedMedia,
};
use crate::telegram::types::{MessageId, PeerNamespace};
use grammers_client::tl;

pub(super) struct ExpectedSend {
    pub random_id: i64,
    pub recipient: QualifiedRecipient,
    pub saved_messages: bool,
    pub content_kind: ContentKind,
    pub reply_to_message_id: Option<MessageId>,
    pub topic_id: Option<MessageId>,
    pub message_link: Option<String>,
    pub media: Option<SubmittedMedia>,
}

pub(super) fn map_ack(
    updates: tl::enums::Updates,
    expected: &ExpectedSend,
) -> Result<SentMessage, SendFailure> {
    let (id, date) = match updates {
        tl::enums::Updates::UpdateShortSentMessage(short) => (short.id, short.date),
        tl::enums::Updates::Updates(full) => matched_message(&full.updates, expected)?,
        tl::enums::Updates::Combined(full) => matched_message(&full.updates, expected)?,
        _ => return Err(SendFailure::unknown()),
    };
    if id <= 0 || date <= 0 {
        return Err(SendFailure::unknown());
    }
    let date =
        chrono::DateTime::from_timestamp(i64::from(date), 0).ok_or_else(SendFailure::unknown)?;
    Ok(SentMessage {
        recipient: expected.recipient,
        saved_messages: expected.saved_messages,
        message_id: MessageId::new(i64::from(id)).map_err(|_| SendFailure::unknown())?,
        date,
        content_kind: expected.content_kind,
        reply_to_message_id: expected.reply_to_message_id,
        topic_id: expected.topic_id,
        message_link: expected.message_link.clone(),
        media: expected.media.clone(),
    })
}

fn matched_message(
    updates: &[tl::enums::Update],
    expected: &ExpectedSend,
) -> Result<(i32, i32), SendFailure> {
    let mut id = None;
    for update in updates {
        if let tl::enums::Update::MessageId(mapping) = update
            && mapping.random_id == expected.random_id
        {
            if mapping.id <= 0 || id.is_some_and(|id| id != mapping.id) {
                return Err(SendFailure::unknown());
            }
            id = Some(mapping.id);
        }
    }
    let id = id.ok_or_else(SendFailure::unknown)?;
    let mut date = None;
    for update in updates {
        let message = match update {
            tl::enums::Update::NewMessage(m) => &m.message,
            tl::enums::Update::NewChannelMessage(m) => &m.message,
            _ => continue,
        };
        let tl::enums::Message::Message(message) = message else {
            continue;
        };
        if message.id != id || peer_identity(&message.peer_id) != Some(expected.recipient) {
            continue;
        }
        if message.date <= 0 || date.is_some_and(|date| date != message.date) {
            return Err(SendFailure::unknown());
        }
        if !matches_options(message, expected) {
            return Err(SendFailure::unknown());
        }
        date = Some(message.date);
    }
    Ok((id, date.ok_or_else(SendFailure::unknown)?))
}

fn matches_options(message: &tl::types::Message, expected: &ExpectedSend) -> bool {
    let header = match &message.reply_to {
        None => None,
        Some(tl::enums::MessageReplyHeader::Header(h)) => Some(h),
        Some(_) => return false,
    };
    if header
        .and_then(|h| h.reply_to_peer_id.as_ref())
        .is_some_and(|peer| peer_identity(peer) != Some(expected.recipient))
    {
        return false;
    }
    let actual_reply = header.and_then(|h| h.reply_to_msg_id);
    if let Some(reply) = expected.reply_to_message_id {
        if actual_reply != reply.as_i32() {
            return false;
        }
    } else if actual_reply.is_some_and(|id| {
        expected
            .topic_id
            .and_then(|t| t.as_i32())
            .filter(|t| *t != 1)
            != Some(id)
    }) {
        return false;
    }
    let actual_topic = if let Some(h) = header.filter(|h| h.forum_topic) {
        let Some(topic) = h.reply_to_top_id.or(h.reply_to_msg_id).filter(|id| *id > 1) else {
            return false;
        };
        Some(topic)
    } else {
        None
    };
    match expected.topic_id {
        Some(topic) => actual_topic.or(Some(1)) == topic.as_i32(),
        None => actual_topic.is_none(),
    }
}

pub(super) fn peer_identity(peer: &tl::enums::Peer) -> Option<QualifiedRecipient> {
    let (namespace, id) = match peer {
        tl::enums::Peer::User(p) => (PeerNamespace::User, p.user_id),
        tl::enums::Peer::Chat(p) => (PeerNamespace::Chat, p.chat_id),
        tl::enums::Peer::Channel(p) => (PeerNamespace::Channel, p.channel_id),
    };
    QualifiedRecipient::try_new(namespace, id).ok()
}
