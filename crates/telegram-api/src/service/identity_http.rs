//! Real loopback API and issuer sockets, with disposable signing keys.

use crate::contracts::{ErrorCode, ErrorResponse, SessionInfo, SessionKind};
use crate::identity::{Audience, AuthorityConfig, CredentialVerifier, SigningAlgorithm, unix_now};
use crate::service::{BrowserConfig, router};
use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap, StatusCode as IssuerStatus, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use reqwest::{Client, StatusCode};
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use url::Url;

const API: &str = "https://service.example/v1";
const MCP: &str = "https://mcp.example/mcp";
const CALLBACK: &str = "https://service.example/auth/callback";
const BROWSER: &str = "browser-client";
const GATEWAY: &str = "mcp-gateway";
const FIRST_SEED: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
const FIRST_PUBLIC: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const SECOND_SEED: &str = "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb";
const SECOND_PUBLIC: &str = "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c";

struct Host {
    base: String,
    task: JoinHandle<()>,
}

impl Drop for Host {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn serve(listener: TcpListener, app: Router) -> Host {
    let base = format!(
        "http://{}",
        listener.local_addr().expect("loopback address")
    );
    let task =
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serve test host") });
    Host { base, task }
}

struct Issuer {
    base: String,
    active: AtomicUsize,
    key_reads: AtomicUsize,
    code_count: AtomicUsize,
    codes: Mutex<HashMap<String, (String, String)>>,
    bad_nonce: AtomicBool,
}

struct Fixture {
    http: Client,
    issuer: Arc<Issuer>,
    verifier: CredentialVerifier,
    api: Host,
    _issuer_socket: Host,
}

impl Fixture {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("issuer socket");
        let issuer = Arc::new(Issuer {
            base: format!("http://{}", listener.local_addr().expect("issuer address")),
            active: AtomicUsize::new(1),
            key_reads: AtomicUsize::new(0),
            code_count: AtomicUsize::new(0),
            codes: Mutex::default(),
            bad_nonce: AtomicBool::new(false),
        });
        let issuer_socket = serve(
            listener,
            Router::new()
                .route("/.well-known/openid-configuration", get(discovery))
                .route("/jwks", get(jwks))
                .route("/authorize", get(authorize))
                .route("/token", post(redeem))
                .with_state(Arc::clone(&issuer)),
        );
        let verifier = CredentialVerifier::discover_loopback(AuthorityConfig {
            issuer: issuer.base.clone(),
            api_audience: API.into(),
            mcp_audience: MCP.into(),
            gateway_actor: GATEWAY.into(),
            subject_claim: Some("stable_sub".into()),
            algorithms: vec![SigningAlgorithm::EdDSA],
        })
        .await
        .expect("test discovery");
        let app = router(
            verifier.clone(),
            BrowserConfig {
                client_id: BROWSER.into(),
                client_secret: SecretString::from("secret + value"),
                callback_url: Url::parse(CALLBACK).expect("public callback URL"),
            },
        )
        .expect("auth router");
        let api = serve(
            TcpListener::bind("127.0.0.1:0").await.expect("API socket"),
            app,
        );
        let http = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .expect("test HTTP client");
        Self {
            http,
            issuer,
            verifier,
            api,
            _issuer_socket: issuer_socket,
        }
    }

    fn claims(&self, audience: &str, client_id: &str, scope: &str) -> Value {
        access_claims(&self.issuer.base, audience, client_id, scope)
    }

    async fn bearer(&self, token: &str) -> reqwest::Response {
        self.http
            .get(format!("{}/v1/session", self.api.base))
            .bearer_auth(token)
            .header("x-owner-id", "forged-owner")
            .header("x-client-id", "forged-client")
            .send()
            .await
            .expect("bearer request")
    }

    async fn start_login(&self) -> (String, Url) {
        let response = self
            .http
            .get(format!("{}/auth/start", self.api.base))
            .send()
            .await
            .expect("login start");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
        let cookie = get_cookie(&response, "__Host-tg_login");
        let location = Url::parse(
            response.headers()[header::LOCATION]
                .to_str()
                .expect("authorization URL"),
        )
        .expect("absolute authorization URL");
        assert!(
            location
                .query_pairs()
                .any(|(name, value)| name == "code_challenge_method" && value == "S256")
        );
        assert!(
            location
                .query_pairs()
                .all(|(name, _)| name != "code_verifier")
        );
        (cookie, location)
    }

    async fn authorization(&self, url: Url) -> Url {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .expect("issuer authorization");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        Url::parse(
            response.headers()[header::LOCATION]
                .to_str()
                .expect("callback URL"),
        )
        .expect("absolute callback URL")
    }

