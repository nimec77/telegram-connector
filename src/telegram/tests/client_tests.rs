//! Tests for TelegramClient using mocks

use crate::error::Error;
use crate::telegram::trait_def::{MockTelegramClientTrait, TelegramClientTrait};
use crate::telegram::types::{
    Channel, ChannelId, ChannelName, ChannelPage, ChatType, MediaType, Message, MessageId,
    QueryMetadata, SearchParams, SearchResult, UserId, Username,
};

// Helper to create test channel
fn create_test_channel(id: i64, name: &str) -> Channel {
    Channel {
        id: ChannelId::new(id).unwrap(),
        name: ChannelName::new(name).unwrap(),
        username: Some(Username::new("testchannel").unwrap()),
        chat_type: ChatType::Channel,
        description: Some("Test channel".to_string()),
        member_count: Some(1000),
        is_verified: false,
        is_public: true,
        is_subscribed: true,
        last_message_date: None,
    }
}

// Helper to create test message
fn create_test_message(id: i32, text: &str, channel_id: i64) -> Message {
    Message {
        id: MessageId::new(id as i64).unwrap(),
        channel_id: ChannelId::new(channel_id).unwrap(),
        channel_name: ChannelName::new("TestChannel").unwrap(),
        channel_username: Some(Username::new("testchannel").unwrap()),
        text: text.to_string(),
        timestamp: chrono::Utc::now(),
        sender_id: Some(UserId::new(123).unwrap()),
        sender_name: Some("Test User".to_string()),
        has_media: false,
        media_type: MediaType::None,
        forwarded_from: None,
        link_preview: None,
        views: None,
        forwards: None,
        reply_to_message_id: None,
        video_info: None,
        audio_info: None,
    }
}

// ========================================
// Mock-based tests
// ========================================

#[tokio::test]
async fn mock_is_connected_returns_true() {
    let mut mock = MockTelegramClientTrait::new();
    mock.expect_is_connected().times(1).returning(|| true);

    assert!(mock.is_connected().await);
}

#[tokio::test]
async fn mock_is_connected_returns_false() {
    let mut mock = MockTelegramClientTrait::new();
    mock.expect_is_connected().times(1).returning(|| false);

    assert!(!mock.is_connected().await);
}

#[tokio::test]
async fn mock_get_subscribed_channels_returns_list() {
    let mut mock = MockTelegramClientTrait::new();

    let expected_channels = vec![
        create_test_channel(1, "Channel1"),
        create_test_channel(2, "Channel2"),
    ];
    let expected_clone = expected_channels.clone();

    mock.expect_get_subscribed_channels()
        .with(mockall::predicate::eq(10), mockall::predicate::eq(0))
        .times(1)
        .returning(move |_, _| {
            Ok(ChannelPage {
                channels: expected_clone.clone(),
                total: 2,
            })
        });

    let result = mock.get_subscribed_channels(10, 0).await;
    assert!(result.is_ok());
    let page = result.unwrap();
    assert_eq!(page.channels.len(), 2);
    assert_eq!(page.total, 2);
    assert_eq!(page.channels[0].name.as_str(), "Channel1");
}

#[tokio::test]
async fn mock_get_subscribed_channels_respects_pagination() {
    let mut mock = MockTelegramClientTrait::new();

    // First page
    mock.expect_get_subscribed_channels()
        .with(mockall::predicate::eq(2), mockall::predicate::eq(0))
        .times(1)
        .returning(|_, _| {
            Ok(ChannelPage {
                channels: vec![
                    create_test_channel(1, "Channel1"),
                    create_test_channel(2, "Channel2"),
                ],
                total: 3,
            })
        });

    // Second page
    mock.expect_get_subscribed_channels()
        .with(mockall::predicate::eq(2), mockall::predicate::eq(2))
        .times(1)
        .returning(|_, _| {
            Ok(ChannelPage {
                channels: vec![create_test_channel(3, "Channel3")],
                total: 3,
            })
        });

    let page1 = mock.get_subscribed_channels(2, 0).await.unwrap();
    assert_eq!(page1.channels.len(), 2);
    assert_eq!(page1.total, 3);

    let page2 = mock.get_subscribed_channels(2, 2).await.unwrap();
    assert_eq!(page2.channels.len(), 1);
    assert_eq!(page2.total, 3);
}

#[tokio::test]
async fn mock_get_channel_info_by_username() {
    let mut mock = MockTelegramClientTrait::new();
    let expected_channel = create_test_channel(123, "TestChannel");
    let expected_clone = expected_channel.clone();

    mock.expect_get_channel_info()
        .with(mockall::predicate::eq("@testchannel"))
        .times(1)
        .returning(move |_| Ok(expected_clone.clone()));

    let result = mock.get_channel_info("@testchannel").await;
    assert!(result.is_ok());
    let channel = result.unwrap();
    assert_eq!(channel.name.as_str(), "TestChannel");
}

