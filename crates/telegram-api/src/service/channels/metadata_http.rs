use crate::contracts::{GrantUpdate, ReadGrant};
use crate::identity_http::Fixture as Issuer;
use crate::service::accounts::{ActiveAccount, Hosted};
use crate::service::{BrowserConfig, build_router};
use grammers_client::tl;
use reqwest::{RequestBuilder, StatusCode};
use secrecy::SecretString;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use telegram_core::operations::{AccountLimits, AccountOperations};
use telegram_core::rate_limiter::RateLimiterTrait as _;
use telegram_core::telegram::client::metadata_fixture::MetadataFixture;
use telegram_core::telegram::client::stalled_client;
use telegram_core::telegram::types::{
    ChannelId, MediaType, MessageId, PeerNamespace, TranscriptionOutcome,
};
use telegram_core::test_helpers::{raw_tl_channel, raw_tl_message};
use tokio::net::TcpListener;

fn fixture_rows() -> MetadataFixture {
    let mut left = raw_tl_channel(88, "Left", None);
    left.left = true;
    let dialogs: Vec<tl::enums::Chat> = vec![
        raw_tl_channel(43, "Not granted", Some("restricted_channel")).into(),
        raw_tl_channel(42, "News", Some("fixture_news")).into(),
        left.into(),
        raw_tl_channel(73, "Private", None).into(),
    ];
    let now = telegram_core::UtcTimestamp::now();
    let date = i32::try_from(now.unix_seconds()).expect("current Telegram date");
    MetadataFixture {
        dialogs: dialogs.clone(),
        full_info: Some(("Fixture description".into(), 120)),
        last_message_date: Some(now),
        history: Some(
            tl::types::messages::Messages {
                messages: vec![
                    raw_tl_message(104, date, 42),
                    raw_tl_message(103, date - 1, 42),
                    raw_tl_message(102, date - 10 * 86400, 42),
                ],
                topics: vec![],
                chats: dialogs.clone(),
                users: vec![],
            }
            .into(),
        ),
        discovery: Some(
            tl::types::contacts::Found {
                my_results: vec![tl::types::PeerChannel { channel_id: 42 }.into()],
                results: vec![tl::types::PeerChannel { channel_id: 43 }.into()],
                chats: vec![dialogs[0].clone(), dialogs[1].clone()],
                users: vec![],
            }
            .into(),
        ),
        transcription: Some(TranscriptionOutcome {
            channel_id: ChannelId::new(42).unwrap(),
            peer_namespace: PeerNamespace::Channel,
            message_id: MessageId::new(104).unwrap(),
            text: "Fixture transcript".into(),
            partial: false,
            media_type: MediaType::Voice,
            duration_seconds: Some(8),
        }),
    }
}

async fn json_request(request: RequestBuilder) -> Value {
    request
        .send()
        .await
        .expect("HTTP reply")
        .error_for_status()
        .expect("successful status")
        .json()
        .await
        .expect("JSON result")
}

