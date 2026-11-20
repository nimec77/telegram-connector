//! Validation, admission and execution for a bounded message search.

use super::validation::{parse_channel_reference, validate_date_window};
use super::{AccountOperations, BoundedOperation, OperationContext, fanout};
use crate::UtcTimestamp;
use crate::error::Error;
use crate::link::ChannelRef;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::{MediaFilter, MessageId, SearchParams, SearchResult};

/// Which dialogs a search may visit.
#[derive(Debug, Clone)]
pub enum SearchScope {
    /// Search the whole account.
    Global,
    /// Search one local channel reference.
    Channel(String),
    /// Search a bounded set of local references, deduplicated before admission.
    Channels(Vec<String>),
}

/// Validated domain inputs are derived from this operation before any charge.
#[derive(Debug, Clone)]
pub struct SearchOperation {
    /// Allowed scope for this operation.
    pub scope: SearchScope,
    /// Text query, optionally empty when a media filter is present.
    pub query: String,
    /// Relative window when no explicit lower bound is supplied.
    pub hours_back: u32,
    /// Maximum number of result posts.
    pub limit: u32,
    /// Optional attachment type filter.
    pub media_filter: Option<MediaFilter>,
    /// Inclusive lower wall-clock boundary.
    pub from_date: Option<UtcTimestamp>,
    /// Inclusive upper wall-clock boundary.
    pub to_date: Option<UtcTimestamp>,
    /// Whether album siblings form a single post.
    pub collapse_albums: bool,
    /// Exclusive older-page cursor for a single channel.
    pub before_id: Option<MessageId>,
    /// Exclusive newer-page cursor for a single channel.
    pub after_id: Option<MessageId>,
}

/// Ordered results and typed per-channel failures before transport trimming.
pub struct SearchPage {
    /// Domain rows, search metrics and pagination metadata.
    pub result: SearchResult,
    /// Failures attributed to the first caller spelling of each channel.
    pub failures: Vec<(String, Error)>,
}

impl<T: TelegramClientTrait, R: RateLimiterTrait> AccountOperations<T, R> {
    /// Validate inputs, charge once for each distinct attempt and run the search.
    ///
    /// # Errors
    /// Returns invalid input, failed admission, cancellation or a whole-operation failure.
    /// Successful fan-out rows retain their deadline and per-channel failure metadata.
    pub async fn search(
        &self,
        operation: SearchOperation,
        context: &OperationContext,
    ) -> Result<SearchPage, Error> {
        let timing = context.bound(self.limits.search_timeout, "search_messages");
        if operation.query.trim().is_empty() && operation.media_filter.is_none() {
            return Err(Error::InvalidInput(
                "Search query cannot be empty (unless media_filter is set to filter by media type)"
                    .into(),
            ));
        }
        if operation.limit == 0 {
            return Err(Error::InvalidInput(
                "Search limit must be greater than 0".into(),
            ));
        }

        let has_cursor = operation.before_id.is_some() || operation.after_id.is_some();
        if has_cursor {
            match operation.scope {
                SearchScope::Global => return Err(Error::InvalidInput(
                    "before_id/after_id require channel_id: cursor pagination is per-channel; global search cannot page by message id".into(),
                )),
                SearchScope::Channels(_) => return Err(Error::InvalidInput(
                    "before_id/after_id require a single channel_id: cursor pagination is per-channel".into(),
                )),
                SearchScope::Channel(_) => {}
            }
        }
        super::validation::validate_cursors(operation.before_id, operation.after_id)?;

        let params = SearchParams {
            channel_namespace: None,
            channel_id: None,
            query: operation.query,
            hours_back: operation.hours_back.min(SearchParams::MAX_HOURS_BACK),
            limit: operation.limit.min(SearchParams::MAX_LIMIT),
            media_filter: operation.media_filter,
            from_date: operation.from_date,
            to_date: operation.to_date,
            collapse_albums: operation.collapse_albums,
            before_id: operation.before_id,
            after_id: operation.after_id,
        };
        let window_from = params.window_start();
        validate_date_window(
            params.from_date,
            params.to_date,
            window_from,
            params.hours_back,
        )
        .map_err(Error::InvalidInput)?;

        let reference = match operation.scope {
            SearchScope::Global => None,
            SearchScope::Channel(reference) => {
                Some(parse_channel_reference(&reference).map_err(Error::InvalidInput)?)
            }
            SearchScope::Channels(references) => {
                let channels =
                    fanout::normalize_channels(&references).map_err(Error::InvalidInput)?;
                timing
                    .run(self.limiter.acquire(channels.len() as u32))
                    .await?;
                let outcomes = fanout::run(channels, |reference| {
                    let params = params.clone();
                    let timing = &timing;
                    async move {
                        let reference =
                            parse_channel_reference(&reference).map_err(Error::InvalidInput)?;
                        self.search_target(Some(reference), params, timing).await
                    }
                })
                .await;
                return fanout::finish(
                    &timing,
                    outcomes,
                    params.limit as usize,
                    params.query,
                    window_from,
                    params.to_date,
                );
            }
        };
        timing.run(self.limiter.acquire(1)).await?;
        let result = self.search_target(reference, params, &timing).await?;
        Ok(SearchPage {
            result,
            failures: Vec::new(),
        })
    }

    async fn search_target(
        &self,
        reference: Option<ChannelRef>,
        mut params: SearchParams,
        timing: &BoundedOperation<'_>,
    ) -> Result<SearchResult, Error> {
        match reference {
            Some(ChannelRef::Id(id)) => params.channel_id = Some(id),
            Some(ChannelRef::Username(username)) => {
                let identity = timing
                    .run(self.client.resolve_channel_identity(&username))
                    .await?;
                params.channel_id = Some(identity.id);
                params.channel_namespace = Some(identity.peer_namespace);
            }
            None => {}
        }
        timing.run(self.client.search_messages(&params)).await
    }
}
