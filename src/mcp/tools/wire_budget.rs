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
        let mut metadata = data.clone();
        metadata.messages.clear();
        metadata.returned = 0;
        metadata.next_cursor = None;
        if runtime::response_size(&contract::render(format, Ok(metadata)))? > budget {
            return Err(Error::ResponseTooLarge);
        }
        loop {
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
        let mut metadata = data.clone();
        metadata.messages.clear();
        metadata.returned = 0;
        if runtime::response_size(&contract::render(format, Ok(metadata)))? > budget {
            return Err(Error::ResponseTooLarge);
        }
        loop {
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
