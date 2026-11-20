//! HTTP DTOs shared by the API and its clients.

use serde::{Deserialize, Serialize};

/// Stable service error category.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Malformed request.
    InvalidRequest,
    /// Missing or invalid credential.
    Unauthenticated,
    /// Authenticated caller lacks permission.
    Forbidden,
    /// Missing or inaccessible resource.
    UnavailableResource,
    /// Request conflicts with current account state.
    Conflict,
    /// Request exceeds the allowed body size.
    PayloadTooLarge,
    /// Request exceeds configured admission or quota.
    RateLimited,
    /// Required service is unavailable.
    Unavailable,
    /// Operation exceeded its deadline.
    Timeout,
}

/// Versioned error envelope.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorResponse {
    /// Safe machine-readable error.
    pub error: ErrorBody,
}

/// Safe error details.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    /// Stable reason code.
    pub code: ErrorCode,
    /// Whole seconds until the request can be retried, when supported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
}

/// OAuth scopes granted by this service's issuer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Scope {
    /// Read granted channels.
    #[serde(rename = "telegram.read")]
    Read,
    /// Send under an enabled account grant.
    #[serde(rename = "telegram.send")]
    Send,
    /// Manage linking and grants using a first-party browser session.
    #[serde(rename = "telegram.link")]
    Link,
}

/// Credential type for an authenticated session.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    /// Direct OAuth access token.
    Direct,
    /// Verified delegated access token.
    Delegated,
    /// First-party owner browser session.
    BrowserOwner,
}

/// Session information for the authenticated caller.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionInfo {
    /// Registered OAuth client.
    pub client_id: String,
    /// Scopes available to the credential.
    pub scopes: Vec<Scope>,
    /// How the caller authenticated.
    pub kind: SessionKind,
    /// Expiry in Unix seconds.
    pub expires_at_unix: u64,
    /// Session-bound token required for browser mutations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csrf_token: Option<String>,
}
