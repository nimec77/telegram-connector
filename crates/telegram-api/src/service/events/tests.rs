use super::*;
use crate::contracts::{GrantUpdate, ReadGrant};
use crate::identity_http::Fixture as Issuer;
use crate::service::accounts::{ActiveAccount, Hosted};
use crate::service::reads::fixture::{FixtureAccount, ReadFixture};
use crate::service::registry::LeaseError;
use crate::service::{AccountGeneration, BrowserConfig, HostedConfig, build_router};
use futures::StreamExt as _;
use secrecy::SecretString;
use serde_json::{Value, json};
use std::net::SocketAddr;
use telegram_core::UtcTimestamp;
use telegram_core::config::RateLimitConfig;
use telegram_core::error::Error;
use telegram_core::operations::{AccountLimits, AccountOperations};
use telegram_core::rate_limiter::{RateLimiter, RateLimiterTrait as _};
use telegram_core::streams::{
    ChannelHead, ChannelHeadLookup, ChannelPost, InterruptionReason, test_support,
};
use telegram_core::telegram::TelegramClient;
use telegram_core::telegram::types::MessageId;
use tokio::io::AsyncWriteExt as _;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

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

async fn seed_stream(
    state: &Hosted,
    generation: &AccountGeneration,
    client: Arc<TelegramClient>,
    heads: Arc<dyn ChannelHeadLookup>,
    initial: bool,
) -> Result<Option<(test_support::StreamInput, Arc<RateLimiter>)>, LeaseError> {
    let mut seeded = None;
    state
        .seed_account(generation.clone(), |limiter| {
            if initial {
                limiter.refund(u32::MAX);
            }
            let (streams, source) = test_support::account_streams(
                heads,
                limiter.clone(),
                state.config.events.stream_limits(),
                Arc::clone(&state.streams),
            );
            seeded = Some((source, limiter.clone()));
            let operations =
                AccountOperations::new(Arc::clone(&client), limiter, AccountLimits::default());
            ActiveAccount {
                client,
                operations,
                streams,
            }
        })
        .await?;
    Ok(seeded)
}

struct EventApi {
    state: Arc<Hosted>,
    generation: AccountGeneration,
    input: test_support::StreamInput,
    limiter: Arc<RateLimiter>,
    address: SocketAddr,
    http: reqwest::Client,
    token: String,
    server: tokio::task::JoinHandle<()>,
    issuer: Issuer,
    _root: tempfile::TempDir,
    _session: tempfile::TempDir,
}

