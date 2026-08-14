use crate::config::ObservabilityConfig;
use crate::config::defaults::{
    default_media_batch_max_total_bytes, default_media_download_cost, default_response_byte_budget,
    default_transcription_cost, default_transcription_default_timeout,
    default_transcription_max_timeout,
};
use crate::error::Error;
use crate::link::{ChannelRef, MessageLink, parse_telegram_link};
use crate::mcp::observability::{InstrumentedTransport, ResponseBuffer, SessionMetrics};
use crate::mcp::tools::fanout;
use crate::mcp::tools::shaping;
use crate::mcp::tools::{
    BufferedResponseEntry, ChannelsResponse, GenerateLinkRequest, GetChannelInfoRequest,
    GetChannelStatsRequest, GetChannelsRequest, GetLastResponsesRequest, GetMessageByLinkRequest,
    GetMessageMediaRequest, GetMessageMediaResponse, GetMessagesBatchRequest,
    GetMessagesMediaBatchRequest, GetRecentMessagesRequest, LastResponsesResponse,
    MediaBatchFailure, MediaBatchSummary, MediaLimits, MessageLinkResponse, MessageResponse,
    MessagesBatchResponse, MissingMessageEntry, OpenMessageRequest, RateLimiterCosts,
    RateLimiterStatus, ResolveChannelsRequest, ResolveChannelsResponse, ResponseFormat,
    SearchPublicChannelsRequest, SearchRequest, SearchResponse, StatusResponse,
    TranscribeVoiceMessageRequest, TranscribeVoiceMessageResponse, dedupe_and_validate_ids,
    json_response, parse_channel_id, parse_message_id, parse_optional_utc, validate_date_window,
};
// Constructed only inside open_message_in_telegram's macOS-only body; an
// unconditional import is an unused-import error on Linux builds.
#[cfg(target_os = "macos")]
use crate::mcp::tools::OpenMessageResponse;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::{ChannelId, HistoryParams, MediaFetchError, MessageId, SearchParams};
use futures::StreamExt;
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CacheScope, CallToolResult, ContentBlock, Implementation, InitializeResult, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct McpServer<T: TelegramClientTrait, R: RateLimiterTrait> {
    telegram_client: Arc<T>,
    rate_limiter: Arc<R>,
    metrics: Arc<SessionMetrics>,
    response_buffer: Arc<ResponseBuffer>,
    slow_write_threshold: Duration,
    media_download_cost: u32,
    transcription_cost: u32,
    transcription_default_timeout_secs: u32,
    transcription_max_timeout_secs: u32,
    response_byte_budget: usize,
    media_batch_max_total_bytes: usize,
    tool_router: ToolRouter<Self>,
}

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub fn new(telegram_client: Arc<T>, rate_limiter: Arc<R>) -> Self {
        let observability = ObservabilityConfig::default();
        Self {
            telegram_client,
            rate_limiter,
            metrics: Arc::new(SessionMetrics::new()),
            response_buffer: Arc::new(ResponseBuffer::new(
                observability.response_buffer_size,
                observability.max_buffered_payload_bytes,
            )),
            slow_write_threshold: Duration::from_millis(observability.slow_write_threshold_ms),
            media_download_cost: default_media_download_cost(),
            transcription_cost: default_transcription_cost(),
            transcription_default_timeout_secs: default_transcription_default_timeout(),
            transcription_max_timeout_secs: default_transcription_max_timeout(),
            response_byte_budget: default_response_byte_budget() as usize,
            media_batch_max_total_bytes: default_media_batch_max_total_bytes() as usize,
            tool_router: Self::tool_router(),
        }
    }

    /// Apply `[observability]` settings (ring buffer capacity, slow-write threshold).
    pub fn with_observability(mut self, config: &ObservabilityConfig) -> Self {
        self.response_buffer = Arc::new(ResponseBuffer::new(
            config.response_buffer_size,
            config.max_buffered_payload_bytes,
        ));
        self.slow_write_threshold = Duration::from_millis(config.slow_write_threshold_ms);
        self
    }

    /// Set the rate-limiter cost charged per get_message_media call
    /// (`[rate_limiting] media_download_cost`, default 3).
    pub fn with_media_download_cost(mut self, cost: u32) -> Self {
        self.media_download_cost = cost;
        self
    }

    /// Set the rate-limiter cost charged per transcribe_voice_message call
    /// (`[rate_limiting] transcription_cost`, default 5).
    pub fn with_transcription_cost(mut self, cost: u32) -> Self {
        self.transcription_cost = cost;
        self
    }

    /// Set the transcription `timeout_seconds` bounds: the default applied when a
    /// request omits it, and the max it is clamped to (`[transcription]`, AD-6).
    pub fn with_transcription_timeouts(mut self, default_secs: u32, max_secs: u32) -> Self {
        self.transcription_default_timeout_secs = default_secs;
        self.transcription_max_timeout_secs = max_secs;
        self
    }

    /// Set the serialized-response byte cap for message-stream tools
    /// (`[limits] response_byte_budget`, default 40 000).
    pub fn with_response_byte_budget(mut self, bytes: u64) -> Self {
        self.response_byte_budget = bytes as usize;
        self
    }

    /// Set the total base64 payload cap for `get_messages_media_batch`
    /// (`[limits] media_batch_max_total_bytes`, default 8 MiB).
    pub fn with_media_batch_max_total_bytes(mut self, bytes: u64) -> Self {
        self.media_batch_max_total_bytes = bytes as usize;
        self
    }

    #[cfg(test)]
    pub(crate) fn media_download_cost(&self) -> u32 {
        self.media_download_cost
    }

    #[cfg(test)]
    pub(crate) fn transcription_cost(&self) -> u32 {
        self.transcription_cost
    }

    #[cfg(test)]
    pub(crate) fn response_byte_budget(&self) -> usize {
        self.response_byte_budget
    }

    #[cfg(test)]
    pub(crate) fn media_batch_max_total_bytes(&self) -> usize {
        self.media_batch_max_total_bytes
    }

    /// Session metrics handle (shared with the transport; used for shutdown logging).
    pub fn metrics(&self) -> Arc<SessionMetrics> {
        Arc::clone(&self.metrics)
    }

    /// Response ring buffer handle (shared with the transport; used in tests).
    pub fn response_buffer(&self) -> Arc<ResponseBuffer> {
        Arc::clone(&self.response_buffer)
    }

    /// `tools/list` payload with SEP-2549 cache hints. The tool list is static
    /// per build, so clients may cache it for an hour; Private scope because
    /// this is a single-user (per-session-file) server.
    pub(crate) fn tools_list_result(&self) -> ListToolsResult {
        ListToolsResult::with_all_items(self.tool_router.list_all())
            .with_ttl_ms(3_600_000)
            .with_cache_scope(CacheScope::Private)
    }

    /// `tools_list_result()`, with the SEP-2549 cache hints stripped for
    /// clients that didn't negotiate MCP 2026-07-28 or later. Mirrors the
    /// `supports_cache_hints` gate in `#[tool_handler]`'s own default
    /// `list_tools` (rmcp-macros' `tool_handler.rs`): legacy-handshake and
    /// pre-2026-07-28 clients don't know `ttlMs`/`cacheScope` exist, so the
    /// reference behavior withholds them rather than sending fields the
    /// client can't interpret.
    pub(crate) fn tools_list_result_for(
        &self,
        protocol_version: Option<rmcp::model::ProtocolVersion>,
    ) -> ListToolsResult {
        let result = self.tools_list_result();
        let supports_cache_hints = protocol_version
            .is_some_and(|version| version >= rmcp::model::ProtocolVersion::V_2026_07_28);
        if supports_cache_hints {
            result
        } else {
            ListToolsResult {
                ttl_ms: None,
                cache_scope: None,
                ..result
            }
        }
    }

    pub async fn run_stdio(self) -> anyhow::Result<()> {
        use rmcp::transport::async_rw::AsyncRwTransport;
        use tokio::io::{stdin, stdout};

        let transport = InstrumentedTransport::new(
            AsyncRwTransport::new_server(stdin(), stdout()),
            Arc::clone(&self.metrics),
            Arc::clone(&self.response_buffer),
            self.slow_write_threshold,
        );

        let server = self.serve(transport).await?;
        server.waiting().await?;

        Ok(())
    }
}

