#[path = "metadata_http.rs"]
mod metadata_http;

use crate::contracts::{ErrorCode, ErrorResponse, GrantUpdate, ReadGrant};
use crate::identity_http::Fixture as Issuer;
use crate::service::accounts::Hosted;
use crate::service::{BrowserConfig, HostedConfig, build_router};
use reqwest::{Client, Method, StatusCode};
use secrecy::SecretString;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

#[tokio::test]
async fn channel_metadata_routes_apply_scope_before_opening_a_telegram_session() {
    let issuer = Issuer::start().await;
    let verifier = issuer.read_verifier();
    let root = tempfile::tempdir().expect("fixture root");
    let config: HostedConfig = serde_json::from_value(json!({
        "data_root":root.path(), "telegram":{"api_id":1,"api_hash":"fixture"},
        "registered_clients":["read-client"], "eligible_channels":["42","43"]
    }))
    .expect("configuration");
    let hosted = Hosted::open(config)
        .await
        .unwrap_or_else(|_| panic!("hosted fixture"));
    let generation = hosted
        .store
        .start(verifier.issuer(), "reader")
        .await
        .expect("account");
    hosted.store.activate(&generation, 1).await.expect("link");
    // A nonregular session path fails local storage admission if an empty-scope
    // regression reaches the client constructor. This fixture never opens a sender.
    std::fs::create_dir(generation.session_file()).expect("nonregular session marker");
    let grant = |revision, read| GrantUpdate {
        expected_revision: revision,
        read,
        send_enabled: true,
    };
    hosted
        .store
        .update_grant(
            &generation,
            "read-client",
            grant(0, ReadGrant::Channels(vec!["42".into()])),
        )
        .await
        .expect("read grant");
    let browser = BrowserConfig {
        client_id: "browser".into(),
        client_secret: SecretString::from("fixture secret"),
        callback_url: url::Url::parse("https://service.example/auth/callback").expect("callback"),
    };
    let app = build_router(verifier, browser, Some(Arc::clone(&hosted))).expect("routes");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let http = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(4))
        .build()
        .expect("client");
    let bearer = issuer.read_token("reader", false);
    let request = |method, path: &str| {
        http.request(method, format!("{base}{path}"))
            .bearer_auth(&bearer)
    };

    for channel in [43, 99] {
        for (method, suffix) in [
            (Method::GET, ""),
            (Method::GET, "/stats"),
            (Method::GET, "/messages/7/link"),
            (Method::POST, "/messages/7/transcription"),
        ] {
            let mut call = request(method.clone(), &format!("/v1/channels/{channel}{suffix}"));
            if method == Method::POST {
                call = call.json(&json!({}));
            }
            let response = call.send().await.expect("scoped reply");
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            let error: ErrorResponse = response.json().await.expect("safe error");
            assert_eq!(error.error.code, ErrorCode::UnavailableResource);
        }
    }
    let unavailable = request(Method::POST, "/v1/channels/discover")
        .json(&json!({"query":"channel"}))
        .send()
        .await
        .expect("discovery reply");
    assert_eq!(unavailable.status(), StatusCode::FORBIDDEN);
    let malformed = request(Method::GET, "/v1/channels/42/messages/0/link")
        .send()
        .await
        .expect("invalid message");
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);

    hosted
        .store
        .update_grant(&generation, "read-client", grant(1, ReadGrant::None))
        .await
        .expect("clear read grant");
    let page: Value = request(Method::GET, "/v1/channels?offset=5&limit=2")
        .send()
        .await
        .expect("list")
        .error_for_status()
        .expect("empty page")
        .json()
        .await
        .expect("JSON page");
    assert_eq!(page, json!({"channels":[],"total":0}));
    let resolution: Value = request(Method::POST, "/v1/channels/resolve")
        .json(&json!({"identifiers":["42","@example","Channel"]}))
        .send()
        .await
        .expect("resolve")
        .error_for_status()
        .expect("empty scope outcome")
        .json()
        .await
        .expect("JSON outcome");
    assert_eq!(
        resolution,
        json!({"items":[
            {"identifier":"42","error":{"code":"unavailable_resource"}},
            {"identifier":"@example","error":{"code":"unavailable_resource"}},
            {"identifier":"Channel","error":{"code":"unavailable_resource"}}
        ]})
    );
    assert!(
        generation.session_file().is_dir(),
        "local scope decisions should avoid session creation"
    );
    hosted.cancellation.cancel();
    task.abort();
}
