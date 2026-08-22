//! Shared helper functions for MCP tools.
//!
//! This module extracts common ID parsing logic used across multiple tools.

use crate::error::Error;
use crate::link::ChannelRef;
use crate::telegram::types::{ChannelId, MessageId};

/// The message inside `InvalidInput`, so tool-level prefixes don't stack on
/// the variant's own `invalid input:` Display prefix (work-order D7).
fn error_detail(e: Error) -> String {
    match e {
        Error::InvalidInput(msg) => msg,
        other => other.to_string(),
    }
}

/// Parse a channel ID string to a type-safe ChannelId.
///
/// # Arguments
/// * `id_str` - String representation of channel ID (must be numeric)
///
/// # Returns
/// * `Ok(ChannelId)` - Valid channel ID
/// * `Err(String)` - Error message describing the issue
///
/// # Example
/// ```ignore
/// let id = parse_channel_id("123456")?;
/// assert_eq!(id.get(), 123456);
/// ```
pub fn parse_channel_id(id_str: &str) -> Result<ChannelId, String> {
    let id_num: i64 = id_str
        .parse()
        .map_err(|_| format!("Invalid channel_id: '{}' is not a valid number", id_str))?;

    ChannelId::new(id_num).map_err(|e| format!("Invalid channel_id: {}", error_detail(e)))
}

/// Classify a channel reference as the list tools receive it: an all-digit
/// string is a numeric id (validated here), anything else is a username left
/// verbatim for the resolve layer. Shared by `search_messages` and
/// `get_recent_messages` so the two agree on what counts as numeric.
pub(crate) fn parse_channel_reference(reference: &str) -> Result<ChannelRef, String> {
    if reference.chars().all(|c| c.is_ascii_digit()) {
        parse_channel_id(reference).map(ChannelRef::Id)
    } else {
        Ok(ChannelRef::Username(reference.to_string()))
    }
}

/// Parse a message ID to a type-safe MessageId.
///
/// # Arguments
/// * `id` - Message ID (must be positive)
///
/// # Returns
/// * `Ok(MessageId)` - Valid message ID
/// * `Err(String)` - Error message describing the issue
pub fn parse_message_id(id: i64) -> Result<MessageId, String> {
    MessageId::new(id).map_err(|e| format!("Invalid message_id: {}", error_detail(e)))
}

/// Dedupe message ids (silently, preserving first-seen order), enforce a
/// per-call cap, and validate each id's sign and i32 range.
///
/// Shared by `get_messages_batch` and `get_messages_media_batch`, which differ
/// only in their cap. Returns the deduped ids alongside their wire (`i32`)
/// forms, in the same order. Dedupe runs before the cap check, so a caller
/// repeating an id is not penalised for it.
pub fn dedupe_and_validate_ids(ids: &[i64], cap: usize) -> Result<(Vec<i64>, Vec<i32>), String> {
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<i64> = ids.iter().copied().filter(|id| seen.insert(*id)).collect();

    if unique.len() > cap {
        return Err(format!(
            "message_ids accepts at most {cap} ids per call, got {}",
            unique.len()
        ));
    }

    let mut wire_ids = Vec::with_capacity(unique.len());
    for id in &unique {
        let parsed = parse_message_id(*id)?;
        wire_ids.push(wire_message_id(parsed)?);
    }

    Ok((unique, wire_ids))
}

/// Parse and cross-validate the optional cursor bounds (A8). Both ids parse
/// via `parse_message_id`; when both are present, `before_id` must exceed
/// `after_id` — the page covers `after_id < id < before_id`.
pub(crate) fn parse_cursor_bounds(
    before_id: Option<i64>,
    after_id: Option<i64>,
) -> Result<(Option<MessageId>, Option<MessageId>), String> {
    let before = before_id
        .map(parse_message_id)
        .transpose()
        .map_err(|e| format!("before_id: {}", e))?;
    let after = after_id
        .map(parse_message_id)
        .transpose()
        .map_err(|e| format!("after_id: {}", e))?;
    if let (Some(b), Some(a)) = (before, after)
        && b.get() <= a.get()
    {
        return Err(format!(
            "before_id ({}) must be greater than after_id ({}): the page covers after_id \
             < id < before_id",
            b.get(),
            a.get()
        ));
    }
    Ok((before, after))
}

