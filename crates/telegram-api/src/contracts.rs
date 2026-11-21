//! HTTP DTOs shared by the API and its clients.

use serde::{Deserialize, Serialize};

pub mod groups;

/// Stable service error category.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
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
    /// A Premium account is required for the requested transcription.
    PremiumRequired,
    /// The selected message is not a supported transcription input.
    NotTranscribable,
    /// The selected audio exceeds Telegram's transcription limit.
    VoiceTooLong,
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
    /// Read granted channels and groups.
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

/// The channels a registered client may read under its account grant.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "channel_ids")]
pub enum ReadGrant {
    /// No access to Telegram content.
    None,
    /// Specific channels, still subject to current access and deployment eligibility.
    Channels(Vec<String>),
    /// All eligible joined channels, usable by direct clients only.
    AllJoined,
}

/// Current permissions of a client in the active generation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrantSnapshot {
    /// Explicit group permissions, independent of the channel read grant.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub group_refs: Vec<String>,
    /// Revision used for compare-and-swap updates.
    pub revision: u64,
    /// Read authorization.
    pub read: ReadGrant,
    /// Send authorization, independent of read authorization.
    pub send_enabled: bool,
}

impl Default for GrantSnapshot {
    fn default() -> Self {
        Self {
            group_refs: Vec::new(),
            revision: 0,
            read: ReadGrant::None,
            send_enabled: false,
        }
    }
}

/// Replace a registered client's permissions if the revision still matches.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrantUpdate {
    /// Revision returned by the last read.
    pub expected_revision: u64,
    /// Proposed read authorization.
    pub read: ReadGrant,
    /// Proposed send authorization.
    pub send_enabled: bool,
}

pub mod hosted_tools;
mod reads;
pub use reads::{
    BatchRequest, BatchResponse, HistoryQuery, HistoryResponse, ReadAlbumInfo, ReadMediaKind,
    ReadMessage, ReadPeerNamespace, SearchChannelError, SearchMediaFilter, SearchRequest,
    SearchResponse,
};
