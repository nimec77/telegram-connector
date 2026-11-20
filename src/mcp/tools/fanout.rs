//! Multi-channel fan-out plumbing for the MCP layer (work-order A3/A6):
//! bounded-concurrency fetch outcomes merged into one flat, newest-first
//! response. Pure over wire/domain types so merging is unit-testable.

use crate::UtcTimestamp;
use crate::error::Error;
use crate::mcp::tools::types::responses::{ChannelFetchError, SearchResponse};
use telegram_core::operations::SearchPage;
pub(crate) use telegram_core::operations::fanout::ChannelFetchOutcome;

pub(crate) use telegram_core::operations::fanout::{FanoutChannel, run, validate_channel_scope};

pub(crate) fn wire_page(page: SearchPage) -> SearchResponse<Error> {
    let mut response = SearchResponse::from_result(page.result);
    if !page.failures.is_empty() {
        response.channel_errors = Some(
            page.failures
                .into_iter()
                .map(|(channel, error)| ChannelFetchError { channel, error })
                .collect(),
        );
    }
    response
}

pub(crate) fn merge_results(
    outcomes: Vec<ChannelFetchOutcome>,
    limit: usize,
    query: String,
    window_from: UtcTimestamp,
    window_to: Option<UtcTimestamp>,
) -> Result<SearchResponse<Error>, Error> {
    telegram_core::operations::fanout::merge_results(outcomes, limit, query, window_from, window_to)
        .map(wire_page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UtcTimestamp;
    use crate::mcp::tools::types::responses::MessageResponse;
    use crate::telegram::types::{QueryMetadata, SearchResult};
    use crate::test_helpers::create_test_message;
    use jiff::SignedDuration;

    fn result_with(ids_and_offsets: &[(i64, i64)], channel: i64, has_more: bool) -> SearchResult {
        let messages = ids_and_offsets
            .iter()
            .map(|(id, mins_ago)| {
                let mut m = create_test_message(*id, "text", channel);
                m.timestamp = UtcTimestamp::now() - SignedDuration::from_mins(*mins_ago);
                m
            })
            .collect::<Vec<_>>();
        SearchResult {
            returned: messages.len() as u64,
            has_more,
            search_time_ms: 1,
            query_metadata: QueryMetadata {
                query: String::new(),
                window_from: UtcTimestamp::now() - SignedDuration::from_hours(48),
                window_to: None,
                channels_scanned: Some(1),
                channels_in_results: 1,
                timed_out: false,
                partial: false,
                pages_fetched: 0,
                messages_scanned: 0,
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
            UtcTimestamp::now() - SignedDuration::from_hours(48),
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
                result: Err(Error::TelegramApi(
                    "invalid input: Channel not found: gone".into(),
                )),
            },
        ];
        let resp = merge_results(
            outcomes,
            20,
            String::new(),
            UtcTimestamp::now() - SignedDuration::from_hours(48),
            None,
        )
        .expect("partial ok");
        assert_eq!(resp.returned, 1);
        assert!(!resp.has_more);
        let errors = resp.channel_errors.expect("errors present");
        assert_eq!(errors[0].channel, "gone");
        assert!(errors[0].error.to_string().contains("not found"));
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
                result: Err(Error::TelegramApi("boom a".into())),
            },
            ChannelFetchOutcome {
                channel: "b".into(),
                result: Err(Error::TelegramApi("boom b".into())),
            },
        ];
        let err = merge_results(
            outcomes,
            20,
            String::new(),
            UtcTimestamp::now() - SignedDuration::from_hours(48),
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("a: telegram API error: boom a")
                && err.to_string().contains("b: telegram API error: boom b"),
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
            UtcTimestamp::now() - SignedDuration::from_hours(48),
            None,
        )
        .expect("merge");
        assert!(resp.has_more);
    }

    #[test]
    fn scope_rejects_both_channel_id_and_channel_ids() {
        let err = validate_channel_scope(&Some("123".to_string()), &Some(vec!["456".to_string()]))
            .unwrap_err();
        assert!(err.to_string().contains("not both"), "got: {err}");
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
        assert_eq!(
            out,
            vec![
                FanoutChannel {
                    reference: "a".into(),
                    original: " a ".into()
                },
                FanoutChannel {
                    reference: "b".into(),
                    original: "b".into()
                },
            ]
        );
    }

    #[test]
    fn scope_rejects_blank_entries() {
        let err = validate_channel_scope(&None, &Some(vec!["a".to_string(), "  ".to_string()]))
            .unwrap_err();
        assert!(err.to_string().contains("blank"), "got: {err}");
    }

    #[test]
    fn scope_rejects_more_than_max_channels() {
        let list: Vec<String> = (1..=21).map(|i| i.to_string()).collect();
        let err = validate_channel_scope(&None, &Some(list)).unwrap_err();
        assert!(err.to_string().contains("20"), "got: {err}");
    }

    #[tokio::test]
    async fn run_pairs_each_channel_with_its_outcome_in_list_order() {
        let outcomes = run(
            vec![
                FanoutChannel {
                    reference: "a".into(),
                    original: " A ".into(),
                },
                FanoutChannel {
                    reference: "b".into(),
                    original: "@B".into(),
                },
            ],
            |reference| async move {
                if reference == "a" {
                    Ok(result_with(&[(1, 5)], 1, false))
                } else {
                    Err(Error::TelegramApi(format!("boom {reference}")))
                }
            },
        )
        .await;
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].channel, " A ");
        assert!(outcomes[0].result.is_ok());
        assert_eq!(outcomes[1].channel, "@B");
        assert_eq!(
            outcomes[1].result.as_ref().unwrap_err().to_string(),
            "telegram API error: boom b"
        );
    }

    #[test]
    fn merge_dedupes_before_limit_and_keeps_the_whole_first_album() {
        use crate::telegram::types::{AlbumInfo, MediaType, MessageId};
        let mut first = result_with(&[(10, 30)], 123, false);
        let message = &mut first.messages[0];
        message.text = "first caption".into();
        message.views = Some(7);
        message.album = Some(AlbumInfo {
            media_count: 2,
            media_types: vec![MediaType::Photo, MediaType::Video],
            message_ids: vec![MessageId::new(10).unwrap(), MessageId::new(11).unwrap()],
        });
        let expected = serde_json::to_value(MessageResponse::from(message.clone())).unwrap();
        let mut second = first.clone();
        second.messages[0].text = "later caption".into();
        second.messages[0].views = Some(99);
        second.messages[0].album = None;
        second.messages[0].timestamp = UtcTimestamp::now();
        second.query_metadata.pages_fetched = 3;
        second.query_metadata.messages_scanned = 30;
        second.query_metadata.partial = true;
        second.query_metadata.timed_out = true;
        let merged = merge_results(
            vec![ok("123", first), ok("alias", second)],
            1,
            "q".into(),
            UtcTimestamp::now(),
            None,
        )
        .unwrap();
        assert_eq!(merged.returned, 1);
        assert!(
            !merged.has_more,
            "duplicate-only overflow is not another result"
        );
        assert_eq!(serde_json::to_value(&merged.messages[0]).unwrap(), expected);
        assert_eq!(merged.query_metadata.channels_scanned, Some(2));
        assert_eq!(merged.query_metadata.pages_fetched, 3);
        assert_eq!(merged.query_metadata.messages_scanned, 30);
        assert!(merged.query_metadata.partial && merged.query_metadata.timed_out);
    }

    #[test]
    fn merge_keeps_equal_ids_in_distinct_sources_and_unknown_namespaces() {
        use crate::telegram::types::PeerNamespace;
        let namespaces = [
            None,
            Some(PeerNamespace::User),
            Some(PeerNamespace::Chat),
            Some(PeerNamespace::Channel),
        ];
        let mut outcomes = Vec::new();
        for namespace in namespaces {
            let mut result = result_with(&[(10, 0)], 123, false);
            result.messages[0].peer_namespace = namespace;
            outcomes.push(ok("alias", result));
        }
        outcomes.push(ok("other", result_with(&[(10, 0)], 456, false)));
        let merged = merge_results(outcomes, 5, String::new(), UtcTimestamp::now(), None).unwrap();
        assert_eq!(merged.returned, 5);
        assert_eq!(merged.query_metadata.channels_in_results, 5);
        assert!(!merged.has_more);
        for namespace in namespaces {
            assert!(
                merged
                    .messages
                    .iter()
                    .any(|message| message.peer_namespace == namespace
                        && message.channel_id.unwrap().get() == 123)
            );
        }
    }

    #[test]
    fn scope_normalizes_equivalent_usernames_and_numeric_spellings() {
        let scope = validate_channel_scope(
            &None,
            &Some(vec![
                " @News ".into(),
                "news".into(),
                "NEWS".into(),
                "+123".into(),
                "00123".into(),
                "123".into(),
            ]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            scope,
            vec![
                FanoutChannel {
                    reference: "news".into(),
                    original: " @News ".into()
                },
                FanoutChannel {
                    reference: "123".into(),
                    original: "+123".into()
                },
            ]
        );
    }

    #[test]
    fn shared_reference_parser_classifies_accepted_plus_numeric_as_id() {
        let reference = crate::mcp::tools::validation::parse_channel_reference("+123").unwrap();
        assert!(matches!(reference, crate::link::ChannelRef::Id(id) if id.get() == 123));
    }
}