/// A validated [`MessageId`] as Telegram's wire (`i32`) form, or a
/// caller-facing error naming the id when it exceeds the wire range.
pub(crate) fn wire_message_id(id: MessageId) -> Result<i32, String> {
    id.as_i32().ok_or_else(|| {
        format!(
            "message_id {} exceeds Telegram's message id range",
            id.get()
        )
    })
}

/// Parse an optional RFC 3339 datetime request field into UTC.
///
/// The value is trimmed first, so a padded-but-valid date parses. A
/// present-but-blank value is *not* silently treated as "absent": it is a caller
/// mistake and produces the same `Invalid {field}` error as any other unparseable
/// input, rather than quietly widening the requested window.
pub(crate) fn parse_optional_utc(
    field: &str,
    value: &Option<String>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
    value
        .as_deref()
        .map(|s| {
            chrono::DateTime::parse_from_rfc3339(s.trim())
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| {
                    format!("Invalid {field}: {e} (expected RFC 3339, e.g. 2026-08-01T00:00:00Z)")
                })
        })
        .transpose()
}

/// Reject a date window that cannot contain any message.
///
/// Two structurally empty shapes:
/// * an inverted range (`from_date` strictly after `to_date`) — equal bounds are
///   fine, both ends are documented as inclusive;
/// * a `to_date` with no `from_date` that lands at or before the `hours_back`
///   window start, which leaves nothing between the two ends. The history walk
///   would silently return `[]` and a global search would burn its whole timeout
///   budget, so this is caught up front and the caller is told to pass
///   `from_date` when reaching further back than `hours_back`.
pub(crate) fn validate_date_window(
    from_date: Option<chrono::DateTime<chrono::Utc>>,
    to_date: Option<chrono::DateTime<chrono::Utc>>,
    window_start: chrono::DateTime<chrono::Utc>,
    hours_back: u32,
) -> Result<(), String> {
    match (from_date, to_date) {
        (Some(f), Some(t)) if f > t => Err("from_date must be earlier than to_date".to_string()),
        (None, Some(t)) if t <= window_start => Err(format!(
            "to_date is at or before the start of the hours_back window (last {hours_back}h), \
             so the requested window is empty; supply from_date to reach further back"
        )),
        _ => Ok(()),
    }
}

