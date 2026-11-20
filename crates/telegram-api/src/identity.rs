//! Issuer-bound OAuth access tokens and OpenID Connect identity.

pub use crate::contracts::Scope;
use jsonwebtoken::jwk::Jwk;
use jsonwebtoken::{Algorithm, DecodingKey, Header, Validation, decode, decode_header};
use reqwest::{Client, Response};
use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};
use tokio::time::Instant;
use url::Url;

const MAX_TOKEN_BYTES: usize = 16 * 1024;
const MAX_DOCUMENT_BYTES: usize = 256 * 1024;
const MAX_KEYS: usize = 64;
const CACHE_TTL: Duration = Duration::from_secs(300);
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const MAX_DELEGATED_LIFETIME: u64 = 300;

/// Distinct resource audiences accepted by the deployment.
#[derive(Clone, Copy)]
pub enum Audience {
    /// Hosted Telegram API.
    Api,
    /// Hosted MCP gateway.
    Mcp,
}

/// Supported asymmetric JWT signing algorithms.
#[derive(Clone, Copy, Deserialize)]
pub enum SigningAlgorithm {
    /// RSA SHA-256.
    RS256,
    /// ECDSA P-256 SHA-256.
    ES256,
    /// EdDSA with Ed25519 keys.
    EdDSA,
}

impl From<SigningAlgorithm> for Algorithm {
    fn from(value: SigningAlgorithm) -> Self {
        match value {
            SigningAlgorithm::RS256 => Self::RS256,
            SigningAlgorithm::ES256 => Self::ES256,
            SigningAlgorithm::EdDSA => Self::EdDSA,
        }
    }
}

/// Operator-provisioned trust settings for one issuer.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityConfig {
    /// Exact OpenID issuer identifier.
    pub issuer: String,
    /// Hosted API resource identifier.
    pub api_audience: String,
    /// MCP gateway resource identifier.
    pub mcp_audience: String,
    /// Expected signed subject of the gateway actor.
    pub gateway_actor: String,
    /// Optional issuer-signed common subject claim for pairwise deployments.
    #[serde(default)]
    pub subject_claim: Option<String>,
    /// Explicit issuer signature algorithm allowlist.
    pub algorithms: Vec<SigningAlgorithm>,
}

/// The verified role conveyed by an OAuth token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialKind {
    /// An independently registered OAuth client.
    Direct,
    /// The trusted gateway acting on behalf of the originating client.
    Delegated,
}

/// Stable account lookup key tied to a verified provider.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct Principal {
    issuer: String,
    subject: String,
}

impl Principal {
    /// Trusted provider identifier.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Stable provider subject for account lookup.
    pub fn subject(&self) -> &str {
        &self.subject
    }
}

/// Identity granted by verified claims.
#[derive(Clone)]
pub struct VerifiedIdentity {
    principal: Principal,
    client_id: String,
    scopes: BTreeSet<Scope>,
    expires_at: u64,
    kind: CredentialKind,
}

impl VerifiedIdentity {
    /// Account lookup key.
    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    /// Originating registered client identity signed by the issuer.
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Accepted Telegram scopes.
    pub fn scopes(&self) -> &BTreeSet<Scope> {
        &self.scopes
    }

    /// Expiry in Unix seconds.
    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }

    /// Verified direct or delegated access.
    pub fn kind(&self) -> CredentialKind {
        self.kind
    }
}

/// Safe identity failure categories.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// Issuer or client settings violate the service's trust policy.
    #[error("invalid identity configuration")]
    Configuration,
    /// A token or session failed authentication.
    #[error("invalid authentication credentials")]
    Credential,
    /// Authenticated credentials lack the required access.
    #[error("access denied")]
    Forbidden,
    /// The trusted provider or local identity state is unavailable.
    #[error("identity service unavailable")]
    Unavailable,
}

/// Shared verifier with rotating issuer keys.
#[derive(Clone)]
pub struct CredentialVerifier(Arc<Authority>);

