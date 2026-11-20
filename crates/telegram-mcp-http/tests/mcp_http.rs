//! Independent HTTP clients exercise the hosted process binding.
use std::process::Command;

#[test]
fn operator_can_discover_how_to_configure_the_gateway() {
    let output = Command::new(env!("CARGO_BIN_EXE_telegram-mcp-http"))
        .arg("--help")
        .output()
        .expect("run gateway");
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("TELEGRAM_MCP_EXCHANGE_SECRET")
    );
}

use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use reqwest::{Client, RequestBuilder};
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::process::Child;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use telegram_api::identity::{
    Audience, AuthorityConfig, CredentialKind, CredentialVerifier, SigningAlgorithm,
};
use tokio::net::TcpListener;

const MCP: &str = "https://gateway.example/mcp";
const API: &str = "https://api.example/v1";
const ACTOR: &str = "gateway";
const REDIRECT: &str = "https://client.example/callback";
const VERIFIER: &str = "wOrJbzsLClO73SbWuz7ldv4r3pvc-aavvtNt4mtAlZM";

fn authority(issuer: String) -> AuthorityConfig {
    AuthorityConfig {
        issuer,
        api_audience: API.into(),
        mcp_audience: MCP.into(),
        gateway_actor: ACTOR.into(),
        subject_claim: Some("account".into()),
        algorithms: vec![SigningAlgorithm::EdDSA],
    }
}

