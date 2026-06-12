use crate::config::ObservabilityConfig;
use crate::link::{ChannelRef, MessageLink, parse_telegram_link};
use crate::mcp::observability::{InstrumentedTransport, ResponseBuffer, SessionMetrics};
use crate::mcp::tools::image::process_image;
use crate::mcp::tools::{
    BufferedResponseEntry, ChannelsResponse, GenerateLinkRequest, GetChannelInfoRequest,
    GetChannelsRequest, GetLastResponsesRequest, GetMessageByLinkRequest, GetMessageMediaRequest,
    GetMessageMediaResponse, GetRecentMessagesRequest, LastResponsesResponse, MessageLinkResponse,
    OpenMessageRequest, OpenMessageResponse, SearchRequest, StatusResponse, parse_channel_id,
    parse_message_id, parse_optional_channel_id,
};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::{HistoryParams, SearchParams};
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, Implementation, InitializeResult, ServerCapabilities};
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
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub fn new(telegram_client: Arc<T>, rate_limiter: Arc<R>) -> Self {
        let observability = ObservabilityConfig::default();
        Self {
            telegram_client,
            rate_limiter,
            metrics: Arc::new(SessionMetrics::new()),
            response_buffer: Arc::new(ResponseBuffer::new(observability.response_buffer_size)),
            slow_write_threshold: Duration::from_millis(observability.slow_write_threshold_ms),
            media_download_cost: 5,
            tool_router: Self::tool_router(),
        }
    }

    /// Apply `[observability]` settings (ring buffer capacity, slow-write threshold).
    pub fn with_observability(mut self, config: &ObservabilityConfig) -> Self {
        self.response_buffer = Arc::new(ResponseBuffer::new(config.response_buffer_size));
        self.slow_write_threshold = Duration::from_millis(config.slow_write_threshold_ms);
        self
    }

    /// Set the rate-limiter cost charged per get_message_media call
    /// (`[rate_limiting] media_download_cost`, default 5).
    pub fn with_media_download_cost(mut self, cost: u32) -> Self {
        self.media_download_cost = cost;
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

    async fn check_mcp_status_impl(&self) -> Result<String, String> {
        let connected = self.telegram_client.is_connected().await;
        let tokens = self.rate_limiter.available_tokens();

        let response = StatusResponse {
            telegram_connected: connected,
            rate_limiter_tokens: tokens,
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            requests_received: self.metrics.requests_received(),
            responses_written: self.metrics.responses_written(),
            last_response_write_age_secs: self.metrics.last_write_age_secs(),
            session_started_at: self.metrics.session_started_at_rfc3339(),
            session_uptime_secs: self.metrics.uptime_secs(),
        };

        serde_json::to_string(&response).map_err(|e| e.to_string())
    }

    async fn get_subscribed_channels_impl(
        &self,
        request: GetChannelsRequest,
    ) -> Result<String, String> {
        let limit = request.limit.unwrap_or(20);
        let offset = request.offset.unwrap_or(0);

        let channels = self
            .telegram_client
            .get_subscribed_channels(limit, offset)
            .await
            .map_err(|e| e.to_string())?;

        let total = channels.len();
        let has_more = total >= limit as usize;

        let response = ChannelsResponse {
            channels,
            total,
            has_more,
        };

        serde_json::to_string(&response).map_err(|e| e.to_string())
    }

    async fn get_channel_info_impl(
        &self,
        request: GetChannelInfoRequest,
    ) -> Result<String, String> {
        let channel = self
            .telegram_client
            .get_channel_info(&request.channel_identifier)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::to_string(&channel).map_err(|e| e.to_string())
    }

    async fn generate_message_link_impl(
        &self,
        request: GenerateLinkRequest,
    ) -> Result<String, String> {
        // Parse and validate IDs using helpers
        let channel_id = parse_channel_id(&request.channel_id)?;
        let message_id = parse_message_id(request.message_id)?;

        // Generate links using existing MessageLink from link.rs
        let link = MessageLink::new(channel_id, message_id);

        // Build response based on include_tg_protocol flag (defaults to true)
        let include_tg = request.include_tg_protocol.unwrap_or(true);

        let response = MessageLinkResponse {
            channel_id: request.channel_id,
            message_id: request.message_id,
            https_link: link.https_link,
            tg_protocol_link: if include_tg {
                Some(link.tg_protocol_link)
            } else {
                None
            },
        };

        serde_json::to_string(&response).map_err(|e| e.to_string())
    }

    async fn open_message_in_telegram_impl(
        &self,
        request: OpenMessageRequest,
    ) -> Result<String, String> {
        // Parse and validate IDs using helpers
        let channel_id = parse_channel_id(&request.channel_id)?;
        let message_id = parse_message_id(request.message_id)?;

        // Generate links
        let link = MessageLink::new(channel_id, message_id);

        // Choose link type (defaults to tg:// protocol)
        let use_tg = request.use_tg_protocol.unwrap_or(true);
        let link_to_open = if use_tg {
            &link.tg_protocol_link
        } else {
            &link.https_link
        };

        // Execute open command (macOS-specific)
        #[cfg(target_os = "macos")]
        let result = tokio::process::Command::new("open")
            .arg(link_to_open)
            .output()
            .await;

        #[cfg(not(target_os = "macos"))]
        let result: Result<std::process::Output, std::io::Error> = Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "open_message_in_telegram is only supported on macOS",
        ));

        let response = match result {
            Ok(output) => {
                let success = output.status.success();
                OpenMessageResponse {
                    success,
                    message: if success {
                        "Message opened in Telegram".to_string()
                    } else {
                        format!("Failed to open: {:?}", output.status)
                    },
                    link_used: link_to_open.clone(),
                    app_opened: success,
                }
            }
            Err(e) => OpenMessageResponse {
                success: false,
                message: format!("Failed to execute open command: {}", e),
                link_used: link_to_open.clone(),
                app_opened: false,
            },
        };

        serde_json::to_string(&response).map_err(|e| e.to_string())
    }

    async fn search_messages_impl(&self, request: SearchRequest) -> Result<String, String> {
        // Validate: query required unless media_filter is set
        if request.query.trim().is_empty() && request.media_filter.is_none() {
            return Err(
                "Search query cannot be empty (unless media_filter is set to filter by media type)"
                    .to_string(),
            );
        }

        // Parse optional channel_id using helper
        let channel_id = parse_optional_channel_id(&request.channel_id)?;

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

        // Acquire rate limiter tokens (1 token per search)
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        // Build search params
        let params = SearchParams {
            query: request.query,
            channel_id,
            hours_back,
            limit,
            media_filter: request.media_filter,
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
            total_found = result.total_found,
            messages_returned = message_ids.len(),
            message_ids = ?message_ids,
            search_time_ms = result.search_time_ms,
            channels_searched = result.query_metadata.channels_searched,
            "Search results"
        );

        serde_json::to_string(&result).map_err(|e| e.to_string())
    }

    async fn get_recent_messages_impl(
        &self,
        request: GetRecentMessagesRequest,
    ) -> Result<String, String> {
        // Validate channel_id is provided
        if request.channel_id.trim().is_empty() {
            return Err("channel_id is required".to_string());
        }

        // Parse channel_id (can be numeric ID or username)
        let original_identifier = request.channel_id.clone();
        let (channel_id, channel_identifier) =
            if request.channel_id.chars().all(|c| c.is_ascii_digit()) {
                // Numeric ID - use helper for validation, no identifier needed
                (parse_channel_id(&request.channel_id)?, None)
            } else {
                // Username provided - resolve via get_channel_info and pass identifier
                let channel = self
                    .telegram_client
                    .get_channel_info(&request.channel_id)
                    .await
                    .map_err(|e| format!("Channel not found: {}", e))?;
                (channel.id, Some(original_identifier))
            };

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

        // Acquire rate limiter tokens (1 token per request)
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        // Build history params
        let params = HistoryParams {
            channel_id,
            channel_identifier,
            hours_back,
            limit,
            media_filter: request.media_filter,
        };

        // Execute history retrieval
        let result = self
            .telegram_client
            .get_recent_messages(&params)
            .await
            .map_err(|e| e.to_string())?;

        // Log results (IDs only, not message text - for privacy and log size)
        let message_ids: Vec<i64> = result.messages.iter().map(|m| m.id.get()).collect();
        tracing::info!(
            channel_id = %params.channel_id,
            media_filter = ?params.media_filter,
            hours_back = params.hours_back,
            limit = params.limit,
            total_found = result.total_found,
            messages_returned = message_ids.len(),
            message_ids = ?message_ids,
            search_time_ms = result.search_time_ms,
            "Recent messages results"
        );

        serde_json::to_string(&result).map_err(|e| e.to_string())
    }

    async fn get_message_by_link_impl(
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

        // Acquire rate limiter token
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        // Fetch the message
        let message = self
            .telegram_client
            .get_message_by_id(&channel_identifier, message_id.get() as i32)
            .await
            .map_err(|e| e.to_string())?;

        tracing::info!(
            link = %request.link,
            channel = %channel_identifier,
            message_id = message_id.get(),
            "Message by link results"
        );

        serde_json::to_string(&message).map_err(|e| e.to_string())
    }

    async fn get_last_responses_impl(
        &self,
        request: GetLastResponsesRequest,
    ) -> Result<String, String> {
        let entries = self.response_buffer.last(request.n.map(|n| n as usize));
        let responses: Vec<BufferedResponseEntry> = entries
            .into_iter()
            .map(|entry| BufferedResponseEntry {
                request_id: entry.request_id,
                tool_name: entry.tool_name,
                written_at: chrono::DateTime::<chrono::Utc>::from(entry.written_at).to_rfc3339(),
                size_bytes: entry.size_bytes,
                // Payload was valid JSON when written; Null only on corruption.
                response: serde_json::from_str(&entry.payload).unwrap_or(serde_json::Value::Null),
            })
            .collect();

        let response = LastResponsesResponse {
            buffered: self.response_buffer.len(),
            responses,
        };

        serde_json::to_string(&response).map_err(|e| e.to_string())
    }

    async fn get_message_media_impl(
        &self,
        request: GetMessageMediaRequest,
    ) -> Result<CallToolResult, String> {
        const DEFAULT_MAX_DIMENSION: u32 = 1280;
        const MIN_DIMENSION: u32 = 64;
        const MAX_DIMENSION: u32 = 2048;

        let message_id = parse_message_id(request.message_id)?;
        let max_dimension = request
            .max_dimension
            .unwrap_or(DEFAULT_MAX_DIMENSION)
            .clamp(MIN_DIMENSION, MAX_DIMENSION);

        // Media downloads are heavier than searches; charge the configured cost.
        self.rate_limiter
            .acquire(self.media_download_cost)
            .await
            .map_err(|e| e.to_string())?;

        let download = self
            .telegram_client
            .download_message_media(&request.channel_id, message_id.get() as i32, max_dimension)
            .await
            .map_err(|e| e.to_string())?;

        let processed = process_image(&download.bytes, max_dimension).map_err(|e| e.to_string())?;

        let metadata = GetMessageMediaResponse {
            channel_id: request.channel_id.clone(),
            message_id: message_id.get(),
            media_type: download.media_type,
            is_thumbnail: download.is_thumbnail,
            caption: download.caption,
            original_width: download.width,
            original_height: download.height,
            original_size_bytes: download.source_size_bytes,
            returned_width: processed.width,
            returned_height: processed.height,
            returned_size_bytes: processed.encoded_size_bytes,
            mime_type: "image/jpeg".to_string(),
        };

        tracing::info!(
            channel = %request.channel_id,
            message_id = message_id.get(),
            media_type = ?metadata.media_type,
            is_thumbnail = metadata.is_thumbnail,
            returned_bytes = metadata.returned_size_bytes,
            "Message media results"
        );

        let metadata_json = serde_json::to_string(&metadata).map_err(|e| e.to_string())?;

        Ok(CallToolResult::success(vec![
            Content::image(processed.base64_jpeg, "image/jpeg"),
            Content::text(metadata_json),
        ]))
    }
}

