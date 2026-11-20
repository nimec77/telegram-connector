//! Bounded search and history fan-out independent of wire presentation.

use super::SearchPage;
use super::validation::{ChannelReferenceKey, channel_reference_key};
use crate::UtcTimestamp;
use crate::error::Error;
use crate::telegram::types::{Message, QueryMetadata, SearchResult};
use futures::StreamExt as _;
use std::future::Future;

/// Hard cap on channels per fan-out call (rate cost is 1 token/channel
/// against a default 30-token bucket).
pub const MAX_FANOUT_CHANNELS: usize = 20;

/// Concurrent per-channel fetches in flight (roadmap: bounded concurrency 4).
pub(crate) const FANOUT_CONCURRENCY: usize = 4;

/// A locally normalized target and its first supplied spelling for error attribution.
#[derive(Debug, PartialEq, Eq)]
pub struct FanoutChannel {
    /// Canonical local reference passed to the fetcher.
    pub reference: String,
    /// First caller spelling retained for reporting failures.
    pub original: String,
}

/// One channel's fetch outcome; retain its typed cause until mode-specific output.
pub struct ChannelFetchOutcome {
    /// First caller spelling for this channel.
    pub channel: String,
    /// Domain result from the attempted backend call.
    pub result: Result<SearchResult, Error>,
}

/// Fetch every channel in `list` through `fetch` with bounded concurrency
/// (`FANOUT_CONCURRENCY`), pairing each reference with its outcome. Outcome
/// order follows `list` (buffered preserves order).
pub async fn run<F, Fut>(list: Vec<FanoutChannel>, fetch: F) -> Vec<ChannelFetchOutcome>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<SearchResult, Error>>,
{
    futures::stream::iter(list.into_iter().map(|channel| {
        let result = fetch(channel.reference);
        async move {
            ChannelFetchOutcome {
                channel: channel.original,
                result: result.await,
            }
        }
    }))
    .buffered(FANOUT_CONCURRENCY)
    .collect()
    .await
}

/// Merge per-channel results into one newest-first page of at most `limit`
/// messages. Errors become `channel_errors` entries; the merge itself fails
/// only when every channel failed.
///
/// # Errors
/// Returns the collected typed failures if every channel failed.
pub fn merge_results(
    outcomes: Vec<ChannelFetchOutcome>,
    limit: usize,
    query: String,
    window_from: UtcTimestamp,
    window_to: Option<UtcTimestamp>,
) -> Result<SearchPage, Error> {
    let attempted = outcomes.len() as u32;
    let mut messages: Vec<Message> = Vec::new();
    let mut errors: Vec<(String, Error)> = Vec::new();
    let mut seen_messages = std::collections::HashSet::new();
    let mut has_more = false;
    let mut search_time_ms = 0u64;
    let mut pages_fetched = 0u32;
    let mut messages_scanned = 0u64;
    let mut timed_out = false;
    let mut partial = false;

    for outcome in outcomes {
        match outcome.result {
            Ok(result) => {
                has_more |= result.has_more;
                search_time_ms = search_time_ms.max(result.search_time_ms);
                // Summed, not maxed: these are the fan-out's total cost.
                pages_fetched = pages_fetched.saturating_add(result.query_metadata.pages_fetched);
                messages_scanned =
                    messages_scanned.saturating_add(result.query_metadata.messages_scanned);
                timed_out |= result.query_metadata.timed_out;
                partial |= result.query_metadata.partial;
                // Ordered outcomes make the first complete observation authoritative.
                messages.extend(result.messages.into_iter().filter(|message| {
                    seen_messages.insert((message.peer_namespace, message.channel_id, message.id))
                }));
            }
            Err(error) => errors.push((outcome.channel, error)),
        }
    }

    if messages.is_empty() && !errors.is_empty() && errors.len() as u32 == attempted {
        return Err(Error::AllChannelsFailed(errors));
    }

    messages.sort_by(|a, b| {
        b.timestamp
            .cmp(&a.timestamp)
            .then(b.id.get().cmp(&a.id.get()))
    });
    if messages.len() > limit {
        messages.truncate(limit);
        has_more = true;
    }

    let channels_in_results = messages
        .iter()
        .map(|message| (message.peer_namespace, message.channel_id))
        .collect::<std::collections::HashSet<_>>()
        .len() as u32;

    let result = SearchResult {
        returned: messages.len() as u64,
        has_more,
        search_time_ms,
        query_metadata: QueryMetadata {
            query,
            window_from,
            window_to,
            channels_scanned: Some(attempted),
            channels_in_results,
            timed_out,
            partial,
            pages_fetched,
            messages_scanned,
        },
        messages,
    };
    Ok(SearchPage {
        result,
        failures: errors,
    })
}

/// Enforce the channel_id XOR channel_ids contract shared by both list tools.
/// `Ok(None)` = single-channel/global path; `Ok(Some(list))` = fan-out with a
/// validated, locally normalized, 1..=MAX_FANOUT_CHANNELS list.
///
/// # Errors
/// Returns the reason the supplied reference set is invalid.
pub fn validate_channel_scope(
    channel_id: &Option<String>,
    channel_ids: &Option<Vec<String>>,
) -> Result<Option<Vec<FanoutChannel>>, String> {
    match (channel_id, channel_ids) {
        (Some(_), Some(_)) => Err("supply either channel_id or channel_ids, not both".to_string()),
        (_, None) => Ok(None),
        (None, Some(list)) => normalize_channels(list).map(Some),
    }
}

/// Validate and deduplicate a set before the operation acquires account tokens.
///
/// # Errors
/// Returns the reason the supplied references are invalid.
pub fn normalize_channels(list: &[String]) -> Result<Vec<FanoutChannel>, String> {
    if list.iter().any(|c| c.trim().is_empty()) {
        return Err("channel_ids must not contain blank entries".to_string());
    }
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::new();
    for original in list {
        // Validate every supplied entry, even a later alias, before acquisition.
        let key = channel_reference_key(original)?;
        let reference = match &key {
            ChannelReferenceKey::Id(id) => id.get().to_string(),
            ChannelReferenceKey::Username(username) => username.clone(),
        };
        if seen.insert(key) {
            deduped.push(FanoutChannel {
                reference,
                original: original.clone(),
            });
        }
    }
    if deduped.is_empty() {
        return Err("channel_ids must contain at least one channel".to_string());
    }
    if deduped.len() > MAX_FANOUT_CHANNELS {
        return Err(format!(
            "channel_ids accepts at most {MAX_FANOUT_CHANNELS} channels per call, got {}",
            deduped.len()
        ));
    }
    Ok(deduped)
}
