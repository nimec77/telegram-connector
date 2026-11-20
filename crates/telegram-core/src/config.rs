//! Settings shared by Telegram account operations.

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::path::PathBuf;

#[doc(hidden)]
pub mod defaults;

/// Account token-bucket capacity, refill speed and weighted operation costs.
#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    /// Charge for an admitted outgoing text operation; never refunded.
    #[serde(default = "default_send_message_cost")]
    pub send_message_cost: u32,
    /// Charge for an admitted media operation, independent of upload part count.
    #[serde(default = "default_send_media_cost")]
    pub send_media_cost: u32,
    /// Initial and maximum token capacity; validation requires at least one token.
    /// Builders do not validate this automatically.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Tokens restored per elapsed second on acquisition or snapshot.
    /// Validation requires a finite, positive value.
    #[serde(default = "default_refill_rate")]
    pub refill_rate: f64,
    /// Tokens charged per get_message_media call (searches cost 1).
    #[serde(default = "default_media_download_cost")]
    pub media_download_cost: u32,
    /// Tokens charged per transcribe_voice_message call (Telegram's weekly
    /// transcription quota makes these calls precious; searches cost 1).
    #[serde(default = "default_transcription_cost")]
    pub transcription_cost: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            send_message_cost: default_send_message_cost(),
            send_media_cost: default_send_media_cost(),
            max_tokens: default_max_tokens(),
            refill_rate: default_refill_rate(),
            media_download_cost: default_media_download_cost(),
            transcription_cost: default_transcription_cost(),
        }
    }
}

impl RateLimitConfig {
    /// Validate new send costs when writes are enabled, preserving disabled read policies.
    ///
    /// # Errors
    /// Rejects zero costs and costs larger than the account bucket.
    pub fn validate_sending_costs(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=self.max_tokens).contains(&self.send_message_cost)
                && (1..=self.max_tokens).contains(&self.send_media_cost),
            "invalid sending account costs"
        );
        Ok(())
    }
}

impl RateLimitConfig {
    /// Require usable capacity and refill speed, and satisfiable read costs.
    ///
    /// Capacity must be at least one token because ordinary reads cost one.
    /// Refill must be finite and positive. Media and transcription costs may
    /// be zero, but cannot exceed capacity. Enabled-send costs are checked
    /// separately by [`Self::validate_sending_costs`].
    ///
    /// # Errors
    /// Returns an error for zero capacity, a nonfinite/nonpositive refill rate,
    /// or a media/transcription cost exceeding bucket capacity.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.max_tokens == 0 {
            anyhow::bail!("rate_limiting.max_tokens must be >= 1: ordinary reads cost one token");
        }
        if !self.refill_rate.is_finite() || self.refill_rate <= 0.0 {
            anyhow::bail!(
                "rate_limiting.refill_rate must be finite and > 0 (got {})",
                self.refill_rate
            );
        }
        if self.media_download_cost > self.max_tokens {
            anyhow::bail!(
                "rate_limiting.media_download_cost ({}) exceeds max_tokens ({}), \
                 so every media call would fail even on a full bucket",
                self.media_download_cost,
                self.max_tokens
            );
        }
        if self.transcription_cost > self.max_tokens {
            anyhow::bail!(
                "rate_limiting.transcription_cost ({}) exceeds max_tokens ({}), \
                 so every transcription call would fail even on a full bucket",
                self.transcription_cost,
                self.max_tokens
            );
        }
        Ok(())
    }
}

fn default_max_tokens() -> u32 {
    60
}

fn default_refill_rate() -> f64 {
    2.0
}

fn default_media_download_cost() -> u32 {
    3
}

fn default_transcription_cost() -> u32 {
    5
}

fn default_send_message_cost() -> u32 {
    1
}

fn default_send_media_cost() -> u32 {
    3
}

/// MTProto connection/session settings; credentials are required only for setup.
#[derive(Debug, Clone, Deserialize)]
pub struct TelegramConfig {
    /// API ID from <https://my.telegram.org> (always required for connection)
    pub api_id: i32,
    /// API hash from <https://my.telegram.org> (required only for --setup)
    #[serde(default, deserialize_with = "deserialize_optional_secret_string")]
    pub api_hash: Option<SecretString>,
    /// Phone number for authentication (required only for --setup)
    #[serde(default, deserialize_with = "deserialize_optional_secret_string")]
    pub phone_number: Option<SecretString>,
    /// Session file path (always used)
    #[serde(default = "defaults::default_session_file")]
    pub session_file: PathBuf,
    /// Per-call-type timeout budgets for grammers network operations.
    #[serde(default = "defaults::default_timeout_config")]
    pub timeouts: TimeoutConfig,
    /// Hard cap (bytes) on a single media download; transfers exceeding it are
    /// rejected before and during streaming. Default 20 MiB. Pairs with
    /// `[rate_limiting] media_download_cost`, which prices the download (AD-6).
    #[serde(default = "defaults::default_max_download_bytes")]
    /// Accepted range is 1..=80 MiB; four process-wide raw slots share 80 MiB.
    pub max_download_bytes: u64,
}