#[tool_router]
impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    // ========================================================================
    // MCP Tools
    // ========================================================================

    /// Tool 1: check_mcp_status - Health check and diagnostics
    #[tool(description = "Check MCP connection status, rate limiter state, and session counters")]
    pub async fn check_mcp_status(&self, id: RequestId) -> Result<String, String> {
        let request_id = id.0.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "check_mcp_status",
            request_id = %request_id,
            "Tool invocation started"
        );
        let result = self.check_mcp_status_impl().await;
        log_tool_outcome("check_mcp_status", &request_id, started, &result);
        result
    }

    /// Tool 2: get_subscribed_channels - List user's Telegram channels with pagination
    #[tool(description = "List user's subscribed Telegram channels with pagination support")]
    pub async fn get_subscribed_channels(
        &self,
        Parameters(request): Parameters<GetChannelsRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let request_id = id.0.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "get_subscribed_channels",
            request_id = %request_id,
            limit = ?request.limit,
            offset = ?request.offset,
            "Tool invocation started"
        );
        let result = self.get_subscribed_channels_impl(request).await;
        log_tool_outcome("get_subscribed_channels", &request_id, started, &result);
        result
    }

    /// Tool 3: get_channel_info - Get detailed information about a Telegram channel
    #[tool(description = "Get detailed information about a Telegram channel by username or ID")]
    pub async fn get_channel_info(
        &self,
        Parameters(request): Parameters<GetChannelInfoRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let request_id = id.0.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "get_channel_info",
            request_id = %request_id,
            channel_identifier = %request.channel_identifier,
            "Tool invocation started"
        );
        let result = self.get_channel_info_impl(request).await;
        log_tool_outcome("get_channel_info", &request_id, started, &result);
        result
    }

    /// Tool 4: generate_message_link - Generate deep links for a Telegram message
    #[tool(description = "Generate tg:// and https://t.me deep links for a Telegram message")]
    pub async fn generate_message_link(
        &self,
        Parameters(request): Parameters<GenerateLinkRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let request_id = id.0.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "generate_message_link",
            request_id = %request_id,
            channel_id = %request.channel_id,
            message_id = request.message_id,
            include_tg_protocol = ?request.include_tg_protocol,
            "Tool invocation started"
        );
        let result = self.generate_message_link_impl(request).await;
        log_tool_outcome("generate_message_link", &request_id, started, &result);
        result
    }

    /// Tool 5: open_message_in_telegram - Open message in Telegram Desktop (macOS)
    #[tool(description = "Open a specific message in Telegram Desktop application (macOS only)")]
    pub async fn open_message_in_telegram(
        &self,
        Parameters(request): Parameters<OpenMessageRequest>,
        id: RequestId,
    ) -> Result<String, String> {
        let request_id = id.0.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "open_message_in_telegram",
            request_id = %request_id,
            channel_id = %request.channel_id,
            message_id = request.message_id,
            use_tg_protocol = ?request.use_tg_protocol,
            "Tool invocation started"
        );
        let result = self.open_message_in_telegram_impl(request).await;
        log_tool_outcome("open_message_in_telegram", &request_id, started, &result);
        result
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
        let request_id = id.0.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "search_messages",
            request_id = %request_id,
            query = %request.query,
            channel_id = ?request.channel_id,
            hours_back = ?request.hours_back,
            limit = ?request.limit,
            media_filter = ?request.media_filter,
            "Tool invocation started"
        );
        let result = self.search_messages_impl(request).await;
        log_tool_outcome("search_messages", &request_id, started, &result);
        result
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
        let request_id = id.0.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "get_recent_messages",
            request_id = %request_id,
            channel_id = %request.channel_id,
            hours_back = ?request.hours_back,
            limit = ?request.limit,
            media_filter = ?request.media_filter,
            "Tool invocation started"
        );
        let result = self.get_recent_messages_impl(request).await;
        log_tool_outcome("get_recent_messages", &request_id, started, &result);
        result
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
        let request_id = id.0.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "get_message_by_link",
            request_id = %request_id,
            link = %request.link,
            "Tool invocation started"
        );
        let result = self.get_message_by_link_impl(request).await;
        log_tool_outcome("get_message_by_link", &request_id, started, &result);
        result
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
        let request_id = id.0.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "get_last_responses",
            request_id = %request_id,
            n = ?request.n,
            "Tool invocation started"
        );
        let result = self.get_last_responses_impl(request).await;
        log_tool_outcome("get_last_responses", &request_id, started, &result);
        result
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
        let request_id = id.0.to_string();
        let started = Instant::now();
        tracing::info!(
            tool = "get_message_media",
            request_id = %request_id,
            channel_id = %request.channel_id,
            message_id = request.message_id,
            max_dimension = ?request.max_dimension,
            "Tool invocation started"
        );
        let result = self.get_message_media_impl(request).await;
        log_tool_outcome("get_message_media", &request_id, started, &result);
        result
    }
}

// Implement ServerHandler trait with tool capabilities
// The #[tool_handler] macro automatically implements list_tools and call_tool
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
