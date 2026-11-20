//! Charged account metadata, discovery, link and transcription operations.

use super::{AccountOperations, OperationContext};
use crate::error::Error;
use crate::link::MessageLink;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::transcription::TranscriptionDeadline;
use crate::telegram::types::{
    Channel, ChannelId, ChannelIdentity, ChannelPage, ChannelResolution, ChannelStats, Message,
    MessageId, PeerNamespace, TranscriptionOutcome,
};
use crate::telegram::{TelegramClient, TelegramClientTrait};
use std::future::Future;
use std::time::Duration;

impl<T: TelegramClientTrait, R: RateLimiterTrait> AccountOperations<T, R> {
    async fn channel_read<V>(
        &self,
        context: &OperationContext,
        timeout: Duration,
        name: &'static str,
        read: impl Future<Output = Result<V, Error>>,
    ) -> Result<V, Error> {
        let timing = context.bound(timeout, name);
        timing.run(self.limiter.acquire(1)).await?;
        timing.run(read).await
    }

    /// Read a page from the account's subscribed dialogs.
    ///
    /// # Errors
    /// Returns a validation, account, deadline or upstream failure.
    pub async fn subscribed_channels(
        &self,
        limit: u32,
        offset: u32,
        context: &OperationContext,
    ) -> Result<ChannelPage, Error> {
        if !(1..=500).contains(&limit) {
            return Err(Error::InvalidInput(
                "limit must be between 1 and 500".into(),
            ));
        }
        self.channel_read(
            context,
            self.limits.history_timeout,
            "list_channels",
            self.client.get_subscribed_channels(limit, offset),
        )
        .await
    }

    /// Resolve a local channel reference and return its basic or full metadata.
    ///
    /// # Errors
    /// Returns a validation, account, deadline or upstream failure.
    pub async fn channel_info_by_reference(
        &self,
        reference: &str,
        full: bool,
        context: &OperationContext,
    ) -> Result<Channel, Error> {
        validate_reference(reference)?;
        self.channel_read(
            context,
            self.limits.history_timeout,
            "channel_info",
            async {
                if full {
                    self.client.get_full_channel_info(reference).await
                } else {
                    self.client.get_channel_info(reference).await
                }
            },
        )
        .await
    }

    /// Resolve a bounded list of local IDs, usernames or titles.
    ///
    /// # Errors
    /// Returns a validation, account or shared lookup failure. Item failures remain in the list.
    pub async fn resolve_channels(
        &self,
        identifiers: &[String],
        context: &OperationContext,
    ) -> Result<Vec<ChannelResolution<Error>>, Error> {
        const MAX_IDENTIFIERS: usize = 20;
        if identifiers.is_empty() {
            return Err(Error::InvalidInput(
                "identifiers must contain at least one entry".into(),
            ));
        }
        if identifiers.len() > MAX_IDENTIFIERS {
            return Err(Error::InvalidInput(format!(
                "identifiers accepts at most {MAX_IDENTIFIERS} entries per call, got {}",
                identifiers.len()
            )));
        }
        if identifiers.iter().any(|item| item.trim().is_empty()) {
            return Err(Error::InvalidInput(
                "identifiers must not contain blank entries".into(),
            ));
        }
        self.channel_read(
            context,
            self.limits.history_timeout,
            "resolve_channels",
            self.client.resolve_channels(identifiers),
        )
        .await
    }

    /// Read channel posting statistics by a local numeric or username reference.
    ///
    /// # Errors
    /// Returns a validation, account, deadline or upstream failure.
    pub async fn channel_stats_by_reference(
        &self,
        reference: &str,
        days: u32,
        context: &OperationContext,
    ) -> Result<ChannelStats, Error> {
        if reference.trim().is_empty() {
            return Err(Error::InvalidInput("channel_id is required".into()));
        }
        if days == 0 {
            return Err(Error::InvalidInput(
                "days_back must be greater than 0".into(),
            ));
        }
        validate_reference(reference)?;
        self.channel_read(
            context,
            self.limits.history_timeout,
            "channel_stats",
            self.client
                .get_channel_stats(reference, days.min(ChannelStats::MAX_DAYS_BACK)),
        )
        .await
    }

    /// Resolve the current channel identity and construct its message links.
    ///
    /// # Errors
    /// Returns a malformed reference, account, deadline or upstream failure.
    pub async fn message_link_by_reference(
        &self,
        reference: &str,
        id: MessageId,
        context: &OperationContext,
    ) -> Result<(ChannelIdentity, MessageLink), Error> {
        validate_reference(reference)?;
        let identity = self
            .channel_read(
                context,
                self.limits.history_timeout,
                "message_link",
                self.client.resolve_channel_identity(reference),
            )
            .await?;
        let link = MessageLink::new(identity.id, id, identity.username.as_deref());
        Ok((identity, link))
    }

