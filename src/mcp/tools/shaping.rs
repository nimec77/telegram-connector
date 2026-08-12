//! Post-fetch response shaping at the MCP layer (work-order B4/A4/A8):
//! text truncation, compact hoisting, and byte-budget fitting. Pure
//! functions over wire types so every rule is unit-testable offline.

use crate::mcp::tools::types::responses::MessageResponse;

/// Default per-message text cap in characters (work-order B4).
pub(crate) const DEFAULT_MAX_TEXT_LENGTH: u32 = 2000;

/// Cut `text` to `max_chars` characters, flagging the cut. Counts
/// characters, not bytes: the corpus is largely Cyrillic UTF-8, where a
/// byte cap would halve the visible text and could split a code point.
pub(crate) fn truncate_text(msg: &mut MessageResponse, max_chars: u32) {
    let total = msg.text.chars().count();
    if total <= max_chars as usize {
        return;
    }
    msg.text = msg.text.chars().take(max_chars as usize).collect();
    msg.text_truncated = Some(true);
    msg.text_full_length = Some(total as u64);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telegram::types::{ChannelId, ChannelName, MediaType, MessageId, Username};

    fn wire_message(id: i64, text: &str) -> MessageResponse {
        MessageResponse {
            id: MessageId::new(id).expect("id"),
            channel_id: ChannelId::new(100).expect("id"),
            channel_name: ChannelName::new("Test").expect("name"),
            channel_username: Username::new("testchan").ok(),
            text: text.to_string(),
            timestamp: chrono::Utc::now(),
            sender_id: None,
            sender_name: None,
            has_media: false,
            media_type: MediaType::None,
            forwarded_from: None,
            link_preview: None,
            views: None,
            forwards: None,
            reply_to_message_id: None,
            video_info: None,
            audio_info: None,
            grouped_id: None,
            link: format!("https://t.me/testchan/{id}"),
            reactions: None,
            reactions_total: None,
            album: None,
            text_truncated: None,
            text_full_length: None,
        }
    }

    #[test]
    fn truncate_counts_characters_not_bytes() {
        // 10 Cyrillic chars = 20 UTF-8 bytes; a 5-char cap must keep 5 chars.
        let mut msg = wire_message(1, "новостидня");
        truncate_text(&mut msg, 5);
        assert_eq!(msg.text, "новос");
        assert_eq!(msg.text_truncated, Some(true));
        assert_eq!(msg.text_full_length, Some(10));
    }

    #[test]
    fn truncate_leaves_short_text_unflagged() {
        let mut msg = wire_message(1, "короткий");
        truncate_text(&mut msg, 2000);
        assert_eq!(msg.text, "короткий");
        assert_eq!(msg.text_truncated, None);
        assert_eq!(msg.text_full_length, None);
    }

    #[test]
    fn truncate_at_exact_length_is_not_truncation() {
        let mut msg = wire_message(1, "пять!");
        truncate_text(&mut msg, 5);
        assert_eq!(msg.text_truncated, None);
    }
}
