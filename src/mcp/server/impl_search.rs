//! `McpServer` inherent `*_impl` methods: Search & history retrieval tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::McpServer;
use crate::link::{ChannelRef, parse_telegram_link};
use crate::mcp::tools::helpers::{parse_channel_reference, parse_cursor_bounds, wire_message_id};
use crate::mcp::tools::{
    GetMessageByLinkRequest, GetRecentMessagesRequest, MessageResponse, ResponseFormat,
    SearchRequest, SearchResponse, fanout, json_response, parse_optional_utc, shaping,
    validate_date_window,
};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::{ChannelId, HistoryParams, SearchParams, SearchResult};
use chrono::{DateTime, Utc};
use std::future::Future;

/// The per-call inputs of a fan-out beyond the per-channel fetch itself:
/// what to merge to and how to shape the merged page.
struct FanoutPage {
    limit: u32,
    query: String,
    window_from: DateTime<Utc>,
    window_to: Option<DateTime<Utc>>,
    format: ResponseFormat,
    max_text_length: u32,
}

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn search_messages_impl(
        &self,
        request: SearchRequest,
    ) -> Result<String, String> {
        // Validate: query required unless media_filter is set
        if request.query.trim().is_empty() && request.media_filter.is_none() {
            return Err(
                "Search query cannot be empty (unless media_filter is set to filter by media type)"
                    .to_string(),
            );
        }

        // `Some(list)` = fan-out; `None` covers both the single-channel_id path
        // and, when channel_id is also absent, the global-search path (A6).
        let scope = fanout::validate_channel_scope(&request.channel_id, &request.channel_ids)?;
        let channel_scoped = request.channel_id.is_some() || scope.is_some();

        if !channel_scoped && (request.before_id.is_some() || request.after_id.is_some()) {
            return Err(
                "before_id/after_id require channel_id: cursor pagination is per-channel; \
                 global search cannot page by message id"
                    .to_string(),
            );
        }

        let format = request.format.unwrap_or_default();
        if !channel_scoped && format == ResponseFormat::Compact {
            return Err(
                "format=compact requires channel_id or channel_ids: the compact header \
                 describes channel scope; use full format for global search"
                    .to_string(),
            );
        }

        // Apply defaults and limits
        let hours_back = request
            .hours_back
            .unwrap_or(SearchParams::DEFAULT_HOURS_BACK)
            .min(SearchParams::MAX_HOURS_BACK);

        let limit = request
            .limit
            .unwrap_or(SearchParams::DEFAULT_LIMIT)
            .min(SearchParams::MAX_LIMIT);

        // Validate limit is greater than 0
        if limit == 0 {
            return Err("Search limit must be greater than 0".to_string());
        }

        // Parse the optional date range
        let from_date = parse_optional_utc("from_date", &request.from_date)?;
        let to_date = parse_optional_utc("to_date", &request.to_date)?;

        let (before_id, after_id) = parse_cursor_bounds(request.before_id, request.after_id)?;
        let max_text_length = shaping::resolve_max_text_length(request.max_text_length)?;

        // Shared parameter template; the channel target (channel_id) is filled
        // in per-path below: once for the single-channel/global path, once per
        // entry for the fan-out path.
        let params_template = SearchParams {
            query: request.query.clone(),
            channel_id: None,
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

        if let Some(list) = scope {
            let fetch = |reference: String| {
                let base = params_template.clone(); // SearchParams minus channel_id
                async move {
                    let channel_id = self.search_channel_id(&reference).await?;
                    let params = SearchParams {
                        channel_id: Some(channel_id),
                        ..base
                    };
                    self.telegram_client
                        .search_messages(&params)
                        .await
                        .map_err(|e| e.to_string())
                }
            };
            let page = FanoutPage {
                limit,
                query: request.query.clone(),
                window_from,
                window_to: to_date,
                format,
                max_text_length,
            };
            return self
                .run_fanout(list, before_id.is_some() || after_id.is_some(), page, fetch)
                .await;
        }

        // Single-channel/global path: 1 token per search, acquired before the
        // username resolve below so that RPC is metered like the fan-out
        // path's per-channel resolve — never free work.
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        // Numeric refs parse locally; a username ref spends one resolve RPC
        // before the search itself (§1.3).
        let channel_id = match &request.channel_id {
            Some(reference) => Some(self.search_channel_id(reference).await?),
            None => None,
        };

        // Build search params
        let params = SearchParams {
            channel_id,
            ..params_template
        };

        // Execute search
        let result = self
            .telegram_client
            .search_messages(&params)
            .await
            .map_err(|e| e.to_string())?;

        // Log search results (IDs only, not message text - for privacy and log size)
        let message_ids: Vec<i64> = result.messages.iter().map(|m| m.id.get()).collect();
        tracing::info!(
            query = %params.query,
            channel_id = ?params.channel_id.map(|c| c.get()),
            media_filter = ?params.media_filter,
            hours_back = params.hours_back,
            limit = params.limit,
            returned = result.returned,
            messages_returned = message_ids.len(),
            message_ids = ?message_ids,
            search_time_ms = result.search_time_ms,
            channels_in_results = result.query_metadata.channels_in_results,
            "Search results"
        );

        let cursor_eligible = params.channel_id.is_some();
        let mut response = SearchResponse::from(result);
        shaping::shape_response(
            &mut response,
            format,
            max_text_length,
            cursor_eligible,
            self.response_byte_budget,
            shaping::CompactScope::Single,
        )?;
        json_response(&response)
    }

    /// Numeric refs parse locally; username refs spend one resolve RPC (§1.3).
    async fn search_channel_id(&self, reference: &str) -> Result<ChannelId, String> {
        match parse_channel_reference(reference)? {
            ChannelRef::Id(channel_id) => Ok(channel_id),
            ChannelRef::Username(username) => self
                .telegram_client
                .resolve_channel_identity(&username)
                .await
                .map(|identity| identity.id)
                .map_err(|e| e.to_string()),
        }
    }

    /// Fan-out tail shared by `search_messages` and `get_recent_messages`:
    /// one atomic acquire for the deduped channel count (so the D5 deficit
    /// message stays accurate), bounded-concurrency fetch, newest-first
    /// merge, multi-channel shaping.
    ///
    /// Tokens are charged per channel *attempted* and never refunded for
    /// channels that end in `channel_errors`. A failed channel still spent a
    /// resolve and/or fetch RPC against Telegram — the same resolve+fetch
    /// shape of work `get_messages_batch` charges `acquire(1)` for — and a
    /// refund would let a caller hammer an unresolvable channel at zero
    /// cost, exactly the flood behaviour the limiter exists to prevent. The
    /// media batch's per-id refund is a different case: its ids share one
    /// fetch RPC, so a per-id charge is pessimistic; per-channel RPCs here
    /// are not.
    async fn run_fanout<F, Fut>(
        &self,
        list: Vec<String>,
        has_cursor: bool,
        page: FanoutPage,
        fetch: F,
    ) -> Result<String, String>
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Result<SearchResult, String>>,
    {
        if has_cursor {
            return Err(
                "before_id/after_id require a single channel_id: cursor pagination is \
                 per-channel"
                    .to_string(),
            );
        }

        self.rate_limiter
            .acquire(list.len() as u32)
            .await
            .map_err(|e| e.to_string())?;

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
            /* cursor_eligible */ false,
            self.response_byte_budget,
            shaping::CompactScope::Multi,
        )?;
        json_response(&response)
    }

    pub(super) async fn get_recent_messages_impl(
        &self,
        request: GetRecentMessagesRequest,
    ) -> Result<String, String> {
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
            return Err("Limit must be greater than 0".to_string());
        }

        // Parse the optional date range
        let from_date = parse_optional_utc("from_date", &request.from_date)?;
        let to_date = parse_optional_utc("to_date", &request.to_date)?;

        let (before_id, after_id) = parse_cursor_bounds(request.before_id, request.after_id)?;
        let max_text_length = shaping::resolve_max_text_length(request.max_text_length)?;

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

        let channels = fanout::validate_channel_scope(&request.channel_id, &request.channel_ids)?;
        // channels: Option<Vec<String>> — Some(list) means fan-out.
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
                    self.telegram_client
                        .get_recent_messages(&params)
                        .await
                        .map_err(|e| e.to_string())
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
                .run_fanout(list, before_id.is_some() || after_id.is_some(), page, fetch)
                .await;
        }

        // Single-channel path. The client owns resolution; the server no longer
        // pre-resolves usernames via get_channel_info (AD-2) — that second
        // resolve was redundant with the client's own resolve_username_peer.
        let channel_id_str = request
            .channel_id
            .ok_or_else(|| "channel_id (or channel_ids) is required".to_string())?;
        let (channel_id, channel_identifier) = history_target(&channel_id_str)?;

        // Build history params
        let params = HistoryParams {
            channel_id,
            channel_identifier,
            ..params_template
        };

        // Acquire rate limiter tokens (1 token per request)
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        // Execute history retrieval
        let result = self
            .telegram_client
            .get_recent_messages(&params)
            .await
            .map_err(|e| e.to_string())?;

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

        let cursor_eligible = true; // get_recent_messages: always single-channel
        let mut response = SearchResponse::from(result);
        shaping::shape_response(
            &mut response,
            format,
            max_text_length,
            cursor_eligible,
            self.response_byte_budget,
            shaping::CompactScope::Single,
        )?;
        json_response(&response)
    }

    pub(super) async fn get_message_by_link_impl(
        &self,
        request: GetMessageByLinkRequest,
    ) -> Result<String, String> {
        // Parse the link
        let (channel_ref, message_id) =
            parse_telegram_link(&request.link).map_err(|e| e.to_string())?;

        // Convert ChannelRef to string identifier for the trait method
        let channel_identifier = match &channel_ref {
            ChannelRef::Username(username) => username.clone(),
            ChannelRef::Id(id) => id.get().to_string(),
        };

        let wire_id = wire_message_id(message_id)?;

        // Acquire rate limiter token
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        // Fetch the message
        let message = self
            .telegram_client
            .get_message_by_id(&channel_identifier, wire_id)
            .await
            .map_err(|e| e.to_string())?;

        tracing::info!(
            link = %request.link,
            channel = %channel_identifier,
            message_id = message_id.get(),
            "Message by link results"
        );

        json_response(&MessageResponse::from(message))
    }
}

/// Split a channel reference into HistoryParams' (channel_id, channel_identifier) pair.
fn history_target(reference: &str) -> Result<(Option<ChannelId>, Option<String>), String> {
    Ok(match parse_channel_reference(reference)? {
        ChannelRef::Id(channel_id) => (Some(channel_id), None),
        ChannelRef::Username(username) => (None, Some(username)),
    })
}
