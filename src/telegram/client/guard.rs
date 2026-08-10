//! Deleted/missing message detection at the grammers boundary.
//!
//! Telegram reports deleted or never-existed ids in `GetMessages` responses as
//! `MessageEmpty` rather than omitting them; grammers wraps that variant in a
//! normal-looking `Message` (epoch date, empty text). Mapping it blindly
//! fabricates a message (work-order B1) — these helpers make the case explicit.

use super::*;

/// True when the raw TL message is the `MessageEmpty` placeholder.
pub(super) fn is_empty_variant(raw: &tl::enums::Message) -> bool {
    matches!(raw, tl::enums::Message::Empty(_))
}

/// The single fetched message, or a not-found error.
///
/// Both an absent slot and the `MessageEmpty` placeholder mean the id does not
/// exist in this channel (deleted, or never existed).
pub(super) fn require_found(
    fetched: Option<grammers_client::message::Message>,
    channel_ref: &str,
    message_id: i32,
) -> Result<grammers_client::message::Message, Error> {
    match fetched {
        Some(msg) if !is_empty_variant(&msg.raw) => Ok(msg),
        _ => {
            tracing::warn!(
                channel_ref = %channel_ref,
                message_id,
                "Message not found or deleted"
            );
            Err(Error::InvalidInput(format!(
                "Message {message_id} not found or deleted in channel {channel_ref}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_raw(id: i32) -> tl::enums::Message {
        tl::enums::Message::Empty(tl::types::MessageEmpty { id, peer_id: None })
    }

    /// The smallest constructible non-empty TL message (Service has 16 fields
    /// vs ~50 on Message; the guard only discriminates Empty vs not-Empty).
    fn service_raw(id: i32) -> tl::enums::Message {
        tl::enums::Message::Service(tl::types::MessageService {
            out: false,
            mentioned: false,
            media_unread: false,
            reactions_are_possible: false,
            silent: false,
            post: true,
            legacy: false,
            id,
            from_id: None,
            peer_id: tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 1 }),
            saved_peer_id: None,
            reply_to: None,
            date: 1_700_000_000,
            action: tl::enums::MessageAction::Empty,
            reactions: None,
            ttl_period: None,
        })
    }

    #[test]
    fn empty_variant_is_detected() {
        assert!(is_empty_variant(&empty_raw(609784)));
    }

    #[test]
    fn non_empty_variant_is_not_detected() {
        assert!(!is_empty_variant(&service_raw(610119)));
    }

    #[test]
    fn require_found_maps_absent_slot_to_not_found_error() {
        let result = require_found(None, "swodki", 999_999_999);

        let err = result.expect_err("absent slot must be an error");
        assert!(matches!(err, Error::InvalidInput(_)));
        assert_eq!(
            err.to_string(),
            "invalid input: Message 999999999 not found or deleted in channel swodki"
        );
    }
}
