//! Serialized wire accounting after typed conversion and fallback assembly.
use crate::config::OutputFormat;
use crate::error::Error;
use crate::mcp::runtime;
use crate::mcp::tools::shaping::CursorPolicy;
use crate::mcp::tools::types::contract;
use crate::mcp::tools::types::responses::{
    MessageResponse, MessagesBatchResponse, NextCursor, SearchResponse, unique_channel_count,
};
use rmcp::model::CallToolResult;

pub(crate) fn search(
    format: OutputFormat,
    result: Result<SearchResponse<Error>, Error>,
    budget: usize,
    cursor: CursorPolicy,
) -> CallToolResult {
    let result = result.and_then(|mut data| {
        let mut message_bytes: Option<MessageBytes> = None;
        // The one-message floor permits message overflow, never metadata overflow.
        loop {
            #[cfg(test)]
            work::record(|counts| {
                counts.metadata_renders += 1;
            });
            let metadata = search_metadata(&data);
            if measured_size(&contract::render(format, Ok(metadata)))? > budget {
                return Err(Error::ResponseTooLarge);
            }
            // Rows alone can prove a candidate impossible, never that it fits.
            // Metadata above is remeasured even for every such proven rejection.
            if message_bytes
                .as_ref()
                .is_none_or(|bytes| bytes.total <= budget)
                || data.messages.len() <= 1
            {
                #[cfg(test)]
                work::record(|counts| {
                    counts.candidate_renders += 1;
                    counts.candidate_rows += data.messages.len();
                });
                let rendered = contract::render(format, Ok(data.clone()));
                if measured_size(&rendered)? <= budget || data.messages.len() <= 1 {
                    return Ok(rendered);
                }
                if message_bytes.is_none() {
                    message_bytes = MessageBytes::from_rendered(format, &rendered, &data.messages)?;
                }
            }
            if let Some(bytes) = &mut message_bytes {
                bytes.pop();
            }
            data.messages.pop();
            data.returned = data.messages.len() as u64;
            data.has_more = true;
            data.next_cursor = match cursor {
                CursorPolicy::SingleChannel => data
                    .messages
                    .last()
                    .map(|last| NextCursor { before_id: last.id }),
                CursorPolicy::Unavailable => None,
            };
            data.query_metadata.channels_in_results = unique_channel_count(&data.messages);
        }
    });
    result.unwrap_or_else(|error| contract::render::<SearchResponse<Error>>(format, Err(error)))
}

pub(crate) fn batch(
    format: OutputFormat,
    result: Result<MessagesBatchResponse, Error>,
    budget: usize,
) -> CallToolResult {
    let result = result.and_then(|mut data| {
        let mut message_bytes: Option<MessageBytes> = None;
        loop {
            #[cfg(test)]
            work::record(|counts| {
                counts.metadata_renders += 1;
            });
            let metadata = batch_metadata(&data);
            if measured_size(&contract::render(format, Ok(metadata)))? > budget {
                return Err(Error::ResponseTooLarge);
            }
            // Rows alone can prove a candidate impossible, never that it fits.
            // Metadata above is remeasured even for every such proven rejection.
            if message_bytes
                .as_ref()
                .is_none_or(|bytes| bytes.total <= budget)
                || data.messages.len() <= 1
            {
                #[cfg(test)]
                work::record(|counts| {
                    counts.candidate_renders += 1;
                    counts.candidate_rows += data.messages.len();
                });
                let rendered = contract::render(format, Ok(data.clone()));
                if measured_size(&rendered)? <= budget || data.messages.len() <= 1 {
                    return Ok(rendered);
                }
                if message_bytes.is_none() {
                    message_bytes = MessageBytes::from_rendered(format, &rendered, &data.messages)?;
                }
            }
            if let Some(bytes) = &mut message_bytes {
                bytes.pop();
            }
            if let Some(message) = data.messages.pop() {
                data.omitted_ids
                    .get_or_insert_with(Vec::new)
                    .insert(0, message.id);
            }
            data.returned = data.messages.len();
        }
    });
    result.unwrap_or_else(|error| contract::render::<MessagesBatchResponse>(format, Err(error)))
}

// A lower bound consisting only of the actual serialized message fragments.
// Canonical rows appear as JSON and again escaped in the matching text fallback;
// legacy rows appear only escaped in that fallback. All other frame bytes are
// ignored. Popping changes metadata but not surviving rows (headers stay intact),
// so this can reject impossible candidates without assuming full sizes decrease.
struct MessageBytes {
    rows: Vec<usize>,
    total: usize,
}

