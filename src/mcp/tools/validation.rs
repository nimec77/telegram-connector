//! Typed validation for operations; public compatibility helpers keep string errors.
use super::{fanout, helpers, shaping};
use crate::UtcTimestamp;
use crate::error::Error;
use crate::link::ChannelRef;
use crate::telegram::types::MessageId;

pub(crate) fn parse_channel_reference(value: &str) -> Result<ChannelRef, Error> {
    helpers::parse_channel_reference(value).map_err(Error::InvalidInput)
}
pub(crate) fn validate_channel_reference(value: &str) -> Result<(), Error> {
    helpers::validate_channel_reference(value).map_err(Error::InvalidInput)
}
pub(crate) fn parse_message_id(value: i64) -> Result<MessageId, Error> {
    helpers::parse_message_id(value).map_err(Error::InvalidInput)
}
pub(crate) fn wire_message_id(value: MessageId) -> Result<i32, Error> {
    helpers::wire_message_id(value).map_err(Error::InvalidInput)
}
pub(crate) fn dedupe_and_validate_ids(
    values: &[i64],
    cap: usize,
) -> Result<(Vec<i64>, Vec<i32>), Error> {
    helpers::dedupe_and_validate_ids(values, cap).map_err(Error::InvalidInput)
}
pub(crate) fn parse_cursor_bounds(
    before: Option<i64>,
    after: Option<i64>,
) -> Result<(Option<MessageId>, Option<MessageId>), Error> {
    helpers::parse_cursor_bounds(before, after).map_err(Error::InvalidInput)
}
pub(crate) fn parse_optional_utc(
    field: &str,
    value: &Option<String>,
) -> Result<Option<UtcTimestamp>, Error> {
    helpers::parse_optional_utc(field, value).map_err(Error::InvalidInput)
}
pub(crate) fn validate_date_window(
    from: Option<UtcTimestamp>,
    to: Option<UtcTimestamp>,
    start: UtcTimestamp,
    hours: u32,
) -> Result<(), Error> {
    helpers::validate_date_window(from, to, start, hours).map_err(Error::InvalidInput)
}
pub(crate) fn validate_channel_scope(
    one: &Option<String>,
    many: &Option<Vec<String>>,
) -> Result<Option<Vec<fanout::FanoutChannel>>, Error> {
    fanout::validate_channel_scope(one, many).map_err(Error::InvalidInput)
}

pub(crate) fn resolve_max_text_length(value: Option<u32>) -> Result<u32, Error> {
    shaping::resolve_max_text_length(value).map_err(Error::InvalidInput)
}
