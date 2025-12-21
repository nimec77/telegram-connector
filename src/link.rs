use crate::telegram::types::{ChannelId, MessageId};
use serde::{Deserialize, Serialize};

/// Generated deep links for a Telegram message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageLink {
    pub channel_id: ChannelId,
    pub message_id: MessageId,
    pub https_link: String,
    pub tg_protocol_link: String,
}

impl MessageLink {
    /// Create links for a specific message in a channel
    pub fn new(channel_id: ChannelId, message_id: MessageId) -> Self {
        let https_link = format!("https://t.me/c/{}/{}?single", channel_id, message_id);
        let tg_protocol_link = format!(
            "tg://resolve?channel={}&post={}&single",
            channel_id, message_id
        );

        Self {
            channel_id,
            message_id,
            https_link,
            tg_protocol_link,
        }
    }
}

// =============================================================================
// Tests (TDD - written first)
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_link_https_format() {
        let channel_id = ChannelId::new(123456789).unwrap();
        let message_id = MessageId::new(42).unwrap();
        let link = MessageLink::new(channel_id, message_id);

        assert_eq!(link.https_link, "https://t.me/c/123456789/42?single");
    }

    #[test]
    fn message_link_tg_protocol_format() {
        let channel_id = ChannelId::new(123456789).unwrap();
        let message_id = MessageId::new(42).unwrap();
        let link = MessageLink::new(channel_id, message_id);

        assert_eq!(
            link.tg_protocol_link,
            "tg://resolve?channel=123456789&post=42&single"
        );
    }

    #[test]
    fn message_link_stores_ids() {
        let channel_id = ChannelId::new(999).unwrap();
        let message_id = MessageId::new(111).unwrap();
        let link = MessageLink::new(channel_id, message_id);

        assert_eq!(link.channel_id, channel_id);
        assert_eq!(link.message_id, message_id);
    }

    #[test]
    fn message_link_serialization() {
        let link = MessageLink::new(ChannelId::new(100).unwrap(), MessageId::new(200).unwrap());

        let json = serde_json::to_string(&link).unwrap();
        let deserialized: MessageLink = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.https_link, link.https_link);
        assert_eq!(deserialized.tg_protocol_link, link.tg_protocol_link);
    }

    #[test]
    fn message_link_different_ids() {
        let link1 = MessageLink::new(ChannelId::new(100).unwrap(), MessageId::new(1).unwrap());
        let link2 = MessageLink::new(ChannelId::new(200).unwrap(), MessageId::new(2).unwrap());

        assert_eq!(link1.https_link, "https://t.me/c/100/1?single");
        assert_eq!(link2.https_link, "https://t.me/c/200/2?single");
        assert_ne!(link1.https_link, link2.https_link);
    }
}
