//! Charged account metadata, discovery, link and transcription operations.

use super::{AccountOperations, OperationContext};
use crate::error::Error;
use crate::link::MessageLink;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::transcription::TranscriptionDeadline;
use crate::telegram::types::{
    Channel, ChannelId, ChannelPage, ChannelStats, MessageId, PeerNamespace, TranscriptionOutcome,
};
use crate::telegram::{TelegramClient, TelegramClientTrait};

impl<T: TelegramClientTrait, R: RateLimiterTrait> AccountOperations<T, R> {
    /// Query the public directory. Embedders supply the disclosure policy.
    ///
    /// # Errors
    /// Rejects empty queries and returns account or bounded upstream failures.
    pub async fn discover_channels(
        &self,
        query: &str,
        limit: u32,
        context: &OperationContext,
    ) -> Result<Vec<Channel>, Error> {
        if query.trim().is_empty() || query.len() > 256 || !(1..=100).contains(&limit) {
            return Err(Error::InvalidInput(
                "invalid channel discovery query".into(),
            ));
        }
        context
            .bound(self.limits.search_timeout, "discover_channels")
            .run(async {
                self.limiter.acquire(1).await?;
                let mut channels = self.client.search_public_channels(query, limit).await?;
                channels.retain(|channel| channel.peer_namespace == Some(PeerNamespace::Channel));
                Ok(channels)
            })
            .await
    }
}

impl<R: RateLimiterTrait> AccountOperations<TelegramClient, R> {
    /// Look up metadata for a current numeric channel after one account charge.
    ///
    /// # Errors
    /// Returns a bounded account, Telegram or authorization failure.
    pub async fn channel_info(
        &self,
        id: ChannelId,
        full: bool,
        context: &OperationContext,
    ) -> Result<Channel, Error> {
        let timing = context.bound(self.limits.history_timeout, "channel_info");
        timing
            .run(async {
                self.limiter.acquire(1).await?;
                let joined = self.client.require_joined_channel(id).await?;
                self.client.read_joined_channel_info(&joined, full).await
            })
            .await
    }

    /// Build a link from the latest resolved channel identity.
    ///
    /// # Errors
    /// Returns validation, eligibility, deadline and account failures.
    pub async fn message_link(
        &self,
        channel: ChannelId,
        id: MessageId,
        context: &OperationContext,
    ) -> Result<MessageLink, Error> {
        if id.as_i32().is_none() {
            return Err(Error::InvalidInput(
                "message ID is outside the Telegram range".into(),
            ));
        }
        context
            .bound(self.limits.history_timeout, "message_link")
            .run(async {
                self.limiter.acquire(1).await?;
                let joined = self.client.require_joined_channel(channel).await?;
                let identity = joined.identity()?;
                Ok(MessageLink::new(
                    identity.id,
                    id,
                    identity.username.as_deref(),
                ))
            })
            .await
    }

    /// Sample recent channel posting statistics.
    ///
    /// # Errors
    /// Returns a safe validation, account or bounded upstream failure.
    pub async fn channel_stats(
        &self,
        channel: ChannelId,
        days: u32,
        context: &OperationContext,
    ) -> Result<ChannelStats, Error> {
        if !(1..=ChannelStats::MAX_DAYS_BACK).contains(&days) {
            return Err(Error::InvalidInput(
                "invalid channel statistics window".into(),
            ));
        }
        context
            .bound(self.limits.history_timeout, "channel_stats")
            .run(async {
                self.limiter.acquire(1).await?;
                let joined = self.client.require_joined_channel(channel).await?;
                self.client.read_joined_channel_stats(&joined, days).await
            })
            .await
    }

    /// Transcribe one voice or video note under the caller's shared deadline.
    ///
    /// # Errors
    /// Returns invalid-media, Premium, account and bounded upstream failures.
    pub async fn transcribe(
        &self,
        channel: ChannelId,
        id: MessageId,
        timeout_seconds: u32,
        cost: u32,
        context: &OperationContext,
    ) -> Result<TranscriptionOutcome, Error> {
        let id = id
            .as_i32()
            .ok_or_else(|| Error::InvalidInput("invalid message ID".into()))?;
        if timeout_seconds == 0 {
            return Err(Error::InvalidInput("invalid transcription deadline".into()));
        }
        let deadline = TranscriptionDeadline::new(timeout_seconds).bounded_by(context.deadline());
        let work = async {
            let cached = self.client.cached_account_status().await;
            if cached.premium == Some(false) {
                return Err(Error::PremiumRequired);
            }
            deadline.run(self.limiter.acquire(cost)).await?;
            if cached.premium.is_none() {
                let status = deadline.run(self.client.refresh_account_status()).await?;
                if !status.authorized {
                    return Err(Error::Auth("Account authorization required".into()));
                }
                if status.premium == Some(false) {
                    return Err(Error::PremiumRequired);
                }
            }
            let joined = deadline
                .run(self.client.require_joined_channel(channel))
                .await?;
            self.client
                .transcribe_joined_channel(&joined, id, deadline)
                .await
        };
        tokio::select! {
            biased;
            _ = context.cancellation().cancelled() => Err(Error::Cancelled),
            result = work => result,
        }
    }

    /// Enumerate a permitted slice of currently joined channels. Pagination and
    /// totals operate on the filtered subset.
    ///
    /// # Errors
    /// Returns a bounded admission or upstream dialog traversal failure.
    pub async fn list_channels(
        &self,
        limit: u32,
        offset: u32,
        permitted: impl Fn(&Channel) -> bool + Send,
        context: &OperationContext,
    ) -> Result<ChannelPage, Error> {
        context
            .bound(self.limits.history_timeout, "list_channels")
            .run(async {
                self.limiter.acquire(1).await?;
                self.client
                    .get_joined_channels_filtered(limit, offset, permitted)
                    .await
            })
            .await
    }
}
