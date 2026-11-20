use super::{MAX_IMAGE_BYTES, TOTAL_BYTES, UploadReply, Uploads, upload};
use crate::contracts::{ErrorCode, GrantUpdate, ReadGrant, Scope};
use crate::identity_http::Fixture as Issuer;
use crate::service::accounts::Hosted;
use crate::service::authorization::ReadAccess;
use crate::service::browser_auth::{BrowserAuth, BrowserConfig};
use crate::service::{ApiError, ServiceState};
use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, header};
use futures::StreamExt as _;
use secrecy::SecretString;
use serde_json::json;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use telegram_core::telegram::sending::SendDeadline;
use telegram_core::telegram::types::sending::SendMediaKind;
use tokio::sync::Semaphore;
use tokio::time::Instant;

struct Fixture {
    state: Arc<ServiceState>,
    issuer: Issuer,
    _data: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let issuer = Issuer::start().await;
        let verifier = issuer.read_verifier();
        let data = tempfile::tempdir().expect("private root");
        let config = serde_json::from_value(json!({
            "data_root": data.path(), "telegram": {"api_id":1,"api_hash":"fixture"},
            "registered_clients":["sender","other"], "sending_enabled":true
        }))
        .expect("config");
        let hosted = Hosted::open(config)
            .await
            .unwrap_or_else(|_| panic!("hosted root"));
        let generation = hosted
            .store
            .start(verifier.issuer(), "owner")
            .await
            .expect("generation");
        hosted
            .store
            .activate(&generation, 17)
            .await
            .expect("activate");
        for client in ["sender", "other"] {
            hosted
                .store
                .update_grant(
                    &generation,
                    client,
                    GrantUpdate {
                        expected_revision: 0,
                        read: ReadGrant::None,
                        send_enabled: true,
                    },
                )
                .await
                .expect("grant");
        }
        let browser = BrowserAuth::new(
            BrowserConfig {
                client_id: "browser".into(),
                client_secret: SecretString::from("test secret"),
                callback_url: url::Url::parse("https://service.example/auth/callback")
                    .expect("callback"),
            },
            &verifier,
        )
        .expect("browser fixture");
        Self {
            state: Arc::new(ServiceState {
                verifier,
                browser,
                bearer_challenge: HeaderValue::from_static("Bearer"),
                admission: Semaphore::new(8),
                hosted: Some(hosted),
            }),
            issuer,
            _data: data,
        }
    }

    fn store(&self) -> &Uploads {
        self.state
            .hosted
            .as_ref()
            .unwrap()
            .uploads
            .as_ref()
            .unwrap()
    }

    fn headers(&self, client: &str, mime: &str, length: Option<u32>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", self.issuer.send_token("owner", client))
                .parse()
                .unwrap(),
        );
        headers.insert(header::CONTENT_TYPE, mime.parse().unwrap());
        if let Some(length) = length {
            headers.insert(header::CONTENT_LENGTH, length.into());
        }
        headers
    }

    fn incoming(&self) -> usize {
        std::fs::read_dir(self.store()._root.path())
            .expect("incoming directory")
            .count()
    }

    async fn wait_for_files(&self, expected: usize) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while self.incoming() != expected {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("staging file count");
    }

    fn send(
        &self,
        headers: HeaderMap,
        body: Body,
    ) -> impl Future<Output = Result<Json<UploadReply>, ApiError>> + Send + 'static {
        upload(State(self.state.clone()), headers, body)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.state.hosted.as_ref().unwrap().cancellation.cancel();
    }
}

fn expect_error<T>(result: Result<T, ApiError>, expected: ErrorCode) {
    match result {
        Err(error) => assert_eq!(error.0, expected),
        Ok(_) => panic!("expected {expected:?}"),
    }
}