    async fn callback(&self, cookie: &str, upstream: &Url) -> reqwest::Response {
        self.http
            .get(format!(
                "{}/auth/callback?{}",
                self.api.base,
                upstream.query().expect("callback query")
            ))
            .header(header::COOKIE, cookie)
            .send()
            .await
            .expect("callback request")
    }
}

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|byte| {
            u8::from_str_radix(std::str::from_utf8(byte).expect("ASCII hex"), 16).expect("hex byte")
        })
        .collect()
}

fn token(key: usize, kid: &str, typ: &str, claims: &Value) -> String {
    let mut header = Header::new(Algorithm::EdDSA);
    header.kid = Some(kid.into());
    header.typ = Some(typ.into());
    let mut pkcs8 = bytes("302e020100300506032b657004220420");
    pkcs8.extend(bytes(if key == 1 { FIRST_SEED } else { SECOND_SEED }));
    encode(&header, claims, &EncodingKey::from_ed_der(&pkcs8)).expect("sign test credential")
}

fn access_claims(issuer: &str, audience: &str, client_id: &str, scope: &str) -> Value {
    let now = unix_now().expect("test time");
    json!({
        "iss": issuer, "sub": "api-pairwise", "stable_sub": "same-user", "aud": audience,
        "exp": now + 120, "iat": now, "jti": "test-token", "client_id": client_id, "scope": scope
    })
}

async fn discovery(State(issuer): State<Arc<Issuer>>) -> Json<Value> {
    Json(json!({
        "issuer": issuer.base, "jwks_uri": format!("{}/jwks", issuer.base),
        "authorization_endpoint": format!("{}/authorize", issuer.base),
        "token_endpoint": format!("{}/token", issuer.base)
    }))
}

async fn jwks(State(issuer): State<Arc<Issuer>>) -> Json<Value> {
    issuer.key_reads.fetch_add(1, Ordering::Relaxed);
    let active = issuer.active.load(Ordering::Relaxed);
    let public = if active == 1 {
        FIRST_PUBLIC
    } else {
        SECOND_PUBLIC
    };
    Json(json!({"keys": [{
        "kty": "OKP", "crv": "Ed25519", "use": "sig", "alg": "EdDSA",
        "kid": active.to_string(), "x": URL_SAFE_NO_PAD.encode(bytes(public))
    }]}))
}

async fn authorize(
    State(issuer): State<Arc<Issuer>>,
    Query(values): Query<BTreeMap<String, String>>,
) -> Response {
    if values.get("client_id").map(String::as_str) != Some(BROWSER)
        || values.get("redirect_uri").map(String::as_str) != Some(CALLBACK)
        || values.get("resource").map(String::as_str) != Some(API)
        || values.get("scope").map(String::as_str) != Some("openid telegram.link")
    {
        return IssuerStatus::BAD_REQUEST.into_response();
    }
    let code = format!(
        "login-{}",
        issuer.code_count.fetch_add(1, Ordering::Relaxed)
    );
    issuer.codes.lock().expect("issuer state").insert(
        code.clone(),
        (values["nonce"].clone(), values["code_challenge"].clone()),
    );
    let mut url = Url::parse(CALLBACK).expect("public callback");
    url.query_pairs_mut().extend_pairs([
        ("iss", issuer.base.as_str()),
        ("code", code.as_str()),
        ("state", &values["state"]),
    ]);
    Redirect::to(url.as_str()).into_response()
}

async fn redeem(
    State(issuer): State<Arc<Issuer>>,
    headers: HeaderMap,
    Form(values): Form<BTreeMap<String, String>>,
) -> Response {
    let credentials = format!(
        "Basic {}",
        STANDARD.encode("browser-client:secret+%2B+value")
    );
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        != Some(credentials.as_str())
        || values.get("grant_type").map(String::as_str) != Some("authorization_code")
        || values.get("redirect_uri").map(String::as_str) != Some(CALLBACK)
        || values.get("resource").map(String::as_str) != Some(API)
    {
        return IssuerStatus::BAD_REQUEST.into_response();
    }
    let grant = issuer
        .codes
        .lock()
        .expect("issuer state")
        .remove(&values["code"]);
    let Some((nonce, challenge)) = grant else {
        return IssuerStatus::BAD_REQUEST.into_response();
    };
    if URL_SAFE_NO_PAD.encode(Sha256::digest(values["code_verifier"].as_bytes())) != challenge {
        return IssuerStatus::BAD_REQUEST.into_response();
    }
    let now = unix_now().expect("test time");
    let id = json!({
        "iss": issuer.base, "sub": "browser-pairwise", "stable_sub": "same-user", "aud": BROWSER,
        "iat": now, "exp": now + 60,
        "nonce": if issuer.bad_nonce.load(Ordering::Relaxed) { "wrong" } else { &nonce }
    });
    Json(json!({
        "id_token": token(1, "1", "JWT", &id),
        "access_token": token(1, "1", "at+jwt", &access_claims(&issuer.base, API, BROWSER, "telegram.link")),
        "token_type": "Bearer"
    })).into_response()
}

