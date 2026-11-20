use super::fixture::{SendFixture, SendOutcome};
use super::*;
use crate::contracts::{GrantUpdate, ReadGrant};
use crate::identity_http::Fixture as Issuer;
use crate::service::accounts::Hosted;
use crate::service::{BrowserConfig, HostedConfig, build_router};
use secrecy::SecretString;
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::net::TcpListener;

#[tokio::test]
async fn send_http_write_grant_selects_writable_peers_and_gates_final_media_dispatch() {
    let issuer = Issuer::start().await;
    let verifier = issuer.read_verifier();
    let root = tempfile::tempdir().expect("root");
    let config: HostedConfig = serde_json::from_value(json!({
        "data_root": root.path(), "telegram": {"api_id":1,"api_hash":"fixture"},
        "registered_clients":["sender","other"], "sending_enabled":true
    }))
    .expect("config");
    let hosted = Hosted::open(config)
        .await
        .unwrap_or_else(|_| panic!("open hosted root"));
    assert!(hosted.send_fixture.set(SendFixture::default()).is_ok());
    let fixture = hosted.send_fixture.get().expect("send fixture");
    let generation = hosted
        .store
        .start(verifier.issuer(), "selected-owner")
        .await
        .expect("generation");
    hosted
        .store
        .activate(&generation, 12)
        .await
        .expect("active");
    let grant = |revision, send_enabled| GrantUpdate {
        expected_revision: revision,
        read: ReadGrant::None,
        send_enabled,
    };
    hosted
        .store
        .update_grant(&generation, "sender", grant(0, true))
        .await
        .expect("send grant");
    hosted
        .store
        .update_grant(&generation, "other", grant(0, true))
        .await
        .expect("send grant");
    let app = build_router(
        verifier,
        BrowserConfig {
            client_id: "browser".into(),
            client_secret: SecretString::from("test secret"),
            callback_url: url::Url::parse("https://service.example/auth/callback")
                .expect("callback"),
        },
        Some(Arc::clone(&hosted)),
    )
    .expect("routes");
    let server = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let base = format!("http://{}", server.local_addr().expect("address"));
    let task = tokio::spawn(async move {
        axum::serve(server, app).await.expect("API server");
    });
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client");
    let token = issuer.send_token("selected-owner", "sender");
    let other = issuer.send_token("selected-owner", "other");
    let text = |id: u64, namespace: &str| {
        http.post(format!("{base}/v1/messages/send")).bearer_auth(&token)
            .json(&json!({"recipient":{"kind":"peer","namespace":namespace,"id":id.to_string()},"text":"sample"}))
    };
    for (id, namespace) in [(42, "channel"), (73, "user")] {
        let reply = text(id, namespace)
            .send()
            .await
            .expect("send")
            .error_for_status()
            .expect("accepted");
        let reply: Value = reply.json().await.expect("receipt");
        assert_eq!(
            reply["recipient"],
            json!({"namespace":namespace,"id":id.to_string()})
        );
    }
    assert_eq!(fixture.dispatches(), 2);

    let mut png = vec![0_u8; 33];
    png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    png[11] = 13;
    png[12..16].copy_from_slice(b"IHDR");
    png[19] = 1;
    png[23] = 1;
    let upload = http
        .post(format!("{base}/v1/uploads"))
        .bearer_auth(&token)
        .header("content-type", "image/png");
    let spoof = upload
        .try_clone()
        .expect("request")
        .body(b"garbage".to_vec())
        .send()
        .await
        .expect("invalid type");
    assert_eq!(spoof.status(), 400);
    let reply = upload
        .body(png)
        .send()
        .await
        .expect("upload")
        .error_for_status()
        .expect("accepted");
    let upload: Value = reply.json().await.expect("handle");
    let media = |bearer: &str| {
        http.post(format!("{base}/v1/messages/send-media")).bearer_auth(bearer)
        .json(&json!({
            "recipient":{"kind":"peer","namespace":"channel","id":"73"},
            "upload_handle":upload["upload_handle"], "media":{"kind":"image"},"caption":"fixture"
        }))
    };
    assert_eq!(
        media(&other).send().await.expect("foreign handle").status(),
        404
    );

    fixture.pause.store(true, Ordering::Release);
    let sending = tokio::spawn(media(&token).send());
    tokio::time::timeout(Duration::from_secs(3), fixture.entered.notified())
        .await
        .expect("pre-dispatch");
    {
        let _transition = hosted.transition.lock().await;
        hosted
            .store
            .update_grant(&generation, "sender", grant(1, false))
            .await
            .expect("revoke");
    }
    fixture.resume.notify_one();
    let response = sending.await.expect("send task").expect("send response");
    assert_eq!(response.status(), 403);
    assert_eq!(
        response.json::<Value>().await.expect("failure")["error"]["delivery"],
        "not_sent"
    );
    assert_eq!(fixture.dispatches(), 2);

    hosted
        .store
        .update_grant(&generation, "sender", grant(2, true))
        .await
        .expect("restore");
    fixture.pause.store(false, Ordering::Release);
    fixture.set_outcome(SendOutcome::Unknown);
    let reply = text(42, "channel").send().await.expect("lost receipt");
    let failure: Value = reply.json().await.expect("ambiguous response");
    assert_eq!(failure["error"]["delivery"], "unknown");
    assert_eq!(failure["error"]["retryable"], false);
    assert_eq!(fixture.dispatches(), 3);

    fixture.set_outcome(SendOutcome::Refused);
    let reply = text(73, "user").send().await.expect("posting refusal");
    assert_eq!(reply.status(), 403);
    let failure: Value = reply.json().await.expect("safe refusal");
    assert_eq!(failure["error"]["code"], "send_forbidden");
    assert_eq!(failure["error"]["delivery"], "not_sent");
    assert_eq!(failure["error"]["retryable"], false);
    assert_eq!(fixture.dispatches(), 4);
    hosted.cancellation.cancel();
    task.abort();
}
