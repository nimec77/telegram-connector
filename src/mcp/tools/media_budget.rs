//! Total-payload budget for multi-image responses.
//!
//! Counts **base64 characters** — the quantity that actually lands in the
//! client's context, and 4/3 the size of the underlying JPEG bytes. Pure: no
//! I/O, no image decoding.

use crate::config::MIN_IMAGE_BASE64_BYTES;
use crate::mcp::tools::image::MAX_BASE64_LEN;

/// Greedy allocator over a batch's total base64 payload budget.
#[derive(Debug)]
pub(crate) struct Base64Budget {
    remaining: usize,
}

impl Base64Budget {
    pub(crate) fn new(total: usize) -> Self {
        Self { remaining: total }
    }

    /// Base64 bytes the next image may occupy, or `None` when the budget is
    /// spent. Never exceeds the per-image cap, however much budget is left.
    pub(crate) fn allowance(&self) -> Option<usize> {
        if self.remaining < MIN_IMAGE_BASE64_BYTES {
            return None;
        }
        Some(self.remaining.min(MAX_BASE64_LEN))
    }

    /// Record what an image actually used. Saturates at zero.
    pub(crate) fn consume(&mut self, actual: usize) {
        self.remaining = self.remaining.saturating_sub(actual);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowance_is_capped_by_the_per_image_limit() {
        let budget = Base64Budget::new(100 * MAX_BASE64_LEN);
        assert_eq!(budget.allowance(), Some(MAX_BASE64_LEN));
    }

    #[test]
    fn allowance_shrinks_to_what_is_left() {
        let mut budget = Base64Budget::new(MAX_BASE64_LEN + 100_000);
        budget.consume(MAX_BASE64_LEN);
        assert_eq!(budget.allowance(), Some(100_000));
    }

    #[test]
    fn allowance_is_none_below_the_floor() {
        let mut budget = Base64Budget::new(MIN_IMAGE_BASE64_BYTES + 10);
        assert!(budget.allowance().is_some());
        budget.consume(11);
        assert_eq!(
            budget.allowance(),
            None,
            "below the floor the batch must stop, not emit an unreadable image"
        );
    }

    #[test]
    fn consuming_more_than_remains_saturates_at_zero() {
        let mut budget = Base64Budget::new(50_000);
        budget.consume(usize::MAX);
        assert_eq!(budget.allowance(), None);
    }

    #[test]
    fn allowance_is_none_when_the_budget_starts_below_the_floor() {
        let budget = Base64Budget::new(MIN_IMAGE_BASE64_BYTES - 1);
        assert_eq!(budget.allowance(), None);
    }

    #[test]
    fn allowance_is_some_exactly_at_the_floor() {
        let budget = Base64Budget::new(MIN_IMAGE_BASE64_BYTES);
        assert_eq!(budget.allowance(), Some(MIN_IMAGE_BASE64_BYTES));
    }
}