#[tokio::test]
async fn declared_observed_and_type_limits_reject_without_retaining_files_or_quota() {
    let fixture = Fixture::new().await;
    let jpeg = telegram_core::test_helpers::create_test_jpeg(8, 8);
    let length = u32::try_from(jpeg.len()).unwrap();
    for (mime, claimed, body, error) in [
        (
            "image/jpeg",
            Some(length + 1),
            jpeg.clone(),
            ErrorCode::InvalidRequest,
        ),
        (
            "image/jpeg",
            Some(length - 1),
            jpeg.clone(),
            ErrorCode::PayloadTooLarge,
        ),
        ("image/png", None, jpeg.clone(), ErrorCode::InvalidRequest),
        (
            "image/jpeg",
            Some(MAX_IMAGE_BYTES + 1),
            vec![],
            ErrorCode::PayloadTooLarge,
        ),
        (
            "image/jpeg",
            None,
            vec![0; MAX_IMAGE_BYTES as usize + 1],
            ErrorCode::PayloadTooLarge,
        ),
        ("image/jpeg", None, vec![], ErrorCode::InvalidRequest),
        ("text/plain", None, vec![1], ErrorCode::InvalidRequest),
    ] {
        expect_error(
            fixture
                .send(fixture.headers("sender", mime, claimed), Body::from(body))
                .await,
            error,
        );
        assert_eq!(fixture.incoming(), 0);
        assert_eq!(fixture.store().capacity.available_permits(), TOTAL_BYTES);
    }

    let headers = fixture.headers("sender", "image/jpeg", Some(length));
    let Json(reply) = fixture
        .send(headers.clone(), Body::from(jpeg))
        .await
        .unwrap_or_else(|_| panic!("valid upload"));
    assert_eq!(reply.size_bytes, length);
    let access = ReadAccess::for_scope(&fixture.state, &headers, Scope::Send)
        .await
        .unwrap_or_else(|_| panic!("authorized"));
    let deadline = SendDeadline::at(access.opening_context().deadline());
    let stage = |id| {
        fixture
            .store()
            .stage(&access, id, &SendMediaKind::Image, deadline)
    };
    expect_error(stage("../outside").await, ErrorCode::UnavailableResource);
    let snapshot = stage(&reply.upload_handle)
        .await
        .unwrap_or_else(|_| panic!("staged image"));
    assert_eq!(snapshot.size_bytes(), u64::from(length));
    drop(snapshot);
    {
        let mut saved = fixture.store().saved.lock().unwrap();
        let object = Arc::get_mut(saved.get_mut(&reply.upload_handle).unwrap()).unwrap();
        object.expires = Instant::now();
    }
    expect_error(
        stage(&reply.upload_handle).await,
        ErrorCode::UnavailableResource,
    );
    fixture.store().reap();
    assert_eq!(fixture.incoming(), 0);
    assert_eq!(fixture.store().capacity.available_permits(), TOTAL_BYTES);
}

#[tokio::test]
async fn per_client_and_aggregate_quotas_include_streams_in_progress_and_release_on_cancellation() {
    let fixture = Fixture::new().await;
    let jpeg = telegram_core::test_helpers::create_test_jpeg(8, 8);
    let stalled = |client: &str, mime: &str| {
        let chunks = futures::stream::once(async { Ok::<_, io::Error>(Bytes::from_static(b"a")) })
            .chain(futures::stream::pending());
        tokio::spawn(fixture.send(
            fixture.headers(client, mime, None),
            Body::from_stream(chunks),
        ))
    };
    let first = stalled("sender", "image/jpeg");
    let second = stalled("sender", "image/jpeg");
    fixture.wait_for_files(2).await;
    expect_error(
        fixture
            .send(
                fixture.headers("sender", "image/jpeg", None),
                Body::from(jpeg.clone()),
            )
            .await,
        ErrorCode::RateLimited,
    );
    assert_eq!(fixture.incoming(), 2);
    for task in [first, second] {
        task.abort();
        assert!(task.await.is_err());
    }
    fixture.wait_for_files(0).await;

    let first = stalled("sender", "audio/mpeg");
    let second = stalled("other", "audio/mpeg");
    fixture.wait_for_files(2).await;
    assert_eq!(fixture.store().capacity.available_permits(), 0);
    expect_error(
        fixture
            .send(
                fixture.headers("sender", "image/jpeg", None),
                Body::from(jpeg),
            )
            .await,
        ErrorCode::RateLimited,
    );
    let hosted = fixture.state.hosted.as_ref().unwrap();
    let generation = hosted
        .store
        .active(fixture.state.verifier.issuer(), "owner")
        .await
        .unwrap()
        .unwrap();
    {
        let _transition = hosted.transition.lock().await;
        hosted
            .store
            .update_grant(
                &generation,
                "sender",
                GrantUpdate {
                    expected_revision: 1,
                    read: ReadGrant::None,
                    send_enabled: false,
                },
            )
            .await
            .expect("revoke sender");
        fixture.store().invalidate(&generation, Some("sender"));
    }
    let result = tokio::time::timeout(Duration::from_secs(2), first)
        .await
        .expect("revoked body reader ended")
        .expect("upload task");
    expect_error(result, ErrorCode::UnavailableResource);
    assert_eq!(fixture.incoming(), 1);
    second.abort();
    assert!(second.await.is_err());
    fixture.wait_for_files(0).await;
    assert_eq!(fixture.store().capacity.available_permits(), TOTAL_BYTES);
}

