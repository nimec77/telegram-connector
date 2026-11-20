//! `McpServer` inherent `*_impl` methods: Search & history retrieval tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::McpServer;
use crate::UtcTimestamp;
use crate::error::Error;
use crate::link::{ChannelRef, parse_telegram_link};
use crate::mcp::tools::validation::{
    parse_channel_reference, parse_cursor_bounds, parse_optional_utc, validate_date_window,
    wire_message_id,
};
use crate::mcp::tools::{
    GetMessageByLinkRequest, GetRecentMessagesRequest, MessageResponse, ResponseFormat,
    SearchRequest, SearchResponse, fanout, shaping,
};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::{ChannelId, HistoryParams, SearchParams, SearchResult};
use std::future::Future;
use telegram_core::operations::{
    AccountOperations, CancellationToken, SearchOperation, SearchScope,
};

/// The per-call inputs of a fan-out beyond the per-channel fetch itself:
/// what to merge to and how to shape the merged page.
struct FanoutPage {
    limit: u32,
    query: String,
    window_from: UtcTimestamp,
    window_to: Option<UtcTimestamp>,
    format: ResponseFormat,
    max_text_length: u32,
}

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn search_messages_data(
        &self,
        request: SearchRequest,
        budget_mode: shaping::BudgetMode,
        cancellation: CancellationToken,
    ) -> Result<SearchResponse<Error>, Error> {
        let scope = match (request.channel_id, request.channel_ids) {
            (Some(_), Some(_)) => {
                return Err(Error::InvalidInput(
                    "supply either channel_id or channel_ids, not both".into(),
                ));
            }
            (Some(channel), None) => SearchScope::Channel(channel),
            (None, Some(channels)) => SearchScope::Channels(channels),
            (None, None) => SearchScope::Global,
        };
        let format = request.format.unwrap_or_default();
        if matches!(scope, SearchScope::Global) && format == ResponseFormat::Compact {
            return Err(Error::InvalidInput(
                "format=compact requires channel_id or channel_ids: the compact header \
                 describes channel scope; use full format for global search"
                    .into(),
            ));
        }
        let cursor_policy = if matches!(scope, SearchScope::Channel(_)) {
            shaping::CursorPolicy::SingleChannel
        } else {
            shaping::CursorPolicy::Unavailable
        };
        let compact_scope = if matches!(scope, SearchScope::Channels(_)) {
            shaping::CompactScope::Multi
        } else {
            shaping::CompactScope::Single
        };
        let (before_id, after_id) = parse_cursor_bounds(request.before_id, request.after_id)?;
        let max_text_length =
            crate::mcp::tools::validation::resolve_max_text_length(request.max_text_length)?;
        let operation = SearchOperation {
            scope,
            query: request.query,
            hours_back: request
                .hours_back
                .unwrap_or(SearchParams::DEFAULT_HOURS_BACK),
            limit: request.limit.unwrap_or(SearchParams::DEFAULT_LIMIT),
            media_filter: request.media_filter,
            from_date: parse_optional_utc("from_date", &request.from_date)?,
            to_date: parse_optional_utc("to_date", &request.to_date)?,
            collapse_albums: request.collapse_albums.unwrap_or(true),
            before_id,
            after_id,
        };
        let operations = AccountOperations::new(
            std::sync::Arc::clone(&self.telegram_client),
            std::sync::Arc::clone(&self.rate_limiter),
            self.account_limits,
        );
        let context = operations.search_context(cancellation)?;
        let mut response = fanout::wire_page(operations.search(operation, &context).await?);
        shaping::shape_response(
            &mut response,
            format,
            max_text_length,
            cursor_policy,
            self.response_byte_budget,
            compact_scope,
            budget_mode,
        )?;
        Ok(response)
    }

    /// Fan-out tail shared by `search_messages` and `get_recent_messages`:
    /// one atomic acquire for the deduped channel count (so the D5 deficit
    /// message stays accurate), bounded-concurrency fetch, newest-first
    /// merge, multi-channel shaping.
    ///
    /// Tokens are charged per channel *attempted* and never refunded for
    /// channels that end in `channel_errors`. A failed channel typically
    /// spent a resolve and/or fetch RPC against Telegram — the same
    /// resolve+fetch shape of work `get_messages_batch` charges `acquire(1)`
    /// for — and a refund would let a caller hammer an unresolvable channel
    /// at zero cost, exactly the flood behaviour the limiter exists to
    /// prevent. Locally malformed entries are rejected before this acquisition. The
    /// media batch's per-id refund is a different case: its ids share one
    /// fetch RPC, so a per-id charge is pessimistic; per-channel RPCs here
    /// are not.
    async fn run_fanout<F, Fut>(
        &self,
        list: Vec<fanout::FanoutChannel>,
        has_cursor: bool,
        page: FanoutPage,
        fetch: F,
        budget_mode: shaping::BudgetMode,
    ) -> Result<SearchResponse<Error>, Error>
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Result<SearchResult, Error>>,
    {
        if has_cursor {
            return Err(Error::InvalidInput(
                "before_id/after_id require a single channel_id: cursor pagination is \
                 per-channel"
                    .to_string(),
            ));
        }

        self.rate_limiter.acquire(list.len() as u32).await?;

        let outcomes = fanout::run(list, fetch).await;

        let mut response = fanout::merge_results(
            outcomes,
            page.limit as usize,
            page.query,
            page.window_from,
            page.window_to,
        )?;
        shaping::shape_response(
            &mut response,
            page.format,
            page.max_text_length,
            shaping::CursorPolicy::Unavailable,
            self.response_byte_budget,
            shaping::CompactScope::Multi,
            budget_mode,
        )?;
        Ok(response)
    }

    pub(super) async fn get_recent_messages_data(
        &self,
        request: GetRecentMessagesRequest,
        budget_mode: shaping::BudgetMode,
    ) -> Result<SearchResponse<Error>, Error> {
        // Apply defaults and limits
        let hours_back = request
            .hours_back
            .unwrap_or(HistoryParams::DEFAULT_HOURS_BACK)
            .min(HistoryParams::MAX_HOURS_BACK);

        let limit = request
            .limit
            .unwrap_or(HistoryParams::DEFAULT_LIMIT)
            .min(HistoryParams::MAX_LIMIT);

        // Validate limit is greater than 0
        if limit == 0 {
            return Err(Error::InvalidInput(
                "Limit must be greater than 0".to_string(),
            ));
        }

        // Parse the optional date range
        let from_date = parse_optional_utc("from_date", &request.from_date)?;
        let to_date = parse_optional_utc("to_date", &request.to_date)?;

        let (before_id, after_id) = parse_cursor_bounds(request.before_id, request.after_id)?;
        let max_text_length =
            crate::mcp::tools::validation::resolve_max_text_length(request.max_text_length)?;

        let format = request.format.unwrap_or_default();

        // Shared parameter template; the channel target (channel_id/channel_identifier)
        // is filled in per-path below: once for the single-channel path, once per
        // entry for the fan-out path.
        let params_template = HistoryParams {
            channel_id: None,
            channel_identifier: None,
            hours_back,
            limit,
            media_filter: request.media_filter,
            from_date,
            to_date,
            collapse_albums: request.collapse_albums.unwrap_or(true),
            before_id,
            after_id,
        };

        // Reject an empty window before spending a token or a network round-trip.
        let window_from = params_template.window_start();
        validate_date_window(
            params_template.from_date,
            params_template.to_date,
            window_from,
            params_template.hours_back,
        )?;

        let channels = crate::mcp::tools::validation::validate_channel_scope(
            &request.channel_id,
            &request.channel_ids,
        )?;
        // A validated, locally deduplicated list means fan-out.
        if let Some(list) = channels {
            let fetch = |reference: String| {
                let base = params_template.clone(); // HistoryParams minus target
                async move {
                    let (channel_id, channel_identifier) = history_target(&reference)?;
                    let params = HistoryParams {
                        channel_id,
                        channel_identifier,
                        ..base
                    };
                    self.telegram_client.get_recent_messages(&params).await
                }
            };
            let page = FanoutPage {
                limit,
                query: String::new(),
                window_from,
                window_to: to_date,
                format,
                max_text_length,
            };
            return self
                .run_fanout(
                    list,
                    before_id.is_some() || after_id.is_some(),
                    page,
                    fetch,
                    budget_mode,
                )
                .await;
        }

        // Single-channel path. The client owns resolution; the server no longer
        // pre-resolves usernames via get_channel_info (AD-2) — that second
        // resolve was redundant with the client's own resolve_username_peer.
        let channel_id_str = request.channel_id.ok_or_else(|| {
            Error::InvalidInput("channel_id (or channel_ids) is required".to_string())
        })?;
        let (channel_id, channel_identifier) = history_target(&channel_id_str)?;

        // Build history params
        let params = HistoryParams {
            channel_id,
            channel_identifier,
            ..params_template
        };

        // Acquire rate limiter tokens (1 token per request)
        self.rate_limiter.acquire(1).await?;

        // Execute history retrieval
        let result = self.telegram_client.get_recent_messages(&params).await?;

        // Log results (IDs only, not message text - for privacy and log size)
        let message_ids: Vec<i64> = result.messages.iter().map(|m| m.id.get()).collect();
        tracing::info!(
            channel_id = ?params.channel_id.map(|c| c.get()),
            media_filter = ?params.media_filter,
            hours_back = params.hours_back,
            limit = params.limit,
            returned = result.returned,
            messages_returned = message_ids.len(),
            message_ids = ?message_ids,
            search_time_ms = result.search_time_ms,
            "Recent messages results"
        );

        let mut response = SearchResponse::<Error>::from_result(result);
        shaping::shape_response(
            &mut response,
            format,
            max_text_length,
            shaping::CursorPolicy::SingleChannel,
            self.response_byte_budget,
            shaping::CompactScope::Single,
            budget_mode,
        )?;
        Ok(response)
    }

    pub(super) async fn get_message_by_link_data(
        &self,
        request: GetMessageByLinkRequest,
    ) -> Result<MessageResponse, Error> {
        // Parse the link
        let (channel_ref, message_id) = parse_telegram_link(&request.link)?;

        // Convert ChannelRef to string identifier for the trait method
        let channel_identifier = match &channel_ref {
            ChannelRef::Username(username) => username.clone(),
            ChannelRef::Id(id) => id.get().to_string(),
        };

        let wire_id = wire_message_id(message_id)?;

        // Acquire rate limiter token
        self.rate_limiter.acquire(1).await?;

        // Fetch the message
        let message = self
            .telegram_client
            .get_message_by_id(&channel_identifier, wire_id)
            .await?;

        tracing::info!(
            link = %request.link,
            channel = %channel_identifier,
            message_id = message_id.get(),
            "Message by link results"
        );

        Ok(MessageResponse::from(message))
    }
}

/// Split a channel reference into HistoryParams' (channel_id, channel_identifier) pair.
fn history_target(reference: &str) -> Result<(Option<ChannelId>, Option<String>), Error> {
    Ok(match parse_channel_reference(reference)? {
        ChannelRef::Id(channel_id) => (Some(channel_id), None),
        ChannelRef::Username(username) => (None, Some(username)),
    })
}
