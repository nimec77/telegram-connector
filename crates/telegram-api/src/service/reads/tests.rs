use super::*;
use crate::contracts::{GrantUpdate, ReadGrant, ReadMediaKind, ReadPeerNamespace};
use crate::identity_http::Fixture as IssuerFixture;
use crate::service::accounts::Hosted;
use crate::service::{BrowserConfig, HostedConfig, build_router};
use axum::body::to_bytes;
use fixture::{FixtureAccount, ReadFixture};
use reqwest::{Client, Method, StatusCode};
use secrecy::SecretString;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use telegram_core::config::RateLimitConfig;
use tokio::net::TcpListener;
use url::Url;

fn message(id: u32, text_len: usize) -> ReadMessage {
    ReadMessage {
        id,
        channel_id: "42".into(),
        peer_namespace: ReadPeerNamespace::Channel,
        channel_name: "Channel".into(),
        channel_username: None,
        text: "x".repeat(text_len),
        timestamp: "2020-01-01T00:00:00Z".into(),
        sender_id: None,
        sender_name: None,
        has_media: false,
        media_type: ReadMediaKind::None,
        grouped_id: None,
        album: None,
        link: String::new(),
    }
}

#[tokio::test]
async fn history_response_fits_with_cursor_from_last_included_row() {
    let result = history_response(HistoryResponse {
        messages: vec![message(109, 30_000), message(105, 15_000), message(101, 20)],
        next_before_id: Some(101),
        partial: true,
        timed_out: true,
    })
    .unwrap_or_else(|_| panic!("fit history"))
    .into_body();
    let bytes = to_bytes(result, 40_000).await.expect("bounded response");
    let page: HistoryResponse = serde_json::from_slice(&bytes).expect("history JSON");
    assert_eq!(
        page.messages.iter().map(|row| row.id).collect::<Vec<_>>(),
        [109]
    );
    assert_eq!(page.next_before_id, Some(109));
    assert!(page.partial);
    assert!(page.timed_out);
}

#[tokio::test]
async fn batch_response_reports_missing_and_omitted_independently() {
    let result = batch_response(BatchResponse {
        channel_id: "42".into(),
        messages: vec![message(109, 30_000), message(105, 15_000), message(101, 20)],
        missing_ids: vec![999],
        omitted_ids: vec![],
    })
    .unwrap_or_else(|_| panic!("fit batch"))
    .into_body();
    let bytes = to_bytes(result, 40_000).await.expect("bounded response");
    let page: BatchResponse = serde_json::from_slice(&bytes).expect("batch JSON");
    assert_eq!(
        page.messages.iter().map(|row| row.id).collect::<Vec<_>>(),
        [109]
    );
    assert_eq!(page.missing_ids, [999]);
    assert_eq!(page.omitted_ids, [105, 101]);
}

fn domain_row(id: i32, channel: i64, text: String, group: Option<i64>) -> Message {
    let mut row: Message = serde_json::from_value(json!({
        "id": id,
        "channel_id": channel,
        "channel_name": "Channel",
        "text": text,
        "timestamp": "2010-01-01T00:00:00Z",
        "has_media": false,
        "media_type": "none",
        "grouped_id": group,
        "link": ""
    }))
    .expect("Telegram message fixture");
    row.peer_namespace = Some(PeerNamespace::Channel);
    row
}

struct HttpFixture {
    issuer: IssuerFixture,
    http: Client,
    base: String,
    first: Arc<FixtureAccount>,
    second: Arc<FixtureAccount>,
    _root: tempfile::TempDir,
    serve: tokio::task::JoinHandle<()>,
    cancellation: telegram_core::operations::CancellationToken,
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.serve.abort();
    }
}