impl MessageBytes {
    fn from_rendered(
        format: OutputFormat,
        rendered: &CallToolResult,
        messages: &[MessageResponse],
    ) -> Result<Option<Self>, Error> {
        let rows = match format {
            OutputFormat::JsonV1 => {
                let Some(rows) = rendered
                    .structured_content
                    .as_ref()
                    .and_then(|value| value.get("data"))
                    .and_then(|data| data.get("messages"))
                    .and_then(serde_json::Value::as_array)
                    .filter(|rows| rows.len() == messages.len())
                else {
                    // A conversion failure has no successful rows to project.
                    return Ok(None);
                };
                rows.iter()
                    .map(|row| Self::row_size(row, true))
                    .collect::<Result<Vec<_>, _>>()?
            }
            OutputFormat::Legacy => messages
                .iter()
                .map(|row| Self::row_size(row, false))
                .collect::<Result<Vec<_>, _>>()?,
        };
        let Some(total) = rows
            .iter()
            .try_fold(0usize, |total, row| total.checked_add(*row))
        else {
            return Ok(None);
        };
        Ok(Some(Self { rows, total }))
    }

    fn row_size(row: &impl serde::Serialize, duplicated: bool) -> Result<usize, Error> {
        #[cfg(test)]
        work::record(|counts| {
            counts.lower_bound_serializations += 2;
            counts.lower_bound_rows += 1;
        });
        let raw = serde_json::to_string(row)
            .map_err(|_| Error::Internal("response serialization failed"))?;
        let escaped = serde_json::to_string(&raw)
            .map_err(|_| Error::Internal("response serialization failed"))?;
        #[cfg(test)]
        work::record(|counts| counts.lower_bound_output_bytes += raw.len() + escaped.len());
        // Remove the outer JSON string quotes; the fragment lives within text.
        Ok(if duplicated { raw.len() } else { 0 } + escaped.len() - 2)
    }

    fn pop(&mut self) {
        if let Some(row) = self.rows.pop() {
            self.total -= row;
        }
    }
}

// Keep the existing renderer authoritative for format-specific metadata conversion.
// These snapshots deliberately never copy message text or other message payloads.
fn search_metadata(data: &SearchResponse<Error>) -> SearchResponse<Error> {
    SearchResponse {
        channel: data.channel.clone(),
        channels: data.channels.clone(),
        channel_errors: data.channel_errors.clone(),
        messages: Vec::new(),
        returned: 0,
        has_more: data.has_more,
        next_cursor: data.next_cursor,
        search_time_ms: data.search_time_ms,
        query_metadata: data.query_metadata.clone(),
    }
}

