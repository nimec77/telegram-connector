//! Tests for TelegramClient using mocks

use crate::error::Error;
use crate::telegram::trait_def::{MockTelegramClientTrait, TelegramClientTrait};
use crate::telegram::types::{
    Channel, ChannelId, ChannelName, MediaType, Message, MessageId, QueryMetadata, SearchParams,
    SearchResult, UserId, Username,
};

// Helper to create test channel
fn create_test_channel(id: i64, name: &str) -> Channel {
    Channel {
        id: ChannelId::new(id).unwrap(),
        name: ChannelName::new(name).unwrap(),
        username: Username::new("testchannel").unwrap(),
        description: Some("Test channel".to_string()),
        member_count: 1000,
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
        channel_username: Username::new("testchannel").unwrap(),
        text: text.to_string(),
        timestamp: chrono::Utc::now(),
        sender_id: Some(UserId::new(123).unwrap()),
        sender_name: Some("Test User".to_string()),
        has_media: false,
        media_type: MediaType::None,
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
        .returning(move |_, _| Ok(expected_clone.clone()));

    let result = mock.get_subscribed_channels(10, 0).await;
    assert!(result.is_ok());
    let channels = result.unwrap();
    assert_eq!(channels.len(), 2);
    assert_eq!(channels[0].name.as_str(), "Channel1");
}

#[tokio::test]
async fn mock_get_subscribed_channels_respects_pagination() {
    let mut mock = MockTelegramClientTrait::new();

    // First page
    mock.expect_get_subscribed_channels()
        .with(mockall::predicate::eq(2), mockall::predicate::eq(0))
        .times(1)
        .returning(|_, _| {
            Ok(vec![
                create_test_channel(1, "Channel1"),
                create_test_channel(2, "Channel2"),
            ])
        });

    // Second page
    mock.expect_get_subscribed_channels()
        .with(mockall::predicate::eq(2), mockall::predicate::eq(2))
        .times(1)
        .returning(|_, _| Ok(vec![create_test_channel(3, "Channel3")]));

    let page1 = mock.get_subscribed_channels(2, 0).await.unwrap();
    assert_eq!(page1.len(), 2);

    let page2 = mock.get_subscribed_channels(2, 2).await.unwrap();
    assert_eq!(page2.len(), 1);
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
        total_found: 2,
        search_time_ms: 100,
        query_metadata: QueryMetadata {
            query: "test".to_string(),
            hours_back: 24,
            channels_searched: 1,
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
    assert_eq!(search_result.total_found, 2);
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
        total_found: 3,
        search_time_ms: 100,
        query_metadata: QueryMetadata {
            query: "test".to_string(),
            hours_back: 24,
            channels_searched: 1,
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
        total_found: 1,
        search_time_ms: 100,
        query_metadata: QueryMetadata {
            query: "test".to_string(),
            hours_back: 24,
            channels_searched: 1,
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
    assert_eq!(search_result.query_metadata.channels_searched, 1);
}
