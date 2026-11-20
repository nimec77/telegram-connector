//! First-party OpenID code flow and bounded in-process browser sessions.

use super::ServiceState;
use super::http_error::ApiError;
use crate::contracts::{ErrorCode, Scope, SessionInfo, SessionKind};
use crate::identity::{
    Audience, AuthError, CredentialKind, CredentialVerifier, VerifiedIdentity, bounded_json,
    unix_now, valid_label,
};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use subtle::ConstantTimeEq as _;
use tokio::time::Instant;
use url::Url;

const LOGIN_COOKIE: &str = "__Host-tg_login";
const SESSION_COOKIE: &str = "__Host-tg_session";
const CSRF_HEADER: header::HeaderName = header::HeaderName::from_static("x-csrf-token");
const ATTEMPT_TTL: Duration = Duration::from_secs(300);
const SESSION_TTL: Duration = Duration::from_secs(900);
const MAX_ATTEMPTS: usize = 256;
const MAX_SESSIONS: usize = 512;
const MAX_CALLBACK_BYTES: usize = 8 * 1024;

/// Registration of the first-party confidential browser client.
pub struct BrowserConfig {
    /// Registered client ID, distinct from either resource audience.
    pub client_id: String,
    /// Provider registration secret.
    pub client_secret: SecretString,
    /// Exact public HTTPS callback registered with the issuer.
    pub callback_url: Url,
}

pub(super) struct BrowserAuth {
    config: BrowserConfig,
    origin: String,
    memory: Mutex<BrowserMemory>,
}

#[derive(Default)]
struct BrowserMemory {
    attempts: HashMap<String, LoginAttempt>,
    sessions: HashMap<String, BrowserSession>,
}

struct LoginRedirect {
    cookie_id: String,
    state: String,
    nonce: String,
    challenge: String,
}

struct LoginAttempt {
    state: String,
    nonce: String,
    verifier: SecretString,
    expires: Instant,
}

#[derive(Clone)]
struct BrowserSession {
    identity: VerifiedIdentity,
    expires: Instant,
    expires_at: u64,
    csrf: String,
}

pub(super) struct OwnerSession {
    pub(super) info: SessionInfo,
    // Retain the authenticated principal for account/grant handlers.
    pub(super) identity: VerifiedIdentity,
}

impl BrowserAuth {
    pub(super) fn new(
        config: BrowserConfig,
        issuer: &CredentialVerifier,
    ) -> Result<Self, AuthError> {
        let callback = &config.callback_url;
        if !valid_label(&config.client_id)
            || config.client_id == issuer.gateway_actor()
            || config.client_id == issuer.audience(Audience::Api)
            || config.client_id == issuer.audience(Audience::Mcp)
            || config.client_secret.expose_secret().is_empty()
            || config.client_secret.expose_secret().len() > 4096
            || callback.scheme() != "https"
            || !callback.has_host()
            || !callback.username().is_empty()
            || callback.password().is_some()
            || callback.query().is_some()
            || callback.fragment().is_some()
            || callback.path() != "/auth/callback"
        {
            return Err(AuthError::Configuration);
        }
        let origin = callback.origin().ascii_serialization();
        Ok(Self {
            config,
            origin,
            memory: Mutex::default(),
        })
    }

    fn begin(&self, headers: &HeaderMap) -> Result<LoginRedirect, ApiError> {
        let mut memory = self.memory.lock().map_err(|_| AuthError::Unavailable)?;
        let now = Instant::now();
        memory.attempts.retain(|_, attempt| attempt.expires > now);
        if let Ok(previous) = cookie(headers, LOGIN_COOKIE) {
            memory.attempts.remove(&previous);
        }
        if memory.attempts.len() >= MAX_ATTEMPTS {
            return Err(ApiError(ErrorCode::RateLimited));
        }
        let id = random_value()?;
        let state = random_value()?;
        let nonce = random_value()?;
        let verifier = random_value()?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        memory.attempts.insert(
            id.clone(),
            LoginAttempt {
                state: state.clone(),
                nonce: nonce.clone(),
                verifier: SecretString::from(verifier),
                expires: now + ATTEMPT_TTL,
            },
        );
        Ok(LoginRedirect {
            cookie_id: id,
            state,
            nonce,
            challenge,
        })
    }