// The inherent `*_impl` methods (the real tool logic) live in themed
// sibling modules under `server/`; the `#[tool]` wrappers below delegate
// to them. Kept out of this file so it stays focused on the macro-bound
// router + handler (LM-3).
mod impl_channels;
mod impl_discovery;
mod impl_links;
mod impl_media;
mod impl_message_batch;
mod impl_resolve;
mod impl_search;
mod impl_stats;
mod impl_status;

#[tool_router]
impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    // ========================================================================
    // MCP Tools
    // ========================================================================

    /// Tool 1: check_mcp_status - Health check and diagnostics
    #[tool(description = "Check MCP connection status, rate limiter state, and session counters")]
    pub async fn check_mcp_status(&self, id: RequestId) -> Result<String, String> {
        let inv = ToolInvocation::start("check_mcp_status", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            "Tool invocation started"
        );
        inv.finish(self.check_mcp_status_impl().await)
    }

    /// Tool 2: get_subscribed_channels - List user's Telegram channels with pagination
    #[tool(description = "List user's subscribed Telegram channels with pagination support")]
    pub async fn get_subscribed_channels(
        &self,
        Parameters(request): Parameters<GetChannelsRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let inv = ToolInvocation::start("get_subscribed_channels", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            limit = ?request.limit,
            offset = ?request.offset,
            "Tool invocation started"
        );
        inv.finish(self.get_subscribed_channels_impl(request).await)
    }

    /// Tool 3: get_channel_info - Get detailed information about a Telegram channel
    #[tool(description = "Get detailed information about a Telegram channel by username or ID")]
    pub async fn get_channel_info(
        &self,
        Parameters(request): Parameters<GetChannelInfoRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let inv = ToolInvocation::start("get_channel_info", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            channel_identifier = %request.channel_identifier,
            "Tool invocation started"
        );
        inv.finish(self.get_channel_info_impl(request).await)
    }

    /// Tool 4: generate_message_link - Generate deep links for a Telegram message
    #[tool(
        description = "Generate shareable deep links for a Telegram message (accepts channel ID or username; public channels get https://t.me/<username> links)"
    )]
    pub async fn generate_message_link(
        &self,
        Parameters(request): Parameters<GenerateLinkRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let inv = ToolInvocation::start("generate_message_link", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            channel_id = %request.channel_id,
            message_id = request.message_id,
            include_tg_protocol = ?request.include_tg_protocol,
            "Tool invocation started"
        );
        inv.finish(self.generate_message_link_impl(request).await)
    }

    /// Tool 5: open_message_in_telegram - Open message in Telegram Desktop (macOS)
    #[tool(
        description = "Open a specific message in Telegram Desktop application (macOS only; accepts channel ID or username)"
    )]
    pub async fn open_message_in_telegram(
        &self,
        Parameters(request): Parameters<OpenMessageRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let inv = ToolInvocation::start("open_message_in_telegram", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            channel_id = %request.channel_id,
            message_id = request.message_id,
            use_tg_protocol = ?request.use_tg_protocol,
            "Tool invocation started"
        );
        inv.finish(self.open_message_in_telegram_impl(request).await)
    }

    /// Tool 6: search_messages - Search messages across Telegram channels
    #[tool(
        description = "Search messages across subscribed Telegram channels with optional \
                       filters. Scoping with channel_id or channel_ids is cheaper than an \
                       unscoped global search and supports cursor pagination \
                       (before_id/after_id); global searches support neither, because there \
                       is no per-channel offset to resume from. A search that exceeds its \
                       configured deadline returns the results gathered so far with \
                       query_metadata.timed_out and .partial set, never an error."
    )]
    pub async fn search_messages(
        &self,
        Parameters(request): Parameters<SearchRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let inv = ToolInvocation::start("search_messages", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            query = %request.query,
            channel_id = ?request.channel_id,
            channel_ids = ?request.channel_ids,
            hours_back = ?request.hours_back,
            limit = ?request.limit,
            media_filter = ?request.media_filter,
            "Tool invocation started"
        );
        inv.finish(self.search_messages_impl(request).await)
    }

    /// Tool 7: get_recent_messages - Get recent messages from a channel by time window
    #[tool(
        description = "Get recent messages from one channel - or fan out over up to 20 channels via channel_ids - by time window (no search query needed)"
    )]
    pub async fn get_recent_messages(
        &self,
        Parameters(request): Parameters<GetRecentMessagesRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let inv = ToolInvocation::start("get_recent_messages", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            channel_id = ?request.channel_id,
            channel_ids = ?request.channel_ids,
            hours_back = ?request.hours_back,
            limit = ?request.limit,
            media_filter = ?request.media_filter,
            "Tool invocation started"
        );
        inv.finish(self.get_recent_messages_impl(request).await)
    }

    /// Tool 8: get_message_by_link - Get a specific message by its t.me link
    #[tool(
        description = "Get a specific Telegram message by its t.me link (e.g. https://t.me/swodki/575403)"
    )]
    pub async fn get_message_by_link(
        &self,
        Parameters(request): Parameters<GetMessageByLinkRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let inv = ToolInvocation::start("get_message_by_link", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            link = %request.link,
            "Tool invocation started"
        );
        inv.finish(self.get_message_by_link_impl(request).await)
    }

    /// Tool 9: get_last_responses - Recover recently written responses
    #[tool(
        description = "Debug/recovery: return the last N tool responses written to stdout, so a response lost in transit can be re-fetched without re-querying Telegram or spending rate-limit budget"
    )]
    pub async fn get_last_responses(
        &self,
        Parameters(request): Parameters<GetLastResponsesRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let inv = ToolInvocation::start("get_last_responses", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            n = ?request.n,
            "Tool invocation started"
        );
        inv.finish(self.get_last_responses_impl(request).await)
    }

    /// Tool 10: get_message_media - Return a message's photo (or video thumbnail) as an image
    #[tool(
        description = "Get a message's photo (or the thumbnail of its video/animation/video note) as an image the model can see, plus a JSON metadata block. Photos are downscaled (max_dimension, default 1280) and re-encoded as JPEG. Heavier than a search: charged media_download_cost rate-limit tokens. For more than one image, use get_messages_media_batch instead of looping this tool."
    )]
    pub async fn get_message_media(
        &self,
        Parameters(request): Parameters<GetMessageMediaRequest>,
        id: RequestId,
    ) -> Result<CallToolResult, String> {
        let inv = ToolInvocation::start("get_message_media", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            channel_id = %request.channel_id,
            message_id = request.message_id,
            max_dimension = ?request.max_dimension,
            "Tool invocation started"
        );
        inv.finish(self.get_message_media_impl(request).await)
    }

    /// Tool 16: get_messages_media_batch - Return several messages' images in one call
    #[tool(
        description = "Get the photos (or video/animation/video-note thumbnails) of up to 10 messages from ONE channel in a single call, as image blocks the model can see, each followed by its JSON metadata and a trailing batch summary. Far cheaper than N get_message_media calls: one channel resolution and one fetch round trip for the whole batch. Ids with no visual media, deleted ids, ids dropped because the total payload cap was exhausted or an image could not be shrunk to fit (retrying that id alone will not help), and the rare id whose downloaded image failed metadata serialization (internal_error) are reported in the summary's `failed` array rather than failing the call. Charged media_download_cost tokens per image actually returned."
    )]
    pub async fn get_messages_media_batch(
        &self,
        Parameters(request): Parameters<GetMessagesMediaBatchRequest>,
        id: RequestId,
    ) -> Result<CallToolResult, String> {
        let inv = ToolInvocation::start("get_messages_media_batch", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            channel_id = %request.channel_id,
            requested = request.message_ids.len(),
            max_dimension = ?request.max_dimension,
            "Tool invocation started"
        );
        inv.finish(self.get_messages_media_batch_impl(request).await)
    }

    /// Tool 11: transcribe_voice_message - Transcribe a voice/video-note message to text
    #[tool(
        description = "Transcribe a voice message or video note (round video) to text using Telegram's server-side transcription (no local ML). REQUIRES Telegram Premium on the connected account; check_mcp_status reports `premium`. Charged transcription_cost rate-limit tokens (more than a search). Returns partial text with partial=true if the wait times out."
    )]
    pub async fn transcribe_voice_message(
        &self,
        Parameters(request): Parameters<TranscribeVoiceMessageRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let inv = ToolInvocation::start("transcribe_voice_message", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            channel_id = %request.channel_id,
            message_id = request.message_id,
            timeout_seconds = ?request.timeout_seconds,
            "Tool invocation started"
        );
        inv.finish(self.transcribe_voice_message_impl(request).await)
    }

    /// Tool 12: search_public_channels - Discover public channels/groups by keyword
    #[tool(
        description = "Search Telegram's public directory for channels and groups by keyword (not limited to your subscriptions). is_subscribed: true is reliable; false is best-effort (the already-subscribed side of the result set is server-capped and prefix-matched). To go deeper on a result that has a real @username, call get_channel_info with that username."
    )]
    pub async fn search_public_channels(
        &self,
        Parameters(request): Parameters<SearchPublicChannelsRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let inv = ToolInvocation::start("search_public_channels", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            query = %request.query,
            limit = ?request.limit,
            "Tool invocation started"
        );
        inv.finish(self.search_public_channels_impl(request).await)
    }

    /// Tool 13: get_messages_batch - Fetch up to 50 specific messages in one call
    #[tool(
        description = "Fetch up to 50 specific messages from one channel by id in a single call. Deleted or missing ids are reported per-id in `missing` instead of failing the batch. The designated way to re-fetch full text after text_truncated, and to verify or deduplicate specific posts without N round trips."
    )]
    pub async fn get_messages_batch(
        &self,
        Parameters(request): Parameters<GetMessagesBatchRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let inv = ToolInvocation::start("get_messages_batch", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            channel_id = %request.channel_id,
            ids = request.message_ids.len(),
            "Tool invocation started"
        );
        inv.finish(self.get_messages_batch_impl(request).await)
    }

    /// Tool 14: resolve_channels - Batch-resolve ids/usernames/titles to channels
    #[tool(
        description = "Batch-resolve up to 20 channel identifiers (numeric ID, @username, or exact title of a subscribed chat) to full channel entities in one call. Use for full entities (subscriber counts, flags) and title-only private chats; forward sources already arrive named in forwarded_from. Failures are reported per-identifier."
    )]
    pub async fn resolve_channels(
        &self,
        Parameters(request): Parameters<ResolveChannelsRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let inv = ToolInvocation::start("resolve_channels", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            identifiers = request.identifiers.len(),
            "Tool invocation started"
        );
        inv.finish(self.resolve_channels_impl(request).await)
    }

    /// Tool 15: get_channel_stats - Posting-rate and engagement statistics
    #[tool(
        description = "Compute posting statistics for one channel from a bounded history sample (default 7 days, max 30, capped at 500 messages): album-collapsed post count, posts/day, median views, media and album share. sample.complete is false when the cap cut the window short. No content classification."
    )]
    pub async fn get_channel_stats(
        &self,
        Parameters(request): Parameters<GetChannelStatsRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let inv = ToolInvocation::start("get_channel_stats", id);
        tracing::info!(
            tool = inv.tool,
            request_id = %inv.request_id,
            channel_id = %request.channel_id,
            days_back = ?request.days_back,
            "Tool invocation started"
        );
        inv.finish(self.get_channel_stats_impl(request).await)
    }
}

