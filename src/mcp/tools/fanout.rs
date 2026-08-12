//! Multi-channel fan-out plumbing for the MCP layer (work-order A3/A6):
//! bounded-concurrency fetch outcomes merged into one flat, newest-first
//! response. Pure over wire/domain types so merging is unit-testable.

use crate::mcp::tools::types::responses::{ChannelFetchError, MessageResponse, SearchResponse};
use crate::telegram::types::{QueryMetadata, SearchResult};
use chrono::{DateTime, Utc};

/// Hard cap on channels per fan-out call (rate cost is 1 token/channel
/// against a default 30-token bucket).
pub(crate) const MAX_FANOUT_CHANNELS: usize = 20;

/// Concurrent per-channel fetches in flight (roadmap: bounded concurrency 4).
pub(crate) const FANOUT_CONCURRENCY: usize = 4;

/// One channel's fetch outcome; `result` is stringly-typed because tool
/// errors already are (`Result<String, String>` contract).
pub(crate) struct ChannelFetchOutcome {
    pub channel: String,
    pub result: Result<SearchResult, String>,
}

/// Merge per-channel results into one newest-first page of at most `limit`
/// messages. Errors become `channel_errors` entries; the merge itself fails
/// only when every channel failed.
pub(crate) fn merge_results(
    outcomes: Vec<ChannelFetchOutcome>,
    limit: usize,
    query: String,
    window_from: DateTime<Utc>,
    window_to: Option<DateTime<Utc>>,
) -> Result<SearchResponse, String> {
    let attempted = outcomes.len() as u32;
    let mut messages: Vec<MessageResponse> = Vec::new();
    let mut errors: Vec<ChannelFetchError> = Vec::new();
    let mut has_more = false;
    let mut search_time_ms = 0u64;

    for outcome in outcomes {
        match outcome.result {
            Ok(result) => {
                has_more |= result.has_more;
                search_time_ms = search_time_ms.max(result.search_time_ms);
                messages.extend(result.messages.into_iter().map(MessageResponse::from));
            }
            Err(error) => errors.push(ChannelFetchError {
                channel: outcome.channel,
                error,
            }),
        }
    }

    if messages.is_empty() && !errors.is_empty() && errors.len() as u32 == attempted {
        return Err(format!(
            "all {attempted} channels failed: {}",
            errors
                .iter()
                .map(|e| format!("{}: {}", e.channel, e.error))
                .collect::<Vec<_>>()
                .join("; ")
        ));
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

    let unique: std::collections::HashSet<i64> = messages
        .iter()
        .filter_map(|m| m.channel_id.as_ref().map(|c| c.get()))
        .collect();

    Ok(SearchResponse {
        channel: None,
        channels: None,
        returned: messages.len() as u64,
        has_more,
        next_cursor: None,
        search_time_ms,
        query_metadata: QueryMetadata {
            query,
            window_from,
            window_to,
            channels_scanned: Some(attempted),
            channels_in_results: unique.len() as u32,
        },
        channel_errors: if errors.is_empty() {
            None
        } else {
            Some(errors)
        },
        messages,
    })
}

/// Enforce the channel_id XOR channel_ids contract shared by both list tools.
/// `Ok(None)` = single-channel/global path; `Ok(Some(list))` = fan-out with a
/// deduped, trimmed, 1..=MAX_FANOUT_CHANNELS list.
pub(crate) fn validate_channel_scope(
    channel_id: &Option<String>,
    channel_ids: &Option<Vec<String>>,
) -> Result<Option<Vec<String>>, String> {
    match (channel_id, channel_ids) {
        (Some(_), Some(_)) => Err("supply either channel_id or channel_ids, not both".to_string()),
        (_, None) => Ok(None),
        (None, Some(list)) => {
            if list.iter().any(|c| c.trim().is_empty()) {
                return Err("channel_ids must not contain blank entries".to_string());
            }
            let mut seen = std::collections::HashSet::new();
            let deduped: Vec<String> = list
                .iter()
                .map(|c| c.trim().to_string())
                .filter(|c| seen.insert(c.clone()))
                .collect();
            if deduped.is_empty() {
                return Err("channel_ids must contain at least one channel".to_string());
            }
            if deduped.len() > MAX_FANOUT_CHANNELS {
                return Err(format!(
                    "channel_ids accepts at most {MAX_FANOUT_CHANNELS} channels per call, got {}",
                    deduped.len()
                ));
            }
            Ok(Some(deduped))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telegram::types::{QueryMetadata, SearchResult};
    use crate::test_helpers::create_test_message;
    use chrono::{Duration, Utc};

    fn result_with(ids_and_offsets: &[(i64, i64)], channel: i64, has_more: bool) -> SearchResult {
        let messages = ids_and_offsets
            .iter()
            .map(|(id, mins_ago)| {
                let mut m = create_test_message(*id, "text", channel);
                m.timestamp = Utc::now() - Duration::minutes(*mins_ago);
                m
            })
            .collect::<Vec<_>>();
        SearchResult {
            returned: messages.len() as u64,
            has_more,
            search_time_ms: 1,
            query_metadata: QueryMetadata {
                query: String::new(),
                window_from: Utc::now() - Duration::hours(48),
                window_to: None,
                channels_scanned: Some(1),
                channels_in_results: 1,
            },
            messages,
        }
    }

    fn ok(channel: &str, r: SearchResult) -> ChannelFetchOutcome {
        ChannelFetchOutcome {
            channel: channel.into(),
            result: Ok(r),
        }
    }

    #[test]
    fn merge_interleaves_by_timestamp_and_truncates_to_limit() {
        let outcomes = vec![
            ok("a", result_with(&[(10, 30), (9, 90)], 1, false)),
            ok("b", result_with(&[(200, 10), (199, 60)], 2, false)),
        ];
        let resp = merge_results(
            outcomes,
            3,
            String::new(),
            Utc::now() - Duration::hours(48),
            None,
        )
        .expect("merge");
        let ids: Vec<i64> = resp.messages.iter().map(|m| m.id.get()).collect();
        assert_eq!(ids, vec![200, 10, 199], "newest-first across channels");
        assert_eq!(resp.returned, 3);
        assert!(resp.has_more, "truncation to limit must set has_more");
        assert_eq!(resp.query_metadata.channels_scanned, Some(2));
        assert_eq!(resp.query_metadata.channels_in_results, 2);
        assert!(resp.next_cursor.is_none());
    }

    #[test]
    fn merge_reports_partial_failures_as_channel_errors() {
        let outcomes = vec![
            ok("a", result_with(&[(10, 30)], 1, false)),
            ChannelFetchOutcome {
                channel: "gone".into(),
                result: Err("invalid input: Channel not found: gone".into()),
            },
        ];
        let resp = merge_results(
            outcomes,
            20,
            String::new(),
            Utc::now() - Duration::hours(48),
            None,
        )
        .expect("partial ok");
        assert_eq!(resp.returned, 1);
        assert!(!resp.has_more);
        let errors = resp.channel_errors.expect("errors present");
        assert_eq!(errors[0].channel, "gone");
        assert!(errors[0].error.contains("not found"));
        assert_eq!(
            resp.query_metadata.channels_scanned,
            Some(2),
            "attempted, not succeeded"
        );
    }

    #[test]
    fn merge_fails_only_when_every_channel_failed() {
        let outcomes = vec![
            ChannelFetchOutcome {
                channel: "a".into(),
                result: Err("boom a".into()),
            },
            ChannelFetchOutcome {
                channel: "b".into(),
                result: Err("boom b".into()),
            },
        ];
        let err = merge_results(
            outcomes,
            20,
            String::new(),
            Utc::now() - Duration::hours(48),
            None,
        )
        .unwrap_err();
        assert!(
            err.contains("a: boom a") && err.contains("b: boom b"),
            "got: {err}"
        );
    }

    #[test]
    fn merge_propagates_per_channel_has_more() {
        let outcomes = vec![ok("a", result_with(&[(10, 30)], 1, true))];
        let resp = merge_results(
            outcomes,
            20,
            String::new(),
            Utc::now() - Duration::hours(48),
            None,
        )
        .expect("merge");
        assert!(resp.has_more);
    }

    #[test]
    fn scope_rejects_both_channel_id_and_channel_ids() {
        let err = validate_channel_scope(&Some("123".to_string()), &Some(vec!["456".to_string()]))
            .unwrap_err();
        assert!(err.contains("not both"), "got: {err}");
    }

    #[test]
    fn scope_returns_none_when_only_channel_id_set() {
        let out = validate_channel_scope(&Some("123".to_string()), &None).expect("ok");
        assert!(out.is_none());
    }

    #[test]
    fn scope_returns_none_when_neither_set() {
        let out = validate_channel_scope(&None, &None).expect("ok");
        assert!(out.is_none());
    }

    #[test]
    fn scope_dedupes_trims_and_preserves_order() {
        let out = validate_channel_scope(
            &None,
            &Some(vec![" a ".to_string(), "b".to_string(), "a".to_string()]),
        )
        .expect("ok")
        .expect("some");
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn scope_rejects_blank_entries() {
        let err = validate_channel_scope(&None, &Some(vec!["a".to_string(), "  ".to_string()]))
            .unwrap_err();
        assert!(err.contains("blank"), "got: {err}");
    }

    #[test]
    fn scope_rejects_more_than_max_channels() {
        let list: Vec<String> = (0..21).map(|i| i.to_string()).collect();
        let err = validate_channel_scope(&None, &Some(list)).unwrap_err();
        assert!(err.contains("20"), "got: {err}");
    }
}
