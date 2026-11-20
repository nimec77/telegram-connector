//! Charged recent-message pages with per-channel boundaries.

use super::validation::{parse_channel_reference, validate_date_window, wire_message_id};
use super::{AccountOperations, BoundedOperation, OperationContext, SearchPage, fanout};
use crate::UtcTimestamp;
use crate::error::Error;
use crate::link::ChannelRef;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::types::{
    ChannelId, HistoryParams, MediaFilter, Message, MessageId, SearchResult,
};
use crate::telegram::{TelegramClient, TelegramClientTrait};

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

/// Explicit ID bounds for one newest-first history page, without a time window.
#[derive(Debug, Clone, Copy)]
pub struct HistoryIdRange {
    /// Channel whose message IDs define the range.
    pub channel_id: ChannelId,
    /// Exclusive lower bound, if older message IDs should be excluded.
    pub after_id: Option<MessageId>,
    /// Inclusive upper bound.
    pub through_id: MessageId,
    /// Exclusive upper bound for a continuation page.
    pub before_id: Option<MessageId>,
    /// Maximum number of individual messages, capped at the history page limit.
    pub limit: u32,
}

/// One newest-first history page with individual album members.
#[derive(Debug, Default)]
pub struct HistoryIdPage {
    /// Messages available within the requested ID interval.
    pub messages: Vec<Message>,
    /// Exclusive continuation from the last returned message if more may remain.
    pub next_before_id: Option<MessageId>,
    /// Whether expiry cut the traversal short after at least one message.
    pub partial: bool,
    /// Whether the request deadline expired during a partial traversal.
    pub timed_out: bool,
}

pub(crate) struct HistoryIdBounds {
    pub(crate) after_id: i32,
    pub(crate) through_id: i32,
    pub(crate) upper_offset: i32,
    pub(crate) limit: usize,
}

impl HistoryIdRange {
    /// Check local ID and limit constraints before access checks or charging.
    ///
    /// # Errors
    /// Returns invalid input for an out-of-range Telegram message ID or a zero limit.
    pub fn validate(&self) -> Result<(), Error> {
        self.wire_bounds().map(|_| ())
    }

    /// Whether no positive message ID can satisfy these bounds.
    pub fn is_empty(&self) -> bool {
        let after = self.after_id.map_or(0, |id| id.get());
        after >= self.through_id.get() || self.before_id.is_some_and(|id| id.get() - 1 <= after)
    }

    pub(crate) fn wire_bounds(&self) -> Result<HistoryIdBounds, Error> {
        if self.limit == 0 {
            return Err(Error::InvalidInput("Limit must be greater than 0".into()));
        }
        let after_id = self.after_id.map(wire_message_id).transpose()?.unwrap_or(0);
        let through_id = wire_message_id(self.through_id)?;
        let before_id = self.before_id.map(wire_message_id).transpose()?;
        let upper_offset = [before_id, through_id.checked_add(1)]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(0);
        Ok(HistoryIdBounds {
            after_id,
            through_id,
            upper_offset,
            limit: self.limit.min(HistoryParams::MAX_LIMIT) as usize,
        })
    }
}

impl<R: RateLimiterTrait> AccountOperations<TelegramClient, R> {
    /// Fetch a joined broadcast channel or supergroup's history by ID.
    ///
    /// The caller admits the requested channel to its local access scope before calling.
    /// Each nonempty attempt charges one token before any RPC. Empty ranges make no
    /// Telegram calls. An expired traversal returns any collected rows and a cursor
    /// for the remaining range.
    ///
    /// # Errors
    /// Returns invalid input, an admission or Telegram failure, cancellation, or a
    /// timeout before a partial result is available.
    pub async fn history_range(
        &self,
        range: HistoryIdRange,
        context: &OperationContext,
    ) -> Result<HistoryIdPage, Error> {
        let timing = context.bound(self.limits.history_timeout, "history_range");
        let bounds = range.wire_bounds()?;
        if range.is_empty() {
            timing.check()?;
            return Ok(HistoryIdPage::default());
        }
        timing.run(self.limiter.acquire(1)).await?;
        self.client
            .read_history_id_range(range.channel_id, bounds, &timing)
            .await
    }
}