#[tokio::test]
async fn mock_get_channel_info_by_id() {
    let mut mock = MockTelegramClientTrait::new();
    let expected_channel = create_test_channel(123, "TestChannel");
    let expected_clone = expected_channel.clone();

    mock.expect_get_channel_info()
        .with(mockall::predicate::eq("123"))
        .times(1)
        .returning(move |_| Ok(expected_clone.clone()));

    let result = mock.get_channel_info("123").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn mock_get_channel_info_empty_identifier_fails() {
    let mut mock = MockTelegramClientTrait::new();

    mock.expect_get_channel_info()
        .with(mockall::predicate::eq(""))
        .times(1)
        .returning(|_| {
            Err(Error::InvalidInput(
                "Channel identifier cannot be empty".to_string(),
            ))
        });

    let result = mock.get_channel_info("").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot be empty"));
}

#[tokio::test]
async fn mock_search_messages_returns_results() {
    let mut mock = MockTelegramClientTrait::new();

    let expected_messages = vec![
        create_test_message(1, "Test message 1", 100),
        create_test_message(2, "Test message 2", 100),
    ];

    let expected_result = SearchResult {
        messages: expected_messages.clone(),
        returned: 2,
        search_time_ms: 100,
        query_metadata: QueryMetadata {
            query: "test".to_string(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(24),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
        },
    };
    let expected_clone = expected_result.clone();

    mock.expect_search_messages()
        .times(1)
        .returning(move |_| Ok(expected_clone.clone()));

    let params = SearchParams::new("test".to_string());
    let result = mock.search_messages(&params).await;

    assert!(result.is_ok());
    let search_result = result.unwrap();
    assert_eq!(search_result.messages.len(), 2);
    assert_eq!(search_result.returned, 2);
}

#[tokio::test]
async fn mock_search_messages_empty_query_fails() {
    let mut mock = MockTelegramClientTrait::new();

    mock.expect_search_messages().times(1).returning(|_| {
        Err(Error::InvalidInput(
            "Search query cannot be empty".to_string(),
        ))
    });

    let params = SearchParams::new("".to_string());
    let result = mock.search_messages(&params).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot be empty"));
}

#[tokio::test]
async fn mock_search_messages_respects_limit() {
    let mut mock = MockTelegramClientTrait::new();

    // Create 5 messages but limit to 3
    let all_messages = vec![
        create_test_message(1, "Message 1", 100),
        create_test_message(2, "Message 2", 100),
        create_test_message(3, "Message 3", 100),
    ];

    let expected_result = SearchResult {
        messages: all_messages.clone(),
        returned: 3,
        search_time_ms: 100,
        query_metadata: QueryMetadata {
            query: "test".to_string(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(24),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
        },
    };
    let expected_clone = expected_result.clone();

    mock.expect_search_messages()
        .times(1)
        .returning(move |_| Ok(expected_clone.clone()));

    let params = SearchParams {
        query: "test".to_string(),
        limit: 3,
        ..Default::default()
    };

    let result = mock.search_messages(&params).await;
    assert!(result.is_ok());
    let search_result = result.unwrap();
    assert_eq!(search_result.messages.len(), 3);
}

#[tokio::test]
async fn mock_search_messages_with_channel_filter() {
    let mut mock = MockTelegramClientTrait::new();

    let expected_messages = vec![create_test_message(1, "Message from specific channel", 100)];

    let expected_result = SearchResult {
        messages: expected_messages.clone(),
        returned: 1,
        search_time_ms: 100,
        query_metadata: QueryMetadata {
            query: "test".to_string(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(24),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
        },
    };
    let expected_clone = expected_result.clone();

    mock.expect_search_messages()
        .times(1)
        .returning(move |_| Ok(expected_clone.clone()));

    let params = SearchParams {
        query: "test".to_string(),
        channel_id: Some(ChannelId::new(100).unwrap()),
        ..Default::default()
    };

    let result = mock.search_messages(&params).await;
    assert!(result.is_ok());
    let search_result = result.unwrap();
    assert_eq!(search_result.query_metadata.channels_scanned, Some(1));
}

#[tokio::test]
async fn mock_search_messages_with_media_filter_photo() {
    use crate::telegram::types::MediaFilter;

    let mut mock = MockTelegramClientTrait::new();

    let expected_messages = vec![create_test_message(1, "Photo message", 100)];

    let expected_result = SearchResult {
        messages: expected_messages.clone(),
        returned: 1,
        search_time_ms: 50,
        query_metadata: QueryMetadata {
            query: "".to_string(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(48),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
        },
    };
    let expected_clone = expected_result.clone();

    // Verify that SearchParams with media_filter is passed correctly
    mock.expect_search_messages()
        .withf(|params| params.media_filter == Some(MediaFilter::Photo) && params.query.is_empty())
        .times(1)
        .returning(move |_| Ok(expected_clone.clone()));

    let params = SearchParams {
        query: "".to_string(),
        media_filter: Some(MediaFilter::Photo),
        ..Default::default()
    };

    let result = mock.search_messages(&params).await;
    assert!(result.is_ok());
    let search_result = result.unwrap();
    assert_eq!(search_result.messages.len(), 1);
}

#[tokio::test]
async fn mock_search_messages_with_media_filter_document() {
    use crate::telegram::types::MediaFilter;

    let mut mock = MockTelegramClientTrait::new();

    let expected_messages = vec![
        create_test_message(1, "Document 1", 100),
        create_test_message(2, "Document 2", 100),
    ];

    let expected_result = SearchResult {
        messages: expected_messages.clone(),
        returned: 2,
        search_time_ms: 75,
        query_metadata: QueryMetadata {
            query: "report".to_string(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(24),
            window_to: None,
            channels_scanned: Some(3),
            channels_in_results: 3,
        },
    };
    let expected_clone = expected_result.clone();

    // Verify that SearchParams with media_filter and query is passed correctly
    mock.expect_search_messages()
        .withf(|params| {
            params.media_filter == Some(MediaFilter::Document)
                && params.query == "report"
                && params.hours_back == 24
        })
        .times(1)
        .returning(move |_| Ok(expected_clone.clone()));

    let params = SearchParams {
        query: "report".to_string(),
        hours_back: 24,
        media_filter: Some(MediaFilter::Document),
        ..Default::default()
    };

    let result = mock.search_messages(&params).await;
    assert!(result.is_ok());
    let search_result = result.unwrap();
    assert_eq!(search_result.messages.len(), 2);
    assert_eq!(search_result.query_metadata.channels_scanned, Some(3));
}

// =============================================================================
// get_recent_messages tests
// =============================================================================

#[tokio::test]
async fn mock_get_recent_messages_returns_results() {
    use crate::telegram::types::HistoryParams;

    let mut mock = MockTelegramClientTrait::new();

    let expected_messages = vec![
        create_test_message(1, "Recent message 1", 100),
        create_test_message(2, "Recent message 2", 100),
        create_test_message(3, "Recent message 3", 100),
    ];

    let expected_result = SearchResult {
        messages: expected_messages.clone(),
        returned: 3,
        search_time_ms: 50,
        query_metadata: QueryMetadata {
            query: String::new(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(48),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
        },
    };
    let expected_clone = expected_result.clone();

    mock.expect_get_recent_messages()
        .times(1)
        .returning(move |_| Ok(expected_clone.clone()));

    let channel_id = ChannelId::new(100).unwrap();
    let params = HistoryParams::new(channel_id);

    let result = mock.get_recent_messages(&params).await;
    assert!(result.is_ok());
    let history_result = result.unwrap();
    assert_eq!(history_result.messages.len(), 3);
    assert_eq!(history_result.query_metadata.channels_scanned, Some(1));
    assert!(history_result.query_metadata.query.is_empty());
}

#[tokio::test]
async fn mock_get_recent_messages_with_media_filter() {
    use crate::telegram::types::{HistoryParams, MediaFilter};

    let mut mock = MockTelegramClientTrait::new();

    let expected_messages = vec![create_test_message(1, "Photo message", 100)];

    let expected_result = SearchResult {
        messages: expected_messages.clone(),
        returned: 1,
        search_time_ms: 30,
        query_metadata: QueryMetadata {
            query: String::new(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(24),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
        },
    };
    let expected_clone = expected_result.clone();

    mock.expect_get_recent_messages()
        .withf(|params| params.media_filter == Some(MediaFilter::Photo) && params.hours_back == 24)
        .times(1)
        .returning(move |_| Ok(expected_clone.clone()));

    let channel_id = ChannelId::new(100).unwrap();
    let params = HistoryParams::new(channel_id)
        .hours_back(24)
        .media_filter(MediaFilter::Photo);

    let result = mock.get_recent_messages(&params).await;
    assert!(result.is_ok());
    let history_result = result.unwrap();
    assert_eq!(history_result.messages.len(), 1);
}

#[tokio::test]
async fn mock_get_recent_messages_respects_limit() {
    use crate::telegram::types::HistoryParams;

    let mut mock = MockTelegramClientTrait::new();

    let expected_messages = vec![
        create_test_message(1, "Message 1", 100),
        create_test_message(2, "Message 2", 100),
        create_test_message(3, "Message 3", 100),
        create_test_message(4, "Message 4", 100),
        create_test_message(5, "Message 5", 100),
    ];

    let expected_result = SearchResult {
        messages: expected_messages.clone(),
        returned: 5,
        search_time_ms: 40,
        query_metadata: QueryMetadata {
            query: String::new(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(48),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
        },
    };
    let expected_clone = expected_result.clone();

    mock.expect_get_recent_messages()
        .withf(|params| params.limit == 5)
        .times(1)
        .returning(move |_| Ok(expected_clone.clone()));

    let channel_id = ChannelId::new(100).unwrap();
    let params = HistoryParams::new(channel_id).limit(5);

    let result = mock.get_recent_messages(&params).await;
    assert!(result.is_ok());
    let history_result = result.unwrap();
    assert_eq!(history_result.messages.len(), 5);
}

#[tokio::test]
async fn mock_get_recent_messages_channel_not_found() {
    use crate::telegram::types::HistoryParams;

    let mut mock = MockTelegramClientTrait::new();

    mock.expect_get_recent_messages()
        .times(1)
        .returning(|params| {
            Err(Error::InvalidInput(format!(
                "Channel not found: {:?}",
                params.channel_id
            )))
        });

    let channel_id = ChannelId::new(999999).unwrap();
    let params = HistoryParams::new(channel_id);

    let result = mock.get_recent_messages(&params).await;
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.to_string().contains("Channel not found"));
}

#[tokio::test]
async fn mock_get_recent_messages_empty_result() {
    use crate::telegram::types::HistoryParams;

    let mut mock = MockTelegramClientTrait::new();

    let expected_result = SearchResult {
        messages: vec![],
        returned: 0,
        search_time_ms: 10,
        query_metadata: QueryMetadata {
            query: String::new(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(1), // Very short time window
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
        },
    };
    let expected_clone = expected_result.clone();

    mock.expect_get_recent_messages()
        .withf(|params| params.hours_back == 1)
        .times(1)
        .returning(move |_| Ok(expected_clone.clone()));

    let channel_id = ChannelId::new(100).unwrap();
    let params = HistoryParams::new(channel_id).hours_back(1);

    let result = mock.get_recent_messages(&params).await;
    assert!(result.is_ok());
    let history_result = result.unwrap();
    assert!(history_result.messages.is_empty());
    assert_eq!(history_result.returned, 0);
}

#[tokio::test]
async fn mock_download_message_media_returns_media_download() {
    use crate::telegram::types::{MediaDownload, MediaType};

    let mut mock = MockTelegramClientTrait::new();
    mock.expect_download_message_media()
        .withf(|channel_ref, msg_id, max_dim| {
            channel_ref == "news" && *msg_id == 42 && *max_dim == 1280
        })
        .return_once(|_, _, _| {
            Ok(MediaDownload {
                bytes: vec![0xff, 0xd8, 0xff],
                media_type: MediaType::Photo,
                is_thumbnail: false,
                caption: None,
                width: Some(1280),
                height: Some(720),
                source_size_bytes: 3,
                video_info: None,
            })
        });

    let result = mock.download_message_media("news", 42, 1280).await.unwrap();
    assert_eq!(result.media_type, MediaType::Photo);
    assert_eq!(result.bytes.len(), 3);
}

// `username_to_resolve` is the pure decision the AD-1 consolidation kept from the
// old inline `get_recent_messages` resolver: attempt a username lookup only when
// the identifier (after stripping `@`) is not purely numeric, otherwise fall back
// to the dialog walk by numeric id. One case per prior branch.
mod username_to_resolve_tests {
    use crate::telegram::client::username_to_resolve;

    #[test]
    fn plain_username_is_resolved() {
        assert_eq!(username_to_resolve("durov"), Some("durov"));
    }

    #[test]
    fn at_prefixed_username_is_resolved_with_prefix_kept() {
        // The `@` is stripped later by resolve_username_peer; the returned ref is
        // the raw identifier so the caller's logs match the original.
        assert_eq!(username_to_resolve("@durov"), Some("@durov"));
    }

    #[test]
    fn purely_numeric_identifier_is_not_resolved_as_username() {
        assert_eq!(username_to_resolve("123456"), None);
    }

    #[test]
    fn at_prefixed_numeric_is_not_resolved_as_username() {
        assert_eq!(username_to_resolve("@123456"), None);
    }

    #[test]
    fn empty_or_bare_at_is_not_resolved_as_username() {
        // "@" strips to "" which `chars().all(..)` treats as all-numeric -> None,
        // matching the original guard.
        assert_eq!(username_to_resolve("@"), None);
        assert_eq!(username_to_resolve(""), None);
    }

    #[test]
    fn alphanumeric_username_is_resolved() {
        assert_eq!(username_to_resolve("channel123"), Some("channel123"));
    }
}