/// Serialize a value to a JSON string for an MCP tool response.
///
/// Centralizes the `serde_json::to_string(..).map_err(|e| e.to_string())` tail
/// repeated by every `*_impl` method, mapping serialization failures to the
/// `String` error half of the rmcp `Result<String, String>` tool contract.
pub fn json_response<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_channel_reference_treats_all_digits_as_a_numeric_id() {
        assert_eq!(
            parse_channel_reference("1144180066"),
            Ok(ChannelRef::Id(ChannelId::new(1144180066).unwrap()))
        );
    }

    #[test]
    fn parse_channel_reference_keeps_a_username_verbatim() {
        // The `@` is preserved: the resolve layer owns username normalization.
        assert_eq!(
            parse_channel_reference("@swodki"),
            Ok(ChannelRef::Username("@swodki".to_string()))
        );
        assert_eq!(
            parse_channel_reference("swodki"),
            Ok(ChannelRef::Username("swodki".to_string()))
        );
    }

    #[test]
    fn parse_channel_reference_treats_the_empty_string_as_an_invalid_numeric_id() {
        // `"".chars().all(..)` is vacuously true, so the empty reference is
        // classified numeric and rejected by the id parser — never handed to
        // the resolve layer as a username.
        let err = parse_channel_reference("").unwrap_err();
        assert!(err.contains("Invalid channel_id"), "got: {err}");
    }

    #[test]
    fn parse_channel_reference_rejects_an_invalid_numeric_id() {
        // All digits, so it is a numeric id — and an invalid one, not a username.
        let err = parse_channel_reference("0").unwrap_err();
        assert!(err.contains("Invalid channel_id"), "got: {err}");
    }

    #[test]
    fn parse_channel_id_valid() {
        let result = parse_channel_id("123456");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().get(), 123456);
    }

    #[test]
    fn parse_channel_id_invalid_string() {
        let result = parse_channel_id("not_a_number");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not a valid number"));
    }

    #[test]
    fn parse_channel_id_negative() {
        let result = parse_channel_id("-123");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid channel_id"));
    }

    #[test]
    fn parse_channel_id_zero() {
        let result = parse_channel_id("0");
        assert!(result.is_err());
    }

    #[test]
    fn parse_message_id_valid() {
        let result = parse_message_id(789);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().get(), 789);
    }

    #[test]
    fn parse_message_id_invalid() {
        let result = parse_message_id(-1);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid message_id"));
    }

    #[test]
    fn parse_message_id_negative_has_single_prefix() {
        let err = parse_message_id(-5).unwrap_err();
        assert_eq!(
            err,
            "Invalid message_id: Message ID must be positive, got -5"
        );
    }

    #[test]
    fn wire_message_id_rejects_beyond_i32() {
        let id = parse_message_id(i64::from(i32::MAX) + 1).expect("positive id parses");
        let err = wire_message_id(id).expect_err("must reject");
        assert!(err.contains("exceeds Telegram's message id range"));
    }

    #[test]
    fn wire_message_id_passes_in_range() {
        let id = parse_message_id(42).expect("valid id");
        assert_eq!(wire_message_id(id), Ok(42));
    }

    #[test]
    fn dedupe_preserves_first_seen_order() {
        let (unique, wire) = dedupe_and_validate_ids(&[3, 1, 3, 2, 1], 10).expect("valid ids");
        assert_eq!(unique, vec![3, 1, 2]);
        assert_eq!(wire, vec![3, 1, 2]);
    }

    #[test]
    fn over_cap_is_rejected_with_the_cap_in_the_message() {
        let err = dedupe_and_validate_ids(&[1, 2, 3], 2).expect_err("over cap");
        assert_eq!(err, "message_ids accepts at most 2 ids per call, got 3");
    }

    #[test]
    fn dedupe_happens_before_the_cap_check() {
        // Three ids but two distinct: under a cap of 2 this must be accepted.
        let (unique, _) = dedupe_and_validate_ids(&[7, 7, 8], 2).expect("duplicates do not count");
        assert_eq!(unique, vec![7, 8]);
    }

    #[test]
    fn an_id_beyond_the_i32_range_is_rejected() {
        let err =
            dedupe_and_validate_ids(&[i64::from(i32::MAX) + 1], 10).expect_err("out of range");
        assert!(
            err.contains("exceeds Telegram's message id range"),
            "got: {err}"
        );
    }

    #[test]
    fn parse_channel_id_negative_has_single_prefix() {
        let err = parse_channel_id("-1").unwrap_err();
        assert_eq!(
            err,
            "Invalid channel_id: Channel ID must be positive, got -1"
        );
    }

    #[test]
    fn json_response_serializes_value() {
        let out = json_response(&vec![1, 2, 3]).expect("serializes");
        assert_eq!(out, "[1,2,3]");
    }

    #[test]
    fn parse_optional_utc_rejects_blank_value() {
        for blank in ["", "   ", "\t"] {
            let err = parse_optional_utc("from_date", &Some(blank.to_string()))
                .expect_err("a blank date must not be treated as absent");
            assert!(err.contains("Invalid from_date"), "got: {err}");
        }
    }

    #[test]
    fn parse_optional_utc_trims_before_parsing() {
        let parsed = parse_optional_utc("from_date", &Some(" 2026-08-01T00:00:00Z ".to_string()))
            .expect("a padded but valid date must parse");
        assert_eq!(parsed, Some("2026-08-01T00:00:00Z".parse().unwrap()));
    }

    #[test]
    fn parse_optional_utc_none_stays_none() {
        assert_eq!(parse_optional_utc("from_date", &None).expect("ok"), None);
    }

    fn utc(s: &str) -> chrono::DateTime<chrono::Utc> {
        s.parse().expect("valid test timestamp")
    }

    #[test]
    fn validate_date_window_accepts_equal_bounds() {
        let instant = utc("2026-08-01T00:00:00Z");
        assert!(validate_date_window(Some(instant), Some(instant), instant, 48).is_ok());
    }

    #[test]
    fn validate_date_window_rejects_inverted_range() {
        let err = validate_date_window(
            Some(utc("2026-08-05T00:00:00Z")),
            Some(utc("2026-08-01T00:00:00Z")),
            utc("2026-07-01T00:00:00Z"),
            48,
        )
        .expect_err("inverted range must be rejected");
        assert!(err.contains("from_date must be earlier than to_date"));
    }

    #[test]
    fn validate_date_window_rejects_to_date_at_or_before_window_start() {
        let window_start = utc("2026-08-01T00:00:00Z");
        for to_date in ["2026-07-30T00:00:00Z", "2026-08-01T00:00:00Z"] {
            let err = validate_date_window(None, Some(utc(to_date)), window_start, 48)
                .expect_err("empty window must be rejected");
            assert!(err.contains("from_date"), "got: {err}");
            assert!(err.contains("48h"), "got: {err}");
        }
    }

    #[test]
    fn validate_date_window_accepts_to_date_inside_window() {
        assert!(
            validate_date_window(
                None,
                Some(utc("2026-08-02T00:00:00Z")),
                utc("2026-08-01T00:00:00Z"),
                48,
            )
            .is_ok()
        );
    }

    #[test]
    fn validate_date_window_ignores_window_start_when_from_date_is_set() {
        // An explicit from_date overrides hours_back, so a to_date older than the
        // hours_back-derived start is a perfectly good historical window.
        assert!(
            validate_date_window(
                Some(utc("2026-01-01T00:00:00Z")),
                Some(utc("2026-01-05T00:00:00Z")),
                utc("2026-08-01T00:00:00Z"),
                48,
            )
            .is_ok()
        );
    }

    #[test]
    fn parse_cursor_bounds_passes_valid_pair() {
        let (before, after) = parse_cursor_bounds(Some(10), Some(5)).expect("valid");
        assert_eq!(before.map(|b| b.get()), Some(10));
        assert_eq!(after.map(|a| a.get()), Some(5));
    }

    #[test]
    fn parse_cursor_bounds_none_stays_none() {
        let (before, after) = parse_cursor_bounds(None, None).expect("valid");
        assert!(before.is_none() && after.is_none());
    }

    #[test]
    fn parse_cursor_bounds_rejects_crossed_pair_naming_both_ids() {
        let err = parse_cursor_bounds(Some(5), Some(10)).unwrap_err();
        assert!(err.contains("before_id (5)"), "got: {err}");
        assert!(err.contains("after_id (10)"), "got: {err}");
    }

    #[test]
    fn parse_cursor_bounds_rejects_equal_pair() {
        assert!(parse_cursor_bounds(Some(7), Some(7)).is_err());
    }

    #[test]
    fn parse_cursor_bounds_prefixes_the_failing_field() {
        let err = parse_cursor_bounds(Some(-1), None).unwrap_err();
        assert!(err.starts_with("before_id:"), "got: {err}");
        let err = parse_cursor_bounds(None, Some(-1)).unwrap_err();
        assert!(err.starts_with("after_id:"), "got: {err}");
    }
}
