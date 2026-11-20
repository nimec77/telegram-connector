use super::*;
use crate::identity_http::Fixture as Issuer;
use axum::body::Bytes;
use futures::StreamExt as _;
use serde_json::json;
use std::time::Duration;
use tokio::net::TcpListener;

#[tokio::test]
async fn stalled_json_body_releases_handler_admission() {
    let issuer = Issuer::start().await;
    let root = tempfile::tempdir().expect("data root");
    let config = serde_json::from_value(json!({
        "data_root":root.path(), "telegram":{"api_id":1,"api_hash":"fixture"},
        "registered_clients":["read-client"], "request_seconds":1, "max_in_flight":1
    }))
    .expect("hosted settings");
    let (app, shutdown) = hosted_router(
        issuer.read_verifier(),
        BrowserConfig {
            client_id: "browser".into(),
            client_secret: SecretString::from("fixture secret"),
            callback_url: url::Url::parse("https://service.example/auth/callback")
                .expect("callback"),
        },
        config,
    )
    .await
    .expect("hosted app");
    let socket = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base = format!("http://{}", socket.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(socket, app).await.expect("serve");
    });
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .expect("client");
    let chunks = futures::stream::once(async { Ok::<_, std::io::Error>(Bytes::from_static(b"{")) })
        .chain(futures::stream::pending());
    let started = tokio::time::Instant::now();
    let response = http
        .post(format!("{base}/v1/messages/batch"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(reqwest::Body::wrap_stream(chunks))
        .send()
        .await
        .expect("timeout reply");
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert!(started.elapsed() < Duration::from_secs(2));
    let response = http
        .get(format!("{base}/v1/session"))
        .send()
        .await
        .expect("next request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    shutdown.cancel();
    server.abort();
}
