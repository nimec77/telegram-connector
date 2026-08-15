//! Tests for transcribe_voice_message tool

use crate::error::Error;
use crate::mcp::server::McpServer;
use crate::mcp::tools::{TranscribeVoiceMessageRequest, TranscribeVoiceMessageResponse};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{MediaType, TranscriptionOutcome};
use mockall::predicate::eq;
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::NumberOrString;
use std::sync::Arc;

fn request(channel: &str, message_id: i64, timeout: Option<u32>) -> TranscribeVoiceMessageRequest {
    TranscribeVoiceMessageRequest {
        channel_id: channel.to_string(),
        message_id,
        timeout_seconds: timeout,
    }
}

fn server(
    client: MockTelegramClientTrait,
    limiter: MockRateLimiterTrait,
) -> McpServer<MockTelegramClientTrait, MockRateLimiterTrait> {
    McpServer::new(Arc::new(client), Arc::new(limiter)).with_transcription_cost(5)
}

#[tokio::test]
async fn returns_transcription_text() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| Some(true));
    client
        .expect_transcribe_audio()
        .withf(|ch, id, t| ch == "news" && *id == 42 && *t == 30)
        .return_once(|_, _, _| {
            Ok(TranscriptionOutcome {
                text: "привет мир".to_string(),
                partial: false,
                media_type: MediaType::Voice,
                duration_seconds: Some(5),
            })
        });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().with(eq(5)).return_once(|_| Ok(()));

    let result = server(client, limiter)
        .transcribe_voice_message(
            Parameters(request("news", 42, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("should succeed");

    let resp: TranscribeVoiceMessageResponse = serde_json::from_str(&result).unwrap();
    assert_eq!(resp.text, "привет мир");
    assert!(!resp.partial);
    assert_eq!(resp.media_type, MediaType::Voice);
    assert_eq!(resp.duration_seconds, Some(5));
}

#[tokio::test]
async fn returns_partial_flag_and_video_note_type() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| Some(true));
    client.expect_transcribe_audio().return_once(|_, _, _| {
        Ok(TranscriptionOutcome {
            text: "часть".to_string(),
            partial: true,
            media_type: MediaType::VideoNote,
            duration_seconds: None,
        })
    });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().with(eq(5)).return_once(|_| Ok(()));

    let result = server(client, limiter)
        .transcribe_voice_message(
            Parameters(request("chan", 1, Some(60))),
            RequestId(NumberOrString::Number(2)),
        )
        .await
        .expect("should succeed");

    let resp: TranscribeVoiceMessageResponse = serde_json::from_str(&result).unwrap();
    assert!(resp.partial);
    assert_eq!(resp.media_type, MediaType::VideoNote);
    assert_eq!(resp.duration_seconds, None);
}

#[tokio::test]
async fn premium_absent_fast_fails_without_calling_transcribe() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| Some(false));
    // No expect_transcribe_audio: mockall panics if it is called.
    // No expect_acquire: the rate limiter must not be charged on fast-fail.
    let limiter = MockRateLimiterTrait::new();

    let err = server(client, limiter)
        .transcribe_voice_message(
            Parameters(request("news", 42, None)),
            RequestId(NumberOrString::Number(3)),
        )
        .await
        .expect_err("should fail without Premium");

    assert!(err.contains("Premium"), "error was: {err}");
}

#[tokio::test]
async fn unknown_premium_proceeds() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| None);
    client.expect_transcribe_audio().return_once(|_, _, _| {
        Ok(TranscriptionOutcome {
            text: "ok".to_string(),
            partial: false,
            media_type: MediaType::Voice,
            duration_seconds: None,
        })
    });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().with(eq(5)).return_once(|_| Ok(()));

    let result = server(client, limiter)
        .transcribe_voice_message(
            Parameters(request("news", 42, None)),
            RequestId(NumberOrString::Number(4)),
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn rejects_non_voice_media() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| Some(true));
    client.expect_transcribe_audio().return_once(|_, _, _| {
        Err(Error::NotTranscribable {
            media_type: "photo".to_string(),
        })
    });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().with(eq(5)).return_once(|_| Ok(()));

    let err = server(client, limiter)
        .transcribe_voice_message(
            Parameters(request("news", 7, None)),
            RequestId(NumberOrString::Number(5)),
        )
        .await
        .expect_err("should reject");

    assert!(err.contains("not transcribable"), "error was: {err}");
    assert!(err.contains("photo"), "error was: {err}");
}

