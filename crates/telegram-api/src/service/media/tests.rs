use super::{MediaOwner, MediaStore};
use crate::contracts::ErrorCode;
use crate::service::AccountStore;
use std::time::Duration;
use telegram_core::telegram::types::ChannelId;
use tokio::time::Instant;

#[tokio::test]
async fn handles_require_generation_client_revision_and_unexpired_authority() {
    let dir = tempfile::tempdir().expect("private fixture root");
    let accounts = AccountStore::open(&dir.path().join("service"))
        .await
        .expect("private account store");
    let first = accounts.start("issuer", "first").await.ok().unwrap();
    let second = accounts
        .start("issuer", "second")
        .await
        .expect("second generation");
    tokio::time::pause();
    let store = MediaStore::default();
    let owner = MediaOwner {
        generation: first,
        client: "direct".into(),
        revision: 1,
    };
    let channel = ChannelId::new(42).unwrap();
    let preview = vec![0xff, 0xd8, 1, 2];
    let expires = Instant::now() + Duration::from_secs(1);
    let handle = store
        .insert(&owner, channel, preview.clone(), expires)
        .ok()
        .unwrap();
    let fetched = store.lookup(&owner, &handle).ok().unwrap();
    assert_eq!(fetched.channel, channel);
    assert_eq!(fetched.jpeg.as_ref(), preview);
    for foreign in [
        MediaOwner {
            generation: second,
            ..owner.clone()
        },
        MediaOwner {
            client: "delegated".into(),
            ..owner.clone()
        },
        MediaOwner {
            revision: 2,
            ..owner.clone()
        },
    ] {
        assert!(
            matches!(store.lookup(&foreign, &handle), Err(error) if error.0 == ErrorCode::UnavailableResource)
        );
    }

    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(
        matches!(store.lookup(&owner, &handle), Err(error) if error.0 == ErrorCode::UnavailableResource)
    );
}

#[tokio::test]
async fn invalidation_releases_cached_capacity_but_in_flight_bytes_remain_charged() {
    let dir = tempfile::tempdir().expect("private fixture root");
    let accounts = AccountStore::open(&dir.path().join("service"))
        .await
        .expect("private account store");
    let generation = accounts.start("issuer", "subject").await.ok().unwrap();
    let store = MediaStore::default();
    let owner = MediaOwner {
        generation,
        client: "direct".into(),
        revision: 1,
    };
    let channel = ChannelId::new(42).unwrap();
    let preview = vec![0xff, 0xd8, 1, 2];
    let expires = Instant::now() + Duration::from_secs(10);
    let handle = store
        .insert(&owner, channel, preview, expires)
        .ok()
        .unwrap();
    let fetched = store.lookup(&owner, &handle).ok().unwrap();
    let remaining = store.global.bytes.available_permits();
    store.invalidate(&owner.generation, Some(&owner.client));
    assert!(store.lookup(&owner, &handle).is_err());
    assert_eq!(store.global.bytes.available_permits(), remaining);
    drop(fetched);
    assert_eq!(store.global.bytes.available_permits(), remaining + 4);
}

#[tokio::test]
async fn api_rejects_foreign_revoked_and_ineligible_handles_without_opening_telegram() {
    use crate::contracts::{GrantUpdate, ReadGrant};
    use crate::identity_http::Fixture as IssuerFixture;
    use crate::service::accounts::Hosted;
    use crate::service::{BrowserConfig, HostedConfig, build_router};
    use reqwest::{Client, StatusCode};
    use secrecy::SecretString;
    use serde_json::{Value, json};
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use url::Url;

    let issuer = IssuerFixture::start().await;
    let verifier = issuer.read_verifier();
    let root = tempfile::tempdir().expect("private account root");
    let config: HostedConfig = serde_json::from_value(json!({
        "data_root": root.path(),
        "telegram": {
            "api_id": 1,
            "api_hash": "fixture-hash",
            "session_file": root.path().join("unused.session")
        },
        "registered_clients": ["read-client"],
        "eligible_channels": ["42"],
        "request_seconds": 5
    }))
    .expect("hosted fixture config");
    let hosted = Hosted::open(config)
        .await
        .unwrap_or_else(|_| panic!("open hosted fixture"));
    let first = hosted
        .store
        .start(verifier.issuer(), "first")
        .await
        .expect("first generation");
    let second = hosted
        .store
        .start(verifier.issuer(), "second")
        .await
        .expect("second generation");
    for (generation, telegram_id) in [(&first, 1), (&second, 2)] {
        hosted
            .store
            .activate(generation, telegram_id)
            .await
            .expect("activate fixture");
        hosted
            .store
            .update_grant(
                generation,
                "read-client",
                GrantUpdate {
                    expected_revision: 0,
                    read: ReadGrant::Channels(vec!["42".into()]),
                    send_enabled: false,
                },
            )
            .await
            .expect("persist grant");
    }
    let owner = MediaOwner {
        generation: first.clone(),
        client: "read-client".into(),
        revision: 1,
    };
    let handle = hosted
        .media
        .insert(
            &owner,
            ChannelId::new(42).unwrap(),
            vec![0xff, 0xd8, 1, 2],
            Instant::now() + Duration::from_secs(120),
        )
        .unwrap_or_else(|_| panic!("seed preview"));
    let router = build_router(
        verifier,
        BrowserConfig {
            client_id: "read-client".into(),
            client_secret: SecretString::from("fixture-secret"),
            callback_url: Url::parse("https://service.example/auth/callback").expect("callback"),
        },
        Some(Arc::clone(&hosted)),
    )
    .expect("hosted router");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("loopback listener");
    let base = format!(
        "http://{}",
        listener.local_addr().expect("loopback address")
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve fixture");
    });
    let http = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(4))
        .build()
        .unwrap();
    let expected = json!({"error": {"code": "unavailable_resource"}});
    let foreign = http
        .get(format!("{base}/v1/media/{handle}"))
        .bearer_auth(issuer.read_token("second", false))
        .send()
        .await
        .expect("foreign handle");
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        foreign.json::<Value>().await.expect("foreign error"),
        expected
    );

    hosted
        .store
        .update_grant(
            &first,
            "read-client",
            GrantUpdate {
                expected_revision: 1,
                read: ReadGrant::None,
                send_enabled: false,
            },
        )
        .await
        .expect("revoke read grant");
    let revoked = http
        .get(format!("{base}/v1/media/{handle}"))
        .bearer_auth(issuer.read_token("first", false))
        .send()
        .await
        .expect("revoked handle");
    assert_eq!(revoked.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        revoked.json::<Value>().await.expect("revoked error"),
        expected
    );

    let ineligible = http
        .post(format!("{base}/v1/media/previews"))
        .bearer_auth(issuer.read_token("second", false))
        .json(&json!({"channel_id":"99","message_ids":[104]}))
        .send()
        .await
        .expect("out-of-scope channel");
    assert_eq!(ineligible.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        ineligible.json::<Value>().await.expect("scope error"),
        expected
    );
    assert!(!first.session_file().exists());
    assert!(!second.session_file().exists());
    hosted.cancellation.cancel();
    server.abort();
}
