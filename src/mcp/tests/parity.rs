//! Response-shaping parity: given the same domain `Message`, every
//! message-returning tool must serialize the same `forwarded_from`.
//!
//! Scope note — these tests mock `TelegramClientTrait`, which is ABOVE the
//! conversion layer, so they cannot prove that fetching enriches. That is
//! covered by `raw_pager`'s envelope-decode test and by the type-level
//! guard in `envelope.rs`. What this file catches is a DTO or compact-format
//! change that drops `forwarded_from` on one tool's response shape only.

use crate::mcp::server::McpServer;
use crate::mcp::tools::types::requests::GetMessagesBatchRequest;
use crate::mcp::tools::{GetMessageByLinkRequest, GetRecentMessagesRequest, SearchRequest};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{Message, MessageBatch, QueryMetadata, SearchResult};
use crate::test_helpers::create_test_message_with_enriched_forward;
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

const CHANNEL_ID: i64 = 1144180066;
const MESSAGE_ID: i64 = 298716;
const FORWARDED_FROM_ID: i64 = 1783384254;

fn fixture() -> Message {
    create_test_message_with_enriched_forward(
        MESSAGE_ID,
        "переслано",
        CHANNEL_ID,
        FORWARDED_FROM_ID,
    )
}

fn search_result(messages: Vec<Message>) -> SearchResult {
    let returned = messages.len() as u64;
    SearchResult {
        messages,
        returned,
        has_more: false,
        search_time_ms: 1,
        query_metadata: QueryMetadata {
            query: String::new(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(48),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
        },
    }
}

fn permissive_limiter() -> MockRateLimiterTrait {
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().returning(|_| Ok(()));
    limiter
}

/// The `forwarded_from` object as it appears on the wire. Handles both
/// response shapes: a `messages` array, and `get_message_by_link`'s bare
/// serialized message.
fn forward_json(response: &str) -> serde_json::Value {
    let parsed: serde_json::Value = serde_json::from_str(response).expect("valid JSON");
    let message = parsed["messages"]
        .as_array()
        .and_then(|m| m.first())
        .cloned()
        .unwrap_or_else(|| parsed.clone());
    message["forwarded_from"].clone()
}

async fn via_get_recent_messages() -> String {
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_get_recent_messages()
        .returning(|_| Ok(search_result(vec![fixture()])));

    let server = McpServer::new(Arc::new(telegram), Arc::new(permissive_limiter()));
    server
        .get_recent_messages(
            Parameters(GetRecentMessagesRequest {
                channel_id: Some(CHANNEL_ID.to_string()),
                channel_ids: None,
                hours_back: None,
                limit: None,
                media_filter: None,
                from_date: None,
                to_date: None,
                collapse_albums: None,
                before_id: None,
                after_id: None,
                max_text_length: None,
                format: None,
            }),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("get_recent_messages ok")
}

async fn via_search_messages() -> String {
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_search_messages()
        .returning(|_| Ok(search_result(vec![fixture()])));

    let server = McpServer::new(Arc::new(telegram), Arc::new(permissive_limiter()));
    // Global search (no channel_id) so no resolve_channel_identity is needed.
    server
        .search_messages(
            Parameters(SearchRequest {
                query: "переслано".to_string(),
                channel_id: None,
                channel_ids: None,
                hours_back: None,
                limit: None,
                media_filter: None,
                ..Default::default()
            }),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("search_messages ok")
}

async fn via_get_message_by_link() -> String {
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_get_message_by_id()
        .return_once(|_, _| Ok(fixture()));

    let server = McpServer::new(Arc::new(telegram), Arc::new(permissive_limiter()));
    server
        .get_message_by_link(
            Parameters(GetMessageByLinkRequest {
                link: format!("https://t.me/testchannel/{MESSAGE_ID}"),
            }),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("get_message_by_link ok")
}

async fn via_get_messages_batch() -> String {
    let mut telegram = MockTelegramClientTrait::new();
    telegram.expect_get_messages_batch().returning(|_, _| {
        Ok(MessageBatch {
            messages: vec![fixture()],
            missing_ids: Vec::new(),
        })
    });

    let server = McpServer::new(Arc::new(telegram), Arc::new(permissive_limiter()));
    server
        .get_messages_batch_impl(GetMessagesBatchRequest {
            channel_id: CHANNEL_ID.to_string(),
            message_ids: vec![MESSAGE_ID],
            max_text_length: None,
        })
        .await
        .expect("get_messages_batch ok")
}

#[tokio::test]
async fn forwarded_from_is_identical_across_every_message_returning_tool() {
    let expected = forward_json(&via_get_recent_messages().await);

    // Guard the guard: a null here would make every comparison below vacuous.
    assert_eq!(
        expected["channel_name"], "Военкор",
        "fixture must carry forward attribution, else this test proves nothing"
    );

    assert_eq!(
        forward_json(&via_search_messages().await),
        expected,
        "search_messages diverged"
    );
    assert_eq!(
        forward_json(&via_get_message_by_link().await),
        expected,
        "get_message_by_link diverged"
    );
    assert_eq!(
        forward_json(&via_get_messages_batch().await),
        expected,
        "get_messages_batch diverged"
    );
}

// get_messages_batch_impl (src/mcp/server/impl_message_batch.rs) hard-caps a
// single call at 50 ids (`MAX_BATCH_IDS`) — anything above that is rejected
// before the client is ever called. "A full batch" therefore means 50, not
// 100 as an earlier draft of this test assumed.
const FULL_BATCH_SIZE: i64 = 50;

#[tokio::test]
async fn converting_a_full_batch_issues_no_resolve_or_download_calls() {
    let mut telegram = MockTelegramClientTrait::new();

    // The batch fetch is the ONLY call permitted. A resolve or a download
    // during conversion would be a zero-extra-call violation — mockall fails
    // the test on any invocation of these.
    telegram.expect_resolve_channels().never();
    telegram.expect_download_message_media().never();

    let messages: Vec<Message> = (0..FULL_BATCH_SIZE)
        .map(|i| {
            create_test_message_with_enriched_forward(
                MESSAGE_ID + i,
                "переслано",
                CHANNEL_ID,
                FORWARDED_FROM_ID,
            )
        })
        .collect();
    telegram
        .expect_get_messages_batch()
        .times(1)
        .return_once(move |_, _| {
            Ok(MessageBatch {
                messages,
                missing_ids: Vec::new(),
            })
        });

    let server = McpServer::new(Arc::new(telegram), Arc::new(permissive_limiter()));
    let out = server
        .get_messages_batch_impl(GetMessagesBatchRequest {
            channel_id: CHANNEL_ID.to_string(),
            message_ids: (0..FULL_BATCH_SIZE).map(|i| MESSAGE_ID + i).collect(),
            max_text_length: None,
        })
        .await
        .expect("batch ok");

    let json: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(
        json["messages"][0]["forwarded_from"]["channel_name"],
        "Военкор"
    );

    // Id accounting at the MCP layer: every requested id is accounted for as
    // returned, missing, or budget-omitted — never silently dropped. (The
    // response is also subject to `[limits] response_byte_budget`, so the
    // full 50 messages may not all survive as `messages`; that's expected
    // and doesn't affect this conservation check.)
    let returned = json["messages"].as_array().map_or(0, |a| a.len());
    let missing = json["missing"].as_array().map_or(0, |a| a.len());
    let omitted = json["omitted_ids"].as_array().map_or(0, |a| a.len());
    assert_eq!(
        returned + missing + omitted,
        FULL_BATCH_SIZE as usize,
        "every requested id must be accounted for exactly once"
    );
}
