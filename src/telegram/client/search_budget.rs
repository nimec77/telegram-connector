//! Wall-clock budget and work counters for a search accumulation loop.

use std::time::Duration;
use tokio::time::Instant;

/// Wall-clock budget for a search accumulation loop, plus the work counters
/// reported back to the caller.
///
/// Bounds the *loop*, not an individual round trip — a single hung MTProto
/// call is `[telegram.timeouts] search_secs`' job. On expiry the loop returns
/// what it gathered; it never turns a slow-but-working search into an error,
/// because partial results are strictly more useful than a failure.
///
/// Uses `tokio::time::Instant` so `tokio::time::pause()` drives it in tests.
/// Note that `ops_search.rs`'s `start_time`/`fetch_start` use
/// `std::time::Instant` — correct in production, where both clocks advance
/// together, but a future integration test under `tokio::time::pause()` would
/// see the budget expire while `search_time_ms` still reports 0.
pub(crate) struct SearchBudget {
    /// `None` disables the deadline entirely.
    deadline: Option<Instant>,
    timed_out: bool,
    pages_fetched: u32,
    messages_scanned: u64,
}

impl SearchBudget {
    pub(crate) fn new(deadline_secs: u64) -> Self {
        Self {
            deadline: (deadline_secs > 0)
                .then(|| Instant::now() + Duration::from_secs(deadline_secs)),
            timed_out: false,
            pages_fetched: 0,
            messages_scanned: 0,
        }
    }

    /// True once the budget is spent. Latches `timed_out`, so a caller cannot
    /// act on expiry without the response reporting it.
    ///
    /// Latching is provable by inspection rather than by test: `timed_out` has
    /// exactly one write site — the line below — and it writes `true`. Under a
    /// monotonic clock no test can distinguish a latched flag from a stateless
    /// recompute, so the guarantee rests on that single write.
    pub(crate) fn expired(&mut self) -> bool {
        if self.deadline.is_some_and(|d| Instant::now() >= d) {
            self.timed_out = true;
        }
        self.timed_out
    }

    /// Record one round trip and the messages it returned.
    pub(crate) fn record_page(&mut self, messages_in_page: usize) {
        self.pages_fetched = self.pages_fetched.saturating_add(1);
        self.messages_scanned = self
            .messages_scanned
            .saturating_add(messages_in_page as u64);
    }

    pub(crate) fn pages_fetched(&self) -> u32 {
        self.pages_fetched
    }

    pub(crate) fn messages_scanned(&self) -> u64 {
        self.messages_scanned
    }

    pub(crate) fn timed_out(&self) -> bool {
        self.timed_out
    }
}

#[cfg(test)]
mod tests {
    use super::SearchBudget;
    use std::time::Duration;
    use tokio::time;

    #[tokio::test(start_paused = true)]
    async fn budget_is_not_expired_within_its_window() {
        let mut budget = SearchBudget::new(20);
        time::advance(Duration::from_secs(19)).await;
        assert!(!budget.expired());
        assert!(!budget.timed_out());
    }

    #[tokio::test(start_paused = true)]
    async fn budget_expires_and_latches_timed_out() {
        let mut budget = SearchBudget::new(20);
        time::advance(Duration::from_secs(21)).await;
        assert!(budget.expired());
        assert!(budget.timed_out(), "checking expiry must latch the flag");
    }

    #[tokio::test(start_paused = true)]
    async fn timed_out_stays_latched_after_a_later_non_expired_check() {
        // Guards against a caller re-checking and clearing the flag.
        let mut budget = SearchBudget::new(20);
        time::advance(Duration::from_secs(21)).await;
        assert!(budget.expired());
        assert!(budget.timed_out());
        assert!(budget.timed_out());
    }

    #[test]
    fn counters_accumulate_across_pages() {
        let mut budget = SearchBudget::new(20);
        budget.record_page(100);
        budget.record_page(37);
        assert_eq!(budget.pages_fetched(), 2);
        assert_eq!(budget.messages_scanned(), 137);
    }

    #[test]
    fn fresh_budget_reports_no_work_done() {
        let budget = SearchBudget::new(20);
        assert_eq!(budget.pages_fetched(), 0);
        assert_eq!(budget.messages_scanned(), 0);
        assert!(!budget.timed_out());
    }

    #[tokio::test(start_paused = true)]
    async fn zero_deadline_is_treated_as_disabled_not_instantly_expired() {
        // Config validation rejects 0, but a disabled budget must never
        // return zero results if one ever reaches here.
        let mut budget = SearchBudget::new(0);
        time::advance(Duration::from_secs(3600)).await;
        assert!(!budget.expired());
    }
}
