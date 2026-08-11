//! Tests for get_subscribed_channels and get_channel_info tools

use crate::mcp::server::McpServer;
use crate::mcp::tools::{ChannelsResponse, GetChannelInfoRequest, GetChannelsRequest};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::Username;
use crate::telegram::{Channel, ChannelId, ChannelName, ChatType};
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

/// Helper to create test channel
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

/// Build `n` distinct test channels (ids 1000.., names "Channel N").
fn n_channels(n: usize) -> Vec<Channel> {
    (0..n)
        .map(|i| create_test_channel(1000 + i as i64, &format!("Channel {i}")))
        .collect()
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
            mockall::predicate::eq(21), // default limit (20) + 1 over-fetch (CQ-5)
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
            mockall::predicate::eq(11), // custom limit (10) + 1 over-fetch (CQ-5)
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
        username: Some(Username::new("testchannel").unwrap()),
        chat_type: ChatType::Channel,
        description: Some("A test channel".to_string()),
        member_count: Some(5000),
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
        include_full: None,
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
    assert_eq!(channel.member_count, Some(5000));
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
        include_full: None,
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

#[tokio::test]
async fn get_channel_info_unfetched_member_count_serializes_as_null() {
    // A channel whose member count was not fetched must cross the MCP boundary
    // as member_count: null, not a misleading 0 (CQ-4).
    let mut mock_client = MockTelegramClientTrait::new();
    let test_channel = Channel {
        id: ChannelId::new(777).unwrap(),
        name: ChannelName::new("Unfetched").unwrap(),
        username: Some(Username::new("unfetched").unwrap()),
        chat_type: ChatType::Channel,
        description: None,
        member_count: None,
        is_verified: false,
        is_public: true,
        is_subscribed: true,
        last_message_date: None,
    };

    mock_client
        .expect_get_channel_info()
        .with(mockall::predicate::eq("unfetched"))
        .return_once(move |_| Ok(test_channel));

    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetChannelInfoRequest {
        channel_identifier: "unfetched".to_string(),
        include_full: None,
    };

    let result = server
        .get_channel_info(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .expect("tool call should succeed");

    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert!(
        json["member_count"].is_null(),
        "unfetched member_count must cross the MCP boundary as null, got {}",
        json["member_count"]
    );
}

#[tokio::test]
async fn include_full_routes_to_full_channel_info() {
    // include_full: true must route to get_full_channel_info (the extra RPC),
    // not the basic get_channel_info path.
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_get_full_channel_info()
        .with(mockall::predicate::eq("@news"))
        .return_once(|_| {
            let mut channel = create_test_channel(42, "News Channel");
            channel.description = Some("full description".to_string());
            channel.member_count = Some(12345);
            Ok(channel)
        });

    // The full path meters its extra RPC.
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetChannelInfoRequest {
        channel_identifier: "@news".to_string(),
        include_full: Some(true),
    };

    let result = server
        .get_channel_info(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .expect("tool call should succeed");

    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(json["description"], "full description");
    assert_eq!(json["member_count"], 12345);
}

#[tokio::test]
async fn include_full_absent_keeps_basic_path() {
    // include_full: None must keep using the cheap basic-info path
    // (get_channel_info), not the extra-RPC full path.
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_get_channel_info()
        .with(mockall::predicate::eq("@news"))
        .return_once(|_| Ok(create_test_channel(42, "News Channel")));

    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetChannelInfoRequest {
        channel_identifier: "@news".to_string(),
        include_full: None,
    };

    let result = server
        .get_channel_info(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn get_subscribed_channels_no_false_has_more_at_exact_boundary() {
    // Exactly `limit` channels exist, so the next page is empty and has_more
    // must be false. Pre-CQ-5 the server fetched exactly `limit` and reported
    // has_more = (len >= limit) = true — a misleading wasted round-trip. The mock
    // returns min(available, requested) so it models a finite dialog list.
    let available = 3usize;
    let limit = 3u32;

    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_get_subscribed_channels()
        .returning(move |requested, _offset| Ok(n_channels((requested as usize).min(available))));

    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetChannelsRequest {
        limit: Some(limit),
        offset: None,
    };

    let result = server
        .get_subscribed_channels(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .expect("tool call should succeed");
    let response: ChannelsResponse = serde_json::from_str(&result).unwrap();

    assert_eq!(response.channels.len(), 3);
    assert_eq!(response.total, 3);
    assert!(
        !response.has_more,
        "exactly `limit` channels exist; has_more must be false"
    );
}

#[tokio::test]
async fn get_subscribed_channels_reports_has_more_when_more_exist() {
    // More than `limit` channels exist: the server over-fetches one extra to
    // detect the next page, reports has_more = true, and truncates to `limit`.
    let available = 5usize;
    let limit = 3u32;

    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_get_subscribed_channels()
        .returning(move |requested, _offset| Ok(n_channels((requested as usize).min(available))));

    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetChannelsRequest {
        limit: Some(limit),
        offset: None,
    };

    let result = server
        .get_subscribed_channels(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .expect("tool call should succeed");
    let response: ChannelsResponse = serde_json::from_str(&result).unwrap();

    assert_eq!(response.channels.len(), 3, "page truncated to limit");
    assert_eq!(response.total, 3);
    assert!(response.has_more, "more than `limit` channels exist");
}

#[tokio::test]
async fn include_full_meters_the_extra_rpc() {
    // The include_full path performs an extra Telegram RPC (channels.getFullChannel),
    // so it must acquire a rate-limiter token like every other RPC-issuing tool.
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_get_full_channel_info()
        .with(mockall::predicate::eq("@news"))
        .return_once(|_| Ok(create_test_channel(42, "News Channel")));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter
        .expect_acquire()
        .with(mockall::predicate::eq(1))
        .times(1)
        .returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetChannelInfoRequest {
        channel_identifier: "@news".to_string(),
        include_full: Some(true),
    };

    let result = server
        .get_channel_info(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_ok(), "got {:?}", result.err());
}

#[tokio::test]
async fn include_full_propagates_rate_limit_error() {
    // A denied token must stop the request before the extra RPC is issued.
    let mock_client = MockTelegramClientTrait::new(); // no client call expected
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| {
        Err(crate::error::Error::RateLimit {
            retry_after_seconds: 5,
        })
    });

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetChannelInfoRequest {
        channel_identifier: "@news".to_string(),
        include_full: Some(true),
    };

    let result = server
        .get_channel_info(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("rate limit"));
}
