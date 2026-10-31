//! Pure assembly of history/search result envelopes without changing message order.

use super::search_budget;
use crate::telegram::types::{QueryMetadata, SearchResult};

/// Build the `SearchResult` envelope shared by `search_messages_impl` and
/// `get_recent_messages_impl`.
///
/// `channels_in_results` is computed here as the count of distinct channels
/// among `messages`. For history — which fetches from exactly one peer —
/// that count is always 0 or 1, which is exactly what the hand-rolled
/// `if messages.is_empty() { 0 } else { 1 }` it replaces used to produce;
/// the two forms are equivalent only because of that single-peer property.
///
/// `partial` is deliberately paired with `timed_out`, never with `has_more`:
/// expiry stopped the walk without proving anything lies beyond the page,
/// while a full page proves the opposite.
///
/// That pairing can still be a conservative false positive: the deadline is
/// checked at the top of an iteration, so expiry landing on what would have
/// been the terminal iteration reports a complete result set as partial.
/// Avoiding that needs lookahead the loop does not have, and the error is in
/// the safe direction — it is not a bug to go fix later.
///
/// Sorting is NOT applied here: `search_messages_impl` sorts by timestamp
/// descending before calling this, while `get_recent_messages_impl` relies on
/// its pager's already-reverse-chronological order. Folding a sort in here
/// would silently start sorting history results.
///
/// Eight positional arguments trips clippy's default `too_many_arguments`
/// threshold; each one is an independent, already-computed value the two
/// call sites have on hand (no natural sub-grouping), so bundling them into
/// a params struct would only relocate the count, not reduce it.
#[allow(clippy::too_many_arguments)]
pub(super) fn assemble_search_result(
    messages: Vec<crate::telegram::Message>,
    budget: &search_budget::SearchBudget,
    has_more: bool,
    query: String,
    window_from: chrono::DateTime<chrono::Utc>,
    window_to: Option<chrono::DateTime<chrono::Utc>>,
    channels_scanned: Option<u32>,
    search_time_ms: u64,
) -> SearchResult {
    let channels_in_results = messages
        .iter()
        .map(|m| m.channel_id.get())
        .collect::<std::collections::HashSet<_>>()
        .len() as u32;
    SearchResult {
        returned: messages.len() as u64,
        has_more,
        search_time_ms,
        query_metadata: QueryMetadata {
            query,
            window_from,
            window_to,
            channels_scanned,
            channels_in_results,
            timed_out: budget.timed_out(),
            partial: budget.timed_out(),
            pages_fetched: budget.pages_fetched(),
            messages_scanned: budget.messages_scanned(),
        },
        messages,
    }
}
