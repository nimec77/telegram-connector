use super::*;
use crate::contracts::groups::{Change, Page};
use crate::contracts::{GrantUpdate, ReadGrant};
use crate::identity_http::Fixture as Issuer;
use crate::service::accounts::Hosted;
use crate::service::group_journal::PendingChange;
use crate::service::{AccountGeneration, BrowserConfig, build_router};
use grammers_client::tl;
use secrecy::SecretString;
use serde_json::{Value, json};
use telegram_core::operations::{AccountLimits, AccountOperations};
use telegram_core::rate_limiter::RateLimiter;
use telegram_core::streams::test_support;
use telegram_core::telegram::client::metadata_fixture::MetadataFixture;
use telegram_core::telegram::client::stalled_client;
use telegram_core::test_helpers::raw_tl_channel;
use tokio::net::TcpListener;

#[tokio::test]
async fn group_pages_replay_and_revocation_hides_the_pending_boundary() {
    let issuer = Issuer::start().await;
    let verifier = issuer.read_verifier();
    let issuer_url = verifier.issuer().to_owned();
    let root = tempfile::tempdir().expect("temporary hosted root");
    let config = serde_json::from_value(json!({
        "data_root":root.path(), "telegram":{"api_id":1,"api_hash":"fixture"},
        "registered_clients":["read-client"],
        "eligible_groups":["chat:42","supergroup:42"],
        "rate_limiting":{"max_tokens":40,"refill_rate":0.001}
    }))
    .expect("hosted config");
    let hosted = Hosted::open_config(config, false)
        .await
        .unwrap_or_else(|_| panic!("hosted root"));
    let generation = hosted
        .store
        .start(verifier.issuer(), "reader")
        .await
        .expect("account");
    hosted.store.activate(&generation, 1).await.expect("link");
    let update = |expected_revision| GrantUpdate {
        expected_revision,
        read: ReadGrant::None,
        send_enabled: false,
    };
    hosted
        .store
        .update_grant_with_groups(
            &generation,
            "read-client",
            update(0),
            Some(vec!["chat:42".into(), "supergroup:42".into()]),
        )
        .await
        .expect("grant");

    let (mut client, _runner, _session) = stalled_client().await;
    let mut supergroup = raw_tl_channel(42, "Fixture supergroup", None);
    supergroup.broadcast = false;
    supergroup.megagroup = true;
    let basic = tl::types::Chat {
        creator: false,
        left: false,
        deactivated: false,
        call_active: false,
        call_not_empty: false,
        noforwards: false,
        id: 42,
        title: "Fixture chat".into(),
        photo: tl::types::ChatPhotoEmpty {}.into(),
        participants_count: 2,
        date: 1,
        version: 1,
        migrated_to: None,
        admin_rights: None,
        default_banned_rights: None,
    };
    client.set_metadata_fixture(MetadataFixture {
        dialogs: vec![basic.into(), supergroup.into()],
        ..MetadataFixture::default()
    });
    let client = Arc::new(client);
    let limiter = seed_fixture(&hosted, &generation, &client).await;

    let journal = hosted.groups.journal().expect("group journal");
    for (group, text) in [
        ("chat:42", "basic edit"),
        ("supergroup:42", "supergroup edit"),
    ] {
        journal.enable(generation.id(), group).await.expect("group");
        journal
            .append(
                generation.id(),
                group,
                vec![PendingChange {
                    source: Some("edit-104".into()),
                    change: Change::Edited {
                        message_id: "104".into(),
                        edited_at: "2026-11-21T10:04:01Z".into(),
                        text: text.into(),
                    },
                }],
            )
            .await
            .expect("capture edit");
    }
    let app = build_router(
        verifier,
        BrowserConfig {
            client_id: "browser".into(),
            client_secret: SecretString::from("test-only-secret"),
            callback_url: url::Url::parse("https://service.example/auth/callback")
                .expect("callback"),
        },
        Some(Arc::clone(&hosted)),
    )
    .expect("routes");
    let socket = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("local API listener");
    let address = socket.local_addr().expect("listener address");
    let stop = hosted.cancellation.clone();
    let server = tokio::spawn(async move {
        crate::service::serve(socket, app, Duration::from_secs(1), async move {
            stop.cancelled().await
        })
        .await
        .expect("server lifecycle");
    });
    let http = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("HTTP client");
    let token = issuer.read_token("reader", false);
    let request = |group: &str, query: &str| {
        http.get(format!(
            "http://{address}/v1/groups/{group}/changes?{query}"
        ))
        .bearer_auth(&token)
    };
    let response = request("chat:42", "start=earliest&limit=1")
        .send()
        .await
        .expect("chat response");
    assert_eq!(response.status(), StatusCode::OK);
    let basic: Page = response.json().await.expect("chat page");
    let response = request("supergroup:42", "start=earliest")
        .send()
        .await
        .expect("supergroup response");
    assert_eq!(response.status(), StatusCode::OK);
    let group: Page = response.json().await.expect("supergroup page");
    assert_ne!(group.stream_id, basic.stream_id);
    assert!(
        matches!(&basic.entries[0].change, Change::Edited { text, .. } if text == "basic edit")
    );
    assert!(
        matches!(&group.entries[0].change, Change::Edited { text, .. } if text == "supergroup edit")
    );
    let returned: Value = request("chat:42", &format!("after={}", basic.base_cursor))
        .send()
        .await
        .expect("replay")
        .json()
        .await
        .expect("JSON replay");
    assert_eq!(
        returned,
        serde_json::to_value(&basic).expect("same committed page")
    );
    let crossed = request("chat:42", &format!("after={}", group.base_cursor))
        .send()
        .await
        .expect("crossed cursor");
    assert_eq!(crossed.status(), StatusCode::BAD_REQUEST);
    let cursor = &basic.entries[0].cursor;
    let empty = request("chat:42", &format!("after={cursor}&wait_ms=20"))
        .send()
        .await
        .expect("empty wait");
    assert_eq!(empty.status(), StatusCode::OK);
    let empty: Page = empty.json().await.expect("empty page");
    assert_eq!(empty.base_cursor, *cursor);
    assert!(empty.entries.is_empty() && !empty.has_more);
    let bad = request("chat:42", "start=earliest&wait_ms=15001")
        .send()
        .await
        .expect("invalid wait");
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);

    let before = limiter.available_tokens();
    let pending = tokio::spawn(request("chat:42", &format!("after={cursor}&wait_ms=15000")).send());
    wait_for_membership(&limiter, before).await;
    journal
        .append(
            generation.id(),
            "chat:42",
            vec![PendingChange {
                source: Some("second-edit-104".into()),
                change: Change::Edited {
                    message_id: "104".into(),
                    edited_at: "2026-11-21T10:04:01Z".into(),
                    text: "second basic edit".into(),
                },
            }],
        )
        .await
        .expect("commit change while waiting");
    hosted.groups.changed.notify_waiters();
    let response = tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .expect("commit woke the poll")
        .expect("poll task")
        .expect("poll response");
    assert_eq!(response.status(), StatusCode::OK);
    let published: Page = response.json().await.expect("committed page");
    assert_eq!(published.base_cursor, *cursor);
    assert_eq!(published.entries.len(), 1);
    assert_eq!(published.entries[0].seq, "2");

    let cursor = &published.entries[0].cursor;
    let before = limiter.available_tokens();
    let pending = tokio::spawn(request("chat:42", &format!("after={cursor}&wait_ms=15000")).send());
    wait_for_membership(&limiter, before).await;
    {
        let _transition = hosted.transition.lock().await;
        hosted
            .store
            .update_grant_with_groups(&generation, "read-client", update(1), Some(Vec::new()))
            .await
            .expect("revoke while waiting");
        let _ = hosted.grant_changes.send(());
    }
    let response = tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .expect("revocation woke the poll")
        .expect("poll task")
        .expect("revocation response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let error: Value = response.json().await.expect("safe error");
    assert_eq!(error, json!({"error":{"code":"unavailable_resource"}}));
    let retired = hosted
        .store
        .retire(&issuer_url, "reader", 1)
        .await
        .expect("unlink")
        .expect("retired account");
    assert!(retired == generation);
    journal
        .retire(generation.id())
        .await
        .expect("purge retired stream");
    let replacement = hosted
        .store
        .start(&issuer_url, "reader")
        .await
        .expect("relink");
    hosted
        .store
        .activate(&replacement, 1)
        .await
        .expect("activate replacement");
    hosted
        .store
        .update_grant_with_groups(
            &replacement,
            "read-client",
            update(0),
            Some(vec!["chat:42".into()]),
        )
        .await
        .expect("grant to replacement link");
    seed_fixture(&hosted, &replacement, &client).await;
    journal
        .enable(replacement.id(), "chat:42")
        .await
        .expect("replacement stream");
    let expired = request("chat:42", &format!("after={cursor}"))
        .send()
        .await
        .expect("expired cursor response");
    assert_eq!(expired.status(), StatusCode::GONE);
    let expired: Value = expired.json().await.expect("recovery boundary");
    assert_eq!(expired["error"]["code"], "cursor_expired");
    assert_eq!(expired["base_seq"], "0");
    assert_ne!(expired["stream_id"], basic.stream_id);

    hosted.cancellation.cancel();
    server.await.expect("server stopped");
}

async fn wait_for_membership(limiter: &RateLimiter, before: f64) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while limiter.available_tokens() >= before {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("long poll authorized and checking membership");
}

async fn seed_fixture(
    hosted: &Hosted,
    generation: &AccountGeneration,
    client: &Arc<TelegramClient>,
) -> Arc<RateLimiter> {
    let mut account_limiter = None;
    hosted
        .seed_account(generation.clone(), |limiter| {
            limiter.refund(40);
            account_limiter = Some(Arc::clone(&limiter));
            let (streams, _) = test_support::account_streams(
                client.clone(),
                limiter.clone(),
                hosted.config.events.stream_limits(),
                Arc::clone(&hosted.streams),
            );
            let operations =
                AccountOperations::new(Arc::clone(client), limiter, AccountLimits::default());
            ActiveAccount {
                client: Arc::clone(client),
                operations,
                streams,
                capture: None,
            }
        })
        .await
        .expect("fixture account");
    account_limiter.expect("fixture limiter")
}