struct Authority {
    config: AuthorityConfig,
    client: Client,
    metadata: ProviderMetadata,
    keys: RwLock<CachedKeys>,
    refresh: Mutex<()>,
}

#[derive(Deserialize)]
struct ProviderMetadata {
    issuer: String,
    jwks_uri: Url,
    authorization_endpoint: Url,
    token_endpoint: Url,
}

struct CachedKeys {
    keys: HashMap<String, Arc<TrustedKey>>,
    expires: Instant,
    refresh_after: Instant,
}

struct TrustedKey {
    key: DecodingKey,
    algorithm: Option<Algorithm>,
}

#[derive(Clone, Copy)]
enum Transport {
    Https,
    #[cfg(test)]
    Loopback,
}

impl Transport {
    fn permits(self, url: &Url) -> bool {
        if !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
            || !url.has_host()
        {
            return false;
        }
        match self {
            Self::Https => url.scheme() == "https",
            #[cfg(test)]
            Self::Loopback => {
                url.scheme() == "http"
                    && match url.host() {
                        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
                        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
                        _ => false,
                    }
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum AudienceClaim {
    One(String),
    List(Vec<String>),
}

impl AudienceClaim {
    fn access_matches(&self, audience: &str) -> bool {
        match self {
            Self::One(one) => one == audience,
            Self::List(list) => matches!(list.as_slice(), [one] if one == audience),
        }
    }

    #[cfg(feature = "service")]
    fn id_matches(&self, audience: &str, azp: Option<&str>) -> bool {
        if azp.is_some_and(|party| party != audience) {
            return false;
        }
        match self {
            Self::One(one) => one == audience,
            Self::List(list) => {
                list.iter().any(|value| value == audience)
                    && (list.len() == 1 || azp == Some(audience))
            }
        }
    }
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: AudienceClaim,
    iat: u64,
    exp: u64,
    #[serde(default)]
    jti: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    act: Option<Actor>,
    #[cfg(feature = "service")]
    #[serde(default)]
    nonce: Option<String>,
    #[cfg(feature = "service")]
    #[serde(default)]
    azp: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Actor {
    sub: String,
    #[serde(default)]
    iss: Option<String>,
}

impl CredentialVerifier {
    /// Discover the configured HTTPS issuer and fetch its signing keys.
    ///
    /// # Errors
    /// Returns a safe configuration or availability error if discovery fails.
    pub async fn discover(config: AuthorityConfig) -> Result<Self, AuthError> {
        Self::discover_with_transport(config, Transport::Https).await
    }

    #[cfg(test)]
    pub(crate) async fn discover_loopback(config: AuthorityConfig) -> Result<Self, AuthError> {
        Self::discover_with_transport(config, Transport::Loopback).await
    }

    async fn discover_with_transport(
        config: AuthorityConfig,
        transport: Transport,
    ) -> Result<Self, AuthError> {
        let issuer = Url::parse(&config.issuer).map_err(|_| AuthError::Configuration)?;
        if !transport.permits(&issuer)
            || !resource_identifier(&config.api_audience)
            || !resource_identifier(&config.mcp_audience)
            || config.api_audience == config.mcp_audience
            || !valid_label(&config.gateway_actor)
            || config.algorithms.is_empty()
            || config.subject_claim.as_deref().is_some_and(|claim| {
                !valid_label(claim)
                    || matches!(
                        claim,
                        "iss"
                            | "sub"
                            | "aud"
                            | "iat"
                            | "exp"
                            | "jti"
                            | "client_id"
                            | "scope"
                            | "act"
                            | "nonce"
                            | "azp"
                    )
            })
        {
            return Err(AuthError::Configuration);
        }
        let discovery = format!(
            "{}/.well-known/openid-configuration",
            config.issuer.trim_end_matches('/')
        );
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| AuthError::Configuration)?;
        let metadata: ProviderMetadata = bounded_json(
            client
                .get(discovery)
                .send()
                .await
                .map_err(|_| AuthError::Unavailable)?,
        )
        .await?;
        if metadata.issuer != config.issuer
            || !transport.permits(&metadata.jwks_uri)
            || !transport.permits(&metadata.authorization_endpoint)
            || !transport.permits(&metadata.token_endpoint)
        {
            return Err(AuthError::Configuration);
        }
        let keys = fetch_keys(&client, &metadata.jwks_uri).await?;
        let now = Instant::now();
        Ok(Self(Arc::new(Authority {
            config,
            client,
            metadata,
            keys: RwLock::new(CachedKeys {
                keys,
                expires: now + CACHE_TTL,
                refresh_after: now,
            }),
            refresh: Mutex::new(()),
        })))
    }

    /// Validate an audience-bound access token, including any gateway actor.
    ///
    /// # Errors
    /// Returns a safe credential, scope or provider-availability error.
    pub async fn verify(
        &self,
        bearer: &SecretString,
        audience: &Audience,
    ) -> Result<VerifiedIdentity, AuthError> {
        let expected = self.audience(*audience);
        let header = self.header(bearer)?;
        if !header.typ.as_deref().is_some_and(|typ| {
            typ.eq_ignore_ascii_case("at+jwt") || typ.eq_ignore_ascii_case("application/at+jwt")
        }) {
            return Err(AuthError::Credential);
        }
        let claims = self.claims(bearer, header, expected).await?;
        if !claims.aud.access_matches(expected) || !claims.jti.as_deref().is_some_and(valid_label) {
            return Err(AuthError::Credential);
        }
        let principal = self.principal(&claims)?;
        let client_id = claims
            .client_id
            .filter(|id| valid_label(id))
            .ok_or(AuthError::Credential)?;
        let scopes: BTreeSet<_> = claims
            .scope
            .as_deref()
            .unwrap_or_default()
            .split_ascii_whitespace()
            .filter_map(|scope| match scope {
                "telegram.read" => Some(Scope::Read),
                "telegram.send" => Some(Scope::Send),
                "telegram.link" => Some(Scope::Link),
                _ => None,
            })
            .collect();
        let kind = if let Some(actor) = claims.act {
            if !matches!(audience, Audience::Api)
                || actor.sub != self.0.config.gateway_actor
                || actor
                    .iss
                    .as_deref()
                    .is_some_and(|issuer| issuer != self.issuer())
                || client_id == actor.sub
                || claims.exp - claims.iat > MAX_DELEGATED_LIFETIME
            {
                return Err(AuthError::Credential);
            }
            if scopes.contains(&Scope::Link) {
                return Err(AuthError::Forbidden);
            }
            CredentialKind::Delegated
        } else {
            if client_id == self.0.config.gateway_actor {
                return Err(AuthError::Credential);
            }
            CredentialKind::Direct
        };
        Ok(VerifiedIdentity {
            principal,
            client_id,
            scopes,
            expires_at: claims.exp,
            kind,
        })
    }

    /// Exact configured issuer identifier.
    pub fn issuer(&self) -> &str {
        &self.0.config.issuer
    }

    /// Configured resource audience value.
    pub fn audience(&self, audience: Audience) -> &str {
        match audience {
            Audience::Api => &self.0.config.api_audience,
            Audience::Mcp => &self.0.config.mcp_audience,
        }
    }

    /// Signed actor identity expected for delegated API credentials.
    pub fn gateway_actor(&self) -> &str {
        &self.0.config.gateway_actor
    }

    #[cfg(feature = "service")]
    pub(crate) fn authorization_endpoint(&self) -> &Url {
        &self.0.metadata.authorization_endpoint
    }
    #[cfg(feature = "service")]
    pub(crate) fn token_endpoint(&self) -> &Url {
        &self.0.metadata.token_endpoint
    }
    #[cfg(feature = "service")]
    pub(crate) fn http_client(&self) -> &Client {
        &self.0.client
    }

    #[cfg(feature = "service")]
    pub(crate) async fn verify_id_token(
        &self,
        token: &SecretString,
        client_id: &str,
        nonce: &str,
    ) -> Result<(Principal, u64), AuthError> {
        use subtle::ConstantTimeEq as _;
        let header = self.header(token)?;
        if header
            .typ
            .as_deref()
            .is_some_and(|typ| !typ.eq_ignore_ascii_case("JWT"))
        {
            return Err(AuthError::Credential);
        }
        let claims = self.claims(token, header, client_id).await?;
        if claims.act.is_some()
            || !claims.aud.id_matches(client_id, claims.azp.as_deref())
            || !claims
                .nonce
                .as_deref()
                .is_some_and(|value| bool::from(value.as_bytes().ct_eq(nonce.as_bytes())))
        {
            return Err(AuthError::Credential);
        }
        Ok((self.principal(&claims)?, claims.exp))
    }

    fn header(&self, token: &SecretString) -> Result<Header, AuthError> {
        if token.expose_secret().len() > MAX_TOKEN_BYTES {
            return Err(AuthError::Credential);
        }
        let header = decode_header(token.expose_secret()).map_err(|_| AuthError::Credential)?;
        if header.crit.is_some()
            || header.jku.is_some()
            || header.jwk.is_some()
            || header.x5u.is_some()
            || !header.kid.as_deref().is_some_and(valid_label)
            || !self
                .0
                .config
                .algorithms
                .iter()
                .any(|algorithm| Algorithm::from(*algorithm) == header.alg)
        {
            return Err(AuthError::Credential);
        }
        Ok(header)
    }

    async fn claims(
        &self,
        token: &SecretString,
        header: Header,
        audience: &str,
    ) -> Result<Claims, AuthError> {
        let key = self
            .key(header.kid.as_deref().ok_or(AuthError::Credential)?)
            .await?;
        if key
            .algorithm
            .is_some_and(|algorithm| algorithm != header.alg)
        {
            return Err(AuthError::Credential);
        }
        let mut validation = Validation::new(header.alg);
        validation.leeway = 0;
        validation.validate_nbf = true;
        validation.set_required_spec_claims(&["iss", "sub", "aud", "exp"]);
        validation.set_issuer(&[self.issuer()]);
        validation.set_audience(&[audience]);
        let claims = decode::<Claims>(token.expose_secret(), &key.key, &validation)
            .map_err(|_| AuthError::Credential)?
            .claims;
        let now = unix_now()?;
        if claims.exp <= now
            || claims.exp <= claims.iat
            || claims.iat > now.saturating_add(30)
            || !valid_label(&claims.sub)
            || claims.iss != self.issuer()
        {
            return Err(AuthError::Credential);
        }
        Ok(claims)
    }

    fn principal(&self, claims: &Claims) -> Result<Principal, AuthError> {
        let subject = match self.0.config.subject_claim.as_deref() {
            Some(claim) => claims
                .extra
                .get(claim)
                .and_then(Value::as_str)
                .ok_or(AuthError::Credential)?,
            None => claims.sub.as_str(),
        };
        if !valid_label(subject) {
            return Err(AuthError::Credential);
        }
        Ok(Principal {
            issuer: self.issuer().to_owned(),
            subject: subject.to_owned(),
        })
    }

    async fn key(&self, kid: &str) -> Result<Arc<TrustedKey>, AuthError> {
        if let Some(result) = self.cached_key(kid).await {
            return result;
        }
        let _refresh = self.0.refresh.lock().await;
        if let Some(result) = self.cached_key(kid).await {
            return result;
        }
        self.0.keys.write().await.refresh_after = Instant::now() + REFRESH_INTERVAL;
        let keys = fetch_keys(&self.0.client, &self.0.metadata.jwks_uri).await?;
        let selected = keys.get(kid).cloned().ok_or(AuthError::Credential);
        let now = Instant::now();
        *self.0.keys.write().await = CachedKeys {
            keys,
            expires: now + CACHE_TTL,
            refresh_after: now + REFRESH_INTERVAL,
        };
        selected
    }

    async fn cached_key(&self, kid: &str) -> Option<Result<Arc<TrustedKey>, AuthError>> {
        let cached = self.0.keys.read().await;
        let now = Instant::now();
        if cached.expires > now
            && let Some(key) = cached.keys.get(kid)
        {
            return Some(Ok(Arc::clone(key)));
        }
        if now < cached.refresh_after {
            return Some(Err(if cached.expires > now {
                AuthError::Credential
            } else {
                AuthError::Unavailable
            }));
        }
        None
    }
}

fn resource_identifier(input: &str) -> bool {
    valid_label(input) && Url::parse(input).is_ok_and(|url| Transport::Https.permits(&url))
}

pub(crate) fn valid_label(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

pub(crate) fn unix_now() -> Result<u64, AuthError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| AuthError::Unavailable)
}

pub(crate) async fn bounded_json<T: DeserializeOwned>(
    mut response: Response,
) -> Result<T, AuthError> {
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > MAX_DOCUMENT_BYTES as u64)
    {
        return Err(AuthError::Unavailable);
    }
    let mut bytes = Vec::new();
    while let Some(part) = response.chunk().await.map_err(|_| AuthError::Unavailable)? {
        if part.len() > MAX_DOCUMENT_BYTES.saturating_sub(bytes.len()) {
            return Err(AuthError::Unavailable);
        }
        bytes.extend_from_slice(&part);
    }
    serde_json::from_slice(&bytes).map_err(|_| AuthError::Unavailable)
}

#[derive(Deserialize)]
struct KeyDocument {
    keys: Vec<Value>,
}

async fn fetch_keys(
    client: &Client,
    uri: &Url,
) -> Result<HashMap<String, Arc<TrustedKey>>, AuthError> {
    let document: KeyDocument = bounded_json(
        client
            .get(uri.clone())
            .send()
            .await
            .map_err(|_| AuthError::Unavailable)?,
    )
    .await?;
    if document.keys.len() > MAX_KEYS {
        return Err(AuthError::Unavailable);
    }
    let mut keys = HashMap::new();
    for raw in document.keys {
        let Some(kid) = raw
            .get("kid")
            .and_then(Value::as_str)
            .filter(|id| valid_label(id))
            .map(str::to_owned)
        else {
            continue;
        };
        let eligible = match raw.get("kty").and_then(Value::as_str) {
            Some("RSA") => true,
            Some("EC") => raw.get("crv").is_some_and(|value| value == "P-256"),
            Some("OKP") => raw.get("crv").is_some_and(|value| value == "Ed25519"),
            _ => false,
        } && raw.get("use").is_none_or(|usage| usage == "sig")
            && raw.get("key_ops").is_none_or(|ops| {
                ops.as_array()
                    .is_some_and(|ops| ops.iter().any(|op| op == "verify"))
            });
        if !eligible {
            continue;
        }
        let algorithm = match raw.get("alg") {
            Some(alg) => match serde_json::from_value(alg.clone()) {
                Ok(algorithm @ (Algorithm::RS256 | Algorithm::ES256 | Algorithm::EdDSA)) => {
                    Some(algorithm)
                }
                _ => continue,
            },
            None => None,
        };
        let jwk: Jwk = serde_json::from_value(raw).map_err(|_| AuthError::Unavailable)?;
        let key = DecodingKey::from_jwk(&jwk).map_err(|_| AuthError::Unavailable)?;
        if keys
            .insert(kid, Arc::new(TrustedKey { key, algorithm }))
            .is_some()
        {
            return Err(AuthError::Unavailable);
        }
    }
    if keys.is_empty() {
        return Err(AuthError::Unavailable);
    }
    Ok(keys)
}
