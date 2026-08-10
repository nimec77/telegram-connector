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

/// Generated deep links for a Telegram message.
///
/// Public channels (with a username) get shareable `t.me/<username>` /
/// `tg://resolve` forms; private chats fall back to the members-only
/// `t.me/c/…` / `tg://privatepost` forms. `internal_link` always carries the
/// members-only https form. Single shared builder for `generate_message_link`
/// and `open_message_in_telegram` (work-order B2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageLink {
    pub channel_id: ChannelId,
    pub message_id: MessageId,
    pub https_link: String,
    pub tg_protocol_link: String,
    pub internal_link: String,
    pub is_public: bool,
}

impl MessageLink {
    /// Create links for a specific message in a channel.
    ///
    /// `username` is the channel's public username, if any — pass `None` for
    /// private chats (never pass sentinel strings like `"unknown"`).
    pub fn new(channel_id: ChannelId, message_id: MessageId, username: Option<&str>) -> Self {
        let internal_link = format!("https://t.me/c/{}/{}", channel_id, message_id);
        let (https_link, tg_protocol_link, is_public) = match username {
            Some(u) => (
                format!("https://t.me/{}/{}", u, message_id),
                format!("tg://resolve?domain={}&post={}", u, message_id),
                true,
            ),
            None => (
                internal_link.clone(),
                format!(
                    "tg://privatepost?channel={}&post={}",
                    channel_id, message_id
                ),
                false,
            ),
        };

        Self {
            channel_id,
            message_id,
            https_link,
            tg_protocol_link,
            internal_link,
            is_public,
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
    fn message_link_public_channel_uses_username_forms() {
        let link = MessageLink::new(
            ChannelId::new(1144180066).unwrap(),
            MessageId::new(610121).unwrap(),
            Some("swodki"),
        );

        assert_eq!(link.https_link, "https://t.me/swodki/610121");
        assert_eq!(
            link.tg_protocol_link,
            "tg://resolve?domain=swodki&post=610121"
        );
        assert_eq!(link.internal_link, "https://t.me/c/1144180066/610121");
        assert!(link.is_public);
    }

    #[test]
    fn message_link_private_channel_uses_internal_forms() {
        let link = MessageLink::new(
            ChannelId::new(123456789).unwrap(),
            MessageId::new(42).unwrap(),
            None,
        );

        assert_eq!(link.https_link, "https://t.me/c/123456789/42");
        assert_eq!(
            link.tg_protocol_link,
            "tg://privatepost?channel=123456789&post=42"
        );
        assert_eq!(link.internal_link, "https://t.me/c/123456789/42");
        assert!(!link.is_public);
    }

    #[test]
    fn message_link_never_emits_single_suffix() {
        for username in [Some("swodki"), None] {
            let link = MessageLink::new(
                ChannelId::new(9).unwrap(),
                MessageId::new(1).unwrap(),
                username,
            );
            assert!(!link.https_link.contains("single"));
            assert!(!link.tg_protocol_link.contains("single"));
            assert!(!link.internal_link.contains("single"));
        }
    }

    #[test]
    fn message_link_stores_ids() {
        let link = MessageLink::new(
            ChannelId::new(111).unwrap(),
            MessageId::new(222).unwrap(),
            None,
        );

        assert_eq!(link.channel_id.get(), 111);
        assert_eq!(link.message_id.get(), 222);
    }

    #[test]
    fn message_link_serialization() {
        let link = MessageLink::new(
            ChannelId::new(123).unwrap(),
            MessageId::new(456).unwrap(),
            Some("testchan"),
        );

        let json = serde_json::to_string(&link).expect("serializes");
        assert!(json.contains("\"https_link\":\"https://t.me/testchan/456\""));
        assert!(json.contains("\"internal_link\":\"https://t.me/c/123/456\""));
        assert!(json.contains("\"is_public\":true"));
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
