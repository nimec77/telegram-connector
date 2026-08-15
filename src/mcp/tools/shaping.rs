//! Post-fetch response shaping at the MCP layer (work-order B4/A4/A8):
//! text truncation, compact hoisting, and byte-budget fitting. Pure
//! functions over wire types so every rule is unit-testable offline.

use crate::mcp::tools::types::requests::ResponseFormat;
use crate::mcp::tools::types::responses::{
    ChannelHeader, MessageResponse, MessagesBatchResponse, NextCursor, SearchResponse,
    unique_channel_count,
};

/// Default per-message text cap in characters (work-order B4).
pub(crate) const DEFAULT_MAX_TEXT_LENGTH: u32 = 2000;

/// Resolve the effective `max_text_length`: default when omitted, rejecting 0.
pub(crate) fn resolve_max_text_length(requested: Option<u32>) -> Result<u32, String> {
    let max_text_length = requested.unwrap_or(DEFAULT_MAX_TEXT_LENGTH);
    if max_text_length == 0 {
        return Err("max_text_length must be greater than 0".to_string());
    }
    Ok(max_text_length)
}

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
fn compact_response(resp: &mut SearchResponse) {
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

/// Which compact header shape applies (work-order A4/A3).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactScope {
    /// One channel: single `channel` header, channel fields stripped.
    Single,
    /// Fan-out: `channels` map keyed by decimal id; per-message channel_id
    /// survives so messages stay attributable.
    Multi,
}

/// Multi-channel compact hoisting (A3): build the id-keyed header map from
/// the messages, strip name/username, keep channel_id on every message.
fn compact_response_multi(resp: &mut SearchResponse) {
    let mut map = std::collections::BTreeMap::new();
    for m in &resp.messages {
        if let (Some(id), Some(name)) = (m.channel_id, m.channel_name.clone()) {
            map.entry(id.get().to_string()).or_insert(ChannelHeader {
                id,
                name,
                username: m.channel_username.clone(),
            });
        }
    }
    for m in &mut resp.messages {
        m.channel_name = None;
        m.channel_username = None;
    }
    resp.channels = if map.is_empty() { None } else { Some(map) };
}

/// Drop trailing (oldest) messages until the serialized response fits
/// `budget` bytes (work-order B4). Pop-until-fits: byte-exact against the
/// cap, and at ≤100 messages the repeated serialization costs a few ms.
/// At least one message always survives — a caller must never receive an
/// empty page with `has_more: true` for the same cursor, so a single
/// over-budget message is returned as-is (the one documented overrun).
/// After popping: `returned`, `has_more`, `next_cursor` (when
/// `cursor_eligible`), and `channels_in_results` (full format only) are
/// recomputed so the metadata stays honest (B6).
fn fit_to_budget(
    resp: &mut SearchResponse,
    budget: usize,
    cursor_eligible: bool,
) -> Result<(), String> {
    let mut popped = false;
    loop {
        let len = serde_json::to_string(resp)
            .map_err(|e| format!("Failed to serialize response: {}", e))?
            .len();
        if len <= budget || resp.messages.len() <= 1 {
            break;
        }
        resp.messages.pop();
        popped = true;
    }
    if popped {
        resp.returned = resp.messages.len() as u64;
        resp.has_more = true;
        if cursor_eligible && let Some(last) = resp.messages.last() {
            resp.next_cursor = Some(NextCursor { before_id: last.id });
        }
        let count = unique_channel_count(&resp.messages);
        if count > 0 {
            resp.query_metadata.channels_in_results = count;
        }
    }
    Ok(())
}

/// Run the full post-fetch shaping pipeline in the order the audit verified:
/// (1) per-message text truncation, (2) `next_cursor` emission when the page
/// was truncated and the scope is single-channel, (3) compact hoisting, then
/// (4) byte-budget fitting. Shared by `search_messages_impl` and
/// `get_recent_messages_impl` so the order and conditions live in one place.
pub(crate) fn shape_response(
    resp: &mut SearchResponse,
    format: ResponseFormat,
    max_text_length: u32,
    cursor_eligible: bool,
    byte_budget: usize,
    scope: CompactScope,
) -> Result<(), String> {
    for msg in &mut resp.messages {
        truncate_text(msg, max_text_length);
    }
    if resp.has_more
        && cursor_eligible
        && let Some(last) = resp.messages.last()
    {
        resp.next_cursor = Some(NextCursor { before_id: last.id });
    }
    if format == ResponseFormat::Compact {
        match scope {
            CompactScope::Single => compact_response(resp),
            CompactScope::Multi => compact_response_multi(resp),
        }
    }
    fit_to_budget(resp, byte_budget, cursor_eligible)
}

