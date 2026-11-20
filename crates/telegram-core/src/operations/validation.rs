//! Local validation shared by the operation facade and transport adapters.

use crate::error::Error;
use crate::link::ChannelRef;
use crate::telegram::types::{ChannelId, MessageId, Username};

fn error_detail(error: Error) -> String {
    match error {
        Error::InvalidInput(message) => message,
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
/// # Errors
/// Returns a string error if the input is not an `i64` or is not positive.
///
/// # Example
/// ```
/// use telegram_core::operations::parse_channel_id;
/// let id = parse_channel_id("123456")?;
/// assert_eq!(id.get(), 123456);
/// assert!(parse_channel_id("0").is_err());
/// # Ok::<(), String>(())
/// ```
pub fn parse_channel_id(id_str: &str) -> Result<ChannelId, String> {
    let id_num: i64 = id_str
        .parse()
        .map_err(|_| format!("Invalid channel_id: '{}' is not a valid number", id_str))?;

    ChannelId::new(id_num).map_err(|e| format!("Invalid channel_id: {}", error_detail(e)))
}

/// Locally comparable references retain their identifier kind until resolution.
#[derive(Debug, PartialEq, Eq, Hash)]
pub(crate) enum ChannelReferenceKey {
    Id(ChannelId),
    Username(String),
}

/// Validate and normalize only spellings that are equivalent without an RPC.
pub(crate) fn channel_reference_key(reference: &str) -> Result<ChannelReferenceKey, String> {
    match parse_channel_reference(reference.trim())? {
        ChannelRef::Id(id) => Ok(ChannelReferenceKey::Id(id)),
        ChannelRef::Username(username) => Ok(ChannelReferenceKey::Username(
            username
                .strip_prefix('@')
                .unwrap_or(&username)
                .to_ascii_lowercase(),
        )),
    }
}

/// Classify accepted numeric spellings consistently with local validation.
/// Username spelling is retained for the resolver; fan-out normalizes separately.
///
/// # Errors
/// Returns a string describing an invalid channel reference.
pub fn parse_channel_reference(reference: &str) -> Result<ChannelRef, String> {
    if reference.parse::<i64>().is_ok() || reference.bytes().all(|b| b.is_ascii_digit()) {
        return parse_channel_id(reference).map(ChannelRef::Id);
    }
    let username = reference.strip_prefix('@').unwrap_or(reference);
    if Username::is_valid_shape(username) {
        Ok(ChannelRef::Username(reference.to_string()))
    } else {
        Err("Invalid channel reference: expected a username or positive numeric ID".to_string())
    }
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
///
/// # Errors
/// Returns the reason the requested interval is empty.
pub fn validate_date_window(
    from_date: Option<crate::UtcTimestamp>,
    to_date: Option<crate::UtcTimestamp>,
    window_start: crate::UtcTimestamp,
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

pub(super) fn wire_message_id(id: MessageId) -> Result<i32, Error> {
    id.as_i32().ok_or_else(|| {
        Error::InvalidInput(format!(
            "message_id {} exceeds Telegram's message id range",
            id.get()
        ))
    })
}

pub(super) fn validate_cursors(
    before_id: Option<MessageId>,
    after_id: Option<MessageId>,
) -> Result<(), Error> {
    for id in [before_id, after_id].into_iter().flatten() {
        wire_message_id(id)?;
    }
    if let (Some(before), Some(after)) = (before_id, after_id)
        && before.get() <= after.get()
    {
        return Err(Error::InvalidInput(format!(
            "before_id ({}) must be greater than after_id ({}): the page covers after_id < id < before_id",
            before.get(),
            after.get()
        )));
    }
    Ok(())
}
