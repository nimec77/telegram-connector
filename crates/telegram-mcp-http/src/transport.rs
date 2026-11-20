//! Stateless Streamable HTTP transport and OAuth protected-resource discovery.

use crate::delegation::{Caller, Gateway, now_unix_ms};
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use secrecy::SecretString;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use telegram_api::identity::{Audience, AuthError, CredentialVerifier};
use tokio::sync::Semaphore;
use tokio::time::Instant;
use url::{Host, Url};

#[derive(Clone)]
struct Admission {
    verifier: CredentialVerifier,
    challenge: HeaderValue,
    origin: String,
    permits: Arc<Semaphore>,
    timeout: Duration,
}

/// Construct the stateless gateway for a trusted HTTPS edge and scoped API.
///
/// The API origin may be HTTPS or numeric loopback HTTP on the same machine.
///
/// # Errors
/// Returns a safe configuration error for invalid resource URLs or timeout settings.
pub fn router(
    verifier: CredentialVerifier,
    api: Url,
    secret: SecretString,
    timeout: Duration,
) -> Result<Router, &'static str> {
    let public =
        Url::parse(verifier.audience(Audience::Mcp)).map_err(|_| "Invalid MCP resource")?;
    if public.scheme() != "https"
        || public.path() != "/mcp"
        || !endpoint(&public)
        || !endpoint(&api)
        || api.path() != "/"
        || !(api.scheme() == "https" || api.scheme() == "http" && numeric_loopback(&api))
        || timeout.is_zero()
        || timeout > Duration::from_secs(120)
        || secrecy::ExposeSecret::expose_secret(&secret).is_empty()
    {
        return Err("Invalid gateway URL, registration, or timeout");
    }
    let origin = public.origin().ascii_serialization();
    let meta_path = "/.well-known/oauth-protected-resource/mcp";
    let challenge = format!("Bearer resource_metadata=\"{origin}{meta_path}\"")
        .parse()
        .map_err(|_| "Invalid gateway challenge")?;
    let auth = Admission {
        verifier: verifier.clone(),
        challenge,
        origin,
        permits: Arc::new(Semaphore::new(128)),
        timeout,
    };
    let discovery = json!({"resource":verifier.audience(Audience::Mcp),"authorization_servers":[verifier.issuer()],
        "scopes_supported":["telegram.read","telegram.send"],"bearer_methods_supported":["header"]});
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(3))
        .timeout(timeout)
        .build()
        .map_err(|_| "Could not initialize gateway client")?;
    let gateway = Gateway {
        http,
        api,
        secret,
        verifier,
    };
    let host = &public[url::Position::BeforeHost..url::Position::AfterPort];
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_stateless_protocol_metadata_required(true)
        .with_max_request_body_bytes(64 * 1024)
        .with_allowed_hosts([host])
        .with_allowed_origins([format!(
            "https://{}:{}",
            public.host_str().ok_or("Missing MCP host")?,
            public.port_or_known_default().ok_or("Missing MCP port")?
        )]);
    let mcp = StreamableHttpService::new(
        move || Ok(gateway.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    Ok(Router::new()
        .route_service("/mcp", mcp)
        .route_layer(middleware::from_fn_with_state(auth, authenticate))
        .route(meta_path, get(async move || Json(discovery)))
        .route("/livez", get(async || StatusCode::NO_CONTENT)))
}

fn endpoint(url: &Url) -> bool {
    url.has_host()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn numeric_loopback(url: &Url) -> bool {
    matches!(url.host(), Some(Host::Ipv4(ip)) if ip.is_loopback())
        || matches!(url.host(),Some(Host::Ipv6(ip)) if ip.is_loopback())
}

async fn authenticate(State(auth): State<Admission>, mut request: Request, next: Next) -> Response {
    let start = Instant::now();
    if request.uri().query().is_some() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let headers = request.headers();
    if headers.get_all(header::ORIGIN).iter().count() > 1
        || headers
            .get(header::ORIGIN)
            .is_some_and(|origin| origin.as_bytes() != auth.origin.as_bytes())
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let challenge = || {
        (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, auth.challenge.clone())],
        )
            .into_response()
    };
    if headers.get_all(header::AUTHORIZATION).iter().count() != 1 {
        return challenge();
    }
    let Some((scheme, bearer)) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split_once(' '))
    else {
        return challenge();
    };
    if !scheme.eq_ignore_ascii_case("Bearer")
        || bearer.is_empty()
        || bearer.len() > 16 * 1024
        || bearer.bytes().any(|b| b.is_ascii_whitespace())
    {
        return challenge();
    }
    let token = SecretString::from(bearer.to_owned());
    request.headers_mut().remove(header::AUTHORIZATION);
    let Ok(_permit) = auth.permits.try_acquire() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let Some(unix_ms) = now_unix_ms() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let deadline = start + auth.timeout;
    let dispatch = async {
        let identity = match auth.verifier.verify(&token, &Audience::Mcp).await {
            Ok(identity) => identity,
            Err(AuthError::Forbidden) => return StatusCode::FORBIDDEN.into_response(),
            Err(AuthError::Unavailable | AuthError::Configuration) => {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
            Err(AuthError::Credential) => return challenge(),
        };
        let expiry = identity.expires_at().saturating_mul(1000);
        let deadline = deadline
            .min(start + Duration::from_millis(expiry.saturating_sub(unix_ms)).min(auth.timeout));
        let unix_ms = expiry.min(unix_ms.saturating_add(auth.timeout.as_millis() as u64));
        request.extensions_mut().insert(Caller {
            token,
            identity,
            deadline: deadline
                .checked_sub(Duration::from_millis(15))
                .unwrap_or(deadline),
            unix_ms,
        });
        tokio::time::timeout_at(deadline, next.run(request))
            .await
            .unwrap_or_else(|_| StatusCode::GATEWAY_TIMEOUT.into_response())
    };
    let mut response = tokio::time::timeout_at(deadline, dispatch)
        .await
        .unwrap_or_else(|_| StatusCode::GATEWAY_TIMEOUT.into_response());
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}