    fn complete_attempt(&self, headers: &HeaderMap, state: &str) -> Result<LoginAttempt, ApiError> {
        let id = cookie(headers, LOGIN_COOKIE)?;
        let mut memory = self.memory.lock().map_err(|_| AuthError::Unavailable)?;
        let attempt = memory.attempts.get(&id).ok_or(AuthError::Credential)?;
        if !same_token(&attempt.state, state) {
            return Err(AuthError::Credential.into());
        }
        let attempt = memory.attempts.remove(&id).ok_or(AuthError::Credential)?;
        if attempt.expires <= Instant::now() {
            return Err(AuthError::Credential.into());
        }
        Ok(attempt)
    }

    fn open_session(
        &self,
        headers: &HeaderMap,
        identity: VerifiedIdentity,
        id_expiry: u64,
    ) -> Result<(String, u64), ApiError> {
        let now = unix_now()?;
        let expires_at = identity
            .expires_at()
            .min(id_expiry)
            .min(now + SESSION_TTL.as_secs());
        if expires_at <= now {
            return Err(AuthError::Credential.into());
        }
        let lifetime = expires_at - now;
        let mut memory = self.memory.lock().map_err(|_| AuthError::Unavailable)?;
        let instant = Instant::now();
        memory
            .sessions
            .retain(|_, session| session.expires > instant && session.expires_at > now);
        if let Ok(old) = cookie(headers, SESSION_COOKIE) {
            memory.sessions.remove(&old);
        }
        if memory.sessions.len() >= MAX_SESSIONS {
            return Err(ApiError(ErrorCode::RateLimited));
        }
        let id = random_value()?;
        memory.sessions.insert(
            id.clone(),
            BrowserSession {
                identity,
                expires: instant + Duration::from_secs(lifetime),
                expires_at,
                csrf: random_value()?,
            },
        );
        Ok((id, lifetime))
    }

    pub(super) fn owner(&self, headers: &HeaderMap) -> Result<(String, OwnerSession), ApiError> {
        if headers.contains_key(header::AUTHORIZATION) {
            return Err(AuthError::Forbidden.into());
        }
        let id = cookie(headers, SESSION_COOKIE)?;
        let now = unix_now()?;
        let mut memory = self.memory.lock().map_err(|_| AuthError::Unavailable)?;
        let session = memory.sessions.get(&id).ok_or(AuthError::Credential)?;
        if session.expires <= Instant::now() || session.expires_at <= now {
            memory.sessions.remove(&id);
            return Err(AuthError::Credential.into());
        }
        Ok((
            id,
            OwnerSession {
                info: SessionInfo {
                    client_id: session.identity.client_id().to_owned(),
                    scopes: session.identity.scopes().iter().copied().collect(),
                    kind: SessionKind::BrowserOwner,
                    expires_at_unix: session.expires_at,
                    csrf_token: Some(session.csrf.clone()),
                },
                identity: session.identity.clone(),
            },
        ))
    }
    pub(super) fn owner_mutation(
        &self,
        headers: &HeaderMap,
    ) -> Result<(String, OwnerSession), ApiError> {
        let (id, owner) = self.owner(headers)?;
        let token = super::single_header(headers, CSRF_HEADER);
        let origin = super::single_header(headers, header::ORIGIN);
        if origin != Some(self.origin.as_str())
            || !owner.identity.scopes().contains(&Scope::Link)
            || !token
                .zip(owner.info.csrf_token.as_deref())
                .is_some_and(|(supplied, stored)| same_token(supplied, stored))
        {
            return Err(AuthError::Forbidden.into());
        }
        Ok((id, owner))
    }
}

#[derive(Deserialize)]
pub(super) struct CallbackQuery {
    state: String,
    code: Option<String>,
    iss: Option<String>,
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: SecretString,
    id_token: SecretString,
    token_type: String,
}

pub(super) async fn start(
    State(state): axum::extract::State<Arc<ServiceState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let login = state.browser.begin(&headers)?;
    let mut authorization = state.verifier.authorization_endpoint().clone();
    authorization.query_pairs_mut().extend_pairs([
        ("response_type", "code"),
        ("client_id", state.browser.config.client_id.as_str()),
        ("redirect_uri", state.browser.config.callback_url.as_str()),
        ("scope", "openid telegram.link"),
        ("resource", state.verifier.audience(Audience::Api)),
        ("state", login.state.as_str()),
        ("nonce", login.nonce.as_str()),
        ("code_challenge", login.challenge.as_str()),
        ("code_challenge_method", "S256"),
    ]);
    let location =
        HeaderValue::from_str(authorization.as_str()).map_err(|_| AuthError::Unavailable)?;
    let mut response = StatusCode::SEE_OTHER.into_response();
    response.headers_mut().insert(header::LOCATION, location);
    response.headers_mut().append(
        header::SET_COOKIE,
        set_cookie(LOGIN_COOKIE, &login.cookie_id, ATTEMPT_TTL.as_secs())?,
    );
    Ok(response)
}

