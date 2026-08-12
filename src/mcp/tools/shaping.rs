//! Post-fetch response shaping at the MCP layer (work-order B4/A4/A8):
//! text truncation, compact hoisting, and byte-budget fitting. Pure
//! functions over wire types so every rule is unit-testable offline.

use crate::mcp::tools::types::responses::{ChannelHeader, MessageResponse, SearchResponse};

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

/// Hoist the (single) channel's identity into a response-level header and
/// strip it from every message (A4). Caller guarantees single-channel
/// scope; the header is read from the first message, and an empty result
/// keeps `channel: None`.
pub(crate) fn compact_response(resp: &mut SearchResponse) {
    resp.channel = resp.messages.first().and_then(|m| {
        Some(ChannelHeader {
            id: m.channel_id?,
            name: m.channel_name.clone()?,
            username: m.channel_username.clone(),
        })
    });
    for m in &mut resp.messages {
        m.channel_id = None;
        m.channel_name = None;
        m.channel_username = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telegram::types::{
        ChannelId, ChannelName, MediaType, MessageId, QueryMetadata, Username,
    };

    fn wire_message(id: i64, text: &str) -> MessageResponse {
        MessageResponse {
            id: MessageId::new(id).expect("id"),
            channel_id: ChannelId::new(100).ok(),
            channel_name: ChannelName::new("Test").ok(),
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

    fn test_metadata() -> QueryMetadata {
        QueryMetadata {
            query: String::new(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(48),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
        }
    }

    #[test]
    fn compact_hoists_channel_and_strips_messages() {
        let mut resp = SearchResponse {
            channel: None,
            messages: vec![wire_message(2, "b"), wire_message(1, "a")],
            returned: 2,
            has_more: false,
            next_cursor: None,
            search_time_ms: 1,
            query_metadata: test_metadata(),
        };
        compact_response(&mut resp);
        let header = resp.channel.expect("header");
        assert_eq!(header.id.get(), 100);
        assert_eq!(
            header.username.as_ref().map(|u| u.as_str()),
            Some("testchan")
        );
        for m in &resp.messages {
            assert!(m.channel_id.is_none());
            assert!(m.channel_name.is_none());
            assert!(m.channel_username.is_none());
        }
    }

    #[test]
    fn compact_on_empty_result_keeps_null_header() {
        let mut resp = SearchResponse {
            channel: None,
            messages: vec![],
            returned: 0,
            has_more: false,
            next_cursor: None,
            search_time_ms: 1,
            query_metadata: test_metadata(),
        };
        compact_response(&mut resp);
        assert!(resp.channel.is_none());
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