#[tokio::test]
async fn hosted_metadata_http_filters_pages_resolves_links_samples_and_charges_transcription() {
    let issuer = Issuer::start().await;
    let verifier = issuer.read_verifier();
    let root = tempfile::tempdir().expect("storage");
    let config = serde_json::from_value(json!({
        "data_root":root.path(), "telegram":{"api_id":1,"api_hash":"fixture"},
        "registered_clients":["read-client"],"discovery_clients":["read-client"],
        "eligible_channels":["42","43","73","88"],
        "rate_limiting":{"max_tokens":40,"refill_rate":0.001}
    }))
    .expect("config");
    let hosted = Hosted::open(config)
        .await
        .unwrap_or_else(|_| panic!("hosted root"));
    let generation = hosted
        .store
        .start(verifier.issuer(), "reader")
        .await
        .expect("generation");
    hosted
        .store
        .activate(&generation, 1)
        .await
        .expect("activate");
    hosted
        .store
        .update_grant(
            &generation,
            "read-client",
            GrantUpdate {
                expected_revision: 0,
                read: ReadGrant::Channels(vec!["42".into(), "73".into(), "88".into()]),
                send_enabled: false,
            },
        )
        .await
        .expect("grant");
    let (mut client, _sender, _session) = stalled_client().await;
    client.set_metadata_fixture(fixture_rows());
    let client = Arc::new(client);
    let mut account_limiter = None;
    hosted
        .seed_account(generation, |limiter| {
            limiter.refund(40);
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
                client: Arc::clone(&client),
                operations,
                streams,
            }
        })
        .await
        .expect("leased account");
    let limiter = account_limiter.expect("account limiter");
    let app = build_router(
        verifier,
        BrowserConfig {
            client_id: "browser".into(),
            client_secret: SecretString::from("fixture secret"),
            callback_url: url::Url::parse("https://service.example/auth/callback")
                .expect("callback"),
        },
        Some(hosted.clone()),
    )
    .expect("routes");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(4))
        .build()
        .unwrap();
    let bearer = issuer.read_token("reader", false);
    let get = |path: &str| {
        http.get(format!("{base}/v1/channels{path}"))
            .bearer_auth(&bearer)
    };
    let post = |path: &str, body: Value| {
        http.post(format!("{base}/v1/channels{path}"))
            .bearer_auth(&bearer)
            .json(&body)
    };

    for (offset, id) in [(0, "42"), (1, "73")] {
        let page = json_request(get(&format!("?limit=1&offset={offset}"))).await;
        assert_eq!(page["total"], 2);
        assert_eq!(page["channels"].as_array().unwrap().len(), 1);
        assert_eq!(page["channels"][0]["channel_id"], id);
    }
    let ids = ["@FIXTURE_NEWS", "Private", "43", "88", "404"];
    let resolve = json_request(post("/resolve", json!({"identifiers":ids}))).await;
    let items = resolve["items"].as_array().unwrap();
    assert_eq!(items.len(), ids.len());
    assert_eq!(items[0]["channel"]["channel_id"], "42");
    assert_eq!(items[1]["channel"]["channel_id"], "73");
    for (item, id) in items[2..].iter().zip(&ids[2..]) {
        assert_eq!(
            item,
            &json!({"identifier":id,"error":{"code":"unavailable_resource"}})
        );
    }

    let full = json_request(get("/42?include_full=true")).await;
    assert_eq!(full["channel_id"], "42");
    assert_eq!(full["peer_namespace"], "channel");
    assert_eq!(full["description"], "Fixture description");
    assert_eq!(full["member_count"], 120);
    assert!(full["last_message_date"].is_string());
    assert!(full["is_subscribed"].as_bool().unwrap());
    let missing = get("/88").send().await.expect("left channel");
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let public = json_request(get("/42/messages/104/link?include_tg_protocol=true")).await;
    assert_eq!(
        public,
        json!({"channel_id":"42","message_id":104,"https_link":"https://t.me/fixture_news/104","tg_link":"tg://resolve?domain=fixture_news&post=104"})
    );
    let private = json_request(get("/73/messages/104/link")).await;
    assert_eq!(
        private,
        json!({"channel_id":"73","message_id":104,"https_link":"https://t.me/c/73/104"})
    );

    let stats = json_request(get("/42/stats?days_back=7")).await;
    assert_eq!(stats["channel_id"], "42");
    assert_eq!(stats["sample"]["messages_scanned"], 2);
    assert_eq!(stats["sample"]["complete"], true);
    assert_eq!(stats["post_count"], 2);
    let discovered = json_request(post("/discover", json!({"query":"news","limit":20}))).await;
    let discovered = discovered["channels"].as_array().unwrap();
    assert!(discovered.iter().any(|hit| hit["channel_id"] == "42"));
    assert!(discovered.iter().all(|hit| hit["channel_id"] != "43"));

    assert_eq!(limiter.available_tokens().floor(), 31.0);
    client.set_fixture_premium(false).await;
    let transcript = || {
        post(
            "/42/messages/104/transcription",
            json!({"timeout_seconds":3}),
        )
    };
    let denied = transcript().send().await.expect("non-Premium reply");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        denied.json::<Value>().await.unwrap(),
        json!({"error":{"code":"premium_required"}})
    );
    assert_eq!(limiter.available_tokens().floor(), 31.0);
    client.set_fixture_premium(true).await;
    let outcome = json_request(transcript()).await;
    assert_eq!(
        outcome,
        json!({"channel_id":"42","peer_namespace":"channel","message_id":104,"text":"Fixture transcript","partial":false,"media_type":"voice","duration_seconds":8})
    );
    assert_eq!(limiter.available_tokens().floor(), 26.0);
    hosted.cancellation.cancel();
    server.abort();
}