// Implement ServerHandler trait with tool capabilities.
// The #[tool_handler] macro fills in any of call_tool/list_tools/get_tool
// not already defined below; list_tools is defined manually here to attach
// SEP-2549 cache hints (ttlMs/cacheScope), so the macro leaves it alone.
#[tool_handler]
impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> ServerHandler
    for McpServer<T, R>
{
    fn get_info(&self) -> InitializeResult {
        let server_info = Implementation::new("telegram-mcp", env!("CARGO_PKG_VERSION"));
        let capabilities = ServerCapabilities::builder().enable_tools().build();

        InitializeResult::new(capabilities)
            .with_server_info(server_info)
            .with_instructions("Telegram MCP Connector - Search Russian Telegram channels")
    }

    // Defining list_tools here (instead of relying on #[tool_handler]'s default)
    // lets us attach SEP-2549 cache hints; the macro skips generating a method
    // that's already present on the impl block, so call_tool/get_tool are
    // still macro-generated as usual. The hints are gated on the negotiated
    // protocol version (tools_list_result_for), matching the macro's own
    // default list_tools.
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        Ok(self.tools_list_result_for(context.protocol_version()))
    }
}

/// Guard binding a tool invocation's name, request id, and start time so the
/// `#[tool]` wrappers don't re-derive them or repeat the tool name in the
/// completion log. Each wrapper does `let inv = ToolInvocation::start(name, id)`,
/// emits its per-tool "started" line via `inv.tool`/`inv.request_id`, then
/// returns `inv.finish(self.<tool>_impl(..).await)` to log the symmetric
/// completed/failed line with the elapsed duration (AD-3).
struct ToolInvocation {
    tool: &'static str,
    request_id: String,
    started: Instant,
}

impl ToolInvocation {
    fn start(tool: &'static str, id: RequestId) -> Self {
        Self {
            tool,
            request_id: id.0.to_string(),
            started: Instant::now(),
        }
    }

    fn finish<T>(self, result: Result<T, String>) -> Result<T, String> {
        log_tool_outcome(self.tool, &self.request_id, self.started, &result);
        result
    }
}

/// Log the symmetric completion entry for a tool invocation.
fn log_tool_outcome<T>(tool: &str, request_id: &str, started: Instant, result: &Result<T, String>) {
    let duration_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(_) => {
            tracing::info!(
                tool = %tool,
                request_id = %request_id,
                duration_ms,
                "Tool invocation completed"
            );
        }
        Err(error) => {
            tracing::warn!(
                tool = %tool,
                request_id = %request_id,
                duration_ms,
                error = %error,
                "Tool invocation failed"
            );
        }
    }
}

// Tests are in the tests/ subdirectory
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
