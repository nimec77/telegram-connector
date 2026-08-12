//! Shared test helpers for creating test fixtures.
//!
//! This module provides factory functions for creating domain objects in tests,
//! reducing duplication across test files.

use crate::telegram::types::{
    Channel, ChannelId, ChannelName, ChatType, ForwardInfo, LinkPreview, MediaType, Message,
    MessageId, QueryMetadata, SearchResult, UserId, Username,
};
use chrono::{DateTime, Utc};

/// Create a test message with commonly used defaults.
///
/// # Arguments
/// * `id` - Message ID (positive integer)
/// * `text` - Message text content
/// * `channel_id` - Channel ID (positive integer)
///
/// # Example
/// ```ignore
/// let msg = create_test_message(1, "Hello world", 100);
/// assert_eq!(msg.text, "Hello world");
/// ```
pub fn create_test_message(id: i64, text: &str, channel_id: i64) -> Message {
    Message {
        id: MessageId::new(id).expect("Test message ID must be positive"),
        channel_id: ChannelId::new(channel_id).expect("Test channel ID must be positive"),
        channel_name: ChannelName::new("Test Channel").expect("Valid channel name"),
        channel_username: Some(Username::new("testchannel").expect("Valid username")),
        text: text.to_string(),
        timestamp: Utc::now(),
        sender_id: None,
        sender_name: None,
        has_media: false,
        media_type: MediaType::None,
        forwarded_from: None,
        link_preview: None,
        views: None,
        forwards: None,
        reply_to_message_id: None,
        video_info: None,
        audio_info: None,
        grouped_id: None,
        link: format!("https://t.me/testchannel/{}", id),
        reactions: None,
        reactions_total: None,
        album: None,
    }
}

/// Create a test message with custom timestamp.
pub fn create_test_message_with_time(
    id: i64,
    text: &str,
    channel_id: i64,
    timestamp: DateTime<Utc>,
) -> Message {
    let mut msg = create_test_message(id, text, channel_id);
    msg.timestamp = timestamp;
    msg
}

/// Create a test message with media.
pub fn create_test_message_with_media(
    id: i64,
    text: &str,
    channel_id: i64,
    media_type: MediaType,
) -> Message {
    let mut msg = create_test_message(id, text, channel_id);
    msg.has_media = media_type != MediaType::None;
    msg.media_type = media_type;
    msg
}

/// Create a test message with sender information.
pub fn create_test_message_with_sender(
    id: i64,
    text: &str,
    channel_id: i64,
    sender_id: i64,
    sender_name: &str,
) -> Message {
    let mut msg = create_test_message(id, text, channel_id);
    msg.sender_id = Some(UserId::new(sender_id).expect("Test sender ID must be positive"));
    msg.sender_name = Some(sender_name.to_string());
    msg
}

/// Create a test message carrying forward attribution (channel forward).
pub fn create_test_message_with_forward(
    id: i64,
    text: &str,
    channel_id: i64,
    forwarded_channel_id: i64,
    original_message_id: i64,
) -> Message {
    let mut msg = create_test_message(id, text, channel_id);
    msg.forwarded_from = Some(ForwardInfo {
        channel_id: Some(
            ChannelId::new(forwarded_channel_id)
                .expect("Test forwarded channel ID must be positive"),
        ),
        channel_name: None,
        channel_username: None,
        sender_name: None,
        original_date: None,
        original_message_id: Some(
            MessageId::new(original_message_id).expect("Test original message ID must be positive"),
        ),
    });
    msg
}

/// Create a test message carrying a link preview.
pub fn create_test_message_with_link_preview(
    id: i64,
    text: &str,
    channel_id: i64,
    url: &str,
) -> Message {
    let mut msg = create_test_message(id, text, channel_id);
    msg.link_preview = Some(LinkPreview {
        url: url.to_string(),
        site_name: None,
        title: None,
        description: None,
    });
    msg
}

/// Create a test channel with commonly used defaults.
///
/// # Arguments
/// * `id` - Channel ID (positive integer)
/// * `username` - Channel username (5-32 alphanumeric chars)
///
/// # Example
/// ```ignore
/// let channel = create_test_channel(100, "technews");
/// assert_eq!(channel.username.as_ref().unwrap().as_str(), "technews");
/// ```
pub fn create_test_channel(id: i64, username: &str) -> Channel {
    Channel {
        id: ChannelId::new(id).expect("Test channel ID must be positive"),
        name: ChannelName::new(format!("Channel {}", username)).expect("Valid channel name"),
        username: Some(Username::new(username).expect("Valid username")),
        chat_type: ChatType::Channel,
        description: Some(format!("Description for {}", username)),
        member_count: Some(1000),
        is_verified: false,
        is_public: true,
        is_subscribed: true,
        last_message_date: Some(Utc::now()),
    }
}