#[tokio::test]
#[ignore = "re-executed by the parent with an ephemeral test issuer"]
async fn gateway_child() {
    let issuer = std::env::var("FIXTURE_ISSUER").unwrap();
    let verifier = CredentialVerifier::discover_loopback(authority(issuer))
        .await
        .unwrap();
    let app = telegram_mcp_http::router(
        verifier,
        std::env::var("FIXTURE_API").unwrap().parse().unwrap(),
        SecretString::from("test-secret"),
        Duration::from_secs(5),
    )
    .expect("gateway initialization");
    let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    std::fs::write(
        std::env::var("FIXTURE_READY").unwrap(),
        socket.local_addr().unwrap().to_string(),
    )
    .unwrap();
    axum::serve(socket, app).await.unwrap();
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Issuer {
    url: String,
    codes: Mutex<HashMap<String, String>>,
    bad_exchange: AtomicUsize,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn signed(claims: Value) -> String {
    // RFC 8032 test vector 1 in PKCS#8.
    let der = (0..48).map(|i| {
        let hex = "302e020100300506032b6570042204209d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
        u8::from_str_radix(&hex[2*i..2*i+2], 16).unwrap()
    }).collect::<Vec<_>>();
    let mut header = Header::new(Algorithm::EdDSA);
    header.kid = Some("fixture".into());
    header.typ = Some("at+jwt".into());
    encode(&header, &claims, &EncodingKey::from_ed_der(&der)).unwrap()
}

fn fixture_scope(client: &str) -> &'static str {
    if client == "mixed" {
        "telegram.read telegram.send"
    } else if client == "writer" {
        "telegram.send"
    } else {
        "telegram.read"
    }
}

fn claims(issuer: &Issuer, client: &str, audience: &str) -> Value {
    json!({"iss":issuer.url,"sub":audience,"account":"owner","client_id":client,"aud":audience,
        "jti":"test","exp":now()+100,"iat":now(),"scope":fixture_scope(client)})
}

async fn metadata(State(issuer): State<Arc<Issuer>>) -> Json<Value> {
    Json(
        json!({"issuer":issuer.url,"authorization_endpoint":format!("{}/authorize",issuer.url),
        "token_endpoint":format!("{}/token",issuer.url),"jwks_uri":format!("{}/jwks",issuer.url)}),
    )
}

async fn authorize(
    State(issuer): State<Arc<Issuer>>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    if query["resource"] != MCP
        || query["response_type"] != "code"
        || query["redirect_uri"] != REDIRECT
        || query["code_challenge_method"] != "S256"
        || query["scope"] != fixture_scope(&query["client_id"])
        || !["reader", "restricted", "writer", "complete", "mixed"]
            .contains(&query["client_id"].as_str())
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let code = format!("code-{}", query["client_id"]);
    issuer
        .codes
        .lock()
        .unwrap()
        .insert(code.clone(), query["code_challenge"].clone());
    Redirect::to(&format!("{REDIRECT}?code={code}&state={}", query["state"])).into_response()
}

async fn token(
    State(issuer): State<Arc<Issuer>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if form["grant_type"] == "authorization_code" {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(form["code_verifier"].as_bytes()));
        if form["redirect_uri"] != REDIRECT
            || form["resource"] != MCP
            || form["code"] != format!("code-{}", form["client_id"])
            || issuer
                .codes
                .lock()
                .unwrap()
                .remove(&form["code"])
                .as_deref()
                != Some(&challenge)
        {
            return StatusCode::BAD_REQUEST.into_response();
        }
        return Json(json!({"access_token":signed(claims(&issuer,&form["client_id"],MCP)),"token_type":"Bearer"})).into_response();
    }
    if headers[header::AUTHORIZATION] != "Basic Z2F0ZXdheTp0ZXN0LXNlY3JldA=="
        || form["resource"] != API
        || ![
            "telegram.read",
            "telegram.send",
            "telegram.read telegram.send",
        ]
        .contains(&form["scope"].as_str())
        || form["grant_type"] != "urn:ietf:params:oauth:grant-type:token-exchange"
        || form["subject_token_type"] != "urn:ietf:params:oauth:token-type:access_token"
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let payload: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(form["subject_token"].split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap();
    let mut delegated = claims(&issuer, payload["client_id"].as_str().unwrap(), API);
    assert!(form["scope"].split_whitespace().all(|scope| {
        payload["scope"]
            .as_str()
            .unwrap()
            .split_whitespace()
            .any(|allowed| allowed == scope)
    }));
    delegated["scope"] = json!(if payload["client_id"] == "mixed" {
        "telegram.read"
    } else {
        &form["scope"]
    });
    delegated["exp"] = json!(now() + 45);
    delegated["act"] = json!({"sub":ACTOR});
    match issuer.bad_exchange.load(Ordering::SeqCst) {
        1 => delegated["account"] = json!("different-subject"),
        2 => delegated["client_id"] = json!("different-client"),
        3 => delegated["scope"] = json!("telegram.read telegram.send"),
        4 => delegated["scope"] = json!("telegram.read unknown.privilege"),
        _ => {}
    }
    Json(json!({"access_token":signed(delegated),"issued_token_type":"urn:ietf:params:oauth:token-type:access_token","token_type":"Bearer"})).into_response()
}

struct Api {
    verifier: CredentialVerifier,
    reads: AtomicUsize,
    requests: Mutex<Vec<(Uri, Value)>>,
    capabilities: AtomicUsize,
    restricted: AtomicUsize,
    slow: tokio::sync::Notify,
    dropped: tokio::sync::Notify,
}

async fn api(
    State(api): State<Arc<Api>>,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let token = SecretString::from(
        headers[header::AUTHORIZATION]
            .to_str()
            .unwrap()
            .strip_prefix("Bearer ")
            .unwrap()
            .to_owned(),
    );
    let identity = api
        .verifier
        .verify(&token, &Audience::Api)
        .await
        .expect("only API-audience bearer reaches API");
    assert_eq!(identity.kind(), CredentialKind::Delegated);
    let deadline = headers["x-telegram-deadline-unix-ms"]
        .to_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert!(deadline > now() * 1000 && deadline <= (now() + 6) * 1000);
    let tools = [
        "search_messages",
        "get_recent_messages",
        "get_messages_batch",
    ];
    if uri.path() == "/v1/capabilities" {
        api.capabilities.fetch_add(1, Ordering::SeqCst);
        if identity.client_id() == "mixed" {
            return Json(json!({"tools":["search_messages","send_message"]})).into_response();
        }
        if identity.client_id() == "complete" {
            return Json(json!({"tools":["get_subscribed_channels","get_channel_info","resolve_channels","get_channel_stats","generate_message_link","transcribe_voice_message","get_message_media","get_messages_media_batch","search_public_channels"]})).into_response();
        }
        if identity.client_id() == "writer" {
            let tools = if api.restricted.load(Ordering::SeqCst) == 0 {
                vec!["send_message", "send_media"]
            } else {
                vec![]
            };
            return Json(json!({"tools":tools})).into_response();
        }
        return Json(json!({"tools":if identity.client_id()== "reader" && api.restricted.load(Ordering::SeqCst)==0 {&tools[..]} else {&tools[1..2]}})).into_response();
    }
    api.reads.fetch_add(1, Ordering::SeqCst);
    let args: Value = serde_json::from_slice(&body).unwrap_or_default();
    if args["query"] == "hold" {
        struct Release(Arc<Api>);
        impl Drop for Release {
            fn drop(&mut self) {
                self.0.dropped.notify_one();
            }
        }
        let _lease = Release(api.clone());
        api.slow.notify_one();
        std::future::pending::<()>().await;
    }
    api.requests
        .lock()
        .unwrap()
        .push((uri.clone(), args.clone()));
    match uri.path() {
        "/v1/messages/send" if args["text"]=="refused" => (StatusCode::FORBIDDEN,Json(json!({"error":{"code":"send_forbidden","delivery":"not_sent","retryable":false}}))).into_response(),
        "/v1/messages/send" if args["text"]=="lost" => StatusCode::BAD_GATEWAY.into_response(),
        "/v1/messages/send" | "/v1/messages/send-media" => Json(json!({"delivery_state":"sent","message_id":5,"recipient":args["recipient"]})).into_response(),
        "/v1/media/previews"=>Json(json!({"items":[{"message_id":3,"handle":"one_preview-9"}]})).into_response(),
        "/v1/media/one_preview-9"=> {
            let mut bytes=vec![0;1_179_648];
            bytes[..3].copy_from_slice(&[0xff,0xd8,0xff]);
            ([(header::CONTENT_TYPE,"image/jpeg")],bytes).into_response()
        },
        "/v1/messages/search" if args["query"]=="malformed" => Json(json!({"upstream_diagnostic":"must-not-escape"})).into_response(),
        "/v1/messages/search" if args["query"] == "large" => Json(json!({"messages":["x".repeat(512*1024)]})).into_response(),
        "/v1/messages/search" if args["query"] == "throttle" => (StatusCode::TOO_MANY_REQUESTS,Json(json!({"error":{"code":"rate_limited","retry_after_seconds":4}}))).into_response(),
        "/v1/messages/search" => Json(json!({"messages":[],"partial":false,"timed_out":false,"has_more":false,"failed_channels":[],"next_before_id":null})).into_response(),
        "/v1/channels/42/messages" => Json(json!({"messages":[],"partial":false,"timed_out":false,"next_before_id":null})).into_response(),
        "/v1/messages/batch" => Json(json!({"channel_id":"42","messages":[],"missing_ids":[1,2],"omitted_ids":[]})).into_response(),
        "/v1/channels" | "/v1/channels/42" | "/v1/channels/resolve" | "/v1/channels/42/stats"
           | "/v1/channels/42/messages/3/link" | "/v1/channels/42/messages/3/transcription" | "/v1/channels/discover" => Json(json!({"fixture":true})).into_response(),
        _=>StatusCode::NOT_FOUND.into_response(),
    }
}

fn http() -> Client {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap()
}
struct Fixture {
    issuer: Arc<Issuer>,
    api: Arc<Api>,
    gateway: String,
    http: Client,
    _process: Process,
}

impl Fixture {
    async fn start() -> Self {
        let issuer_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = Arc::new(Issuer {
            url: format!("http://{}", issuer_listener.local_addr().unwrap()),
            codes: Mutex::default(),
            bad_exchange: AtomicUsize::default(),
        });
        let app=Router::new().route("/.well-known/openid-configuration",get(metadata)).route("/authorize",get(authorize))
            .route("/token",post(token)).route("/jwks",get(||async {Json(json!({"keys":[{"kty":"OKP","crv":"Ed25519","use":"sig","alg":"EdDSA","kid":"fixture","x":"11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"}]}))})).with_state(issuer.clone());
        tokio::spawn(async move { axum::serve(issuer_listener, app).await.unwrap() });
        let verifier = CredentialVerifier::discover_loopback(authority(issuer.url.clone()))
            .await
            .unwrap();
        let api = Arc::new(Api {
            verifier,
            reads: 0.into(),
            requests: Mutex::default(),
            capabilities: 0.into(),
            restricted: 0.into(),
            slow: Default::default(),
            dropped: Default::default(),
        });
        let api_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", api_listener.local_addr().unwrap());
        let app = Router::new().fallback(self::api).with_state(api.clone());
        tokio::spawn(async move { axum::serve(api_listener, app).await.unwrap() });
        let dir = tempfile::tempdir().unwrap();
        let ready = dir.path().join("ready");
        let process = Process(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "gateway_child", "--ignored"])
                .env("FIXTURE_API", endpoint)
                .env("FIXTURE_ISSUER", &issuer.url)
                .env("FIXTURE_READY", &ready)
                .spawn()
                .unwrap(),
        );
        let addr = tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                if let Ok(s) = std::fs::read_to_string(&ready)
                    && let Ok(addr) = s.parse::<SocketAddr>()
                {
                    break addr;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        Self {
            issuer,
            api,
            gateway: format!("http://{addr}"),
            http: http(),
            _process: process,
        }
    }

    async fn login(&self, client: &str) -> String {
        let discovery: Value = self
            .http
            .get(format!(
                "{}/.well-known/oauth-protected-resource/mcp",
                self.gateway
            ))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(discovery["resource"], MCP);
        let issuer = discovery["authorization_servers"][0].as_str().unwrap();
        let metadata: Value = self
            .http
            .get(format!("{issuer}/.well-known/openid-configuration"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(VERIFIER.as_bytes()));
        let response = self
            .http
            .get(metadata["authorization_endpoint"].as_str().unwrap())
            .query(&[
                ("client_id", client),
                ("code_challenge", &challenge),
                ("code_challenge_method", "S256"),
                ("state", "nonce"),
                ("response_type", "code"),
                ("redirect_uri", REDIRECT),
                ("scope", fixture_scope(client)),
                ("resource", MCP),
            ])
            .send()
            .await
            .unwrap();
        assert!(response.status().is_redirection());
        let location =
            url::Url::parse(response.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        let query = location.query_pairs().collect::<HashMap<_, _>>();
        assert_eq!(query["state"], "nonce");
        let token: Value = self
            .http
            .post(metadata["token_endpoint"].as_str().unwrap())
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", client),
                ("code_verifier", VERIFIER),
                ("code", &query["code"]),
                ("redirect_uri", REDIRECT),
                ("resource", MCP),
            ])
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        token["access_token"].as_str().unwrap().into()
    }
    fn rpc(&self, bearer: &str, method: &str, mut params: Value) -> RequestBuilder {
        params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientInfo":{"name":"independent","version":"1"},"io.modelcontextprotocol/clientCapabilities":{}});
        let mut req = self
            .http
            .post(format!("{}/mcp", self.gateway))
            .header("Host", "gateway.example")
            .bearer_auth(bearer)
            .header("accept", "application/json, text/event-stream")
            .header("Mcp-Protocol-Version", "2026-07-28")
            .header("Mcp-Method", method);
        if let Some(name) = params["name"].as_str() {
            req = req.header("Mcp-Name", name);
        }
        req.json(&json!({"id":7,"jsonrpc":"2.0","method":method,"params":params}))
    }
    async fn call(&self, bearer: &str, name: &str, args: Value) -> Value {
        self.rpc(bearer, "tools/call", json!({"name":name,"arguments":args}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn clients_complete_discovery_authorization_exchange_and_scoped_reads() {
    let f = Fixture::start().await;
    let reader = f.login("reader").await;
    let restricted = f.login("restricted").await;
    for (token, names) in [
        (
            &reader,
            vec![
                "search_messages",
                "get_recent_messages",
                "get_messages_batch",
            ],
        ),
        (&restricted, vec!["get_recent_messages"]),
    ] {
        let list: Value = f
            .rpc(token, "tools/list", json!({}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let tools = list["result"]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("missing tools {list}"));
        assert_eq!(
            tools
                .iter()
                .map(|t| t["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            names
        );
        for t in tools {
            assert_eq!(t["inputSchema"]["type"], "object");
            assert!(t["outputSchema"].is_object());
            assert!(!t["inputSchema"].to_string().contains("$ref"));
        }
    }
    let search = json!({"channel_ids":["42"],"query":"bird","limit":5});
    let batch = json!({"channel_id":"42","message_ids":[1,2]});
    for (name, args, path) in [
        ("search_messages", search.clone(), "/v1/messages/search"),
        (
            "get_recent_messages",
            json!({"channel_id":"42","limit":5}),
            "/v1/channels/42/messages",
        ),
        ("get_messages_batch", batch, "/v1/messages/batch"),
    ] {
        let response = f.call(&reader, name, args.clone()).await;
        let r = &response["result"];
        assert_eq!(r["isError"], false, "{response}");
        assert_eq!(
            serde_json::from_str::<Value>(r["content"][0]["text"].as_str().unwrap()).unwrap(),
            r["structuredContent"]
        );
        assert_eq!(r["structuredContent"]["data"]["messages"], json!([]));
        let (uri, received) = f.api.requests.lock().unwrap().pop().expect("one API read");
        assert_eq!(uri.path(), path);
        if name == "get_recent_messages" {
            assert!(uri.query().unwrap().contains("limit=5"));
        } else {
            assert_eq!(received, args);
        }
    }
    assert_eq!(f.api.reads.load(Ordering::SeqCst), 3);
    f.api.restricted.store(1, Ordering::SeqCst);
    assert_eq!(
        f.call(&reader, "search_messages", search.clone()).await["result"]["isError"],
        true
    );
    assert_eq!(
        f.call(&restricted, "search_messages", search.clone()).await["result"]["isError"],
        true
    );
    let permitted = f.api.capabilities.load(Ordering::SeqCst);
    for mode in 1..=4 {
        f.issuer.bad_exchange.store(mode, Ordering::SeqCst);
        assert_eq!(
            f.call(&reader, "get_recent_messages", json!({"channel_id":"42"}))
                .await["result"]["isError"],
            true
        );
    }
    assert_eq!(f.api.capabilities.load(Ordering::SeqCst), permitted);
    assert_eq!(f.api.reads.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn mcp_challenges_and_rejects_mismatched_binding_before_api_access() {
    let f = Fixture::start().await;
    let url = format!("{}/mcp", f.gateway);
    assert!(
        f.http
            .get(format!("{}/livez", f.gateway))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    let response = f
        .http
        .post(&url)
        .header("Host", "gateway.example")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        response.headers()[header::WWW_AUTHENTICATE]
            .to_str()
            .unwrap()
            .contains("https://gateway.example/.well-known/oauth-protected-resource/mcp")
    );
    let bearer = f.login("reader").await;
    assert_eq!(
        f.rpc(&bearer, "tools/list", json!({}))
            .header("origin", "https://untrusted.example")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    for (method, name, version) in [
        ("tools/call", "wrong", "2026-07-28"),
        ("tools/list", "get_recent_messages", "2026-07-28"),
        ("tools/call", "get_recent_messages", "2025-11-25"),
    ] {
        let mut req = f
            .rpc(
                &bearer,
                "tools/call",
                json!({"name":"get_recent_messages","arguments":{"channel_id":"42"}}),
            )
            .build()
            .unwrap();
        req.headers_mut()
            .insert("Mcp-Method", method.parse().unwrap());
        req.headers_mut().insert("Mcp-Name", name.parse().unwrap());
        req.headers_mut()
            .insert("Mcp-Protocol-Version", version.parse().unwrap());
        let response = f.http.execute(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let old:Value=f.http.post(&url).header("Host","gateway.example").bearer_auth(bearer)
        .header("accept","application/json, text/event-stream").header("mcp-method","initialize").json(&json!({"jsonrpc":"2.0","id":5,"method":"initialize",
        "params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}})).send().await.unwrap().json().await.unwrap();
    assert_eq!(old["error"]["data"]["supported"], json!(["2026-07-28"]));
    assert_eq!(f.api.capabilities.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn client_disconnect_releases_pending_api_request() {
    let f = Fixture::start().await;
    let reader = f.login("reader").await;
    let call = f.rpc(
        &reader,
        "tools/call",
        json!({"name":"search_messages","arguments":{"channel_ids":["42"],"query":"hold"}}),
    );
    let task = tokio::spawn(call.send());
    tokio::time::timeout(Duration::from_secs(3), f.api.slow.notified())
        .await
        .expect("API operation starts");
    task.abort();
    tokio::time::timeout(Duration::from_secs(2), f.api.dropped.notified())
        .await
        .expect("disconnect drops pending API future");
}

#[tokio::test]
async fn gateway_enforces_budgets_and_preserves_safe_rate_limit_errors() {
    let f = Fixture::start().await;
    let reader = f.login("reader").await;
    let request = |query: &str| json!({"channel_ids":["42"],"query":query,"limit":5});
    for (query, code) in [
        ("large", "response_too_large"),
        ("malformed", "unavailable"),
        ("throttle", "rate_limited"),
    ] {
        let response = f.call(&reader, "search_messages", request(query)).await;
        let result = &response["result"];
        assert_eq!(result["isError"], true);
        let error = &result["structuredContent"]["error"];
        assert_eq!(error["code"], code);
        if query == "throttle" {
            assert_eq!(error["retry_after_seconds"], 4);
            assert_eq!(error["retryable"], true);
        }
    }
    let calls = f.api.reads.load(Ordering::SeqCst);
    let status = f
        .rpc(
            &reader,
            "tools/call",
            json!({"name":"search_messages","arguments":request(&"x".repeat(70*1024))}),
        )
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(f.api.reads.load(Ordering::SeqCst), calls);
}

#[tokio::test]
async fn hosted_reads_and_caller_scoped_resources_use_api_permissions() {
    let f = Fixture::start().await;
    let token = f.login("complete").await;
    let cases = [
        (
            "get_subscribed_channels",
            json!({"limit":5,"offset":0}),
            "/v1/channels",
        ),
        (
            "get_channel_info",
            json!({"channel_id":"42","include_full":true}),
            "/v1/channels/42",
        ),
        (
            "resolve_channels",
            json!({"identifiers":["@example"]}),
            "/v1/channels/resolve",
        ),
        (
            "get_channel_stats",
            json!({"channel_id":"42","days_back":2}),
            "/v1/channels/42/stats",
        ),
        (
            "generate_message_link",
            json!({"channel_id":"42","message_id":3}),
            "/v1/channels/42/messages/3/link",
        ),
        (
            "transcribe_voice_message",
            json!({"channel_id":"42","message_id":3}),
            "/v1/channels/42/messages/3/transcription",
        ),
        (
            "search_public_channels",
            json!({"query":"test"}),
            "/v1/channels/discover",
        ),
        (
            "get_message_media",
            json!({"channel_id":"42","message_id":3}),
            "/v1/media/previews",
        ),
        (
            "get_messages_media_batch",
            json!({"channel_id":"42","message_ids":[3]}),
            "/v1/media/previews",
        ),
    ];
    for (name, args, path) in cases {
        let r = f.call(&token, name, args).await;
        assert_eq!(r["result"]["isError"], false, "{name}: {r}");
        let (uri, _) = f.api.requests.lock().unwrap().pop().expect("API call");
        assert_eq!(uri.path(), path);
        if path == "/v1/media/previews" {
            assert_eq!(
                r["result"]["content"][1]["uri"],
                "telegram-media://preview/one_preview-9"
            );
        }
    }
    let uri = "telegram-media://preview/one_preview-9";
    let read = |token: &str| {
        f.rpc(token, "resources/read", json!({"uri":uri}))
            .header("Mcp-Name", uri)
            .send()
    };
    let body: Value = read(&token).await.unwrap().json().await.unwrap();
    assert_eq!(
        body["result"]["contents"][0]["mimeType"], "image/jpeg",
        "{body}"
    );
    let restricted = f.login("restricted").await;
    let body: Value = read(&restricted).await.unwrap().json().await.unwrap();
    assert!(body.get("error").is_some());
}

#[tokio::test]
async fn send_only_grant_is_independent_and_uncertain_delivery_is_never_retried() {
    let f = Fixture::start().await;
    let writer = f.login("writer").await;
    let list: Value = f
        .rpc(&writer, "tools/list", json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 2);
    let a = json!({"kind":"saved_messages"});
    let b = json!({"kind":"peer","namespace":"channel","id":"987"});
    for recipient in [&a, &b] {
        let response = f
            .call(
                &writer,
                "send_message",
                json!({"recipient":recipient,"text":"post"}),
            )
            .await;
        assert_eq!(
            response["result"]["structuredContent"]["data"]["recipient"],
            *recipient
        );
    }
    let media = f
        .call(
            &writer,
            "send_media",
            json!({"recipient":b,"upload_handle":"upload_X","media":{"kind":"image"}}),
        )
        .await;
    assert_eq!(
        media["result"]["structuredContent"]["data"]["message_id"],
        5
    );
    for (text, delivery, code) in [
        ("refused", "not_sent", "send_forbidden"),
        ("lost", "unknown", "delivery_unknown"),
    ] {
        let r = f
            .call(&writer, "send_message", json!({"recipient":b,"text":text}))
            .await;
        let err = &r["result"]["structuredContent"]["error"];
        assert_eq!(err["delivery_state"], delivery);
        assert_eq!(err["retryable"], false);
        assert_eq!(err["code"], code);
    }
    assert_eq!(f.api.reads.load(Ordering::SeqCst), 5);
    f.api.restricted.store(1, Ordering::SeqCst);
    let r = f
        .call(
            &writer,
            "send_message",
            json!({"recipient":a,"text":"after revocation"}),
        )
        .await;
    assert_eq!(
        r["result"]["structuredContent"]["error"]["delivery_state"],
        "not_sent"
    );
    assert_eq!(f.api.reads.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn issuer_can_narrow_dual_scope_catalog() {
    let f = Fixture::start().await;
    let bearer = f.login("mixed").await;
    let r: Value = f
        .rpc(&bearer, "tools/list", json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let tools = r["result"]["tools"].as_array().expect("narrowed catalog");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "search_messages");
}
