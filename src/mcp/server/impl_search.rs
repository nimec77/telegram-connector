//! `McpServer` inherent `*_impl` methods: Search & history retrieval tools.
//!
//! These hold the real tool logic; the `#[tool]` wrappers in `server.rs`
//! delegate to them. Split out per LM-3 (`server.rs` was 880 lines).

use super::McpServer;
use crate::error::Error;
use crate::link::{ChannelRef, parse_telegram_link};
use crate::mcp::tools::validation::{parse_cursor_bounds, parse_optional_utc};
use crate::mcp::tools::{
    GetMessageByLinkRequest, GetRecentMessagesRequest, MessageResponse, ResponseFormat,
    SearchRequest, SearchResponse, fanout, shaping,
};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::{HistoryParams, SearchParams};
use telegram_core::operations::{
    CancellationToken, HistoryOperation, HistoryScope, SearchOperation, SearchScope,
};

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
        let operations = self.account_operations();
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

    pub(super) async fn get_recent_messages_data(
        &self,
        request: GetRecentMessagesRequest,
        budget_mode: shaping::BudgetMode,
        cancellation: CancellationToken,
    ) -> Result<SearchResponse<Error>, Error> {
        let scope = match (request.channel_id, request.channel_ids) {
            (Some(_), Some(_)) => {
                return Err(Error::InvalidInput(
                    "supply either channel_id or channel_ids, not both".into(),
                ));
            }
            (Some(channel), None) => HistoryScope::Channel(channel),
            (None, Some(channels)) => HistoryScope::Channels(channels),
            (None, None) => {
                return Err(Error::InvalidInput(
                    "channel_id (or channel_ids) is required".into(),
                ));
            }
        };
        let multi = matches!(scope, HistoryScope::Channels(_));
        let (before_id, after_id) = parse_cursor_bounds(request.before_id, request.after_id)?;
        let max_text_length =
            crate::mcp::tools::validation::resolve_max_text_length(request.max_text_length)?;
        let operation = HistoryOperation {
            scope,
            hours_back: request
                .hours_back
                .unwrap_or(HistoryParams::DEFAULT_HOURS_BACK),
            limit: request.limit.unwrap_or(HistoryParams::DEFAULT_LIMIT),
            media_filter: request.media_filter,
            from_date: parse_optional_utc("from_date", &request.from_date)?,
            to_date: parse_optional_utc("to_date", &request.to_date)?,
            collapse_albums: request.collapse_albums.unwrap_or(true),
            before_id,
            after_id,
        };
        let operations = self.account_operations();
        let context = operations.history_context(cancellation)?;
        let mut response = fanout::wire_page(operations.recent(operation, &context).await?);
        let (cursor, compact) = if multi {
            (
                shaping::CursorPolicy::Unavailable,
                shaping::CompactScope::Multi,
            )
        } else {
            (
                shaping::CursorPolicy::SingleChannel,
                shaping::CompactScope::Single,
            )
        };
        shaping::shape_response(
            &mut response,
            request.format.unwrap_or_default(),
            max_text_length,
            cursor,
            self.response_byte_budget,
            compact,
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

        let operations = self.account_operations();
        let context = operations.history_context(crate::mcp::runtime::operation_cancellation())?;
        let message = operations
            .message_by_reference(&channel_identifier, message_id, &context)
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
