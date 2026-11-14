use super::*;
use crate::sending::SendingRuntimeConfig;
use crate::telegram::types::sending::{
    ContentKind, QualifiedRecipient, SendFailure, SendFailureKind, SentMessage,
};
use crate::telegram::types::{MessageId, PeerNamespace};

async fn enabled(
    client: MockTelegramClientTrait,
    limiter: MockRateLimiterTrait,
) -> McpServer<MockTelegramClientTrait, MockRateLimiterTrait> {
    let mut config: crate::Config = toml::from_str("[telegram]\napi_id=1").expect("config");
    config.sending.enabled = true;
    McpServer::new(Arc::new(client), Arc::new(limiter))
        .with_sending(
            SendingRuntimeConfig::try_from_config(
                &config,
                std::path::Path::new("/tmp/offline-config.toml"),
            )
            .expect("config"),
        )
        .await
        .expect("enabled server")
}
fn call(id: Value, arguments: Value) -> Value {
    let mut r = modern(id, "tools/call");
    r["params"]["name"] = json!("send_message");
    r["params"]["arguments"] = arguments;
    r
}
fn receipt() -> SentMessage {
    SentMessage {
        recipient: QualifiedRecipient::try_new(PeerNamespace::User, 91).expect("ID"),
        saved_messages: true,
        message_id: MessageId::new(42).expect("ID"),
        date: chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("date"),
        content_kind: ContentKind::Text,
        reply_to_message_id: None,
        topic_id: None,
        message_link: None,
        media: None,
    }
}
fn assert_error(response: &Value, code: &str, state: &str) {
    assert_eq!(response["result"]["isError"], true, "{response}");
    let s = &response["result"]["structuredContent"];
    assert_eq!(s["data"], Value::Null);
    assert_eq!(s["error"]["code"], code);
    assert_eq!(s["error"]["delivery_state"], state);
    assert_eq!(
        serde_json::from_str::<Value>(
            response["result"]["content"][0]["text"]
                .as_str()
                .expect("text")
        )
        .expect("JSON"),
        *s
    );
}
#[tokio::test]
async fn sending_text_disabled_catalog_and_direct_dispatch_are_guarded() {
    let mut wire = Wire::start();
    wire.send(modern(json!(1), "tools/list")).await;
    assert_eq!(
        wire.recv().await["result"]["tools"]
            .as_array()
            .expect("tools")
            .len(),
        16
    );
    wire.send(call(
        json!(2),
        json!({"recipient":{"kind":"saved_messages"},"text":"secret"}),
    ))
    .await;
    assert_error(&wire.recv().await, "sending_disabled", "not_sent");
    wire.close().await;
}
#[tokio::test]
async fn sending_text_enabled_catalog_has_strict_self_contained_schemas_and_annotations() {
    let mut wire = Wire::with_handler(
        enabled(MockTelegramClientTrait::new(), MockRateLimiterTrait::new()).await,
    );
    wire.send(modern(json!(1), "tools/list")).await;
    let response = wire.recv().await;
    let tools = response["result"]["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 18);
    for name in ["send_message", "send_media"] {
        let t = tools.iter().find(|t| t["name"] == name).expect("tool");
        assert_eq!(t["annotations"]["readOnlyHint"], false);
        assert_eq!(t["annotations"]["idempotentHint"], false);
        assert_eq!(t["annotations"]["destructiveHint"], false);
        assert_eq!(t["annotations"]["openWorldHint"], true);
        assert_eq!(t["inputSchema"]["additionalProperties"], false);
        assert!(
            !t["inputSchema"].to_string().contains("$ref"),
            "self-contained {name}"
        );
        jsonschema::validator_for(&t["inputSchema"]).expect("actual input schema");
    }
    wire.close().await;
}
#[tokio::test]
async fn sending_text_local_failures_do_not_charge_or_call_client() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_send_message().times(0);
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().times(0);
    let mut wire = Wire::with_handler(enabled(client, limiter).await);
    for (id, args) in [
        json!({}),
        json!({"recipient":{"kind":"saved_messages"},"text":" "}),
        json!({"recipient":{"kind":"saved_messages","unknown":true},"text":"hello"}),
        json!({"recipient":{"kind":"saved_messages"},"text":"🦀".repeat(2049)}),
    ]
    .into_iter()
    .enumerate()
    {
        wire.send(call(json!(id), args)).await;
        assert_error(&wire.recv().await, "invalid_input", "not_sent");
    }
    wire.close().await;
}
#[tokio::test]
async fn sending_text_one_charge_precedes_send_and_receipt_matches_both_representations() {
    let mut sequence = mockall::Sequence::new();
    let mut limiter = MockRateLimiterTrait::new();
    limiter
        .expect_acquire()
        .withf(|cost| *cost == 1)
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_| Ok(()));
    limiter.expect_refund().times(0);
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_send_message()
        .withf(|r, d| {
            r.text == "  literal *text*\n" && r.options.silent && !r.link_preview && !d.expired()
        })
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_, _| Ok(receipt()));
    let mut wire = Wire::with_handler(enabled(client, limiter).await);
    wire.send(call(
        json!(1),
        json!({"recipient":{"kind":"saved_messages"},"text":"  literal *text*\n","silent":true}),
    ))
    .await;
    let r = wire.recv().await;
    let s = &r["result"]["structuredContent"];
    assert_eq!(r["result"]["isError"], false);
    assert_eq!(s["data"]["delivery_state"], "sent");
    assert_eq!(s["data"]["recipient"]["peer_id"], "91");
    assert_eq!(s["data"]["message_id"], 42);
    assert_eq!(s["data"]["date"], "2023-11-14T22:13:20Z");
    assert!(!s.to_string().contains("literal"));
    assert_eq!(
        serde_json::from_str::<Value>(r["result"]["content"][0]["text"].as_str().expect("text"))
            .expect("JSON"),
        *s
    );
    wire.close().await;
}
#[tokio::test]
async fn sending_text_unknown_never_becomes_a_retryable_generic_timeout() {
    for failure in [
        SendFailure::unknown(),
        SendFailure::not_sent(SendFailureKind::Timeout, None),
    ] {
        let mut limiter = MockRateLimiterTrait::new();
        limiter.expect_acquire().times(1).returning(|_| Ok(()));
        limiter.expect_refund().times(0);
        let mut client = MockTelegramClientTrait::new();
        client
            .expect_send_message()
            .times(1)
            .returning(move |_, _| Err(failure));
        let mut wire = Wire::with_handler(enabled(client, limiter).await);
        wire.send(call(
            json!(1),
            json!({"recipient":{"kind":"saved_messages"},"text":"hello"}),
        ))
        .await;
        let r = wire.recv().await;
        assert_error(
            &r,
            failure.kind().as_str(),
            if failure.retryable() {
                "not_sent"
            } else {
                "unknown"
            },
        );
        assert_eq!(
            r["result"]["structuredContent"]["error"]["retryable"],
            failure.retryable()
        );
        wire.close().await;
    }
}
#[tokio::test]
async fn sending_text_long_escaped_request_id_budget_refuses_before_charge() {
    let mut client = MockTelegramClientTrait::new();
    client.expect_send_message().times(0);
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().times(0);
    let mut wire = Wire::with_handler(
        enabled(client, limiter)
            .await
            .with_response_byte_budget(1500),
    );
    wire.send(call(
        json!("\"\n".repeat(500)),
        json!({"recipient":{"kind":"saved_messages"},"text":"hello"}),
    ))
    .await;
    assert_error(&wire.recv().await, "response_too_large", "not_sent");
    wire.close().await;
}
#[tokio::test]
async fn sending_text_invalid_receipt_after_send_is_unknown() {
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().times(1).returning(|_| Ok(()));
    let mut client = MockTelegramClientTrait::new();
    client.expect_send_message().times(1).returning(|_, _| {
        let mut r = receipt();
        r.message_id = MessageId::new(i64::MAX).expect("domain ID");
        Ok(r)
    });
    let mut wire = Wire::with_handler(enabled(client, limiter).await);
    wire.send(call(
        json!(1),
        json!({"recipient":{"kind":"saved_messages"},"text":"hello"}),
    ))
    .await;
    assert_error(&wire.recv().await, "delivery_unknown", "unknown");
    wire.close().await;
}

