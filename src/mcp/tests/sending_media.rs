use super::*;
use crate::sending::SendingRuntimeConfig;
use crate::sending::test_support::Fixture;
use crate::telegram::types::sending::{
    ContentKind, QualifiedRecipient, SendFailure, SendFailureKind, SentMessage, SubmittedMedia,
};
use crate::telegram::types::{MessageId, PeerNamespace};

async fn enabled_media(
    fixture: &Fixture,
    client: MockTelegramClientTrait,
    limiter: MockRateLimiterTrait,
) -> McpServer<MockTelegramClientTrait, MockRateLimiterTrait> {
    let config: SendingRuntimeConfig = fixture.config.clone();
    McpServer::new(Arc::new(client), Arc::new(limiter))
        .with_sending(config)
        .await
        .expect("media enabled")
}
fn call(id: i32, path: &str) -> Value {
    let mut r = modern(json!(id), "tools/call");
    r["params"]["name"] = json!("send_media");
    r["params"]["arguments"] = json!({"recipient":{"kind":"saved_messages"},"source":{"root":"outgoing","relative_path":path},"media":{"kind":"image"},"caption":"private caption"});
    r
}
#[tokio::test]
async fn sending_media_stages_before_one_charge_and_returns_only_submitted_metadata() {
    let fixture = Fixture::new();
    let size = fixture.bytes.len() as u64;
    let mut seq = mockall::Sequence::new();
    let mut limiter = MockRateLimiterTrait::new();
    limiter
        .expect_acquire()
        .withf(|n| *n == 3)
        .times(1)
        .in_sequence(&mut seq)
        .returning(|_| Ok(()));
    limiter.expect_refund().times(0);
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_send_media()
        .times(1)
        .in_sequence(&mut seq)
        .returning(move |r, d| {
            assert_eq!(r.caption, "private caption");
            assert_eq!(r.staged.size_bytes(), size);
            assert_eq!(r.staged.mime_type(), "image/jpeg");
            assert!(!d.expired());
            Ok(SentMessage {
                recipient: QualifiedRecipient::try_new(PeerNamespace::User, 91).expect("ID"),
                saved_messages: true,
                message_id: MessageId::new(42).expect("ID"),
                date: chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("date"),
                content_kind: ContentKind::Image,
                reply_to_message_id: None,
                topic_id: None,
                message_link: None,
                media: Some(SubmittedMedia {
                    mime_type: "image/jpeg".into(),
                    size_bytes: size,
                }),
            })
        });
    let mut wire = Wire::with_handler(enabled_media(&fixture, client, limiter).await);
    wire.send(call(1, "image.jpg")).await;
    let r = wire.recv().await;
    let data = &r["result"]["structuredContent"]["data"];
    assert_eq!(r["result"]["isError"], false);
    assert_eq!(data["delivery_state"], "sent");
    assert_eq!(data["content_kind"], "image");
    assert_eq!(data["media"]["mime_type"], "image/jpeg");
    assert_eq!(data["media"]["size_bytes"], size);
    assert!(!r.to_string().contains("private caption"));
    assert!(
        !r.to_string()
            .contains(fixture.source.to_str().expect("path"))
    );
    wire.close().await;
}
#[tokio::test]
async fn sending_media_source_failures_are_free_and_do_not_reach_the_client() {
    let fixture = Fixture::new();
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().times(0);
    let mut client = MockTelegramClientTrait::new();
    client.expect_send_media().times(0);
    let mut wire = Wire::with_handler(enabled_media(&fixture, client, limiter).await);
    for (id, path) in ["../image.jpg", "missing.jpg", ""].into_iter().enumerate() {
        wire.send(call(id as i32, path)).await;
        let r = wire.recv().await;
        assert_eq!(r["result"]["isError"], true);
        assert_eq!(
            r["result"]["structuredContent"]["error"]["delivery_state"],
            "not_sent"
        );
    }
    wire.close().await;
}
#[tokio::test]
async fn sending_media_upload_failure_and_unknown_delivery_never_refund_the_charge() {
    for failure in [
        SendFailure::not_sent(SendFailureKind::UploadFailed, None),
        SendFailure::unknown(),
    ] {
        let fixture = Fixture::new();
        let mut limiter = MockRateLimiterTrait::new();
        limiter.expect_acquire().times(1).returning(|_| Ok(()));
        limiter.expect_refund().times(0);
        let mut client = MockTelegramClientTrait::new();
        client
            .expect_send_media()
            .times(1)
            .returning(move |_, _| Err(failure));
        let mut wire = Wire::with_handler(enabled_media(&fixture, client, limiter).await);
        wire.send(call(1, "image.jpg")).await;
        let r = wire.recv().await;
        assert_eq!(
            r["result"]["structuredContent"]["error"]["code"],
            failure.kind().as_str()
        );
        assert_eq!(
            r["result"]["structuredContent"]["error"]["retryable"],
            false
        );
        wire.close().await;
    }
}
