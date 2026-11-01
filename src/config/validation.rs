//! Operational bounds and setup credential validation.

use super::{Config, LimitsConfig, RateLimitConfig, SearchConfig, ServerConfig, TimeoutConfig};

impl ServerConfig {
    /// Reject unbounded input/admission and unreasonable drain budgets.
    ///
    /// # Errors
    /// Returns an error when request bytes or in-flight admission are outside their
    /// supported ranges, or the shutdown budget exceeds one hour.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=16_777_216).contains(&self.max_request_bytes),
            "server.max_request_bytes must be 1..=16777216"
        );
        anyhow::ensure!(
            (1..=1024).contains(&self.max_in_flight_requests),
            "server.max_in_flight_requests must be 1..=1024"
        );
        anyhow::ensure!(
            self.shutdown_timeout_seconds <= 3600,
            "server.shutdown_timeout_seconds must be <= 3600"
        );
        Ok(())
    }
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

/// Floor below which an image would be downscaled past usefulness. Once a
/// batch's remaining budget drops under this, `Base64Budget::allowance`
/// returns `None` rather than emitting an unreadable thumbnail — which is why
/// `limits.media_batch_max_total_bytes` is validated against it here.
pub(crate) const MIN_IMAGE_BASE64_BYTES: usize = 32_768;

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

impl RateLimitConfig {
    /// Reject a bucket that can never refill or a per-call cost it can never
    /// satisfy — configurations that are not merely tight but unsatisfiable.
    ///
    /// A `refill_rate` of zero (or negative, or NaN) is a permanent lockout:
    /// the bucket drains once and never recovers, and the rejection's retry
    /// hint is `ceil(deficit / 0)`, which saturates to `u64::MAX` seconds.
    /// A cost above `max_tokens` means every call of that kind fails even on
    /// a full bucket; the check also keeps per-call costs proportionate to
    /// the bucket's capacity.
    ///
    /// # Errors
    /// Returns an error for a NaN/non-positive refill rate or a media/transcription
    /// cost exceeding bucket capacity.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.refill_rate.is_nan() || self.refill_rate <= 0.0 {
            anyhow::bail!(
                "rate_limiting.refill_rate must be > 0 (got {}): the bucket would never \
                 refill, so every call would fail once it is drained",
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

impl LimitsConfig {
    /// Reject a zero budget — it would make every response over-cap. Also
    /// reject a cap below `MIN_IMAGE_BASE64_BYTES`: `Base64Budget::allowance`
    /// returns `None` once the remaining budget drops under that floor, so a
    /// cap set below it makes every image in every batch call report
    /// `payload_cap_reached` after the downloads already happened — a silent,
    /// costly no-op at runtime. Failing fast at startup is cheaper.
    ///
    /// # Errors
    /// Returns an error for a zero response budget or a media batch budget below
    /// the minimum useful image allowance.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.response_byte_budget == 0 {
            anyhow::bail!("limits.response_byte_budget must be > 0");
        }
        if self.media_batch_max_total_bytes == 0 {
            anyhow::bail!("limits.media_batch_max_total_bytes must be > 0");
        }
        if self.media_batch_max_total_bytes < MIN_IMAGE_BASE64_BYTES as u64 {
            anyhow::bail!(
                "limits.media_batch_max_total_bytes must be >= {MIN_IMAGE_BASE64_BYTES} bytes \
                 (below that floor every image reports payload_cap_reached), got {}",
                self.media_batch_max_total_bytes
            );
        }
        Ok(())
    }
}

impl Config {
    /// Validate that authentication credentials are present (required for --setup mode)
    /// Only checks the presence of nonempty api_hash and phone_number; it does
    /// not authenticate, validate API ID ranges, or contact Telegram.
    ///
    /// # Errors
    /// Returns a secret-free description of required fields when either is absent.
    pub fn validate_for_setup(&self) -> anyhow::Result<()> {
        if !self.telegram.has_auth_credentials() {
            anyhow::bail!(
                "Authentication credentials required for setup mode.\n\
                Please ensure these are set in your config.toml:\n\
                - telegram.api_hash\n\
                - telegram.phone_number\n\n\
                You can use environment variables: api_hash = \"${{YOUR_ENV_VAR}}\"\n\
                Get your API credentials from: https://my.telegram.org"
            );
        }
        Ok(())
    }
}

impl super::TelegramConfig {
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
