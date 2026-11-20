//! Opt-in, thread-scoped work measurements; no allocator or timing assertions.
use super::*;
use crate::mcp::tools::shaping::{self, CompactScope};
use crate::mcp::tools::types::requests::ResponseFormat;
use crate::telegram::types::{ChannelId, PeerNamespace};
use rmcp::model::{ProtocolVersion, RequestId};
use std::cell::RefCell;

#[derive(Debug, Default)]
pub(in crate::mcp::tools) struct Counts {
    pub preliminary_serializations: usize,
    pub preliminary_rows: usize,
    pub preliminary_output_bytes: usize,
    pub lower_bound_serializations: usize,
    pub lower_bound_rows: usize,
    pub lower_bound_output_bytes: usize,
    pub metadata_renders: usize,
    pub metadata_body_clone_rows: usize,
    pub candidate_renders: usize,
    pub candidate_rows: usize,
    pub final_renders: usize,
    pub runtime_size_serializations: usize,
    pub runtime_frame_bytes: usize,
}
thread_local! {
    static COUNTS: RefCell<Option<Counts>> = const { RefCell::new(None) };
}
pub(in crate::mcp::tools) fn record(update: impl FnOnce(&mut Counts)) {
    COUNTS.with_borrow_mut(|counts| {
        if let Some(counts) = counts {
            update(counts);
        }
    });
}
fn measure(operation: impl FnOnce() -> CallToolResult) -> (CallToolResult, Counts) {
    COUNTS.with_borrow_mut(|counts| *counts = Some(Counts::default()));
    let result = operation();
    let counts = COUNTS.with_borrow_mut(|counts| counts.take().expect("measurement started"));
    (result, counts)
}
fn fixture(rows: i64) -> SearchResponse<Error> {
    let timestamp = crate::UtcTimestamp::from_unix_seconds(1_800_000_000).expect("timestamp");
    let messages = (1..=rows)
        .rev()
        .map(|id| {
            let mut message =
                crate::test_helpers::create_test_message(id, &"я\"\\🦀".repeat(40), 100);
            message.timestamp = timestamp;
            message
        })
        .collect();
    let mut result = crate::test_helpers::create_test_search_result(messages, "q", 1);
    result.query_metadata.window_from = timestamp;
    SearchResponse::from_result(result)
}
fn summary(result: &CallToolResult, format: OutputFormat) -> (usize, Vec<i64>) {
    let value = if format == OutputFormat::JsonV1 {
        result
            .structured_content
            .clone()
            .expect("structured result")["data"]
            .clone()
    } else {
        let text = result.content[0].as_text().expect("fallback");
        serde_json::from_str(&text.text).expect("legacy result")
    };
    let ids = value["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .map(|message| message["id"].as_i64().expect("id"))
        .collect();
    (
        runtime::response_payload(result)
            .expect("actual frame")
            .len(),
        ids,
    )
}

#[test]
#[ignore = "descriptive O02 serialization-work measurement; run explicitly with nocapture"]
fn o02_serialization_work() {
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    for format in [OutputFormat::JsonV1, OutputFormat::Legacy] {
        for response_format in [ResponseFormat::Full, ResponseFormat::Compact] {
            for modern in [true, false] {
                for pruning in [false, true] {
                    let budget = if pruning { 14_000 } else { 1_000_000 };
                    let id = RequestId::String("escaped\"\\я🦀".repeat(30).into());
                    let version = if modern {
                        ProtocolVersion::V_2026_07_28
                    } else {
                        ProtocolVersion::V_2025_11_25
                    };
                    executor.block_on(runtime::scope_response(id, Some(version), async {
                        for operation in ["search", "history", "batch"] {
                            let sizes: &[i64] = if operation == "batch" { &[1, 20, 50] } else { &[1, 20, 100] };
                            for &rows in sizes {
                                let input = fixture(rows);
                                let (result, counts) = if operation == "batch" {
                                    let input = MessagesBatchResponse {
                                        resolved_channel: Some((ChannelId::new(100).expect("channel"), PeerNamespace::Channel)),
                                        channel_id: "@testchannel".into(),
                                        messages: input.messages,
                                        returned: rows as usize,
                                        missing: vec![],
                                        omitted_ids: None,
                                    };
                                    measure(|| {
                                        // Wire callers skip preliminary DTO fitting.
                                        batch(format, Ok(input), budget)
                                    })
                                } else {
                                    let mut input = input;
                                    measure(|| {
                                        shaping::shape_response(&mut input, response_format, 2_000,
                                            CursorPolicy::SingleChannel, budget, CompactScope::Single, shaping::BudgetMode::Wire).expect("shape");
                                        search(format, Ok(input), budget, CursorPolicy::SingleChannel)
                                    })
                                };
                                assert_eq!(counts.metadata_body_clone_rows, 0);
                                assert_eq!(counts.final_renders, 0);
                                let (bytes, ids) = summary(&result, format);
                                assert!(bytes <= budget || ids.len() == 1);
                                println!("O02 {operation} rows={rows} output={format:?} response={} modern={modern} budget={budget} bytes={bytes} ids={ids:?} {counts:?}",
                                    if response_format == ResponseFormat::Full { "full" } else { "compact" });
                            }
                        }
                    }));
                }
            }
        }
    }
}
