//! Serialized wire accounting after typed conversion and fallback assembly.
use crate::config::OutputFormat;
use crate::error::Error;
use crate::mcp::runtime;
use crate::mcp::tools::shaping::CursorPolicy;
use crate::mcp::tools::types::contract;
use crate::mcp::tools::types::responses::{
    MessagesBatchResponse, NextCursor, SearchResponse, unique_channel_count,
};
use rmcp::model::CallToolResult;

pub(crate) fn search(
    format: OutputFormat,
    result: Result<SearchResponse<Error>, Error>,
    budget: usize,
    cursor: CursorPolicy,
) -> CallToolResult {
    let result = result.and_then(|mut data| {
        // The one-message floor permits message overflow, never metadata overflow.
        loop {
            let mut metadata = data.clone();
            metadata.messages.clear();
            metadata.returned = 0;
            metadata.next_cursor = None;
            if runtime::response_size(&contract::render(format, Ok(metadata)))? > budget {
                return Err(Error::ResponseTooLarge);
            }
            let rendered = contract::render(format, Ok(data.clone()));
            if runtime::response_size(&rendered)? <= budget || data.messages.len() <= 1 {
                return Ok(data);
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
    contract::render(format, result)
}

pub(crate) fn batch(
    format: OutputFormat,
    result: Result<MessagesBatchResponse, Error>,
    budget: usize,
) -> CallToolResult {
    let result = result.and_then(|mut data| {
        loop {
            let mut metadata = data.clone();
            metadata.messages.clear();
            metadata.returned = 0;
            if runtime::response_size(&contract::render(format, Ok(metadata)))? > budget {
                return Err(Error::ResponseTooLarge);
            }
            let rendered = contract::render(format, Ok(data.clone()));
            if runtime::response_size(&rendered)? <= budget || data.messages.len() <= 1 {
                return Ok(data);
            }
            if let Some(message) = data.messages.pop() {
                data.omitted_ids
                    .get_or_insert_with(Vec::new)
                    .insert(0, message.id);
            }
            data.returned = data.messages.len();
        }
    });
    contract::render(format, result)
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
