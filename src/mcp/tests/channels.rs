//! Tests for get_subscribed_channels and get_channel_info tools

use crate::mcp::server::McpServer;
use crate::mcp::tools::{ChannelsResponse, GetChannelInfoRequest, GetChannelsRequest};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::Username;
use crate::telegram::{Channel, ChannelId, ChannelName};
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

/// Helper to create test channel
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

#[tokio::test]
async fn get_subscribed_channels_returns_list() {
    // Given: Mock client returning test channels
    let mut mock_client = MockTelegramClientTrait::new();
    let test_channels = vec![
        create_test_channel(123, "Channel 1"),
        create_test_channel(456, "Channel 2"),
    ];
    let expected = test_channels.clone();

    mock_client
        .expect_get_subscribed_channels()
        .with(
            mockall::predicate::eq(20), // default limit
            mockall::predicate::eq(0),  // default offset
        )
        .return_once(move |_, _| Ok(expected));

    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Call get_subscribed_channels with defaults
    let request = GetChannelsRequest {
        limit: None,
        offset: None,
    };

    let result = server
        .get_subscribed_channels(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns success with channel list
    assert!(result.is_ok());
    let response: ChannelsResponse = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(response.channels.len(), 2);
    assert_eq!(response.total, 2);
    assert!(!response.has_more); // 2 channels < 20 limit
}

#[tokio::test]
async fn get_subscribed_channels_respects_pagination() {
    // Given: Mock client with custom pagination parameters
    let mut mock_client = MockTelegramClientTrait::new();
    let test_channels = vec![create_test_channel(789, "Channel 3")];
    let expected = test_channels.clone();

    mock_client
        .expect_get_subscribed_channels()
        .with(
            mockall::predicate::eq(10), // custom limit
            mockall::predicate::eq(5),  // custom offset
        )
        .return_once(move |_, _| Ok(expected));

    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Call with custom pagination
    let request = GetChannelsRequest {
        limit: Some(10),
        offset: Some(5),
    };

    let result = server
        .get_subscribed_channels(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns success with correct pagination values
    assert!(result.is_ok());
    let response: ChannelsResponse = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(response.channels.len(), 1);
    assert_eq!(response.total, 1);
    assert!(!response.has_more); // 1 channel < 10 limit
}

#[tokio::test]
async fn get_channel_info_returns_channel_details() {
    // Given: Mock client returning channel details
    let mut mock_client = MockTelegramClientTrait::new();
    let test_channel = Channel {
        id: ChannelId::new(12345).unwrap(),
        name: ChannelName::new("Test Channel").unwrap(),
        username: Username::new("testchannel").unwrap(),
        description: Some("A test channel".to_string()),
        member_count: 5000,
        is_verified: true,
        is_public: true,
        is_subscribed: false,
        last_message_date: None,
    };
    let expected = test_channel.clone();

    mock_client
        .expect_get_channel_info()
        .with(mockall::predicate::eq("testchannel"))
        .return_once(move |_| Ok(expected));

    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Call get_channel_info
    let request = GetChannelInfoRequest {
        channel_identifier: "testchannel".to_string(),
    };

    let result = server
        .get_channel_info(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns channel details
    assert!(result.is_ok());
    let channel: Channel = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(channel.id, ChannelId::new(12345).unwrap());
    assert_eq!(channel.name.as_str(), "Test Channel");
    assert!(channel.is_verified);
    assert_eq!(channel.member_count, 5000);
}

#[tokio::test]
async fn get_channel_info_handles_error() {
    use crate::error::Error;

    // Given: Mock client returning error
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_get_channel_info()
        .with(mockall::predicate::eq("nonexistent"))
        .return_once(move |_| Err(Error::TelegramApi("Channel not found".to_string())));

    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Call get_channel_info with nonexistent channel
    let request = GetChannelInfoRequest {
        channel_identifier: "nonexistent".to_string(),
    };

    let result = server
        .get_channel_info(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns error
    assert!(result.is_err());
    if let Err(error_msg) = result {
        assert!(error_msg.contains("Channel not found"));
    }
}
