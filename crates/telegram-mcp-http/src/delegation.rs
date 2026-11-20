//! Exchange a verified MCP bearer at its issuer before contacting the API.

use crate::catalog::scope;
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use telegram_api::contracts::{ErrorCode, ErrorResponse, Scope};
use telegram_api::identity::{Audience, CredentialKind, CredentialVerifier, VerifiedIdentity};
use tokio::time::Instant;
use url::Url;

const ACCESS_TOKEN: &str = "urn:ietf:params:oauth:token-type:access_token";
const API_BODY_BYTES: usize = 512 * 1024;
// The API caps each processed JPEG at 1,152 KiB before base64 framing.
const PREVIEW_BYTES: usize = 1152 * 1024;

#[derive(Clone)]
pub(crate) struct Gateway {
    pub verifier: CredentialVerifier,
    pub http: Client,
    pub api: Url,
    pub secret: SecretString,
}

#[derive(Clone)]
pub(crate) struct Caller {
    pub token: SecretString,
    pub identity: VerifiedIdentity,
    pub deadline: Instant,
    pub unix_ms: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct Failure {
    pub code: &'static str,
    pub message: &'static str,
    pub retry_after: Option<u64>,
    pub unknown_delivery: bool,
}

impl Failure {
    pub const INVALID: Self = Self::new("invalid_input", "Invalid tool arguments");
    pub const UNAVAILABLE: Self = Self::new("unavailable", "Telegram API unavailable");
    pub const DENIED: Self = Self::new("forbidden", "Access denied");
    pub const TIMEOUT: Self = Self::new("timeout", "The request deadline expired");
    pub const LARGE: Self = Self::new("response_too_large", "The response exceeds the size limit");

    const fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            retry_after: None,
            unknown_delivery: false,
        }
    }

    pub const UNKNOWN: Self = Self {
        unknown_delivery: true,
        ..Self::new("delivery_unknown", "Delivery could not be confirmed")
    };

    fn upstream(error: ErrorResponse) -> Self {
        match error.error.code {
            ErrorCode::InvalidRequest => Self::INVALID,
            ErrorCode::Unauthenticated => Self::new("unauthenticated", "API credential rejected"),
            ErrorCode::Forbidden => Self::DENIED,
            ErrorCode::UnavailableResource => {
                Self::new("unavailable_resource", "Resource unavailable")
            }
            ErrorCode::Conflict => Self::new("conflict", "Account state changed"),
            ErrorCode::PayloadTooLarge => Self::LARGE,
            ErrorCode::RateLimited => Self {
                retry_after: error.error.retry_after_seconds,
                ..Self::new("rate_limited", "Request rate limit reached")
            },
            ErrorCode::Unavailable => Self::UNAVAILABLE,
            ErrorCode::Timeout => Self::TIMEOUT,
            ErrorCode::PremiumRequired => {
                Self::new("premium_required", "Telegram Premium is required")
            }
            ErrorCode::NotTranscribable => {
                Self::new("not_transcribable", "The message has no supported audio")
            }
            ErrorCode::VoiceTooLong => Self::new(
                "voice_too_long",
                "The audio exceeds Telegram's transcription limit",
            ),
        }
    }
}

#[derive(Deserialize)]
struct Exchange {
    access_token: SecretString,
    token_type: String,
    issued_token_type: String,
}

#[derive(Deserialize)]
struct Capabilities {
    tools: Vec<String>,
}

pub(crate) struct Delegation {
    token: SecretString,
    pub expires_unix: u64,
    scopes: BTreeSet<Scope>,
}

pub(crate) fn now_unix_ms() -> Option<u64> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis();
    u64::try_from(ms).ok()
}