impl EventApi {
    async fn start(heads: Arc<dyn ChannelHeadLookup>, events: Value) -> Self {
        let issuer = Issuer::start().await;
        let verifier = issuer.read_verifier();
        let data = tempfile::tempdir().expect("data");
        let config: HostedConfig = serde_json::from_value(json!({
            "data_root": data.path(), "telegram": {"api_id":1,"api_hash":"fixture"},
            "registered_clients":["read-client"], "eligible_channels":["42","73"],
            "events":events
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
        let (client, _runner, session) = telegram_core::telegram::client::stalled_client().await;
        let (input, limiter) = seed_stream(&state, &generation, Arc::new(client), heads, true)
            .await
            .expect("seed production account lease")
            .expect("fixture update source");
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
        let address = server.local_addr().expect("address");
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
        Self {
            state,
            generation,
            input,
            limiter,
            address,
            http: reqwest::Client::builder()
                .no_proxy()
                .build()
                .expect("HTTP client"),
            token: issuer.read_token("listener", false),
            server: task,
            issuer,
            _root: data,
            _session: session,
        }
    }

    fn events(&self, ids: &str) -> reqwest::RequestBuilder {
        self.http
            .get(format!("http://{}/v1/events?{ids}", self.address))
            .bearer_auth(&self.token)
            .header("last-event-id", "old-connection")
    }

    fn assert_released(&self) {
        assert_eq!(self.state.streams.used_bytes(), 0);
        assert_eq!(
            self.state.subscribers.available_permits(),
            self.state.config.events.max_total_subscriptions
        );
    }

    async fn stop(&mut self) {
        self.state.cancellation.cancel();
        (&mut self.server).await.expect("server stop");
        self.assert_released();
    }
}

impl Drop for EventApi {
    fn drop(&mut self) {
        self.state.cancellation.cancel();
        self.server.abort();
    }
}

#[tokio::test]
async fn reconnect_only_delivers_posts_after_the_new_head() {
    let heads = Arc::new(Heads(std::sync::atomic::AtomicI64::new(103)));
    let mut api = EventApi::start(heads.clone(), json!({"heartbeat_seconds":1})).await;
    let mut retained: telegram_core::telegram::types::Message = serde_json::from_value(json!({
        "id":104,"channel_id":42,"channel_name":"Channel","text":"A",
        "timestamp":"2023-11-14T22:13:20Z","has_media":false,"media_type":"none","link":""
    }))
    .expect("history fixture row");
    retained.peer_namespace = Some(telegram_core::telegram::types::PeerNamespace::Channel);
    let read_fixture = ReadFixture::new(
        RateLimitConfig::default(),
        [(
            api.generation.clone(),
            Arc::new(FixtureAccount {
                channels: [(ChannelId::new(42).expect("channel"), vec![retained])].into(),
                fetches: std::sync::atomic::AtomicUsize::new(0),
                history_timeout: false,
            }),
        )]
        .into(),
    );
    read_fixture.warm(&api.generation).await;
    assert!(api.state.reads_fixture.set(read_fixture).is_ok());
    let mut continuous = Frames::new(
        api.events("channel_id=42&channel_id=42&channel_id=73")
            .send()
            .await
            .expect("stream"),
    );
    let mut returning = Frames::new(api.events("channel_id=42").send().await.expect("stream"));
    let ready = continuous.read().await;
    assert_eq!(
        ready,
        "event: ready\ndata: {\"channels\":[{\"channel_id\":\"42\",\"head_message_id\":103},{\"channel_id\":\"73\",\"head_message_id\":null}]}"
    );
    assert!(returning.read().await.starts_with("event: ready"));
    drop(returning);
    heads.0.store(104, std::sync::atomic::Ordering::Release);
    api.input.publish(post(104, "A"));
    let mut returning = Frames::new(api.events("channel_id=42").send().await.expect("reconnect"));
    assert!(returning.read().await.contains("\"head_message_id\":104"));
    api.input.publish(post(105, "B"));
    let a = continuous.read().await;
    assert!(a.contains("\"channel_id\":\"42\"") && a.contains("\"message_id\":104"));
    assert!(
        a.contains("\"published_at\":\"2023-11-14T22:13:20Z\"") && a.contains("\"media\":null")
    );
    assert!(continuous.read().await.contains("\"message_id\":105"));
    assert!(returning.read().await.contains("\"message_id\":105"));
    let history = api
        .http
        .get(format!(
            "http://{}/v1/channels/42/messages?after_id=103&through_id=104&limit=10",
            api.address
        ))
        .bearer_auth(&api.token)
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
    api.state
        .store
        .update_grant(
            &api.generation,
            "read-client",
            GrantUpdate {
                expected_revision: 1,
                read: ReadGrant::None,
                send_enabled: false,
            },
        )
        .await
        .expect("revoke");
    let _ = api.state.grant_changes.send(());
    assert!(
        tokio::time::timeout(Duration::from_secs(2), returning.stream.next())
            .await
            .expect("revocation")
            .is_none_or(|result| result.is_err())
    );
    drop(continuous);
    api.state
        .store
        .update_grant(
            &api.generation,
            "read-client",
            GrantUpdate {
                expected_revision: 2,
                read: ReadGrant::Channels(vec!["42".into()]),
                send_enabled: false,
            },
        )
        .await
        .expect("restore");
    let mut draining = Frames::new(
        api.events("channel_id=42")
            .send()
            .await
            .expect("fresh stream"),
    );
    assert!(draining.read().await.contains("\"head_message_id\":104"));
    api.state.cancellation.cancel();
    let terminal = draining.read().await;
    assert!(terminal.starts_with("event: interrupted\n"));
    assert!(terminal.contains("service_draining"));
    api.stop().await;
}

struct HeadRequest {
    channels: Vec<ChannelId>,
    reply: oneshot::Sender<Result<Vec<ChannelHead>, Error>>,
}

struct HeldHeads(mpsc::UnboundedSender<HeadRequest>);

#[async_trait::async_trait]
impl ChannelHeadLookup for HeldHeads {
    async fn channel_heads(&self, channels: &[ChannelId]) -> Result<Vec<ChannelHead>, Error> {
        let (reply, result) = oneshot::channel();
        self.0
            .send(HeadRequest {
                channels: channels.to_vec(),
                reply,
            })
            .unwrap_or_else(|_| panic!("fixture waiting for head request"));
        result.await.expect("fixture head result")
    }
}

async fn head_request(requests: &mut mpsc::UnboundedReceiver<HeadRequest>) -> HeadRequest {
    tokio::time::timeout(Duration::from_secs(2), requests.recv())
        .await
        .expect("head request deadline")
        .expect("head request")
}

async fn assert_http_error(response: reqwest::Response, status: u16, code: &str) {
    assert_eq!(response.status(), status);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert!(!response.headers().contains_key("x-accel-buffering"));
    let problem: Value = response.json().await.expect("HTTP error payload");
    assert_eq!(problem["error"]["code"], code);
}

#[tokio::test]
async fn invalid_query_never_starts_setup_or_reserves_stream_capacity() {
    let (heads, mut requests) = mpsc::unbounded_channel();
    let mut api = EventApi::start(
        Arc::new(HeldHeads(heads)),
        json!({"max_channels":1,"max_account_subscriptions":1,"max_total_subscriptions":1}),
    )
    .await;
    for (query, status, code) in [
        ("", 400, "invalid_request"),
        ("channel_id=0", 400, "invalid_request"),
        ("channel_id=42&channel_id=bad", 400, "invalid_request"),
        ("channel_id=42&channel_id=73", 400, "invalid_request"),
        ("channel_id=42&access_token=example", 400, "invalid_request"),
        ("channel_id=99", 404, "unavailable_resource"),
    ] {
        let response = api.events(query).send().await.expect("error response");
        assert_http_error(response, status, code).await;
        assert!(matches!(
            requests.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        api.assert_released();
    }

    let selected = tokio::spawn(api.events("channel_id=42&channel_id=42").send());
    let HeadRequest { channels, reply } = head_request(&mut requests).await;
    assert_eq!(channels, vec![ChannelId::new(42).expect("channel")]);
    assert!(!selected.is_finished());
    let excess = api
        .events("channel_id=42")
        .send()
        .await
        .expect("over capacity");
    assert_http_error(excess, 429, "rate_limited").await;
    assert!(matches!(
        requests.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    reply
        .send(Ok(vec![ChannelHead {
            channel_id: channels[0],
            head_message_id: None,
        }]))
        .expect("finish setup");
    let mut frames = Frames::new(selected.await.expect("request task").expect("HTTP"));
    assert_eq!(
        frames.read().await,
        "event: ready\ndata: {\"channels\":[{\"channel_id\":\"42\",\"head_message_id\":null}]}"
    );
    api.stop().await;
}

#[tokio::test]
async fn failed_setup_returns_normal_http_errors_and_releases_reservations() {
    let (heads, mut requests) = mpsc::unbounded_channel();
    let mut api = EventApi::start(
        Arc::new(HeldHeads(heads)),
        json!({
            "max_queue_items":1,"setup_seconds":3,
            "max_account_subscriptions":1,"max_total_subscriptions":1
        }),
    )
    .await;

    for (status, code) in [
        (404, "unavailable_resource"),
        (429, "rate_limited"),
        (504, "timeout"),
        (503, "unavailable"),
    ] {
        let response = tokio::spawn(api.events("channel_id=42").send());
        let HeadRequest { reply, .. } = head_request(&mut requests).await;
        assert!(!response.is_finished());
        api.input.publish(post(104, "pending"));
        assert!(api.state.streams.used_bytes() > 0);
        match status {
            404 => reply
                .send(Err(Error::ChannelNotFound("42".into())))
                .expect("deny membership"),
            429 => api.input.publish(post(105, "overflow")),
            504 => {
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(3)).await;
                tokio::time::resume();
            }
            503 => api
                .input
                .interrupt(InterruptionReason::UpstreamDisconnected),
            _ => unreachable!(),
        }
        let response = tokio::time::timeout(Duration::from_secs(2), response)
            .await
            .expect("setup must end")
            .expect("request task")
            .expect("HTTP error response");
        assert_http_error(response, status, code).await;
        api.assert_released();
    }

    api.stop().await;
}

#[tokio::test]
async fn disconnect_during_setup_releases_the_head_request_and_both_admissions() {
    let (heads, mut requests) = mpsc::unbounded_channel();
    let mut api = EventApi::start(
        Arc::new(HeldHeads(heads)),
        json!({"setup_seconds":30,"max_account_subscriptions":1,"max_total_subscriptions":1}),
    )
    .await;

    let mut client = TcpStream::connect(api.address).await.expect("connect");
    client
        .write_all(
            format!(
                "GET /v1/events?channel_id=42 HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\n\r\n",
                api.address, api.token
            )
            .as_bytes(),
        )
        .await
        .expect("start event setup");
    let HeadRequest { mut reply, .. } = head_request(&mut requests).await;
    api.input.publish(post(104, "pending"));
    assert!(api.state.streams.used_bytes() > 0);
    drop(client);
    tokio::time::timeout(Duration::from_secs(2), reply.closed())
        .await
        .expect("client disconnect must cancel the head lookup");
    api.assert_released();

    let reconnect = tokio::spawn(api.events("channel_id=42").send());
    let HeadRequest { channels, reply } = head_request(&mut requests).await;
    reply
        .send(Ok(vec![ChannelHead {
            channel_id: channels[0],
            head_message_id: Some(MessageId::new(104).expect("new head")),
        }]))
        .expect("accept fresh head");
    let mut frames = Frames::new(reconnect.await.expect("request task").expect("HTTP"));
    assert!(frames.read().await.contains("\"head_message_id\":104"));
    api.input.publish(post(105, "fresh"));
    assert!(frames.read().await.contains("\"message_id\":105"));
    api.stop().await;
}

#[tokio::test]
async fn membership_loss_discards_queued_posts_and_drain_ends_the_other_full_stream() {
    let mut api = EventApi::start(
        Arc::new(Heads(std::sync::atomic::AtomicI64::new(103))),
        json!({"max_queue_items":1,"max_account_subscriptions":2,"max_total_subscriptions":3}),
    )
    .await;
    assert_eq!(api.state.streams.diagnostics().active_readers, 1);
    let mut departing = Frames::new(api.events("channel_id=42").send().await.expect("stream"));
    let mut staying = Frames::new(api.events("channel_id=73").send().await.expect("stream"));
    assert!(departing.read().await.contains("\"head_message_id\":103"));
    assert!(staying.read().await.contains("\"head_message_id\":null"));

    let excess = api
        .events("channel_id=42")
        .send()
        .await
        .expect("over capacity");
    assert_http_error(excess, 429, "rate_limited").await;
    assert_eq!(api.state.streams.diagnostics().rejected_setups, 1);
    let visible = ChannelId::new(73).expect("unrelated channel");
    api.input.publish(post(104, "revoked before writing"));
    assert!(api.state.streams.used_bytes() > 0);
    api.input
        .membership_lost(ChannelId::new(42).expect("departed channel"));
    api.input.publish(ChannelPost {
        channel_id: visible,
        ..post(1, "still visible")
    });
    assert!(
        tokio::time::timeout(Duration::from_secs(2), departing.stream.next())
            .await
            .expect("membership loss must close the stream")
            .is_none_or(|result| result.is_err())
    );
    assert_eq!(api.state.streams.diagnostics().membership_losses, 1);
    assert!(staying.read().await.contains("\"text\":\"still visible\""));

    api.input.publish(ChannelPost {
        channel_id: visible,
        ..post(2, "queued during drain")
    });
    assert!(api.state.streams.used_bytes() > 0);
    api.state.cancellation.cancel();
    assert_eq!(
        staying.read().await,
        "event: interrupted\ndata: {\"reason\":\"service_draining\"}"
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(2), staying.stream.next())
            .await
            .expect("drained EOF")
            .is_none()
    );
    api.stop().await;
    let diagnostics = api.state.streams.diagnostics();
    assert_eq!(diagnostics.active_readers, 0);
    assert_eq!(diagnostics.queued_bytes, 0);
    assert_eq!(diagnostics.service_drains, 1);
}

#[tokio::test]
async fn reader_replacement_reconnects_at_a_fresh_head_with_the_same_limiter() {
    let mut api = EventApi::start(
        Arc::new(Heads(std::sync::atomic::AtomicI64::new(103))),
        json!({"max_account_subscriptions":1,"max_total_subscriptions":1}),
    )
    .await;
    let mut old = Frames::new(api.events("channel_id=42").send().await.expect("stream"));
    assert!(old.read().await.contains("\"head_message_id\":103"));
    api.input
        .interrupt(InterruptionReason::UpstreamDisconnected);
    assert_eq!(
        old.read().await,
        "event: interrupted\ndata: {\"reason\":\"upstream_disconnected\"}"
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(2), old.stream.next())
            .await
            .expect("reader shutdown EOF")
            .is_none()
    );
    api.assert_released();
    assert_eq!(api.state.streams.diagnostics().upstream_disconnects, 1);

    let (replacement, _runner, _session) = telegram_core::telegram::client::stalled_client().await;
    let replacement = Arc::new(replacement);
    let head = Arc::new(Heads(std::sync::atomic::AtomicI64::new(106)));
    tokio::time::pause();
    let (source, limiter) = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match seed_stream(
                &api.state,
                &api.generation,
                Arc::clone(&replacement),
                head.clone(),
                false,
            )
            .await
            {
                Ok(Some(seeded)) => break seeded,
                Ok(None) | Err(LeaseError::Capacity | LeaseError::Cancelled) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(error) => panic!("reader replacement rejected: {error:?}"),
            }
        }
    })
    .await
    .expect("maintenance reaps the closed reader");
    tokio::time::resume();
    assert!(Arc::ptr_eq(&limiter, &api.limiter));

    let mut fresh = Frames::new(api.events("channel_id=42").send().await.expect("reconnect"));
    assert!(fresh.read().await.contains("\"head_message_id\":106"));
    api.input.publish(post(107, "closed reader"));
    source.publish(post(106, "already at head"));
    source.publish(post(107, "new reader"));
    assert!(fresh.read().await.contains("\"text\":\"new reader\""));
    api.stop().await;
}

#[tokio::test]
async fn encoded_frame_keeps_admission_until_the_transport_releases_it() {
    let mut api = EventApi::start(
        Arc::new(Heads(std::sync::atomic::AtomicI64::new(103))),
        json!({}),
    )
    .await;
    let identity = api
        .issuer
        .read_verifier()
        .verify(
            &SecretString::from(api.token.clone()),
            &crate::identity::Audience::Api,
        )
        .await
        .expect("fixture credential");
    let context = telegram_core::operations::OperationContext::new(
        Instant::now() + Duration::from_secs(5),
        CancellationToken::new(),
    );
    let account = api
        .state
        .lease(&identity, &api.generation, &context)
        .await
        .unwrap_or_else(|_| panic!("fixture account lease"));
    let mut subscription = account
        .client()
        .streams
        .subscribe_channel_posts(&[ChannelId::new(42).expect("channel")], &context)
        .await
        .expect("subscription");
    let ready = subscription.next_queued().await.expect("ready frame");
    let lease = Arc::new(StreamLease {
        _account: account,
        _permit: Arc::clone(&api.state.subscribers)
            .try_acquire_owned()
            .expect("service stream permit"),
    });
    let held = Arc::downgrade(&lease);
    let frame = encode(ready, Arc::clone(&lease));
    drop(subscription);
    drop(lease);
    assert!(held.upgrade().is_some());
    assert_eq!(
        api.state.subscribers.available_permits(),
        api.state.config.events.max_total_subscriptions - 1
    );
    drop(frame);
    assert!(held.upgrade().is_none());
    api.assert_released();
    api.stop().await;
}
