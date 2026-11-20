//! Offline Telegram source for loopback tests using real grants and account admission.

use super::*;
use crate::service::AccountGeneration;
use crate::service::registry::AccountRegistry;
use crate::service::storage::StorageError;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use telegram_core::config::RateLimitConfig;
use telegram_core::rate_limiter::RateLimiterTrait as _;
use telegram_core::telegram::types::{QueryMetadata, SearchResult};

pub(in crate::service) struct ReadFixture {
    registry: AccountRegistry<Arc<FixtureAccount>>,
    accounts: HashMap<AccountGeneration, Arc<FixtureAccount>>,
}

pub(in crate::service) struct FixtureAccount {
    pub(in crate::service) channels: HashMap<ChannelId, Vec<Message>>,
    pub(in crate::service) fetches: AtomicUsize,
    pub(in crate::service) history_timeout: bool,
}

pub(super) struct FixtureLease {
    lease: AccountLease<Arc<FixtureAccount>>,
    context: OperationContext,
}

impl ReadFixture {
    pub(in crate::service) fn new(
        rates: RateLimitConfig,
        accounts: HashMap<AccountGeneration, Arc<FixtureAccount>>,
    ) -> Self {
        Self {
            registry: AccountRegistry::new(4, Duration::from_secs(60), rates),
            accounts,
        }
    }

    pub(in crate::service) async fn warm(&self, key: &AccountGeneration) {
        let context = OperationContext::new(
            tokio::time::Instant::now() + Duration::from_secs(5),
            telegram_core::operations::CancellationToken::new(),
        );
        let lease = self
            .registry
            .lease(key.clone(), &context, async |_| {
                self.accounts.get(key).cloned().ok_or(StorageError::Missing)
            })
            .await
            .expect("initialize offline account");
        lease.limiter().refund(u32::MAX);
    }

    pub(super) async fn lease(&self, access: &ReadAccess) -> Result<FixtureLease, ApiError> {
        let lease = self
            .registry
            .lease(
                access.generation.clone(),
                &access.opening_context(),
                async |_| {
                    self.accounts
                        .get(&access.generation)
                        .cloned()
                        .ok_or(StorageError::Missing)
                },
            )
            .await
            .map_err(ApiError::from)?;
        let context = access.context(&lease);
        Ok(FixtureLease { lease, context })
    }
}

impl FixtureLease {
    async fn charge(&self, count: u32) -> Result<(), Error> {
        if self.context.cancellation().is_cancelled() {
            return Err(Error::Cancelled);
        }
        self.lease.limiter().acquire(count).await
    }

    fn rows(&self, id: ChannelId) -> Result<&[Message], Error> {
        let account = self.lease.client();
        let rows = account
            .channels
            .get(&id)
            .ok_or_else(|| Error::ChannelNotFound(id.to_string()))?;
        if rows
            .iter()
            .any(|row| row.peer_namespace != Some(PeerNamespace::Channel))
        {
            return Err(Error::ChannelNotFound(id.to_string()));
        }
        account.fetches.fetch_add(1, Ordering::Relaxed);
        Ok(rows)
    }

    pub(super) async fn search(&self, operation: SearchOperation) -> Result<SearchPage, Error> {
        let channels = match operation.scope {
            SearchScope::Channel(channel) => vec![channel],
            SearchScope::Channels(channels) => channels,
            SearchScope::Global => panic!("explicit channel scope required"),
        };
        self.charge(channels.len() as u32).await?;
        let mut messages = Vec::new();
        for channel in channels {
            let channel =
                ChannelId::new(channel.parse().expect("validated channel")).expect("positive");
            messages.extend(
                self.rows(channel)?
                    .iter()
                    .filter(|row| row.text.contains(&operation.query))
                    .cloned(),
            );
        }
        messages.sort_by_key(|row| Reverse(row.id.get()));
        let limit = operation.limit.min(SearchParams::MAX_LIMIT) as usize;
        let has_more = messages.len() > limit;
        messages.truncate(limit);
        Ok(SearchPage {
            result: SearchResult {
                returned: messages.len() as u64,
                has_more,
                search_time_ms: 0,
                query_metadata: QueryMetadata {
                    query: operation.query,
                    window_from: UtcTimestamp::from_unix_seconds(0).expect("epoch"),
                    window_to: operation.to_date,
                    channels_scanned: None,
                    channels_in_results: 0,
                    timed_out: false,
                    partial: false,
                    pages_fetched: 0,
                    messages_scanned: 0,
                },
                messages,
            },
            failures: Vec::new(),
        })
    }

    pub(super) async fn history(&self, range: HistoryIdRange) -> Result<HistoryIdPage, Error> {
        self.charge(1).await?;
        let mut messages: Vec<_> = self
            .rows(range.channel_id)?
            .iter()
            .filter(|row| {
                let id = row.id.get();
                range.after_id.is_none_or(|after| id > after.get())
                    && range.before_id.is_none_or(|before| id < before.get())
                    && id <= range.through_id.get()
            })
            .cloned()
            .collect();
        messages.sort_by_key(|row| Reverse(row.id.get()));
        let limit = range.limit.min(100) as usize;
        let has_more = messages.len() > limit;
        messages.truncate(limit);
        let timed_out = self.lease.client().history_timeout;
        Ok(HistoryIdPage {
            next_before_id: if has_more || timed_out {
                messages.last().map(|message| message.id)
            } else {
                None
            },
            messages,
            partial: timed_out,
            timed_out,
        })
    }

    pub(super) async fn batch(
        &self,
        channel: ChannelId,
        ids: Vec<MessageId>,
    ) -> Result<MessageBatch, Error> {
        self.charge(1).await?;
        let rows = self.rows(channel)?;
        let mut messages = Vec::new();
        let mut missing_ids = Vec::new();
        for id in ids {
            if let Some(message) = rows.iter().find(|row| row.id == id) {
                messages.push(message.clone());
            } else {
                missing_ids.push(id.get());
            }
        }
        Ok(MessageBatch {
            channel_id: channel,
            peer_namespace: PeerNamespace::Channel,
            messages,
            missing_ids,
        })
    }
}
