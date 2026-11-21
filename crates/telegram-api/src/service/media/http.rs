use crate::contracts::{GrantUpdate, ReadGrant};
use crate::identity_http::Fixture as Issuer;
use crate::service::accounts::{ActiveAccount, Hosted};
use crate::service::{BrowserConfig, build_router};
use axum::http::header;
use reqwest::StatusCode;
use secrecy::SecretString;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use telegram_core::error::Error;
use telegram_core::operations::{AccountLimits, AccountOperations, JoinedMediaSource};
use telegram_core::rate_limiter::RateLimiterTrait as _;
use telegram_core::telegram::types::{
    ChannelId, MediaBatch, MediaDownload, MediaFetchOutcome, MediaType, PeerNamespace,
};
use tokio::net::TcpListener;

#[derive(Default)]
struct JoinedImages {
    left: AtomicBool,
    checks: AtomicUsize,
    downloads: AtomicUsize,
}

#[async_trait::async_trait]
impl JoinedMediaSource for JoinedImages {
    async fn verify_media_channel(&self, channel: ChannelId) -> Result<(), Error> {
        self.checks.fetch_add(1, Ordering::Relaxed);
        if self.left.load(Ordering::Acquire) || channel != ChannelId::new(42).unwrap() {
            return Err(Error::ChannelNotFound(channel.to_string()));
        }
        Ok(())
    }

    async fn joined_media(
        &self,
        channel: ChannelId,
        ids: &[i32],
        _: u32,
    ) -> Result<MediaBatch, Error> {
        self.verify_media_channel(channel).await?;
        self.downloads.fetch_add(1, Ordering::Relaxed);
        assert_eq!(ids, [104, 105]);
        let outcomes = ids
            .iter()
            .map(|&message_id| {
                let bytes = if message_id == 104 {
                    telegram_core::test_helpers::create_test_jpeg(80, 30)
                } else {
                    b"unreadable fixture".to_vec()
                };
                MediaFetchOutcome {
                    message_id,
                    result: Ok(MediaDownload {
                        lease: None,
                        channel_id: channel,
                        peer_namespace: PeerNamespace::Channel,
                        source_size_bytes: bytes.len() as u64,
                        bytes,
                        media_type: MediaType::Photo,
                        is_thumbnail: false,
                        caption: None,
                        width: None,
                        height: None,
                        video_info: None,
                        largest_width: None,
                        largest_height: None,
                    }),
                }
            })
            .collect();
        Ok(MediaBatch {
            channel_id: channel,
            peer_namespace: PeerNamespace::Channel,
            outcomes,
            deadline: None,
            stream: None,
        })
    }
}

#[tokio::test]
async fn media_http_renders_charges_and_rechecks_membership_before_serving_cached_jpeg() {
    let issuer = Issuer::start().await;
    let verifier = issuer.read_verifier();
    let root = tempfile::tempdir().expect("data root");
    let config = serde_json::from_value(json!({
        "data_root": root.path(), "telegram": {"api_id":1,"api_hash":"fixture"},
        "registered_clients":["read-client"], "eligible_channels":["42"],
        "rate_limiting": {"max_tokens":10, "refill_rate":0.001, "media_download_cost":3}
    }))
    .expect("config");
    let hosted = Hosted::open(config)
        .await
        .unwrap_or_else(|_| panic!("open hosted root"));
    let generation = hosted
        .store
        .start(verifier.issuer(), "listener")
        .await
        .expect("generation");
    hosted
        .store
        .activate(&generation, 17)
        .await
        .expect("activate");
    hosted
        .store
        .update_grant(
            &generation,
            "read-client",
            GrantUpdate {
                expected_revision: 0,
                read: ReadGrant::Channels(vec!["42".into()]),
                send_enabled: false,
            },
        )
        .await
        .expect("grant");
    let (mut client, _runner, _session) = telegram_core::telegram::client::stalled_client().await;
    let images = Arc::new(JoinedImages::default());
    client.set_media_source(images.clone());
    let client = Arc::new(client);
    let mut account_limiter = None;
    hosted
        .seed_account(generation, |limiter| {
            limiter.refund(10);
            account_limiter = Some(limiter.clone());
            let (streams, _) = telegram_core::streams::test_support::account_streams(
                client.clone(),
                limiter.clone(),
                hosted.config.events.stream_limits(),
                Arc::clone(&hosted.streams),
            );
            let operations =
                AccountOperations::new(Arc::clone(&client), limiter, AccountLimits::default());
            ActiveAccount {
                client,
                streams,
                capture: None,
                operations,
            }
        })
        .await
        .expect("seed leased account");
    let limiter = account_limiter.expect("limiter");
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
    .expect("router");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(4))
        .build()
        .expect("client");
    let token = issuer.read_token("listener", false);
    let response = http
        .post(format!("{base}/v1/media/previews"))
        .bearer_auth(&token)
        .json(&json!({"channel_id":"42","message_ids":[104,105,104],"max_dimension":64}))
        .send()
        .await
        .expect("preview")
        .error_for_status()
        .expect("authorized");
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    let body: Value = response.json().await.expect("items");
    let items = body["items"].as_array().expect("ordered items");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["message_id"], 104);
    assert_eq!(items[0]["width"], 64);
    assert_eq!(items[0]["height"], 24);
    assert_eq!(items[0]["mime_type"], "image/jpeg");
    assert_eq!(
        items[1],
        json!({"message_id":105,"error":{"code":"download_failed"}})
    );
    assert_eq!(limiter.available_tokens().floor(), 7.0);
    let handle = items[0]["handle"].as_str().expect("opaque handle");
    let get = || {
        http.get(format!("{base}/v1/media/{handle}"))
            .bearer_auth(&token)
    };
    let response = get()
        .send()
        .await
        .expect("download")
        .error_for_status()
        .expect("joined");
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/jpeg");
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    let jpeg = response.bytes().await.expect("image body");
    assert_eq!(items[0]["size_bytes"], jpeg.len());
    assert!(jpeg.starts_with(&[0xff, 0xd8]) && jpeg.ends_with(&[0xff, 0xd9]));
    images.left.store(true, Ordering::Release);
    let response = get().send().await.expect("membership removed");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.json::<Value>().await.expect("safe error"),
        json!({"error":{"code":"unavailable_resource"}})
    );
    images.left.store(false, Ordering::Release);
    hosted
        .media
        .state
        .lock()
        .unwrap()
        .objects
        .get_mut(handle)
        .unwrap()
        .expires_at = tokio::time::Instant::now();
    let response = get().send().await.expect("expired handle");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(limiter.available_tokens().floor(), 5.0);
    assert_eq!(images.checks.load(Ordering::Relaxed), 3);
    assert_eq!(images.downloads.load(Ordering::Relaxed), 1);
    hosted.cancellation.cancel();
    server.abort();
}
