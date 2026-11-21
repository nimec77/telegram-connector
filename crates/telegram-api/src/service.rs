//! Hosted HTTP edge for configured identity and browser sessions.

mod accounts;
mod authorization;
mod browser_auth;
mod channels;
mod event_writer;
mod events;
mod group_capture;
mod group_changes;
mod group_journal;
mod http_error;
mod linking;
mod media;
mod reads;
mod registry;
mod sends;
mod shutdown;
mod storage;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub mod test_support;
mod uploads;

pub use accounts::HostedConfig;
pub use events::EventConfig;
pub use group_changes::GroupChangesConfig;
pub use storage::{
    AccountGeneration, AccountStore, LinkState, RetiredGeneration, RetirementOutcome,
};

use crate::contracts::{ErrorCode, SessionInfo, SessionKind};
use crate::identity::{Audience, AuthError, CredentialKind, CredentialVerifier};
use axum::body::{Body, to_bytes};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
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

tokio::task_local! {
    static REQUEST_DEADLINE: tokio::time::Instant;
}

fn request_deadline() -> tokio::time::Instant {
    REQUEST_DEADLINE
        .try_with(|deadline| *deadline)
        .unwrap_or_else(|_| tokio::time::Instant::now() + std::time::Duration::from_secs(30))
}

struct ServiceState {
    verifier: CredentialVerifier,
    browser: BrowserAuth,
    admission: Semaphore,
    bearer_challenge: HeaderValue,
    hosted: Option<Arc<accounts::Hosted>>,
}

impl ServiceState {
    fn hosted(&self) -> Result<&Arc<accounts::Hosted>, ApiError> {
        self.hosted
            .as_ref()
            .ok_or(ApiError::new(ErrorCode::Unavailable))
    }
}

/// Assemble the sign-in and session routes without an account store.
///
/// # Errors
/// Returns a configuration error if browser registration settings are invalid.
pub fn router(verifier: CredentialVerifier, browser: BrowserConfig) -> Result<Router, AuthError> {
    build_router(verifier, browser, None)
}

/// Initialize the data root and assemble the complete hosted router.
///
/// # Errors
/// Returns an error for invalid settings or unavailable private account storage.
pub async fn hosted_router(
    verifier: CredentialVerifier,
    browser: BrowserConfig,
    config: HostedConfig,
) -> Result<(Router, telegram_core::operations::CancellationToken), ErrorCode> {
    let (router, hosted) = open_hosted_router(verifier, browser, config).await?;
    Ok((router, hosted.cancellation.clone()))
}

async fn open_hosted_router(
    verifier: CredentialVerifier,
    browser: BrowserConfig,
    config: HostedConfig,
) -> Result<(Router, Arc<accounts::Hosted>), ErrorCode> {
    let hosted = accounts::Hosted::open(config)
        .await
        .map_err(|error| error.0)?;
    let router = build_router(verifier, browser, Some(Arc::clone(&hosted)))
        .map_err(|_| ErrorCode::Unavailable)?;
    Ok((router, hosted))
}

#[cfg(test)]
pub(crate) async fn account_router_fixture(
    verifier: CredentialVerifier,
    browser: BrowserConfig,
    config: HostedConfig,
) -> Result<(Router, Arc<AccountStore>), ErrorCode> {
    let (router, hosted) = open_hosted_router(verifier, browser, config).await?;
    Ok((router, Arc::clone(&hosted.store)))
}

/// Run the hosted API and stop the listener and its account tasks under one deadline.
///
/// # Errors
/// Returns a fixed diagnostic if initialization or the hosted listener fails.
pub async fn serve_hosted(
    listener: tokio::net::TcpListener,
    verifier: CredentialVerifier,
    browser: BrowserConfig,
    config: HostedConfig,
    shutdown_signal: impl Future<Output = ()> + Send + 'static,
) -> Result<(), &'static str> {
    let write_timeout = std::time::Duration::from_secs(config.events.stalled_write_seconds);
    let (app, hosted) = open_hosted_router(verifier, browser, config)
        .await
        .map_err(|_| "Could not initialize the hosted account service")?;
    shutdown::serve(listener, app, write_timeout, Some(hosted), shutdown_signal)
        .await
        .map_err(|_| "Hosted API listener failed")
}