#[tokio::test]
async fn revocation_before_publication_discards_the_completed_file() {
    let fixture = Fixture::new().await;
    let hosted = fixture.state.hosted.as_ref().unwrap();
    let generation = hosted
        .store
        .active(fixture.state.verifier.issuer(), "owner")
        .await
        .unwrap()
        .unwrap();
    let (tx, rx) = futures::channel::mpsc::unbounded::<Result<Bytes, io::Error>>();
    let task = tokio::spawn(fixture.send(
        fixture.headers("sender", "image/jpeg", None),
        Body::from_stream(rx),
    ));
    fixture.wait_for_files(1).await;
    {
        let _transition = hosted.transition.lock().await;
        let jpeg = telegram_core::test_helpers::create_test_jpeg(8, 8);
        let size = jpeg.len();
        tx.unbounded_send(Ok(jpeg.into())).expect("supply image");
        drop(tx);
        tokio::time::timeout(Duration::from_secs(2), async {
            while fixture.store().capacity.available_permits() != TOTAL_BYTES - size {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("type check completed");
        assert!(fixture.store().saved.lock().unwrap().is_empty());
        hosted
            .store
            .update_grant(
                &generation,
                "sender",
                GrantUpdate {
                    expected_revision: 1,
                    read: ReadGrant::None,
                    send_enabled: false,
                },
            )
            .await
            .expect("revoke");
        fixture.store().invalidate(&generation, Some("sender"));
    }
    expect_error(
        task.await.expect("upload task"),
        ErrorCode::UnavailableResource,
    );
    assert!(fixture.store().saved.lock().unwrap().is_empty());
    assert_eq!(fixture.incoming(), 0);
    assert_eq!(fixture.store().capacity.available_permits(), TOTAL_BYTES);
}

#[tokio::test]
async fn disabled_restart_purges_uploads_left_by_an_abruptly_terminated_service() {
    const ROOT: &str = "TELEGRAM_TEST_CRASHED_UPLOAD_ROOT";
    let config = |root: &std::path::Path, sending_enabled: bool| {
        serde_json::from_value(json!({
            "data_root":root, "telegram":{"api_id":1,"api_hash":"fixture"},
            "registered_clients":["sender"], "sending_enabled":sending_enabled
        }))
        .expect("crash fixture config")
    };
    if let Some(root) = std::env::var_os(ROOT) {
        let hosted = Hosted::open(config(root.as_ref(), true))
            .await
            .unwrap_or_else(|_| panic!("enabled root"));
        let uploads = hosted.uploads.as_ref().expect("upload store");
        for dir in [uploads._root.path(), uploads._snapshots.path()] {
            std::fs::write(dir.join("unfinished"), b"disposable upload fixture")
                .expect("staged fixture");
        }
        let generation = hosted
            .store
            .start("issuer", "owner")
            .await
            .expect("durable owner");
        hosted
            .store
            .activate(&generation, 17)
            .await
            .expect("active fixture");
        // OS closes the service lock; TempDir and staging destructors do not run.
        std::process::exit(0);
    }
    let data = tempfile::tempdir().expect("data root");
    let root = data.path().join("service");
    let child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact","service::uploads::tests::disabled_restart_purges_uploads_left_by_an_abruptly_terminated_service"])
        .env(ROOT,&root).kill_on_drop(true).output();
    let output = tokio::time::timeout(Duration::from_secs(10), child)
        .await
        .expect("fixture stopped")
        .expect("child test");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for name in ["incoming", "snapshots"] {
        let mut entries = std::fs::read_dir(root.join(name)).expect("crashed subtree");
        let temp = entries.next().unwrap().unwrap().path();
        assert!(temp.join("unfinished").is_file());
    }
    let adjacent = root.join("keep");
    std::fs::write(&adjacent, b"unrelated data").unwrap();
    let hosted = Hosted::open(config(&root, false))
        .await
        .unwrap_or_else(|_| panic!("disabled restart"));
    assert!(hosted.uploads.is_none());
    assert!(
        hosted
            .store
            .active("issuer", "owner")
            .await
            .expect("account data")
            .is_some()
    );
    for name in ["incoming", "snapshots"] {
        assert!(!root.join(name).exists(), "stale {name}");
    }
    assert_eq!(std::fs::read(adjacent).unwrap(), b"unrelated data");
    hosted.cancellation.cancel();
}