    /// Read one message by a local channel reference and its channel-local ID.
    ///
    /// # Errors
    /// Returns validation, account, deadline, missing-message or upstream failure.
    pub async fn message_by_reference(
        &self,
        reference: &str,
        id: MessageId,
        context: &OperationContext,
    ) -> Result<Message, Error> {
        validate_reference(reference)?;
        let id = super::validation::wire_message_id(id)?;
        self.channel_read(
            context,
            self.limits.history_timeout,
            "message",
            self.client.get_message_by_id(reference, id),
        )
        .await
    }

    /// Read public directory matches for a local account operation.
    ///
    /// # Errors
    /// Returns a blank query, account, deadline or upstream failure.
    pub async fn public_channels(
        &self,
        query: &str,
        limit: u32,
        context: &OperationContext,
    ) -> Result<Vec<Channel>, Error> {
        if query.trim().is_empty() {
            return Err(Error::InvalidInput("Search query cannot be empty".into()));
        }
        self.channel_read(
            context,
            self.limits.search_timeout,
            "discover_channels",
            self.client.search_public_channels(query, limit),
        )
        .await
    }

    /// Transcribe voice or video-note audio selected by a local channel reference.
    ///
    /// # Errors
    /// Returns validation, deadline, admission, Premium or upstream failure.
    pub async fn transcribe_by_reference(
        &self,
        reference: &str,
        id: MessageId,
        deadline: TranscriptionDeadline,
        cost: u32,
        context: &OperationContext,
        prepared: impl Future<Output = ()> + Send,
    ) -> Result<TranscriptionOutcome, Error> {
        validate_reference(reference)?;
        let id = super::validation::wire_message_id(id)?;
        self.transcribe_with(deadline, cost, context, prepared, |deadline| {
            self.client.transcribe_audio(reference, id, deadline)
        })
        .await
    }

    async fn transcribe_with<F>(
        &self,
        deadline: TranscriptionDeadline,
        cost: u32,
        context: &OperationContext,
        prepared: impl Future<Output = ()>,
        fetch: impl FnOnce(TranscriptionDeadline) -> F,
    ) -> Result<TranscriptionOutcome, Error>
    where
        F: Future<Output = Result<TranscriptionOutcome, Error>>,
    {
        let deadline = deadline.bounded_by(context.deadline());
        let work = async {
            let cached = deadline
                .run(async { Ok(self.client.cached_account_status().await) })
                .await?;
            if cached.premium == Some(false) {
                return Err(Error::PremiumRequired);
            }
            deadline.run(self.limiter.acquire(cost)).await?;
            if cached.premium.is_none() {
                let status = deadline.run(self.client.refresh_account_status()).await?;
                if !status.authorized {
                    return Err(Error::Auth("Telegram account is not authenticated".into()));
                }
                if status.premium == Some(false) {
                    return Err(Error::PremiumRequired);
                }
            }
            deadline
                .run(async {
                    prepared.await;
                    Ok(())
                })
                .await?;
            fetch(deadline).await
        };
        tokio::select! {
            biased;
            _ = context.cancellation().cancelled() => Err(Error::Cancelled),
            result = work => result,
        }
    }

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
        let mut channels = self.public_channels(query, limit, context).await?;
        channels.retain(|channel| channel.peer_namespace == Some(PeerNamespace::Channel));
        Ok(channels)
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
        self.channel_read(
            context,
            self.limits.history_timeout,
            "channel_info",
            async {
                let joined = self.client.require_joined_channel(id).await?;
                self.client.read_joined_channel_info(&joined, full).await
            },
        )
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
        self.channel_read(
            context,
            self.limits.history_timeout,
            "message_link",
            async {
                let joined = self.client.require_joined_channel(channel).await?;
                let identity = joined.identity()?;
                Ok(MessageLink::new(
                    identity.id,
                    id,
                    identity.username.as_deref(),
                ))
            },
        )
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
        self.channel_read(
            context,
            self.limits.history_timeout,
            "channel_stats",
            async {
                let joined = self.client.require_joined_channel(channel).await?;
                self.client.read_joined_channel_stats(&joined, days).await
            },
        )
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
        let deadline = TranscriptionDeadline::new(timeout_seconds);
        self.transcribe_with(
            deadline,
            cost,
            context,
            std::future::ready(()),
            |deadline| async move {
                let joined = deadline
                    .run(self.client.require_joined_channel(channel))
                    .await?;
                self.client
                    .transcribe_joined_channel(&joined, id, deadline)
                    .await
            },
        )
        .await
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
        self.channel_read(
            context,
            self.limits.history_timeout,
            "list_channels",
            async {
                self.client
                    .get_joined_channels_filtered(limit, offset, permitted)
                    .await
            },
        )
        .await
    }
}

fn validate_reference(reference: &str) -> Result<(), Error> {
    super::validation::parse_channel_reference(reference)
        .map(|_| ())
        .map_err(Error::InvalidInput)
}