fn batch_metadata(data: &MessagesBatchResponse) -> MessagesBatchResponse {
    MessagesBatchResponse {
        resolved_channel: data.resolved_channel,
        channel_id: data.channel_id.clone(),
        messages: Vec::new(),
        returned: 0,
        missing: data.missing.clone(),
        omitted_ids: data.omitted_ids.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::types::responses::{MessageResponse, MissingMessageEntry};
    use crate::telegram::types::{ChannelId, MessageId, PeerNamespace};
    use serde_json::json;

    #[test]
    fn task8_batch_floor_never_exempts_metadata_added_by_omission_tracking() {
        let data = MessagesBatchResponse {
            resolved_channel: Some((ChannelId::new(100).expect("id"), PeerNamespace::Channel)),
            channel_id: "100".into(),
            messages: (1..=30)
                .map(|id| {
                    MessageResponse::from(crate::test_helpers::create_test_message(id, "", 100))
                })
                .collect(),
            returned: 30,
            missing: (31..=50)
                .map(|id| MissingMessageEntry {
                    id: MessageId::new(id).expect("id"),
                    error: "not found or deleted".into(),
                })
                .collect(),
            omitted_ids: None,
        };
        for budget in (3_000..16_000).step_by(53) {
            let result = batch(OutputFormat::JsonV1, Ok(data.clone()), budget);
            if result.is_error == Some(true) {
                continue;
            }
            let mut result = serde_json::to_value(result).expect("result");
            let body = &mut result["structuredContent"];
            body["data"]["messages"] = json!([]);
            body["data"]["returned"] = json!(0);
            let text = serde_json::to_string(body).expect("fallback");
            result["content"][0]["text"] = json!(text);
            let bytes = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":0,"result":result}))
                .expect("wire")
                .len();
            assert!(
                bytes <= budget,
                "metadata with omitted IDs used {bytes} bytes under {budget}"
            );
        }
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn review_one_message_floor_counts_real_cursor_metadata_in_both_modes() {
        for format in [OutputFormat::JsonV1, OutputFormat::Legacy] {
            let mut response = SearchResponse::<Error>::from_result(
                crate::test_helpers::create_test_search_result(
                    vec![crate::test_helpers::create_test_message(1, "", 100)],
                    "q",
                    1,
                ),
            );
            response.has_more = true;
            response.next_cursor = Some(NextCursor {
                before_id: crate::telegram::types::MessageId::new(1).expect("cursor"),
            });
            let mut metadata = response.clone();
            metadata.messages.clear();
            metadata.returned = 0;
            let rendered = contract::render(format, Ok(metadata));
            let budget = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":0,"result":rendered}))
                .expect("wire metadata")
                .len()
                - 1;
            let result = search(format, Ok(response), budget, CursorPolicy::SingleChannel);
            assert_eq!(
                result.is_error,
                Some(true),
                "cursor-bearing metadata escaped the cap in {format:?}"
            );
        }
    }
}

fn measured_size(result: &CallToolResult) -> Result<usize, Error> {
    #[cfg(test)]
    work::record(|counts| counts.runtime_size_serializations += 1);
    let bytes = runtime::response_size(result)?;
    #[cfg(test)]
    work::record(|counts| counts.runtime_frame_bytes += bytes);
    Ok(bytes)
}

#[cfg(test)]
#[path = "wire_work.rs"]
pub(super) mod work;

#[cfg(test)]
mod lower_bound_tests {
    use super::*;
    use crate::mcp::tools::shaping::{self, BudgetMode, CompactScope};
    use crate::mcp::tools::types::requests::ResponseFormat;

    fn assert_search_row_bound() {
        for format in [OutputFormat::JsonV1, OutputFormat::Legacy] {
            for response_format in [ResponseFormat::Full, ResponseFormat::Compact] {
                for scope in [CompactScope::Single, CompactScope::Multi] {
                    let mut data = SearchResponse::<Error>::from_result(
                        crate::test_helpers::create_test_search_result(
                            (1..=20)
                                .map(|id| {
                                    crate::test_helpers::create_test_message(
                                        id,
                                        &"я\"\\\n🦀".repeat(10),
                                        100,
                                    )
                                })
                                .collect(),
                            "q",
                            1,
                        ),
                    );
                    shaping::shape_response(
                        &mut data,
                        response_format,
                        2_000,
                        CursorPolicy::SingleChannel,
                        usize::MAX,
                        scope,
                        BudgetMode::Wire,
                    )
                    .expect("shape");
                    let rendered = contract::render(format, Ok(data.clone()));
                    let mut bound = MessageBytes::from_rendered(format, &rendered, &data.messages)
                        .expect("row serialization")
                        .expect("successful rows");
                    while !data.messages.is_empty() {
                        let rendered = contract::render(format, Ok(data.clone()));
                        assert!(
                            bound.total < runtime::response_size(&rendered).expect("actual frame")
                        );
                        let current =
                            MessageBytes::from_rendered(format, &rendered, &data.messages)
                                .expect("row serialization")
                                .expect("successful rows");
                        assert_eq!(
                            bound.total, current.total,
                            "popping changes metadata, never surviving row bytes"
                        );
                        data.messages.pop();
                        data.returned = data.messages.len() as u64;
                        data.has_more = true;
                        data.next_cursor = data.messages.last().map(|message| NextCursor {
                            before_id: message.id,
                        });
                        bound.pop();
                    }
                    assert_eq!(bound.total, 0);
                }
            }
        }
    }
    fn assert_batch_row_bound() {
        use crate::mcp::tools::types::responses::MissingMessageEntry;
        use crate::telegram::types::{ChannelId, MessageId, PeerNamespace};
        for format in [OutputFormat::JsonV1, OutputFormat::Legacy] {
            let mut data = MessagesBatchResponse {
                resolved_channel: Some((
                    ChannelId::new(100).expect("channel"),
                    PeerNamespace::Channel,
                )),
                channel_id: "@testchannel".into(),
                messages: (1..=20)
                    .map(|id| {
                        MessageResponse::from(crate::test_helpers::create_test_message(
                            id,
                            &"я\"\\\n🦀".repeat(10),
                            100,
                        ))
                    })
                    .collect(),
                returned: 20,
                missing: vec![MissingMessageEntry {
                    id: MessageId::new(21).expect("missing"),
                    error: "not found or deleted".into(),
                }],
                omitted_ids: Some(vec![MessageId::new(22).expect("omitted")]),
            };
            let rendered = contract::render(format, Ok(data.clone()));
            let mut bound = MessageBytes::from_rendered(format, &rendered, &data.messages)
                .expect("row serialization")
                .expect("successful rows");
            while let Some(message) = data.messages.pop() {
                bound.pop();
                data.returned = data.messages.len();
                data.omitted_ids
                    .as_mut()
                    .expect("omissions")
                    .insert(0, message.id);
                let rendered = contract::render(format, Ok(data.clone()));
                assert!(
                    bound.total
                        < runtime::response_size(&rendered).expect("actual frame with omissions")
                );
                let current = MessageBytes::from_rendered(format, &rendered, &data.messages)
                    .expect("row serialization")
                    .expect("successful rows");
                assert_eq!(bound.total, current.total);
            }
        }
    }

    #[test]
    fn required_row_bytes_bound_actual_frames_across_protocols_ids_and_metadata() {
        use rmcp::model::{ProtocolVersion, RequestId};
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        for version in [ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25] {
            for id in [
                RequestId::Number(99),
                RequestId::String("long\"\\я🦀".repeat(100).into()),
            ] {
                executor.block_on(runtime::scope_response(id, Some(version.clone()), async {
                    assert_search_row_bound();
                    assert_batch_row_bound();
                }));
            }
        }
    }
}
