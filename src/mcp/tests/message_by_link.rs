//! Tests for get_message_by_link tool

use crate::error::Error;
use crate::mcp::server::McpServer;
use crate::mcp::tools::GetMessageByLinkRequest;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::Message;
use crate::test_helpers::create_test_message;
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

#[tokio::test]
async fn get_message_by_link_public_link_returns_message() {
    // Given: Mock client that returns a message for username + message ID
    let mut mock_client = MockTelegramClientTrait::new();
    let expected_message = create_test_message(575403, "Hello from Telegram", 999);
    let expected = expected_message.clone();

    mock_client
        .expect_get_message_by_id()
        .withf(|channel_ref, msg_id| channel_ref == "swodki" && *msg_id == 575403)
        .return_once(move |_, _| Ok(expected));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Get message by public link
    let request = GetMessageByLinkRequest {
        link: "https://t.me/swodki/575403".to_string(),
    };

    let result = server
        .get_message_by_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns the message
    assert!(result.is_ok());
    let message: Message = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(message.text, "Hello from Telegram");
    assert_eq!(message.id.get(), 575403);
}

#[tokio::test]
async fn get_message_by_link_private_link_returns_message() {
    // Given: Mock client that returns a message for numeric channel ID
    let mut mock_client = MockTelegramClientTrait::new();
    let expected_message = create_test_message(42, "Private channel post", 1234567);
    let expected = expected_message.clone();

    mock_client
        .expect_get_message_by_id()
        .withf(|channel_ref, msg_id| channel_ref == "1234567" && *msg_id == 42)
        .return_once(move |_, _| Ok(expected));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Get message by private link
    let request = GetMessageByLinkRequest {
        link: "https://t.me/c/1234567/42".to_string(),
    };

    let result = server
        .get_message_by_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns the message
    assert!(result.is_ok());
    let message: Message = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(message.text, "Private channel post");
    assert_eq!(message.id.get(), 42);
}

#[tokio::test]
async fn get_message_by_link_invalid_link_returns_error() {
    // Given: Server (no mock expectations — parse should fail before API call)
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Invalid link
    let request = GetMessageByLinkRequest {
        link: "https://example.com/not/telegram".to_string(),
    };

    let result = server
        .get_message_by_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns parse error (no API call made)
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Not a valid t.me link"));
}

#[tokio::test]
async fn get_message_by_link_channel_not_found() {
    // Given: Mock client that returns channel-not-found error
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_get_message_by_id().return_once(|_, _| {
        Err(Error::InvalidInput(
            "Channel not found: nonexistent".to_string(),
        ))
    });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Link to non-existent channel
    let request = GetMessageByLinkRequest {
        link: "https://t.me/nonexistent/123".to_string(),
    };

    let result = server
        .get_message_by_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns error from client
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Channel not found"));
}

#[tokio::test]
async fn get_message_by_link_message_not_found() {
    // Given: Mock client that returns message-not-found error
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_get_message_by_id().return_once(|_, _| {
        Err(Error::InvalidInput(
            "Message 999999 not found or deleted in channel swodki".to_string(),
        ))
    });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Link to non-existent message
    let request = GetMessageByLinkRequest {
        link: "https://t.me/swodki/999999".to_string(),
    };

    let result = server
        .get_message_by_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns error
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Message 999999 not found"));
}

#[tokio::test]
async fn get_message_by_link_rate_limited() {
    // Given: Rate limiter that rejects
    let mock_client = MockTelegramClientTrait::new();
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| {
        Err(Error::RateLimit {
            retry_after_seconds: 5,
            detail: String::new(),
        })
    });

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetMessageByLinkRequest {
        link: "https://t.me/swodki/575403".to_string(),
    };

    // When: Rate limited
    let result = server
        .get_message_by_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    // Then: Returns rate limit error (no API call made)
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("rate limit"));
}
