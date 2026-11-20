use super::*;
use crate::contracts::{GrantUpdate, ReadGrant};
use crate::identity_http::Fixture as Issuer;
use crate::service::accounts::{ActiveAccount, Hosted};
use crate::service::reads::fixture::{FixtureAccount, ReadFixture};
use crate::service::{BrowserConfig, HostedConfig, build_router};
use futures::StreamExt as _;
use secrecy::SecretString;
use serde_json::json;
use telegram_core::UtcTimestamp;
use telegram_core::config::RateLimitConfig;
use telegram_core::error::Error;
use telegram_core::operations::{AccountLimits, AccountOperations};
use telegram_core::rate_limiter::RateLimiterTrait as _;
use telegram_core::streams::{ChannelHead, ChannelHeadLookup, ChannelPost, test_support};
use telegram_core::telegram::types::MessageId;
use tokio::net::TcpListener;

struct Heads(std::sync::atomic::AtomicI64);

#[async_trait::async_trait]
impl ChannelHeadLookup for Heads {
    async fn channel_heads(&self, ids: &[ChannelId]) -> Result<Vec<ChannelHead>, Error> {
        Ok(ids
            .iter()
            .map(|id| ChannelHead {
                channel_id: *id,
                head_message_id: if id.get() == 42 {
                    MessageId::new(self.0.load(std::sync::atomic::Ordering::Acquire)).ok()
                } else {
                    None
                },
            })
            .collect())
    }
}

fn post(id: i64, text: &str) -> ChannelPost {
    ChannelPost {
        channel_id: ChannelId::new(42).expect("channel"),
        message_id: MessageId::new(id).expect("message"),
        published_at: UtcTimestamp::from_unix_seconds(1_700_000_000).expect("timestamp"),
        text: text.to_owned(),
        media: None,
        album_id: None,
    }
}

struct Frames {
    stream: std::pin::Pin<Box<dyn futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    pending: String,
}

impl Frames {
    fn new(response: reqwest::Response) -> Self {
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(response.headers()["x-accel-buffering"], "no");
        Self {
            stream: Box::pin(response.bytes_stream()),
            pending: String::new(),
        }
    }

    async fn read(&mut self) -> String {
        tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                if let Some((frame, rest)) = self.pending.split_once("\n\n") {
                    let frame = frame.to_owned();
                    self.pending = rest.to_owned();
                    assert!(!frame.lines().any(|line| line.starts_with("id:")));
                    return frame;
                }
                let piece = self
                    .stream
                    .next()
                    .await
                    .expect("open stream")
                    .expect("stream bytes");
                self.pending
                    .push_str(std::str::from_utf8(&piece).expect("text"));
            }
        })
        .await
        .expect("frame deadline")
    }
}