fn get_cookie(response: &reqwest::Response, name: &str) -> String {
    let raw = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|header| header.to_str().ok())
        .find(|value| value.starts_with(&format!("{name}=")))
        .expect("expected response cookie");
    assert!(raw.contains("; Path=/; Secure; HttpOnly; SameSite=Lax"));
    raw.split(';').next().expect("cookie pair").to_owned()
}

async fn expect_error(response: reqwest::Response, status: StatusCode, code: ErrorCode) {
    assert_eq!(response.status(), status, "response to invalid identity");
    let error: ErrorResponse = response.json().await.expect("safe error body");
    assert_eq!(error.error.code, code);
}

#[tokio::test]
async fn service_binds_credentials_to_verified_issuer_audience_and_client() {
    let fixture = Fixture::start().await;
    let mut claims = fixture.claims(API, "signed-client", "telegram.read");
    let bearer = token(1, "1", "at+jwt", &claims);
    let session: SessionInfo = fixture
        .bearer(&bearer)
        .await
        .error_for_status()
        .expect("authorized session")
        .json()
        .await
        .expect("session");
    assert_eq!(session.client_id, "signed-client");
    assert_eq!(session.kind, SessionKind::Direct);
    assert!(session.csrf_token.is_none());
    assert_eq!(
        fixture
            .http
            .get(format!("{}/readyz", fixture.api.base))
            .send()
            .await
            .expect("ready")
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        fixture
            .http
            .get(format!("{}/v1/session", fixture.api.base))
            .header("X-Owner-Id", "same-user")
            .send()
            .await
            .expect("no credentials")
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let missing = fixture
        .http
        .get(format!("{}/v1/session", fixture.api.base))
        .send()
        .await
        .expect("missing credential response");
    assert_eq!(
        missing.headers()[header::WWW_AUTHENTICATE],
        "Bearer resource_metadata=\"https://service.example/.well-known/oauth-protected-resource/v1\""
    );
    let resource: Value = fixture
        .http
        .get(format!(
            "{}/.well-known/oauth-protected-resource/v1",
            fixture.api.base
        ))
        .send()
        .await
        .expect("protected resource metadata")
        .error_for_status()
        .expect("metadata route")
        .json()
        .await
        .expect("metadata JSON");
    assert_eq!(resource["resource"], API);
    assert_eq!(
        resource["authorization_servers"],
        json!([fixture.issuer.base])
    );
    let provider_only = fixture.claims(MCP, "signed-client", "telegram.read");
    let mcp_bearer = token(1, "1", "at+jwt", &provider_only);
    fixture
        .verifier
        .verify(&SecretString::from(mcp_bearer.clone()), &Audience::Mcp)
        .await
        .expect("MCP audience");
    for invalid in [
        mcp_bearer,
        token(2, "1", "at+jwt", &claims),
        token(1, "1", "JWT", &claims),
        {
            claims["aud"] = json!([API, MCP]);
            token(1, "1", "at+jwt", &claims)
        },
        {
            claims["aud"] = json!(API);
            claims["iss"] = json!("https://foreign.example");
            token(1, "1", "at+jwt", &claims)
        },
        {
            claims["iss"] = json!(fixture.issuer.base);
            claims["exp"] = json!(unix_now().expect("time") - 1);
            token(1, "1", "at+jwt", &claims)
        },
    ] {
        expect_error(
            fixture.bearer(&invalid).await,
            StatusCode::UNAUTHORIZED,
            ErrorCode::Unauthenticated,
        )
        .await;
    }
}

#[tokio::test]
async fn service_reloads_rotated_keys_once_and_limits_unknown_id_refreshes() {
    let fixture = Fixture::start().await;
    assert_eq!(fixture.issuer.key_reads.load(Ordering::Relaxed), 1);
    let claims = fixture.claims(API, "signed-client", "telegram.read");
    fixture.issuer.active.store(2, Ordering::Relaxed);
    assert_eq!(
        fixture
            .bearer(&token(2, "2", "at+jwt", &claims))
            .await
            .status(),
        StatusCode::OK
    );
    for kid in ["old", "1", "fake", "unknown"] {
        assert_eq!(
            fixture
                .bearer(&token(2, kid, "at+jwt", &claims))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(fixture.issuer.key_reads.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn service_accepts_only_narrow_delegation_and_requires_browser_for_mutations() {
    let fixture = Fixture::start().await;
    let mut claims = fixture.claims(API, "originator", "telegram.read telegram.send");
    claims["act"] = json!({"sub": GATEWAY, "iss": fixture.issuer.base});
    let bearer = token(1, "1", "at+jwt", &claims);
    let session: SessionInfo = fixture
        .bearer(&bearer)
        .await
        .error_for_status()
        .expect("delegated session")
        .json()
        .await
        .expect("session");
    assert_eq!(session.client_id, "originator");
    assert_eq!(session.kind, SessionKind::Delegated);
    let mutation = fixture
        .http
        .delete(format!("{}/v1/session", fixture.api.base))
        .bearer_auth(&bearer)
        .header("x-owner-id", "same-user")
        .header("x-csrf-token", "forged")
        .send()
        .await
        .expect("mutation");
    expect_error(mutation, StatusCode::FORBIDDEN, ErrorCode::Forbidden).await;
    claims["scope"] = json!("telegram.read telegram.link");
    expect_error(
        fixture.bearer(&token(1, "1", "at+jwt", &claims)).await,
        StatusCode::FORBIDDEN,
        ErrorCode::Forbidden,
    )
    .await;
    claims["act"] = json!({"sub": "foreign-actor"});
    expect_error(
        fixture.bearer(&token(1, "1", "at+jwt", &claims)).await,
        StatusCode::UNAUTHORIZED,
        ErrorCode::Unauthenticated,
    )
    .await;
}

#[tokio::test]
async fn browser_flow_binds_cookie_state_pkce_nonce_and_csrf() {
    let fixture = Fixture::start().await;
    let (pending, authorization) = fixture.start_login().await;
    let callback = fixture.authorization(authorization).await;
    let mut bad_state = callback.clone();
    bad_state.set_query(Some("state=invalid&code=invalid"));
    expect_error(
        fixture.callback(&pending, &bad_state).await,
        StatusCode::UNAUTHORIZED,
        ErrorCode::Unauthenticated,
    )
    .await;
    let signed_in = fixture.callback(&pending, &callback).await;
    assert_eq!(signed_in.status(), StatusCode::SEE_OTHER);
    let session = get_cookie(&signed_in, "__Host-tg_session");
    assert_eq!(
        fixture.callback(&pending, &callback).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let session_url = format!("{}/v1/session", fixture.api.base);
    let info: SessionInfo = fixture
        .http
        .get(&session_url)
        .header(header::COOKIE, &session)
        .send()
        .await
        .expect("browser session")
        .error_for_status()
        .expect("valid cookie")
        .json()
        .await
        .expect("session info");
    assert_eq!(info.kind, SessionKind::BrowserOwner);
    let csrf = info.csrf_token.expect("browser session CSRF");
    let logout = || {
        fixture
            .http
            .delete(&session_url)
            .header(header::COOKIE, &session)
    };
    assert_eq!(
        logout().send().await.expect("without CSRF").status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        logout()
            .header("origin", "https://foreign.example")
            .header("x-csrf-token", &csrf)
            .send()
            .await
            .expect("foreign origin")
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        logout()
            .header("origin", "https://service.example")
            .header("x-csrf-token", csrf)
            .send()
            .await
            .expect("logout")
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        fixture
            .http
            .get(&session_url)
            .header(header::COOKIE, &session)
            .send()
            .await
            .expect("retired cookie")
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let (pending, mut authorization) = fixture.start_login().await;
    let pairs: Vec<_> = authorization
        .query_pairs()
        .into_owned()
        .filter(|(name, _)| name != "code_challenge")
        .collect();
    authorization.set_query(None);
    authorization
        .query_pairs_mut()
        .extend_pairs(pairs)
        .append_pair("code_challenge", "wrong");
    let callback = fixture.authorization(authorization).await;
    assert_eq!(
        fixture.callback(&pending, &callback).await.status(),
        StatusCode::UNAUTHORIZED
    );

    fixture.issuer.bad_nonce.store(true, Ordering::Relaxed);
    let (pending, authorization) = fixture.start_login().await;
    let callback = fixture.authorization(authorization).await;
    assert_eq!(
        fixture.callback(&pending, &callback).await.status(),
        StatusCode::UNAUTHORIZED
    );
}