#[tokio::test]
async fn surfaces_voice_too_long() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| Some(true));
    client
        .expect_transcribe_audio()
        .return_once(|_, _, _| Err(Error::VoiceTooLong));
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().with(eq(5)).return_once(|_| Ok(()));

    let err = server(client, limiter)
        .transcribe_voice_message(
            Parameters(request("chan", 1, None)),
            RequestId(NumberOrString::Number(6)),
        )
        .await
        .expect_err("should fail");

    assert!(err.contains("length limit"), "error was: {err}");
}

#[tokio::test]
async fn surfaces_flood_wait_retry_after() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| Some(true));
    client.expect_transcribe_audio().return_once(|_, _, _| {
        Err(Error::RateLimit {
            retry_after_seconds: 42,
            detail: String::new(),
        })
    });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().with(eq(5)).return_once(|_| Ok(()));

    let err = server(client, limiter)
        .transcribe_voice_message(
            Parameters(request("chan", 1, None)),
            RequestId(NumberOrString::Number(7)),
        )
        .await
        .expect_err("should fail");

    assert!(err.contains("retry after 42"), "error was: {err}");
}

#[test]
fn media_type_wire_name_is_consistent_across_endpoints() {
    // search_messages serializes MediaType directly; transcribe_voice_message
    // embeds the same MediaType. Both must emit the identical wire string for a
    // round video, or the two endpoints disagree — the bug this guards against.
    let search_name = serde_json::to_string(&MediaType::VideoNote).unwrap();
    assert_eq!(search_name, "\"video_note\"");

    let resp = TranscribeVoiceMessageResponse {
        text: String::new(),
        partial: false,
        duration_seconds: None,
        media_type: MediaType::VideoNote,
    };
    let value = serde_json::to_value(&resp).unwrap();
    assert_eq!(value["media_type"], serde_json::json!("video_note"));
}

#[tokio::test]
async fn clamps_request_timeout_to_configured_max() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_is_premium().return_once(|| Some(true));
    client
        .expect_transcribe_audio()
        // The configured max is 45; a 999s request must clamp to it, proving the
        // config value drives the clamp rather than the old hardcoded 120 (AD-6).
        .withf(|_, _, t| *t == 45)
        .return_once(|_, _, _| {
            Ok(TranscriptionOutcome {
                text: "ok".to_string(),
                partial: false,
                media_type: MediaType::Voice,
                duration_seconds: None,
            })
        });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().return_once(|_| Ok(()));

    let server = McpServer::new(Arc::new(client), Arc::new(limiter))
        .with_transcription_cost(5)
        .with_transcription_timeouts(30, 45);

    server
        .transcribe_voice_message(
            Parameters(request("news", 42, Some(999))),
            RequestId(NumberOrString::Number(1)),
        )
        .await
        .expect("should succeed");
}

#[tokio::test]
async fn rejects_message_id_beyond_wire_range() {
    // No expectations: neither premium check, rate limiter, nor the RPC
    // may run for an id that cannot exist on the wire.
    let server = server(MockTelegramClientTrait::new(), MockRateLimiterTrait::new());
    let result = server
        .transcribe_voice_message(
            Parameters(request("news", i64::from(i32::MAX) + 1, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;
    let err = result.expect_err("out-of-range id must be rejected");
    assert!(
        err.contains("exceeds Telegram's message id range"),
        "got: {err}"
    );
}