impl Gateway {
    pub async fn delegate(
        &self,
        caller: &Caller,
        requested: &[Scope],
    ) -> Result<Delegation, Failure> {
        let scopes: BTreeSet<_> = requested
            .iter()
            .copied()
            .filter(|scope| caller.identity.scopes().contains(scope))
            .collect();
        let scope = match (scopes.contains(&Scope::Read), scopes.contains(&Scope::Send)) {
            (true, true) => "telegram.read telegram.send",
            (true, false) => "telegram.read",
            (false, true) => "telegram.send",
            (false, false) => return Err(Failure::DENIED),
        };
        if scopes.contains(&Scope::Link) {
            return Err(Failure::DENIED);
        }
        let client = form_component(self.verifier.gateway_actor());
        let secret = SecretString::from(form_component(self.secret.expose_secret()));
        let response = self
            .http
            .post(self.verifier.token_endpoint().clone())
            .basic_auth(client, Some(secret.expose_secret()))
            .form(&[
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:token-exchange",
                ),
                ("subject_token_type", ACCESS_TOKEN),
                ("subject_token", caller.token.expose_secret()),
                ("requested_token_type", ACCESS_TOKEN),
                ("resource", self.verifier.audience(Audience::Api)),
                ("scope", scope),
            ])
            .send()
            .await
            .map_err(network_failure)?;
        if !response.status().is_success() {
            return Err(if response.status().is_client_error() {
                Failure::DENIED
            } else {
                Failure::UNAVAILABLE
            });
        }
        let exchange: Exchange = bounded(response, 32 * 1024).await?;
        if !exchange.token_type.eq_ignore_ascii_case("bearer")
            || exchange.issued_token_type != ACCESS_TOKEN
        {
            return Err(Failure::DENIED);
        }
        let delegated = self
            .verifier
            .verify(&exchange.access_token, &Audience::Api)
            .await
            .map_err(|_| Failure::DENIED)?;
        if delegated.kind() != CredentialKind::Delegated
            || delegated.principal() != caller.identity.principal()
            || delegated.client_id() != caller.identity.client_id()
            || delegated.expires_at() > caller.identity.expires_at()
            || delegated.scopes().is_empty()
            || !delegated.scopes().is_subset(&scopes)
        {
            return Err(Failure::DENIED);
        }
        Ok(Delegation {
            token: exchange.access_token,
            expires_unix: delegated.expires_at(),
            scopes: delegated.scopes().clone(),
        })
    }

    pub async fn capabilities(
        &self,
        caller: &Caller,
        delegation: &Delegation,
    ) -> Result<Vec<String>, Failure> {
        let endpoint = self
            .api
            .join("v1/capabilities")
            .map_err(|_| Failure::UNAVAILABLE)?;
        let response = self
            .send(self.http.get(endpoint), caller, delegation)
            .await?;
        let capabilities: Capabilities = bounded(response, 8192).await?;
        Ok(capabilities
            .tools
            .into_iter()
            .filter(|name| delegation.scopes.contains(&scope(name)))
            .collect())
    }

    pub async fn read(
        &self,
        name: &str,
        request: RequestBuilder,
        caller: &Caller,
        delegation: &Delegation,
    ) -> Result<Value, Failure> {
        use telegram_api::contracts::{BatchResponse, HistoryResponse, SearchResponse};
        let response = self.send(request, caller, delegation).await?;
        let data = match name {
            "search_messages" => {
                serde_json::to_value(bounded::<SearchResponse>(response, API_BODY_BYTES).await?)
            }
            "get_recent_messages" => {
                serde_json::to_value(bounded::<HistoryResponse>(response, API_BODY_BYTES).await?)
            }
            "get_messages_batch" => {
                serde_json::to_value(bounded::<BatchResponse>(response, API_BODY_BYTES).await?)
            }
            _ => Ok(bounded::<Value>(response, API_BODY_BYTES).await?),
        }
        .map_err(|_| Failure::UNAVAILABLE)?;
        if !data.is_object() {
            return Err(Failure::UNAVAILABLE);
        }
        Ok(data)
    }

    pub async fn mutation(
        &self,
        request: RequestBuilder,
        caller: &Caller,
        delegation: &Delegation,
    ) -> Result<Value, Failure> {
        let request = self.authorized(request, caller, delegation)?;
        let response = request.send().await.map_err(|_| Failure::UNKNOWN)?;
        let status = response.status();
        if !status.is_success() {
            let bytes = bounded_bytes(response, 8192)
                .await
                .map_err(|_| Failure::UNKNOWN)?;
            return Err(mutation_failure(&bytes, status));
        }
        let data: Value = bounded(response, API_BODY_BYTES)
            .await
            .map_err(|_| Failure::UNKNOWN)?;
        if !data.is_object() {
            return Err(Failure::UNKNOWN);
        }
        Ok(data)
    }

    pub async fn preview(
        &self,
        handle: &str,
        caller: &Caller,
        delegation: &Delegation,
    ) -> Result<Vec<u8>, Failure> {
        let endpoint = self
            .api
            .join(&format!("v1/media/{handle}"))
            .map_err(|_| Failure::UNAVAILABLE)?;
        let response = self
            .send(self.http.get(endpoint), caller, delegation)
            .await?;
        if response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            != Some("image/jpeg")
        {
            return Err(Failure::UNAVAILABLE);
        }
        let bytes = bounded_bytes(response, PREVIEW_BYTES).await?;
        if !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            return Err(Failure::UNAVAILABLE);
        }
        Ok(bytes)
    }

    async fn send(
        &self,
        request: RequestBuilder,
        caller: &Caller,
        delegation: &Delegation,
    ) -> Result<Response, Failure> {
        let response = self
            .authorized(request, caller, delegation)?
            .send()
            .await
            .map_err(network_failure)?;
        if !response.status().is_success() {
            if response.status() == StatusCode::GATEWAY_TIMEOUT {
                return Err(Failure::TIMEOUT);
            }
            let err: ErrorResponse = bounded(response, 8192).await?;
            return Err(Failure::upstream(err));
        }
        Ok(response)
    }

    fn authorized(
        &self,
        request: RequestBuilder,
        caller: &Caller,
        delegation: &Delegation,
    ) -> Result<RequestBuilder, Failure> {
        let deadline = caller
            .unix_ms
            .min(delegation.expires_unix.saturating_mul(1000));
        let remaining = deadline
            .checked_sub(now_unix_ms().ok_or(Failure::UNAVAILABLE)?)
            .filter(|ms| *ms > 0)
            .ok_or(Failure::TIMEOUT)?;
        Ok(request
            .bearer_auth(delegation.token.expose_secret())
            .header("X-Telegram-Deadline-Unix-Ms", deadline)
            .timeout(Duration::from_millis(remaining)))
    }
}

