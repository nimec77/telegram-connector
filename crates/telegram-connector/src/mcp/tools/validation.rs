//! Typed validation for operations; public compatibility helpers keep string errors.
use super::{helpers, shaping};
use crate::UtcTimestamp;
use crate::error::Error;
use crate::telegram::types::MessageId;

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
pub(crate) fn resolve_max_text_length(value: Option<u32>) -> Result<u32, Error> {
    shaping::resolve_max_text_length(value).map_err(Error::InvalidInput)
}