/// Pop trailing messages from a batch response until it serializes within
/// `budget`, recording popped ids in `omitted_ids` (work-order A1 + B4).
/// Distinct from `missing`: omitted ids exist and can be re-requested.
/// At least one message always survives (same floor as `fit_to_budget`).
pub(crate) fn fit_batch_to_budget(
    resp: &mut MessagesBatchResponse,
    budget: usize,
) -> Result<(), String> {
    let mut omitted = Vec::new();
    loop {
        let len = serde_json::to_string(resp)
            .map_err(|e| format!("Failed to serialize response: {}", e))?
            .len();
        if len <= budget || resp.messages.len() <= 1 {
            break;
        }
        if let Some(popped) = resp.messages.pop() {
            omitted.push(popped.id);
        }
    }
    if !omitted.is_empty() {
        omitted.reverse(); // request order, matching messages
        resp.returned = resp.messages.len();
        resp.omitted_ids = Some(omitted);
    }
    Ok(())
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
            document_info: None,
            poll_info: None,
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
            timed_out: false,
            partial: false,
            pages_fetched: 0,
            messages_scanned: 0,
        }
    }

    #[test]
    fn compact_hoists_channel_and_strips_messages() {
        let mut resp = SearchResponse {
            channel: None,
            channels: None,
            messages: vec![wire_message(2, "b"), wire_message(1, "a")],
            returned: 2,
            has_more: false,
            next_cursor: None,
            search_time_ms: 1,
            query_metadata: test_metadata(),
            channel_errors: None,
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
            channels: None,
            messages: vec![],
            returned: 0,
            has_more: false,
            next_cursor: None,
            search_time_ms: 1,
            query_metadata: test_metadata(),
            channel_errors: None,
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

    fn budget_fixture(n: i64, text_len: usize) -> SearchResponse {
        SearchResponse {
            channel: None,
            channels: None,
            messages: (1..=n)
                .rev() // newest (highest id) first, matching fetch order
                .map(|i| wire_message(i, &"я".repeat(text_len)))
                .collect(),
            returned: n as u64,
            has_more: false,
            next_cursor: None,
            search_time_ms: 1,
            query_metadata: test_metadata(),
            channel_errors: None,
        }
    }

    #[test]
    fn budget_pops_oldest_and_sets_cursor() {
        let mut resp = budget_fixture(50, 400);
        fit_to_budget(&mut resp, 10_000, true).expect("fit");
        let len = serde_json::to_string(&resp).expect("json").len();
        assert!(len <= 10_000, "must end under budget, got {len}");
        assert!(resp.messages.len() < 50, "must have dropped messages");
        assert!(resp.has_more);
        assert_eq!(resp.returned, resp.messages.len() as u64);
        // Newest kept, oldest dropped; cursor = oldest surviving id.
        let last_id = resp.messages.last().expect("nonempty").id;
        assert_eq!(resp.next_cursor.expect("cursor").before_id, last_id);
        assert_eq!(resp.messages.first().expect("nonempty").id.get(), 50);
    }

    #[test]
    fn budget_keeps_at_least_one_message() {
        let mut resp = budget_fixture(3, 5_000);
        fit_to_budget(&mut resp, 100, true).expect("fit");
        assert_eq!(
            resp.messages.len(),
            1,
            "one message must survive even over-budget"
        );
        assert!(resp.has_more);
    }

    #[test]
    fn budget_leaves_fitting_response_untouched() {
        let mut resp = budget_fixture(2, 10);
        fit_to_budget(&mut resp, 40_000, true).expect("fit");
        assert_eq!(resp.messages.len(), 2);
        assert!(!resp.has_more);
        assert!(resp.next_cursor.is_none());
    }

    #[test]
    fn budget_without_cursor_eligibility_sets_no_cursor() {
        let mut resp = budget_fixture(50, 400);
        fit_to_budget(&mut resp, 10_000, false).expect("fit");
        assert!(resp.has_more);
        assert!(
            resp.next_cursor.is_none(),
            "global search: has_more without cursor"
        );
    }

    #[test]
    fn multi_compact_builds_channels_map_and_keeps_channel_id() {
        let mut resp = SearchResponse {
            channel: None,
            channels: None,
            messages: vec![wire_message(2, "b"), wire_message(1, "a")],
            returned: 2,
            has_more: false,
            next_cursor: None,
            search_time_ms: 1,
            query_metadata: test_metadata(),
            channel_errors: None,
        };
        compact_response_multi(&mut resp);
        let map = resp.channels.expect("map");
        assert!(map.contains_key("100"), "keyed by decimal channel id");
        for m in &resp.messages {
            assert!(
                m.channel_id.is_some(),
                "channel_id survives in multi compact"
            );
            assert!(m.channel_name.is_none());
            assert!(m.channel_username.is_none());
        }
    }

    #[test]
    fn batch_budget_pops_tail_into_omitted_ids() {
        let mut resp = MessagesBatchResponse {
            channel_id: "swodki".into(),
            messages: (1..=5)
                .map(|i| wire_message(i, &"я".repeat(2_000)))
                .collect(),
            returned: 5,
            missing: vec![],
            omitted_ids: None,
        };
        fit_batch_to_budget(&mut resp, 10_000).expect("fit");
        assert!(serde_json::to_string(&resp).expect("json").len() <= 10_000);
        assert_eq!(resp.returned, resp.messages.len());
        let omitted = resp.omitted_ids.expect("omitted");
        assert!(!omitted.is_empty());
        // Tail (highest fixture ids) got popped; survivors keep request order.
        assert_eq!(resp.messages.first().expect("some").id.get(), 1);
    }

    #[test]
    fn resolve_max_text_length_defaults_when_omitted() {
        assert_eq!(resolve_max_text_length(None), Ok(DEFAULT_MAX_TEXT_LENGTH));
    }

    #[test]
    fn resolve_max_text_length_passes_explicit_value() {
        assert_eq!(resolve_max_text_length(Some(64)), Ok(64));
    }

    #[test]
    fn resolve_max_text_length_rejects_zero() {
        let err = resolve_max_text_length(Some(0)).unwrap_err();
        assert!(err.contains("greater than 0"), "got: {err}");
    }
}