impl HttpFixture {
    async fn start(tokens: u32, first_times_out: bool) -> Self {
        let issuer = IssuerFixture::start().await;
        let verifier = issuer.read_verifier();
        let root = tempfile::tempdir().expect("temporary private data root");
        let rates = RateLimitConfig {
            max_tokens: tokens,
            refill_rate: 0.001,
            media_download_cost: 1,
            transcription_cost: 1,
            ..RateLimitConfig::default()
        };
        let config: HostedConfig = serde_json::from_value(json!({
            "data_root": root.path(),
            "telegram": {
                "api_id": 1,
                "api_hash": "fixture-hash",
                "session_file": root.path().join("unused.session")
            },
            "registered_clients": ["read-client"],
            "eligible_channels": ["42", "43", "44", "98", "99"],
            "max_accounts": 4,
            "idle_seconds": 60,
            "request_seconds": 5
        }))
        .expect("hosted fixture config");
        let hosted = Hosted::open(config)
            .await
            .unwrap_or_else(|_| panic!("open hosted service"));
        let first_key = hosted
            .store
            .start(verifier.issuer(), "first")
            .await
            .expect("first generation");
        let second_key = hosted
            .store
            .start(verifier.issuer(), "second")
            .await
            .expect("second generation");
        hosted
            .store
            .activate(&first_key, 1)
            .await
            .expect("link first account");
        hosted
            .store
            .activate(&second_key, 2)
            .await
            .expect("link second account");
        for (key, channels) in [
            (
                &first_key,
                vec!["42".into(), "43".into(), "98".into(), "99".into()],
            ),
            (&second_key, vec!["42".into(), "44".into()]),
        ] {
            hosted
                .store
                .update_grant(
                    key,
                    "read-client",
                    GrantUpdate {
                        expected_revision: 0,
                        read: ReadGrant::Channels(channels),
                        send_enabled: false,
                    },
                )
                .await
                .expect("grant access");
        }
        let id = |value| ChannelId::new(value).expect("channel ID");
        let mut basic_group_row = domain_row(104, 98, "same group-only ID".into(), None);
        basic_group_row.peer_namespace = Some(PeerNamespace::Chat);
        let first = Arc::new(FixtureAccount {
            channels: [
                (
                    id(42),
                    vec![
                        domain_row(i32::MAX, 42, "newest".into(), Some(41)),
                        domain_row(104, 42, "same first".into(), Some(41)),
                        domain_row(101, 42, "old".into(), None),
                    ],
                ),
                (
                    id(43),
                    vec![
                        domain_row(109, 43, "x".repeat(30_000), None),
                        domain_row(105, 43, "x".repeat(15_000), None),
                        domain_row(101, 43, "small".into(), None),
                    ],
                ),
                (id(98), vec![basic_group_row]),
            ]
            .into(),
            fetches: AtomicUsize::new(0),
            history_timeout: first_times_out,
        });
        let second = Arc::new(FixtureAccount {
            channels: [
                (
                    id(42),
                    vec![domain_row(104, 42, "same second".into(), None)],
                ),
                (id(44), vec![domain_row(104, 44, "foreign".into(), None)]),
            ]
            .into(),
            fetches: AtomicUsize::new(0),
            history_timeout: false,
        });
        let fixture = ReadFixture::new(
            rates,
            [
                (first_key.clone(), Arc::clone(&first)),
                (second_key.clone(), Arc::clone(&second)),
            ]
            .into(),
        );
        fixture.warm(&first_key).await;
        fixture.warm(&second_key).await;
        assert!(hosted.reads_fixture.set(fixture).is_ok());
        let cancellation = hosted.cancellation.clone();
        let router = build_router(
            verifier,
            BrowserConfig {
                client_id: "read-client".into(),
                client_secret: SecretString::from("fixture-secret"),
                callback_url: Url::parse("https://service.example/auth/callback")
                    .expect("callback"),
            },
            Some(hosted),
        )
        .expect("router");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("API test listener");
        let base = format!(
            "http://{}",
            listener.local_addr().expect("API test address")
        );
        let serve =
            tokio::spawn(async move { axum::serve(listener, router).await.expect("serve API") });
        let http = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(4))
            .build()
            .expect("loopback HTTP client");
        Self {
            issuer,
            http,
            base,
            first,
            second,
            _root: root,
            serve,
            cancellation,
        }
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        subject: &str,
        delegated: bool,
    ) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.issuer.read_token(subject, delegated))
    }
}