pub(super) async fn callback(
    State(state): axum::extract::State<Arc<ServiceState>>,
    headers: HeaderMap,
    uri: Uri,
) -> Result<Response, ApiError> {
    if uri.query().is_none_or(|raw| raw.len() > MAX_CALLBACK_BYTES) {
        return Err(ApiError(ErrorCode::InvalidRequest));
    }
    let Query(query) = Query::<CallbackQuery>::try_from_uri(&uri)
        .map_err(|_| ApiError(ErrorCode::InvalidRequest))?;
    if query
        .iss
        .as_deref()
        .is_some_and(|iss| iss != state.verifier.issuer())
    {
        return Err(AuthError::Credential.into());
    }
    let attempt = state.browser.complete_attempt(&headers, &query.state)?;
    let code = query
        .code
        .filter(|code| !code.is_empty() && code.len() <= 4096)
        .ok_or(AuthError::Credential)?;
    let registration = &state.browser.config;
    let response = state
        .verifier
        .http_client()
        .post(state.verifier.token_endpoint().clone())
        .basic_auth(
            form_escape(&registration.client_id),
            Some(form_escape(registration.client_secret.expose_secret())),
        )
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("code_verifier", attempt.verifier.expose_secret()),
            ("redirect_uri", registration.callback_url.as_str()),
            ("resource", state.verifier.audience(Audience::Api)),
        ])
        .send()
        .await
        .map_err(|_| AuthError::Unavailable)?;
    if response.status().is_client_error() {
        return Err(AuthError::Credential.into());
    }
    let reply: TokenReply = bounded_json(response).await?;
    if !reply.token_type.eq_ignore_ascii_case("Bearer") {
        return Err(AuthError::Credential.into());
    }
    let (principal, expiry) = state
        .verifier
        .verify_id_token(&reply.id_token, &registration.client_id, &attempt.nonce)
        .await?;
    let identity = state
        .verifier
        .verify(&reply.access_token, &Audience::Api)
        .await?;
    if identity.kind() != CredentialKind::Direct
        || identity.client_id() != registration.client_id
        || identity.principal() != &principal
        || !identity.scopes().contains(&Scope::Link)
    {
        return Err(AuthError::Forbidden.into());
    }
    let (session, lifetime) = state.browser.open_session(&headers, identity, expiry)?;
    let mut response = StatusCode::SEE_OTHER.into_response();
    let outgoing = response.headers_mut();
    outgoing.insert(header::LOCATION, HeaderValue::from_static("/v1/session"));
    outgoing.append(header::SET_COOKIE, set_cookie(LOGIN_COOKIE, "", 0)?);
    outgoing.append(
        header::SET_COOKIE,
        set_cookie(SESSION_COOKIE, &session, lifetime)?,
    );
    Ok(response)
}

pub(super) async fn logout(
    State(state): axum::extract::State<Arc<ServiceState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (id, _) = state.browser.owner_mutation(&headers)?;
    state
        .browser
        .memory
        .lock()
        .map_err(|_| AuthError::Unavailable)?
        .sessions
        .remove(&id);
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, set_cookie(SESSION_COOKIE, "", 0)?);
    Ok(response)
}

fn random_value() -> Result<String, AuthError> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| AuthError::Unavailable)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn same_token(expected: &str, presented: &str) -> bool {
    bool::from(expected.as_bytes().ct_eq(presented.as_bytes()))
}

fn form_escape(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn set_cookie(name: &str, value: &str, max_age: u64) -> Result<HeaderValue, AuthError> {
    HeaderValue::from_str(&format!(
        "{name}={value}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={max_age}"
    ))
    .map_err(|_| AuthError::Unavailable)
}

fn cookie(headers: &HeaderMap, name: &str) -> Result<String, AuthError> {
    let mut found = None;
    for header in headers.get_all(header::COOKIE) {
        for part in header
            .to_str()
            .map_err(|_| AuthError::Credential)?
            .split(';')
        {
            if let Some((key, value)) = part.trim().split_once('=')
                && key == name
            {
                if found.is_some()
                    || value.len() != 43
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                {
                    return Err(AuthError::Credential);
                }
                found = Some(value.to_owned());
            }
        }
    }
    found.ok_or(AuthError::Credential)
}
