//! get_messages_batch tool tests (work-order A1).

use crate::mcp::server::McpServer;
use crate::mcp::tools::types::requests::GetMessagesBatchRequest;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::MessageBatch;
use crate::test_helpers::create_test_message;
use std::sync::Arc;

fn request(ids: Vec<i64>) -> GetMessagesBatchRequest {
    GetMessagesBatchRequest {
        channel_id: "swodki".to_string(),
        message_ids: ids,
        max_text_length: None,
    }
}

#[tokio::test]
async fn batch_returns_found_and_missing() {
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_get_messages_batch()
        .withf(|c, ids| c == "swodki" && ids == [610119, 609784])
        .returning(|_, _| {
            Ok(MessageBatch {
                messages: vec![create_test_message(610119, "text", 1144180066)],
                missing_ids: vec![609784],
            })
        });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().times(1).returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(telegram), Arc::new(limiter));
    let out = server
        .get_messages_batch_impl(request(vec![610119, 609784]))
        .await
        .expect("ok");
    let json: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(json["returned"], 1);
    assert_eq!(json["messages"][0]["id"], 610119);
    assert_eq!(json["missing"][0]["id"], 609784);
    assert_eq!(json["missing"][0]["error"], "not found or deleted");
    assert!(json.get("omitted_ids").is_none());
}

#[tokio::test]
async fn batch_rejects_empty_and_oversized_id_lists() {
    let server = McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    );
    let err = server
        .get_messages_batch_impl(request(vec![]))
        .await
        .unwrap_err();
    assert!(err.contains("message_ids"), "got: {err}");

    let err = server
        .get_messages_batch_impl(request((1..=51).collect()))
        .await
        .unwrap_err();
    assert!(err.contains("50"), "cap must be named: {err}");
}

#[tokio::test]
async fn batch_dedupes_ids_preserving_order() {
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_get_messages_batch()
        .withf(|_, ids| ids == [7, 3])
        .returning(|_, _| {
            Ok(MessageBatch {
                messages: vec![],
                missing_ids: vec![7, 3],
            })
        });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(telegram), Arc::new(limiter));
    server
        .get_messages_batch_impl(request(vec![7, 3, 7]))
        .await
        .expect("deduped call succeeds");
}
