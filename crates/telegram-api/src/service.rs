//! Hosted HTTP edge for configured identity and browser sessions.

mod browser_auth;
mod http_error;

use crate::contracts::{ErrorCode, SessionInfo, SessionKind};
use crate::identity::{Audience, AuthError, CredentialKind, CredentialVerifier};
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use browser_auth::BrowserAuth;
use http_error::ApiError;
use secrecy::SecretString;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Semaphore;

pub use browser_auth::BrowserConfig;

const MAX_IN_FLIGHT: usize = 256;
const MAX_BODY_BYTES: usize = 16 * 1024;

struct ServiceState {
    verifier: CredentialVerifier,
    browser: BrowserAuth,
    admission: Semaphore,
    bearer_challenge: HeaderValue,
}

/// Assemble the auth routes for a loopback service behind a trusted HTTPS proxy.
///
/// # Errors
/// Returns a configuration error if browser registration settings are invalid.
pub fn router(verifier: CredentialVerifier, browser: BrowserConfig) -> Result<Router, AuthError> {
    let mut metadata_url =
        url::Url::parse(verifier.audience(Audience::Api)).map_err(|_| AuthError::Configuration)?;
    let suffix = if metadata_url.path() == "/" {
        ""
    } else {
        metadata_url.path()
    };
    let metadata_path = format!("/.well-known/oauth-protected-resource{suffix}");
    metadata_url.set_path(&metadata_path);
    let bearer_challenge =
        HeaderValue::from_str(&format!("Bearer resource_metadata=\"{metadata_url}\""))
            .map_err(|_| AuthError::Configuration)?;
    let state = Arc::new(ServiceState {
        browser: BrowserAuth::new(browser, &verifier)?,
        verifier,
        admission: Semaphore::new(MAX_IN_FLIGHT),
        bearer_challenge,
    });
    Ok(Router::new()
        .route("/readyz", get(ready))
        .route(&metadata_path, get(resource))
        .route("/auth/start", get(browser_auth::start))
        .route("/auth/callback", get(browser_auth::callback))
        .route("/v1/session", get(session).delete(browser_auth::logout))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(Arc::clone(&state), admit))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            private_response,
        ))
        .with_state(state))
}

async fn admit(State(state): State<Arc<ServiceState>>, request: Request, next: Next) -> Response {
    let Ok(_permit) = state.admission.try_acquire() else {
        return ApiError(ErrorCode::RateLimited).into_response();
    };
    next.run(request).await
}

async fn private_response(
    State(state): State<Arc<ServiceState>>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if response.status() == StatusCode::UNAUTHORIZED {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, state.bearer_challenge.clone());
    }
    response
}

async fn ready() -> StatusCode {
    StatusCode::NO_CONTENT
}

#[derive(Serialize)]
struct ProtectedResource<'a> {
    resource: &'a str,
    authorization_servers: [&'a str; 1],
    scopes_supported: [&'a str; 3],
}

async fn resource(
    State(state): State<Arc<ServiceState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let metadata = ProtectedResource {
        resource: state.verifier.audience(Audience::Api),
        authorization_servers: [state.verifier.issuer()],
        scopes_supported: ["telegram.read", "telegram.send", "telegram.link"],
    };
    serde_json::to_value(metadata)
        .map(Json)
        .map_err(|_| ApiError(ErrorCode::Unavailable))
}

async fn session(
    State(state): State<Arc<ServiceState>>,
    headers: HeaderMap,
) -> Result<Json<SessionInfo>, ApiError> {
    if !headers.contains_key(header::AUTHORIZATION) {
        return state
            .browser
            .owner(&headers)
            .map(|(_, session)| Json(session.info));
    }
    let header = single_header(&headers, header::AUTHORIZATION)
        .ok_or(ApiError(ErrorCode::Unauthenticated))?;
    let (kind, token) = header
        .split_once(' ')
        .ok_or(ApiError(ErrorCode::Unauthenticated))?;
    if !kind.eq_ignore_ascii_case("Bearer")
        || token.is_empty()
        || token.len() > MAX_BODY_BYTES
        || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(ApiError(ErrorCode::Unauthenticated));
    }
    let identity = state
        .verifier
        .verify(&SecretString::from(token.to_owned()), &Audience::Api)
        .await?;
    Ok(Json(SessionInfo {
        client_id: identity.client_id().to_owned(),
        scopes: identity.scopes().iter().copied().collect(),
        expires_at_unix: identity.expires_at(),
        kind: match identity.kind() {
            CredentialKind::Direct => SessionKind::Direct,
            CredentialKind::Delegated => SessionKind::Delegated,
        },
        csrf_token: None,
    }))
}

fn single_header(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    let all = headers.get_all(name);
    let mut values = all.iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}
