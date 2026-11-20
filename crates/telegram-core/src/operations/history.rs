//! Charged recent-message pages with per-channel boundaries.

use super::validation::{parse_channel_reference, validate_date_window};
use super::{AccountOperations, BoundedOperation, OperationContext, SearchPage, fanout};
use crate::UtcTimestamp;
use crate::error::Error;
use crate::link::ChannelRef;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::{HistoryParams, MediaFilter, MessageId, SearchResult};

/// Explicit channel scope for a history page.
#[derive(Debug, Clone)]
pub enum HistoryScope {
    /// One channel with optional per-channel cursors.
    Channel(String),
    /// Bounded set of channels merged in newest-first order.
    Channels(Vec<String>),
}

/// Domain input for reading recent messages within a time or cursor window.
#[derive(Debug, Clone)]
pub struct HistoryOperation {
    /// One channel or a bounded fan-out.
    pub scope: HistoryScope,
    /// Relative window with no explicit start.
    pub hours_back: u32,
    /// Number of result posts requested.
    pub limit: u32,
    /// Optional attachment kind.
    pub media_filter: Option<MediaFilter>,
    /// Inclusive wall-clock start.
    pub from_date: Option<UtcTimestamp>,
    /// Inclusive wall-clock end.
    pub to_date: Option<UtcTimestamp>,
    /// Group Telegram album siblings into one result.
    pub collapse_albums: bool,
    /// Exclusive older-page cursor for a single channel.
    pub before_id: Option<MessageId>,
    /// Exclusive newer-page cursor for a single channel.
    pub after_id: Option<MessageId>,
}

impl<T: TelegramClientTrait, R: RateLimiterTrait> AccountOperations<T, R> {
    /// Read a recent page after validating and charging its distinct attempts.
    ///
    /// # Errors
    /// Returns invalid input, admission failure, cancellation or a whole-operation failure.
    pub async fn recent(
        &self,
        request: HistoryOperation,
        context: &OperationContext,
    ) -> Result<SearchPage, Error> {
        let timing = context.bound(self.limits.history_timeout, "get_recent_messages");
        if request.limit == 0 {
            return Err(Error::InvalidInput("Limit must be greater than 0".into()));
        }
        let params = HistoryParams {
            channel_id: None,
            channel_identifier: None,
            hours_back: request.hours_back.min(HistoryParams::MAX_HOURS_BACK),
            limit: request.limit.min(HistoryParams::MAX_LIMIT),
            media_filter: request.media_filter,
            from_date: request.from_date,
            to_date: request.to_date,
            collapse_albums: request.collapse_albums,
            before_id: request.before_id,
            after_id: request.after_id,
        };
        let window_from = params.window_start();
        validate_date_window(
            params.from_date,
            params.to_date,
            window_from,
            params.hours_back,
        )
        .map_err(Error::InvalidInput)?;
        match request.scope {
            HistoryScope::Channel(reference) => {
                let target = parse_channel_reference(&reference).map_err(Error::InvalidInput)?;
                super::validation::validate_cursors(params.before_id, params.after_id)?;
                timing.run(self.limiter.acquire(1)).await?;
                let result = self.history_target(target, params, &timing).await?;
                Ok(SearchPage {
                    result,
                    failures: Vec::new(),
                })
            }
            HistoryScope::Channels(references) => {
                let channels =
                    fanout::normalize_channels(&references).map_err(Error::InvalidInput)?;
                if params.before_id.is_some() || params.after_id.is_some() {
                    return Err(Error::InvalidInput(
                        "before_id/after_id require a single channel_id: cursor pagination is per-channel".into(),
                    ));
                }
                timing
                    .run(self.limiter.acquire(channels.len() as u32))
                    .await?;
                let outcomes = fanout::run(channels, |reference| {
                    let params = params.clone();
                    let timing = &timing;
                    async move {
                        let target =
                            parse_channel_reference(&reference).map_err(Error::InvalidInput)?;
                        self.history_target(target, params, timing).await
                    }
                })
                .await;
                fanout::finish(
                    &timing,
                    outcomes,
                    params.limit as usize,
                    String::new(),
                    window_from,
                    params.to_date,
                )
            }
        }
    }

    async fn history_target(
        &self,
        reference: ChannelRef,
        mut params: HistoryParams,
        timing: &BoundedOperation<'_>,
    ) -> Result<SearchResult, Error> {
        match reference {
            ChannelRef::Id(id) => params.channel_id = Some(id),
            ChannelRef::Username(name) => params.channel_identifier = Some(name),
        }
        timing.run(self.client.get_recent_messages(&params)).await
    }
}
