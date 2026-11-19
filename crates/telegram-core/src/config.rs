//! Settings shared by Telegram account operations.

use serde::Deserialize;

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