#[tokio::test]
async fn sending_text_cached_status_exposes_limits_without_paths_or_charge() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_cached_account_status()
        .times(1)
        .returning(crate::telegram::AccountStatus::default);
    client.expect_refresh_account_status().times(0);
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().times(0);
    limiter.expect_try_available_tokens().returning(|| Ok(40.0));
    limiter.expect_capacity().return_const(50.0);
    limiter.expect_refill_rate().return_const(2.0);
    let mut wire = Wire::with_handler(enabled(client, limiter).await);
    let mut request = modern(json!(1), "tools/call");
    request["params"]["name"] = json!("check_mcp_status");
    request["params"]["arguments"] = json!({});
    wire.send(request).await;
    let response = wire.recv().await;
    let status = &response["result"]["structuredContent"]["data"]["sending"];
    assert_eq!(status["enabled"], true);
    assert_eq!(status["media_enabled"], false);
    assert_eq!(status["send_secs"], 30);
    assert_eq!(status["upload_send_secs"], 120);
    assert_eq!(status["send_message_cost"], 1);
    assert_eq!(status["send_media_cost"], 3);
    assert_eq!(status["max_media_bytes"], 104_857_600);
    assert!(!status.to_string().contains("/tmp"));
    wire.close().await;
}

#[tokio::test]
async fn sending_text_duplicate_recovery_key_is_a_not_sent_failure_for_this_call() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_send_message()
        .times(1)
        .returning(|_, _| Ok(receipt()));
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().times(1).returning(|_| Ok(()));
    let mut wire = Wire::with_handler(enabled(client, limiter).await);
    let args =
        json!({"recipient":{"kind":"saved_messages"},"text":"hello","recovery_key":"unique-send"});
    wire.send(call(json!(1), args.clone())).await;
    assert_eq!(wire.recv().await["result"]["isError"], false);
    wire.send(call(json!(2), args)).await;
    assert_error(&wire.recv().await, "duplicate_recovery_key", "not_sent");
    wire.close().await;
}