/// Timeout budgets (seconds) applied to grammers calls in [`crate::telegram::client`].
///
/// Call categories have separate budgets. Multi-iteration walks
/// (`iter_messages.next()`, `search_iter.next()`) measure *total* elapsed time across all
/// iterations, not per-iteration.
#[derive(Debug, Clone, Deserialize)]
pub struct TimeoutConfig {
    /// Total text-send budget, starting before local admission.
    #[serde(default = "defaults::default_send_secs")]
    pub send_secs: u64,
    /// Total media-send budget including staging, upload and final acknowledgement.
    #[serde(default = "defaults::default_upload_send_secs")]
    pub upload_send_secs: u64,
    /// Resolve/dialog lookups, account checks and authentication RPCs. Interactive
    /// code/password entry is not timed; each login RPC gets this budget.
    #[serde(default = "defaults::default_resolve_secs")]
    pub resolve_secs: u64,
    /// History/message fetches, subscribed-channel enumeration and transcription RPCs.
    #[serde(default = "defaults::default_history_secs")]
    pub history_secs: u64,
    /// Search walks (single-channel `search_iter`, global `search_all_messages`).
    #[serde(default = "defaults::default_search_secs")]
    pub search_secs: u64,
    /// Wall-clock budget for a full in-memory media download.
    #[serde(default = "defaults::default_download_secs")]
    pub download_secs: u64,
}

impl TelegramConfig {
    /// Check whether nonempty authentication credentials are present (api_hash, phone_number).
    /// Note: api_id is always required for connection, not just setup
    pub fn has_auth_credentials(&self) -> bool {
        self.auth_credentials().is_some()
    }

    /// Authentication credentials as `(api_hash, phone_number)`, or `None`
    /// when either is absent or empty.
    ///
    /// This is the single definition of "credentials are present";
    /// [`Self::has_auth_credentials`] is its predicate form. The returned strings
    /// expose secret material to the caller; do not log or include them in errors.
    pub fn auth_credentials(&self) -> Option<(&str, &str)> {
        fn non_empty(secret: &SecretString) -> Option<&str> {
            Some(secret.expose_secret()).filter(|value| !value.is_empty())
        }
        Some((
            self.api_hash.as_ref().and_then(non_empty)?,
            self.phone_number.as_ref().and_then(non_empty)?,
        ))
    }
}

// Helper function for deserializing optional SecretString
fn deserialize_optional_secret_string<'de, D>(
    deserializer: D,
) -> Result<Option<SecretString>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt
        .filter(|s| !s.is_empty())
        .map(|s| SecretString::new(s.into_boxed_str())))
}

/// Cooperative search accumulation deadline, in addition to network timeouts.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    /// Wall-clock budget for a search's accumulation loop. On expiry the
    /// search returns the results gathered so far with `timed_out`/`partial`
    /// set — never an error. Must stay below `[telegram.timeouts] search_secs`
    /// (default 120) to be reachable.
    #[serde(default = "defaults::default_search_deadline_seconds")]
    pub deadline_seconds: u64,
}

impl TimeoutConfig {
    /// Reject zero values — a zero budget would cancel every call instantly.
    ///
    /// # Errors
    /// Returns an error when any resolution, history, search, or download budget is zero.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.resolve_secs == 0 {
            anyhow::bail!("telegram.timeouts.resolve_secs must be > 0");
        }
        if self.history_secs == 0 {
            anyhow::bail!("telegram.timeouts.history_secs must be > 0");
        }
        if self.search_secs == 0 {
            anyhow::bail!("telegram.timeouts.search_secs must be > 0");
        }
        if self.download_secs == 0 {
            anyhow::bail!("telegram.timeouts.download_secs must be > 0");
        }
        for seconds in [self.send_secs, self.upload_send_secs] {
            crate::telegram::sending::SendDeadline::new(seconds).map_err(|_| {
                anyhow::anyhow!("sending timeouts must be nonzero and fit the clock")
            })?;
        }
        Ok(())
    }
}

/// Upper bound accepted for `[search] deadline_seconds`.
///
/// `SearchBudget::new` computes `Instant::now() + Duration::from_secs(deadline_secs)`,
/// which panics on overflow in every build profile. This cap keeps a pathological
/// TOML value from turning every search into a panic. It is deliberately permissive:
/// a useful value sits well below `[telegram.timeouts] search_secs` (default 120),
/// which is the actual practical ceiling.
const MAX_SEARCH_DEADLINE_SECONDS: u64 = 3600;

impl SearchConfig {
    /// Reject a zero deadline (would end every search before its first page) or a
    /// deadline above `MAX_SEARCH_DEADLINE_SECONDS` (would overflow the `Instant`
    /// arithmetic in `SearchBudget::new`).
    ///
    /// # Errors
    /// Returns an error when the search deadline is outside 1..=3600 seconds.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.deadline_seconds == 0 {
            anyhow::bail!("search.deadline_seconds must be > 0");
        }
        if self.deadline_seconds > MAX_SEARCH_DEADLINE_SECONDS {
            anyhow::bail!(
                "search.deadline_seconds must be between 1 and {MAX_SEARCH_DEADLINE_SECONDS}, \
                 got {}",
                self.deadline_seconds
            );
        }
        Ok(())
    }
}

impl TelegramConfig {
    /// Require one permitted download to fit the process-wide raw-input allowance.
    ///
    /// # Errors
    /// Rejects zero bytes or a cap above 80 MiB; input reservations share that allowance.
    pub fn validate_media_limits(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=crate::telegram::media_pipeline::RAW_MEDIA_MEMORY_BYTES as u64)
                .contains(&self.max_download_bytes),
            "telegram.max_download_bytes must be 1..=83886080 to fit global raw-memory admission"
        );
        Ok(())
    }
}
