use crate::link::MessageLink;
use crate::mcp::tools::{
    ChannelsResponse, GenerateLinkRequest, GetChannelInfoRequest, GetChannelsRequest,
    GetRecentMessagesRequest, MessageLinkResponse, OpenMessageRequest, OpenMessageResponse,
    SearchRequest, StatusResponse,
};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::Channel;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::{ChannelId, HistoryParams, MessageId, SearchParams, SearchResult};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, InitializeResult, ProtocolVersion, ServerCapabilities};
use rmcp::{Json, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use std::sync::Arc;

#[derive(Clone)]
pub struct McpServer<T: TelegramClientTrait, R: RateLimiterTrait> {
    telegram_client: Arc<T>,
    rate_limiter: Arc<R>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub fn new(telegram_client: Arc<T>, rate_limiter: Arc<R>) -> Self {
        Self {
            telegram_client,
            rate_limiter,
            tool_router: Self::tool_router(),
        }
    }

    pub async fn run_stdio(self) -> anyhow::Result<()> {
        use tokio::io::{stdin, stdout};

        // Create stdio transport
        let transport = (stdin(), stdout());

        // Start MCP server with stdio transport
        let server = self.serve(transport).await?;

        // Wait for shutdown signal (blocks until server terminates)
        server.waiting().await?;

        Ok(())
    }
}

#[tool_router]
impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    // ========================================================================
    // MCP Tools
    // ========================================================================

    /// Tool 1: check_mcp_status - Health check and diagnostics
    #[tool(description = "Check MCP connection status and rate limiter state")]
    pub async fn check_mcp_status(&self) -> Result<Json<StatusResponse>, String> {
        let connected = self.telegram_client.is_connected().await;
        let tokens = self.rate_limiter.available_tokens();

        Ok(Json(StatusResponse {
            telegram_connected: connected,
            rate_limiter_tokens: tokens,
            server_version: env!("CARGO_PKG_VERSION").to_string(),
        }))
    }

    /// Tool 2: get_subscribed_channels - List user's Telegram channels with pagination
    #[tool(description = "List user's subscribed Telegram channels with pagination support")]
    pub async fn get_subscribed_channels(
        &self,
        Parameters(request): Parameters<GetChannelsRequest>,
    ) -> Result<Json<ChannelsResponse>, String> {
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

        Ok(Json(response))
    }

    /// Tool 3: get_channel_info - Get detailed information about a Telegram channel
    #[tool(description = "Get detailed information about a Telegram channel by username or ID")]
    pub async fn get_channel_info(
        &self,
        Parameters(request): Parameters<GetChannelInfoRequest>,
    ) -> Result<Json<Channel>, String> {
        let channel = self
            .telegram_client
            .get_channel_info(&request.channel_identifier)
            .await
            .map_err(|e| e.to_string())?;

        Ok(Json(channel))
    }

    /// Tool 4: generate_message_link - Generate deep links for a Telegram message
    #[tool(description = "Generate tg:// and https://t.me deep links for a Telegram message")]
    pub async fn generate_message_link(
        &self,
        Parameters(request): Parameters<GenerateLinkRequest>,
    ) -> Result<Json<MessageLinkResponse>, String> {
        // Parse channel_id string to i64
        let channel_id_num: i64 = request.channel_id.parse().map_err(|_| {
            format!(
                "Invalid channel_id: '{}' is not a valid number",
                request.channel_id
            )
        })?;

        // Create type-safe IDs
        let channel_id =
            ChannelId::new(channel_id_num).map_err(|e| format!("Invalid channel_id: {}", e))?;
        let message_id =
            MessageId::new(request.message_id).map_err(|e| format!("Invalid message_id: {}", e))?;

        // Generate links using existing MessageLink from link.rs
        let link = MessageLink::new(channel_id, message_id);

        // Build response based on include_tg_protocol flag (defaults to true)
        let include_tg = request.include_tg_protocol.unwrap_or(true);

        Ok(Json(MessageLinkResponse {
            channel_id: request.channel_id,
            message_id: request.message_id,
            https_link: link.https_link,
            tg_protocol_link: if include_tg {
                Some(link.tg_protocol_link)
            } else {
                None
            },
        }))
    }

    /// Tool 5: open_message_in_telegram - Open message in Telegram Desktop (macOS)
    #[tool(description = "Open a specific message in Telegram Desktop application (macOS only)")]
    pub async fn open_message_in_telegram(
        &self,
        Parameters(request): Parameters<OpenMessageRequest>,
    ) -> Result<Json<OpenMessageResponse>, String> {
        // Parse channel_id string to i64
        let channel_id_num: i64 = request.channel_id.parse().map_err(|_| {
            format!(
                "Invalid channel_id: '{}' is not a valid number",
                request.channel_id
            )
        })?;

        // Create type-safe IDs
        let channel_id =
            ChannelId::new(channel_id_num).map_err(|e| format!("Invalid channel_id: {}", e))?;
        let message_id =
            MessageId::new(request.message_id).map_err(|e| format!("Invalid message_id: {}", e))?;

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

        match result {
            Ok(output) => {
                let success = output.status.success();
                Ok(Json(OpenMessageResponse {
                    success,
                    message: if success {
                        "Message opened in Telegram".to_string()
                    } else {
                        format!("Failed to open: {:?}", output.status)
                    },
                    link_used: link_to_open.clone(),
                    app_opened: success,
                }))
            }
            Err(e) => Ok(Json(OpenMessageResponse {
                success: false,
                message: format!("Failed to execute open command: {}", e),
                link_used: link_to_open.clone(),
                app_opened: false,
            })),
        }
    }

    /// Tool 6: search_messages - Search messages across Telegram channels
    #[tool(
        description = "Search messages across subscribed Telegram channels with optional filters"
    )]
    pub async fn search_messages(
        &self,
        Parameters(request): Parameters<SearchRequest>,
    ) -> Result<Json<SearchResult>, String> {
        // Validate: query required unless media_filter is set
        if request.query.trim().is_empty() && request.media_filter.is_none() {
            return Err(
                "Search query cannot be empty (unless media_filter is set to filter by media type)"
                    .to_string(),
            );
        }

        // Parse optional channel_id
        let channel_id = match &request.channel_id {
            Some(id_str) => {
                let id_num: i64 = id_str.parse().map_err(|_| {
                    format!("Invalid channel_id: '{}' is not a valid number", id_str)
                })?;
                Some(ChannelId::new(id_num).map_err(|e| format!("Invalid channel_id: {}", e))?)
            }
            None => None,
        };

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
            "Search completed"
        );

        Ok(Json(result))
    }

    /// Tool 7: get_recent_messages - Get recent messages from a channel by time window
    #[tool(
        description = "Get recent messages from a specific channel by time window (no search query needed)"
    )]
    pub async fn get_recent_messages(
        &self,
        Parameters(request): Parameters<GetRecentMessagesRequest>,
    ) -> Result<Json<SearchResult>, String> {
        // Validate channel_id is provided
        if request.channel_id.trim().is_empty() {
            return Err("channel_id is required".to_string());
        }

        // Parse channel_id (can be numeric ID or username)
        let channel_id = if let Ok(id_num) = request.channel_id.parse::<i64>() {
            ChannelId::new(id_num).map_err(|e| format!("Invalid channel_id: {}", e))?
        } else {
            // Username provided - need to resolve it first via get_channel_info
            let channel = self
                .telegram_client
                .get_channel_info(&request.channel_id)
                .await
                .map_err(|e| format!("Channel not found: {}", e))?;
            channel.id
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
            "Get recent messages completed"
        );

        Ok(Json(result))
    }
}

// Implement ServerHandler trait with tool capabilities
// The #[tool_handler] macro automatically implements list_tools and call_tool
#[tool_handler]
impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> ServerHandler
    for McpServer<T, R>
{
    fn get_info(&self) -> InitializeResult {
        InitializeResult {
            protocol_version: ProtocolVersion::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "telegram-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Telegram MCP Connector - Search Russian Telegram channels".to_string(),
            ),
        }
    }
}

// Tests are in the tests/ subdirectory
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