/// Create a test channel with custom member count and verification status.
pub fn create_test_channel_detailed(
    id: i64,
    username: &str,
    name: &str,
    member_count: u64,
    is_verified: bool,
) -> Channel {
    Channel {
        id: ChannelId::new(id).expect("Test channel ID must be positive"),
        name: ChannelName::new(name).expect("Valid channel name"),
        username: Some(Username::new(username).expect("Valid username")),
        chat_type: ChatType::Channel,
        description: None,
        member_count: Some(member_count),
        is_verified,
        is_public: true,
        is_subscribed: true,
        last_message_date: None,
    }
}

/// Create a test search result with the given messages.
pub fn create_test_search_result(
    messages: Vec<Message>,
    query: &str,
    channels_in_results: u32,
) -> SearchResult {
    SearchResult {
        returned: messages.len() as u64,
        has_more: false,
        search_time_ms: 100,
        query_metadata: QueryMetadata {
            query: query.to_string(),
            window_from: Utc::now() - chrono::Duration::hours(48),
            window_to: None,
            channels_scanned: Some(channels_in_results),
            channels_in_results,
        },
        messages,
    }
}

/// Create an empty search result.
pub fn create_empty_search_result(query: &str) -> SearchResult {
    create_test_search_result(vec![], query, 0)
}

/// Generate an in-memory JPEG with a noisy gradient (compresses poorly, so
/// payload-cap tests can trigger the shrink loop with a small cap).
pub fn create_test_jpeg(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([
            (x % 256) as u8,
            (y % 256) as u8,
            ((x * 7 + y * 13) % 256) as u8,
        ])
    });
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut buf, 90,
        ))
        .expect("test JPEG encoding cannot fail");
    buf.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_test_message_works() {
        let msg = create_test_message(1, "Hello", 100);
        assert_eq!(msg.id.get(), 1);
        assert_eq!(msg.text, "Hello");
        assert_eq!(msg.channel_id.get(), 100);
        assert!(!msg.has_media);
    }

    #[test]
    fn create_test_message_with_media_works() {
        let msg = create_test_message_with_media(2, "Photo caption", 100, MediaType::Photo);
        assert!(msg.has_media);
        assert_eq!(msg.media_type, MediaType::Photo);
    }

    #[test]
    fn create_test_message_with_sender_works() {
        let msg = create_test_message_with_sender(3, "Hello", 100, 42, "Alice");
        assert_eq!(msg.sender_id.unwrap().get(), 42);
        assert_eq!(msg.sender_name.unwrap(), "Alice");
    }

    #[test]
    fn create_test_message_with_forward_works() {
        let msg = create_test_message_with_forward(4, "Forwarded", 100, 555, 42);
        let fwd = msg.forwarded_from.expect("forward attribution present");
        assert_eq!(fwd.channel_id.unwrap().get(), 555);
        assert_eq!(fwd.original_message_id.unwrap().get(), 42);
        assert!(fwd.channel_name.is_none());
        assert!(fwd.channel_username.is_none());
    }

    #[test]
    fn create_test_message_with_link_preview_works() {
        let msg = create_test_message_with_link_preview(5, "Link", 100, "https://example.com");
        let preview = msg.link_preview.expect("link preview present");
        assert_eq!(preview.url, "https://example.com");
        assert!(preview.title.is_none());
    }

    #[test]
    fn create_test_channel_works() {
        let channel = create_test_channel(100, "technews");
        assert_eq!(channel.id.get(), 100);
        assert_eq!(channel.username.as_ref().unwrap().as_str(), "technews");
        assert!(channel.is_subscribed);
    }

    #[test]
    fn create_test_channel_detailed_works() {
        let channel =
            create_test_channel_detailed(200, "verified_ch", "Verified Channel", 50000, true);
        assert_eq!(channel.id.get(), 200);
        assert!(channel.is_verified);
        assert_eq!(channel.member_count, Some(50000));
    }

    #[test]
    fn create_test_search_result_works() {
        let msg = create_test_message(1, "Test", 100);
        let result = create_test_search_result(vec![msg], "test query", 5);
        assert_eq!(result.returned, 1);
        assert_eq!(result.query_metadata.query, "test query");
        assert_eq!(result.query_metadata.channels_scanned, Some(5));
        assert_eq!(result.query_metadata.channels_in_results, 5);
    }

    #[test]
    fn create_empty_search_result_works() {
        let result = create_empty_search_result("empty");
        assert_eq!(result.returned, 0);
        assert!(result.messages.is_empty());
    }
}