#[tokio::test]
async fn sending_text_later_legacy_builder_cannot_enable_a_write() {
    let server = enabled(MockTelegramClientTrait::new(), MockRateLimiterTrait::new())
        .await
        .with_output_format(crate::config::OutputFormat::Legacy);
    let request =
        serde_json::from_value(json!({"recipient":{"kind":"saved_messages"},"text":"hello"}))
            .expect("request");
    let error = server
        .send_message_data(request)
        .await
        .expect_err("legacy guard");
    assert_eq!(error.kind(), SendFailureKind::InvalidInput);
    assert_eq!(server.tools_list_result().tools.len(), 16);
}

#[tokio::test]
async fn sending_text_unexpected_receipt_metadata_cannot_leak_or_claim_success() {
    for case in 0..4 {
        let mut client = MockTelegramClientTrait::new();
        client
            .expect_send_message()
            .times(1)
            .returning(move |_, _| {
                let mut r = receipt();
                match case {
                    0 => r.message_link = Some("file:///private/secret".into()),
                    1 => {
                        r.content_kind = ContentKind::Image;
                        r.media = Some(crate::telegram::types::sending::SubmittedMedia {
                            mime_type: "image/jpeg".into(),
                            size_bytes: 42,
                        });
                    }
                    2 => {
                        r.media = Some(crate::telegram::types::sending::SubmittedMedia {
                            mime_type: "secret media payload".into(),
                            size_bytes: 42,
                        })
                    }
                    _ => r.reply_to_message_id = Some(MessageId::new(44).expect("ID")),
                }
                Ok(r)
            });
        let mut limiter = MockRateLimiterTrait::new();
        limiter.expect_acquire().times(1).returning(|_| Ok(()));
        let mut wire = Wire::with_handler(enabled(client, limiter).await);
        wire.send(call(
            json!(1),
            json!({"recipient":{"kind":"saved_messages"},"text":"hello"}),
        ))
        .await;
        let r = wire.recv().await;
        assert_error(&r, "delivery_unknown", "unknown");
        assert!(!r.to_string().contains("secret"));
        wire.close().await;
    }
}

#[test]
fn sending_text_serializer_failure_preserves_unknown_delivery() {
    #[derive(schemars::JsonSchema)]
    struct Failing;
    impl serde::Serialize for Failing {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("secret serializer payload"))
        }
    }
    let result = crate::mcp::tools::types::contract::render_send_data(Ok(Failing));
    let value = serde_json::to_value(result).expect("safe fallback");
    assert_eq!(
        value["structuredContent"]["error"]["code"],
        "delivery_unknown"
    );
    assert_eq!(
        value["structuredContent"]["error"]["delivery_state"],
        "unknown"
    );
    assert_eq!(value["structuredContent"]["error"]["retryable"], false);
    assert!(!value.to_string().contains("secret"));
}

#[tokio::test]
async fn sending_text_legacy_protocol_still_uses_canonical_receipt() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_send_message()
        .times(1)
        .returning(|_, _| Ok(receipt()));
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().times(1).returning(|_| Ok(()));
    let mut wire = Wire::with_handler(enabled(client, limiter).await);
    wire.send(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"offline","version":"1"}}})).await;
    assert_eq!(wire.recv().await["result"]["protocolVersion"], "2025-11-25");
    wire.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .await;
    wire.send(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"send_message","arguments":{"recipient":{"kind":"saved_messages"},"text":"hello"}}})).await;
    let r = wire.recv().await;
    assert_eq!(r["result"]["structuredContent"]["data"]["message_id"], 42);
    assert!(r["result"].get("resultType").is_none());
    wire.close().await;
}