fn build_router(
    verifier: CredentialVerifier,
    browser: BrowserConfig,
    hosted: Option<Arc<accounts::Hosted>>,
) -> Result<Router, AuthError> {
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
        admission: Semaphore::new(
            hosted
                .as_ref()
                .map_or(MAX_IN_FLIGHT, |hosted| hosted.config.max_in_flight),
        ),
        bearer_challenge,
        hosted,
    });
    let mut router = Router::new()
        .route("/readyz", get(ready))
        .route(&metadata_path, get(resource))
        .route("/auth/start", get(browser_auth::start))
        .route("/auth/callback", get(browser_auth::callback))
        .route("/v1/session", get(session).delete(browser_auth::logout));
    if state.hosted.is_some() {
        router = router
            .route("/", get(accounts::page))
            .route("/account", get(accounts::page))
            .route("/v1/account", get(accounts::status))
            .route("/v1/account/channels", get(accounts::joined_channels))
            .route("/v1/capabilities", get(authorization::capabilities))
            .route("/v1/events", get(events::subscribe))
            .route("/v1/groups/{group_ref}/changes", get(group_changes::read))
            .route("/v1/messages/search", post(reads::search))
            .route("/v1/channels/{id}/messages", get(reads::history))
            .route("/v1/messages/batch", post(reads::batch))
            .route("/v1/channels", get(channels::list))
            .route("/v1/channels/{id}", get(channels::info))
            .route("/v1/channels/resolve", post(channels::resolve))
            .route("/v1/channels/discover", post(channels::discover))
            .route("/v1/channels/{id}/stats", get(channels::stats))
            .route(
                "/v1/channels/{id}/messages/{message_id}/link",
                get(channels::link),
            )
            .route(
                "/v1/channels/{id}/messages/{message_id}/transcription",
                post(channels::transcribe),
            )
            .route("/v1/messages/send", post(sends::text))
            .route("/v1/messages/send-media", post(sends::media))
            .route("/v1/uploads", post(uploads::upload))
            .route("/v1/media/previews", post(media::previews))
            .route("/v1/media/{handle}", get(media::download))
            .route(
                "/v1/account/link",
                post(accounts::start).delete(accounts::unlink),
            )
            .route("/v1/account/link/{challenge}/code", post(accounts::code))
            .route(
                "/v1/account/link/{challenge}/password",
                post(accounts::password),
            )
            .route(
                "/v1/account/grants/{client}",
                get(accounts::grant).put(accounts::update_grant),
            );
    }
    Ok(router
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(Arc::clone(&state), admit))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            private_response,
        ))
        .with_state(state))
}

async fn admit(State(state): State<Arc<ServiceState>>, request: Request, next: Next) -> Response {
    if request.uri().path() == "/readyz" {
        return next.run(request).await;
    }
    if state
        .hosted
        .as_ref()
        .is_some_and(|hosted| hosted.cancellation.is_cancelled())
    {
        return ApiError::new(ErrorCode::Unavailable).into_response();
    }
    let Ok(_permit) = state.admission.try_acquire() else {
        return ApiError::new(ErrorCode::RateLimited).into_response();
    };
    let seconds = state
        .hosted
        .as_ref()
        .map_or(30, |hosted| hosted.config.request_seconds);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(seconds);
    REQUEST_DEADLINE
        .scope(deadline, async move {
            let request = if matches!(
                *request.method(),
                Method::POST | Method::PUT | Method::DELETE
            ) && request.uri().path() != "/v1/uploads"
            {
                let (parts, body) = request.into_parts();
                let bytes = match tokio::time::timeout_at(deadline, to_bytes(body, MAX_BODY_BYTES))
                    .await
                {
                    Ok(Ok(bytes)) => bytes,
                    Ok(Err(_)) => return ApiError::new(ErrorCode::PayloadTooLarge).into_response(),
                    Err(_) => return ApiError::new(ErrorCode::Timeout).into_response(),
                };
                Request::from_parts(parts, Body::from(bytes))
            } else {
                request
            };
            next.run(request).await
        })
        .await
}

async fn private_response(
    State(state): State<Arc<ServiceState>>,
    request: Request,
    next: Next,
) -> Response {
    let stream_setup = request.method() == Method::GET && request.uri().path() == "/v1/events";
    let mut response = next.run(request).await;
    if stream_setup
        && !response.status().is_success()
        && let Ok(hosted) = state.hosted()
    {
        hosted.streams.record_rejected_setup();
    }
    response
        .headers_mut()
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-store"));
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

async fn ready(State(state): State<Arc<ServiceState>>) -> StatusCode {
    if state
        .hosted
        .as_ref()
        .is_some_and(|hosted| hosted.cancellation.is_cancelled())
    {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::NO_CONTENT
    }
}

fn json_error(error: JsonRejection) -> ApiError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::new(ErrorCode::PayloadTooLarge)
    } else {
        ApiError::new(ErrorCode::InvalidRequest)
    }
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
        .map_err(|_| ApiError::new(ErrorCode::Unavailable))
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
    let identity = bearer_identity(&state, &headers).await?;
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

async fn bearer_identity(
    state: &ServiceState,
    headers: &HeaderMap,
) -> Result<crate::identity::VerifiedIdentity, ApiError> {
    let header = single_header(headers, header::AUTHORIZATION)
        .ok_or(ApiError::new(ErrorCode::Unauthenticated))?;
    let (kind, token) = header
        .split_once(' ')
        .ok_or(ApiError::new(ErrorCode::Unauthenticated))?;
    if !kind.eq_ignore_ascii_case("Bearer")
        || token.is_empty()
        || token.len() > MAX_BODY_BYTES
        || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(ApiError::new(ErrorCode::Unauthenticated));
    }
    state
        .verifier
        .verify(&SecretString::from(token.to_owned()), &Audience::Api)
        .await
        .map_err(ApiError::from)
}

fn single_header(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    let all = headers.get_all(name);
    let mut values = all.iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}

/// Serve the hosted router over a trusted loopback listener with bounded writes.
///
/// # Errors
/// Returns a listener failure.
pub async fn serve(
    listener: tokio::net::TcpListener,
    app: Router,
    write_timeout: std::time::Duration,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    shutdown::serve(listener, app, write_timeout, None, shutdown).await
}

#[cfg(test)]
#[path = "service/body_tests.rs"]
mod body_tests;
