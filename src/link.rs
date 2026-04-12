use crate::error::Error;
use crate::telegram::types::{ChannelId, MessageId};
use serde::{Deserialize, Serialize};

/// Reference to a channel — either by username or numeric ID.
#[derive(Debug, Clone, PartialEq)]
pub enum ChannelRef {
    Username(String),
    Id(ChannelId),
}

/// Parse a Telegram message link into channel reference and message ID.
///
/// Supported formats:
/// - `https://t.me/username/12345`
/// - `https://t.me/c/12345/67890` (private channel)
/// - `http://t.me/...`
/// - `t.me/...` (no scheme)
///
/// Query parameters (e.g. `?single`) and trailing slashes are stripped.
pub fn parse_telegram_link(link: &str) -> Result<(ChannelRef, MessageId), Error> {
    let path = link
        .strip_prefix("https://")
        .or_else(|| link.strip_prefix("http://"))
        .unwrap_or(link);

    let path = path
        .strip_prefix("t.me/")
        .ok_or_else(|| Error::InvalidInput(format!("Not a valid t.me link: {}", link)))?;

    let path = path
        .split('?')
        .next()
        .expect("split always yields at least one element");
    let path = path.trim_end_matches('/');

    let segments: Vec<&str> = path.split('/').collect();

    match segments.as_slice() {
        ["c", channel_id_str, message_id_str] => {
            let channel_id: i64 = channel_id_str.parse().map_err(|_| {
                Error::InvalidInput(format!("Invalid channel ID in link: {}", channel_id_str))
            })?;
            let message_id: i64 = message_id_str.parse().map_err(|_| {
                Error::InvalidInput(format!("Invalid message ID in link: {}", message_id_str))
            })?;
            Ok((
                ChannelRef::Id(ChannelId::new(channel_id)?),
                MessageId::new(message_id)?,
            ))
        }
        [username, message_id_str] if *username != "c" => {
            let message_id: i64 = message_id_str.parse().map_err(|_| {
                Error::InvalidInput(format!("Invalid message ID in link: {}", message_id_str))
            })?;
            Ok((
                ChannelRef::Username(username.to_string()),
                MessageId::new(message_id)?,
            ))
        }
        _ => Err(Error::InvalidInput(format!(
            "Invalid t.me link format: {}. \
             Expected t.me/username/message_id or t.me/c/channel_id/message_id",
            link
        ))),
    }
}

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
            "tg://privatepost?channel={}&post={}&single",
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
            "tg://privatepost?channel=123456789&post=42&single"
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

    // =========================================================================
    // parse_telegram_link Tests
    // =========================================================================

    #[test]
    fn parse_public_link_https() {
        let (channel_ref, msg_id) = parse_telegram_link("https://t.me/swodki/575403").unwrap();
        assert_eq!(channel_ref, ChannelRef::Username("swodki".to_string()));
        assert_eq!(msg_id.get(), 575403);
    }

    #[test]
    fn parse_public_link_http() {
        let (channel_ref, msg_id) = parse_telegram_link("http://t.me/swodki/575403").unwrap();
        assert_eq!(channel_ref, ChannelRef::Username("swodki".to_string()));
        assert_eq!(msg_id.get(), 575403);
    }

    #[test]
    fn parse_public_link_no_scheme() {
        let (channel_ref, msg_id) = parse_telegram_link("t.me/swodki/575403").unwrap();
        assert_eq!(channel_ref, ChannelRef::Username("swodki".to_string()));
        assert_eq!(msg_id.get(), 575403);
    }

    #[test]
    fn parse_private_link() {
        let (channel_ref, msg_id) = parse_telegram_link("https://t.me/c/1234567/575403").unwrap();
        assert_eq!(
            channel_ref,
            ChannelRef::Id(ChannelId::new(1234567).unwrap())
        );
        assert_eq!(msg_id.get(), 575403);
    }

    #[test]
    fn parse_link_with_query_params() {
        let (channel_ref, msg_id) =
            parse_telegram_link("https://t.me/swodki/575403?single").unwrap();
        assert_eq!(channel_ref, ChannelRef::Username("swodki".to_string()));
        assert_eq!(msg_id.get(), 575403);
    }

    #[test]
    fn parse_link_with_trailing_slash() {
        let (channel_ref, msg_id) = parse_telegram_link("https://t.me/swodki/575403/").unwrap();
        assert_eq!(channel_ref, ChannelRef::Username("swodki".to_string()));
        assert_eq!(msg_id.get(), 575403);
    }

    #[test]
    fn parse_link_invalid_domain() {
        let result = parse_telegram_link("https://example.com/swodki/575403");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Not a valid t.me link")
        );
    }

    #[test]
    fn parse_link_missing_message_id() {
        let result = parse_telegram_link("https://t.me/swodki");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid t.me link format")
        );
    }

    #[test]
    fn parse_link_non_numeric_message_id() {
        let result = parse_telegram_link("https://t.me/swodki/abc");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid message ID")
        );
    }

    #[test]
    fn parse_link_empty_string() {
        let result = parse_telegram_link("");
        assert!(result.is_err());
    }

    #[test]
    fn parse_private_link_invalid_channel_id() {
        let result = parse_telegram_link("https://t.me/c/abc/575403");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid channel ID")
        );
    }

    #[test]
    fn parse_private_link_missing_message_id() {
        let result = parse_telegram_link("https://t.me/c/1234567");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid t.me link format")
        );
    }
}
