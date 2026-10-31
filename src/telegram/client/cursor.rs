//! Pure conversion of channel-scoped cursor bounds to Telegram wire IDs.

use crate::error::Error;
use crate::telegram::types::MessageId;

/// Convert the optional cursor bounds to Telegram's `i32` wire ids (A8).
/// MTProto message ids are `i32`; an out-of-range cursor cannot be sent, so
/// it is rejected with the field name in the error.
pub(super) fn cursor_wire_bounds(
    before_id: Option<MessageId>,
    after_id: Option<MessageId>,
) -> Result<(Option<i32>, Option<i32>), Error> {
    fn wire(field: &str, id: Option<MessageId>) -> Result<Option<i32>, Error> {
        match id {
            Some(id) => id
                .as_i32()
                .ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "{field} {} exceeds Telegram's message id range",
                        id.get()
                    ))
                })
                .map(Some),
            None => Ok(None),
        }
    }
    Ok((wire("before_id", before_id)?, wire("after_id", after_id)?))
}