#[tokio::test]
async fn reconnect_only_delivers_posts_after_the_new_head() {
    let issuer = Issuer::start().await;
    let verifier = issuer.read_verifier();
    let data = tempfile::tempdir().expect("data");
    let config: HostedConfig = serde_json::from_value(json!({
        "data_root": data.path(), "telegram": {"api_id":1,"api_hash":"fixture"},
        "registered_clients":["read-client"], "eligible_channels":["42","73"],
        "events":{"heartbeat_seconds":1}
    }))
    .expect("config");
    let state = Hosted::open(config)
        .await
        .unwrap_or_else(|_| panic!("hosted root"));
    let generation = state
        .store
        .start(verifier.issuer(), "listener")
        .await
        .expect("start");
    state.store.activate(&generation, 17).await.expect("link");
    state
        .store
        .update_grant(
            &generation,
            "read-client",
            GrantUpdate {
                expected_revision: 0,
                read: ReadGrant::Channels(vec!["42".into(), "73".into()]),
                send_enabled: false,
            },
        )
        .await
        .expect("grant");
    let heads = Arc::new(Heads(std::sync::atomic::AtomicI64::new(103)));
    let (client, _runner, _session) = telegram_core::telegram::client::stalled_client().await;
    let client = Arc::new(client);
    let mut input = None;
    state
        .seed_account(generation.clone(), |limiter| {
            limiter.refund(u32::MAX);
            let (streams, source) = test_support::account_streams(
                heads.clone(),
                limiter.clone(),
                state.config.events.stream_limits(),
                Arc::clone(&state.streams),
            );
            input = Some(source);
            let operations =
                AccountOperations::new(Arc::clone(&client), limiter, AccountLimits::default());
            ActiveAccount {
                client,
                operations,
                streams,
            }
        })
        .await
        .expect("seed production account lease");
    let input = input.expect("fixture update source");
    let mut retained: telegram_core::telegram::types::Message = serde_json::from_value(json!({
        "id":104,"channel_id":42,"channel_name":"Channel","text":"A",
        "timestamp":"2023-11-14T22:13:20Z","has_media":false,"media_type":"none","link":""
    }))
    .expect("history fixture row");
    retained.peer_namespace = Some(telegram_core::telegram::types::PeerNamespace::Channel);
    let read_fixture = ReadFixture::new(
        RateLimitConfig::default(),
        [(
            generation.clone(),
            Arc::new(FixtureAccount {
                channels: [(ChannelId::new(42).expect("channel"), vec![retained])].into(),
                fetches: std::sync::atomic::AtomicUsize::new(0),
                history_timeout: false,
            }),
        )]
        .into(),
    );
    read_fixture.warm(&generation).await;
    assert!(state.reads_fixture.set(read_fixture).is_ok());
    let app = build_router(
        verifier,
        BrowserConfig {
            client_id: "browser".into(),
            client_secret: SecretString::from("test-secret"),
            callback_url: url::Url::parse("https://service.example/auth/callback")
                .expect("callback"),
        },
        Some(Arc::clone(&state)),
    )
    .expect("routes");
    let server = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let base = format!("http://{}", server.local_addr().expect("address"));
    let cancel = state.cancellation.clone();
    let managed = Arc::clone(&state);
    let task = tokio::spawn(async move {
        super::super::shutdown::serve(
            server,
            app,
            Duration::from_secs(1),
            Some(managed),
            async move { cancel.cancelled().await },
        )
        .await
        .expect("server");
    });
    let http = reqwest::Client::new();
    let token = issuer.read_token("listener", false);
    let fetch = |ids: &'static str| {
        http.get(format!("{base}/v1/events?{ids}"))
            .bearer_auth(&token)
            .header("last-event-id", "old-connection")
    };
    let bad = fetch("channel_id=99")
        .send()
        .await
        .expect("unavailable channel");
    assert_eq!(bad.status(), 404);
    let malformed = fetch("channel_id=0").send().await.expect("invalid ID");
    assert_eq!(malformed.status(), 400);
    let mut continuous = Frames::new(
        fetch("channel_id=42&channel_id=42&channel_id=73")
            .send()
            .await
            .expect("stream"),
    );
    let mut returning = Frames::new(fetch("channel_id=42").send().await.expect("stream"));
    let ready = continuous.read().await;
    assert!(ready.starts_with("event: ready\n"));
    assert!(ready.contains("\"channel_id\":\"42\",\"head_message_id\":103"));
    assert!(ready.contains("\"channel_id\":\"73\",\"head_message_id\":null"));
    assert!(returning.read().await.starts_with("event: ready"));
    drop(returning);
    heads.0.store(104, std::sync::atomic::Ordering::Release);
    input.publish(post(104, "A"));
    let mut returning = Frames::new(fetch("channel_id=42").send().await.expect("reconnect"));
    assert!(returning.read().await.contains("\"head_message_id\":104"));
    input.publish(post(105, "B"));
    let a = continuous.read().await;
    assert!(a.contains("\"message_id\":104") && a.contains("\"media\":null"));
    assert!(continuous.read().await.contains("\"message_id\":105"));
    assert!(returning.read().await.contains("\"message_id\":105"));
    let history = http
        .get(format!(
            "{base}/v1/channels/42/messages?after_id=103&through_id=104&limit=10"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("catch-up HTTP")
        .error_for_status()
        .expect("catch-up status");
    let history: crate::contracts::HistoryResponse =
        history.json().await.expect("catch-up payload");
    assert_eq!(history.messages.len(), 1);
    assert_eq!(history.messages[0].id, 104);
    assert_eq!(history.messages[0].text, "A");
    assert_eq!(history.next_before_id, None);
    assert_eq!(returning.read().await, ": heartbeat");
    state
        .store
        .update_grant(
            &generation,
            "read-client",
            GrantUpdate {
                expected_revision: 1,
                read: ReadGrant::None,
                send_enabled: false,
            },
        )
        .await
        .expect("revoke");
    let _ = state.grant_changes.send(());
    assert!(
        tokio::time::timeout(Duration::from_secs(2), returning.stream.next())
            .await
            .expect("revocation")
            .is_none_or(|result| result.is_err())
    );
    drop(continuous);
    state
        .store
        .update_grant(
            &generation,
            "read-client",
            GrantUpdate {
                expected_revision: 2,
                read: ReadGrant::Channels(vec!["42".into()]),
                send_enabled: false,
            },
        )
        .await
        .expect("restore");
    let mut draining = Frames::new(fetch("channel_id=42").send().await.expect("fresh stream"));
    assert!(draining.read().await.contains("\"head_message_id\":104"));
    state.cancellation.cancel();
    let terminal = draining.read().await;
    assert!(terminal.starts_with("event: interrupted\n"));
    assert!(terminal.contains("service_draining"));
    task.await.expect("server stop");
    assert_eq!(
        state.subscribers.available_permits(),
        state.config.events.max_total_subscriptions
    );
}
