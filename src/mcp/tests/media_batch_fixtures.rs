//! Shared fixtures for the media-batch test files.

use crate::mcp::tools::{GetMessagesMediaBatchRequest, MediaBatchSummary};
use crate::telegram::types::{MediaDownload, MediaFetchError, MediaFetchOutcome, MediaType};
use crate::test_helpers::create_test_jpeg;
use rmcp::model::ContentBlock;

pub(super) fn photo_download(width: u32, height: u32) -> MediaDownload {
    let bytes = create_test_jpeg(width, height);
    let source_size_bytes = bytes.len() as u64;
    MediaDownload {
        bytes,
        media_type: MediaType::Photo,
        is_thumbnail: false,
        caption: Some("benchmark chart".to_string()),
        width: Some(width),
        height: Some(height),
        source_size_bytes,
        video_info: None,
        largest_width: None,
        largest_height: None,
    }
}

pub(super) fn ok_outcome(message_id: i32, width: u32, height: u32) -> MediaFetchOutcome {
    MediaFetchOutcome {
        message_id,
        result: Ok(photo_download(width, height)),
    }
}

pub(super) fn err_outcome(message_id: i32, error: MediaFetchError) -> MediaFetchOutcome {
    MediaFetchOutcome {
        message_id,
        result: Err(error),
    }
}

pub(super) fn no_media(message_id: i32) -> MediaFetchOutcome {
    err_outcome(
        message_id,
        MediaFetchError::NoVisualMedia {
            media_type: "document".to_string(),
        },
    )
}

pub(super) fn not_found(message_id: i32) -> MediaFetchOutcome {
    err_outcome(message_id, MediaFetchError::NotFound)
}

pub(super) fn request(channel: &str, ids: Vec<i64>) -> GetMessagesMediaBatchRequest {
    GetMessagesMediaBatchRequest {
        channel_id: channel.to_string(),
        message_ids: ids,
        max_dimension: None,
    }
}

pub(super) fn summary_of(content: &[ContentBlock]) -> MediaBatchSummary {
    let ContentBlock::Text(text) = content.last().expect("summary block") else {
        panic!("last content block must be the summary text block");
    };
    serde_json::from_str(&text.text).expect("summary must be valid JSON")
}
