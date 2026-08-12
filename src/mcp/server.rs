use crate::config::ObservabilityConfig;
use crate::error::Error;
use crate::link::{ChannelRef, MessageLink, parse_telegram_link};
use crate::mcp::observability::{InstrumentedTransport, ResponseBuffer, SessionMetrics};
use crate::mcp::tools::image::process_image;
use crate::mcp::tools::shaping;
use crate::mcp::tools::{
    BufferedResponseEntry, ChannelsResponse, GenerateLinkRequest, GetChannelInfoRequest,
    GetChannelsRequest, GetLastResponsesRequest, GetMessageByLinkRequest, GetMessageMediaRequest,
    GetMessageMediaResponse, GetRecentMessagesRequest, LastResponsesResponse, MessageLinkResponse,
    MessageResponse, OpenMessageRequest, ResponseFormat, SearchPublicChannelsRequest,
    SearchRequest, SearchResponse, StatusResponse, TranscribeVoiceMessageRequest,
    TranscribeVoiceMessageResponse, json_response, parse_channel_id, parse_message_id,
    parse_optional_channel_id, parse_optional_utc, validate_date_window,
};
// Constructed only inside open_message_in_telegram's macOS-only body; an
// unconditional import is an unused-import error on Linux builds.
#[cfg(target_os = "macos")]
use crate::mcp::tools::OpenMessageResponse;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::{HistoryParams, SearchParams};
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

/// Default serialized-response byte cap for message-stream tools
/// (`[limits] response_byte_budget`, work-order B4).
const DEFAULT_RESPONSE_BYTE_BUDGET: usize = 40_000;

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
            media_download_cost: 5,
            transcription_cost: 5,
            transcription_default_timeout_secs: 30,
            transcription_max_timeout_secs: 120,
            response_byte_budget: DEFAULT_RESPONSE_BYTE_BUDGET,
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
    /// (`[rate_limiting] media_download_cost`, default 5).
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
mod impl_search;
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
        description = "Search messages across subscribed Telegram channels with optional filters"
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
            hours_back = ?request.hours_back,
            limit = ?request.limit,
            media_filter = ?request.media_filter,
            "Tool invocation started"
        );
        inv.finish(self.search_messages_impl(request).await)
    }

    /// Tool 7: get_recent_messages - Get recent messages from a channel by time window
    #[tool(
        description = "Get recent messages from a specific channel by time window (no search query needed)"
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
            channel_id = %request.channel_id,
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
        description = "Get a message's photo (or the thumbnail of its video/animation/video note) as an image the model can see, plus a JSON metadata block. Photos are downscaled (max_dimension, default 1280) and re-encoded as JPEG. Heavier than a search: charged media_download_cost rate-limit tokens."
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