#[tokio::test]
async fn read_http_same_ids_return_each_accounts_own_content() {
    let fixture = HttpFixture::start(10, false).await;
    for (subject, text) in [("first", "same first"), ("second", "same second")] {
        let found: SearchResponse = fixture
            .request(Method::POST, "/v1/messages/search", subject, false)
            .json(&json!({"channel_ids": ["42"], "query": "same"}))
            .send()
            .await
            .expect("search request")
            .error_for_status()
            .expect("search success")
            .json()
            .await
            .expect("search page");
        assert_eq!(found.messages.len(), 1);
        assert_eq!(found.messages[0].id, 104);
        assert_eq!(found.messages[0].channel_id, "42");
        assert_eq!(found.messages[0].text, text);
        let batch: BatchResponse = fixture
            .request(Method::POST, "/v1/messages/batch", subject, false)
            .json(&json!({"channel_id": "42", "message_ids": [104]}))
            .send()
            .await
            .expect("batch request")
            .error_for_status()
            .expect("batch success")
            .json()
            .await
            .expect("batch page");
        assert_eq!(batch.messages[0].text, text);
    }
}

#[tokio::test]
async fn read_http_foreign_channel_matches_missing() {
    let fixture = HttpFixture::start(10, false).await;
    let expected = json!({"error": {"code": "unavailable_resource"}});
    for channel in ["44", "98", "99"] {
        let search = fixture
            .request(Method::POST, "/v1/messages/search", "first", false)
            .json(&json!({"channel_ids":[channel], "query": "same"}));
        let history = fixture.request(
            Method::GET,
            &format!("/v1/channels/{channel}/messages?after_id=1&through_id=100"),
            "first",
            false,
        );
        let batch = fixture
            .request(Method::POST, "/v1/messages/batch", "first", false)
            .json(&json!({"channel_id": channel, "message_ids":[104]}));
        for request in [search, history, batch] {
            let response = request.send().await.expect("read request");
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert_eq!(
                response.json::<Value>().await.expect("safe error"),
                expected
            );
        }
    }
    assert_eq!(fixture.first.fetches.load(Ordering::Relaxed), 0);
    assert_eq!(fixture.second.fetches.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn read_http_inclusive_max_through_id_pages_without_duplicates() {
    let fixture = HttpFixture::start(10, false).await;
    let history = "/v1/channels/42/messages?after_id=100&through_id=2147483647";
    let first: HistoryResponse = fixture
        .request(Method::GET, &format!("{history}&limit=1"), "first", false)
        .send()
        .await
        .expect("history request")
        .error_for_status()
        .expect("first history")
        .json()
        .await
        .expect("first page");
    assert_eq!(
        first.messages.iter().map(|row| row.id).collect::<Vec<_>>(),
        [i32::MAX as u32]
    );
    assert_eq!(first.messages[0].grouped_id, Some(41));
    let next = first.next_before_id.expect("continuation");
    assert_eq!(next, i32::MAX as u32);
    let older: HistoryResponse = fixture
        .request(
            Method::GET,
            &format!("{history}&before_id={next}"),
            "first",
            false,
        )
        .send()
        .await
        .expect("history request")
        .error_for_status()
        .expect("older history")
        .json()
        .await
        .expect("older page");
    assert_eq!(
        older.messages.iter().map(|row| row.id).collect::<Vec<_>>(),
        [104, 101]
    );
    assert_eq!(older.messages[0].grouped_id, Some(41));
    assert!(older.messages[1].timestamp.starts_with("2010-01-01"));
    assert!(older.next_before_id.is_none());
}

#[tokio::test]
async fn read_http_empty_bounds_skip_search_and_partial_bytes_report_what_was_returned() {
    let fixture = HttpFixture::start(10, true).await;
    for query in [
        "after_id=50&through_id=50",
        "after_id=60&through_id=50",
        "after_id=20&through_id=50&before_id=21",
    ] {
        let page: HistoryResponse = fixture
            .request(
                Method::GET,
                &format!("/v1/channels/42/messages?{query}"),
                "first",
                false,
            )
            .send()
            .await
            .expect("empty request")
            .error_for_status()
            .expect("empty history")
            .json()
            .await
            .expect("empty page");
        assert!(page.messages.is_empty());
        assert!(page.next_before_id.is_none());
        assert!(!page.partial);
    }
    assert_eq!(fixture.first.fetches.load(Ordering::Relaxed), 0);
    let response = fixture
        .request(
            Method::GET,
            "/v1/channels/43/messages?after_id=0",
            "first",
            false,
        )
        .send()
        .await
        .expect("history request")
        .error_for_status()
        .expect("history success");
    let bytes = response.bytes().await.expect("read history response");
    assert!(bytes.len() <= RESULT_BYTE_BUDGET);
    let history: HistoryResponse = serde_json::from_slice(&bytes).expect("history page");
    assert_eq!(
        history
            .messages
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        [109]
    );
    assert_eq!(history.next_before_id, Some(109));
    assert!(history.partial && history.timed_out);

    let response = fixture
        .request(Method::POST, "/v1/messages/batch", "first", false)
        .json(&json!({"channel_id":"43", "message_ids":[109,105,999,101]}))
        .send()
        .await
        .expect("batch request")
        .error_for_status()
        .expect("batch success");
    let bytes = response.bytes().await.expect("read batch response");
    assert!(bytes.len() <= RESULT_BYTE_BUDGET);
    let batch: BatchResponse = serde_json::from_slice(&bytes).expect("batch page");
    assert_eq!(
        batch.messages.iter().map(|row| row.id).collect::<Vec<_>>(),
        [109]
    );
    assert_eq!(batch.missing_ids, [999]);
    assert_eq!(batch.omitted_ids, [105, 101]);
}

#[tokio::test]
async fn read_http_direct_and_delegated_requests_share_one_account_allowance() {
    let fixture = HttpFixture::start(1, false).await;
    let direct = fixture.request(
        Method::GET,
        "/v1/channels/42/messages?limit=1",
        "first",
        false,
    );
    let delegated = fixture
        .request(Method::POST, "/v1/messages/batch", "first", true)
        .json(&json!({"channel_id":"42", "message_ids":[104]}));
    let (direct, delegated) = tokio::join!(direct.send(), delegated.send());
    let mut statuses = [
        direct.expect("direct response").status(),
        delegated.expect("delegated response").status(),
    ];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::TOO_MANY_REQUESTS]);
    assert_eq!(fixture.first.fetches.load(Ordering::Relaxed), 1);
}

#[test]
fn hosted_search_row_retains_collapsed_album_members() {
    let mut row = domain_row(104, 42, "album".into(), Some(41));
    row.album = Some(telegram_core::telegram::types::AlbumInfo {
        media_count: 2,
        media_types: vec![MediaType::Photo, MediaType::Video],
        message_ids: vec![MessageId::new(103).expect("sibling"), row.id],
    });
    let rows = render_messages(vec![row]).unwrap_or_else(|_| panic!("render typed album row"));
    let album = rows[0].album.as_ref().expect("collapsed album data");
    assert_eq!(album.media_count, 2);
    assert_eq!(
        album.media_types,
        [ReadMediaKind::Photo, ReadMediaKind::Video]
    );
    assert_eq!(album.message_ids, [103, 104]);
}