fn form_component(value: &str) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    serializer.append_key_only(value);
    serializer.finish()
}

fn network_failure(error: reqwest::Error) -> Failure {
    if error.is_timeout() {
        Failure::TIMEOUT
    } else {
        Failure::UNAVAILABLE
    }
}

async fn bounded<T: DeserializeOwned>(response: Response, limit: usize) -> Result<T, Failure> {
    serde_json::from_slice(&bounded_bytes(response, limit).await?).map_err(|_| Failure::UNAVAILABLE)
}

async fn bounded_bytes(mut response: Response, limit: usize) -> Result<Vec<u8>, Failure> {
    if response
        .content_length()
        .is_some_and(|len| len > limit as u64)
    {
        return Err(Failure::LARGE);
    }
    let mut buffer = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(network_failure)? {
        if chunk.len() > limit.saturating_sub(buffer.len()) {
            return Err(Failure::LARGE);
        }
        buffer.extend_from_slice(&chunk);
    }
    Ok(buffer)
}

fn mutation_failure(bytes: &[u8], status: StatusCode) -> Failure {
    if let Ok(err) = serde_json::from_slice::<ErrorResponse>(bytes) {
        return if status.is_client_error() {
            Failure::upstream(err)
        } else {
            Failure::UNKNOWN
        };
    }
    let Ok(body) = serde_json::from_slice::<Value>(bytes) else {
        return Failure::UNKNOWN;
    };
    let error = &body["error"];
    if error["delivery"] != "not_sent" {
        return Failure::UNKNOWN;
    }
    const CODES: &[&str] = &[
        "sending_disabled",
        "recipient_not_found",
        "recipient_mismatch",
        "send_forbidden",
        "invalid_reply_target",
        "upload_source_invalid",
        "unsupported_media",
        "upload_failed",
        "send_capacity_reached",
        "invalid_input",
        "media_too_large",
        "rate_limited",
        "unauthenticated",
        "timeout",
        "response_too_large",
        "unavailable",
        "internal_error",
    ];
    let Some(code) = CODES.iter().copied().find(|code| error["code"] == *code) else {
        return Failure::UNKNOWN;
    };
    Failure {
        retry_after: error["retry_after_seconds"].as_u64(),
        ..Failure::new(code, "The message was not sent")
    }
}
