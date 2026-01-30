//! Telegram client implementation wrapping grammers-client

use crate::config::TelegramConfig;
use crate::error::Error;
use crate::telegram::converters::{
    convert_media_filter, convert_message, convert_peer_to_channel, matches_media_filter,
};
use crate::telegram::trait_def::TelegramClientTrait;
use crate::telegram::types::{HistoryParams, QueryMetadata, SearchParams, SearchResult};
use chrono::{Duration, Utc};
use grammers_client::Client;
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::task::JoinHandle;

/// Telegram client wrapping grammers-client
pub struct TelegramClient {
    client: Client,
    session: Arc<SqliteSession>,
    session_path: PathBuf,
    _runner_handle: JoinHandle<()>,
}

impl TelegramClient {
    /// Create a new Telegram client
    ///
    /// This handles both first-time setup (no session) and returning users (with session).
    /// If session exists, it will be loaded and used. Otherwise, a new session is created.
    ///
    /// After creation, check `is_connected()` to determine if authentication is needed.
    pub async fn new(config: &TelegramConfig) -> Result<Self, Error> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = config.session_file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                Error::Auth(format!(
                    "Failed to create session directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        // Open or create SQLite session
        tracing::info!("Opening session from {:?}", config.session_file);
        let session = Arc::new(
            SqliteSession::open(&config.session_file)
                .map_err(|e| Error::Auth(format!("Failed to open session: {}", e)))?,
        );

        // Create sender pool
        tracing::info!("Creating sender pool...");
        let pool = SenderPool::new(Arc::clone(&session), config.api_id);

        // Create client before spawning runner (Client::new takes &SenderPool)
        let client = Client::new(&pool);

        // Spawn the runner in the background
        let runner_handle = tokio::spawn(async move {
            pool.runner.run().await;
        });

        tracing::info!("Telegram client created successfully");

        Ok(Self {
            client,
            session,
            session_path: config.session_file.clone(),
            _runner_handle: runner_handle,
        })
    }

    /// Get access to the underlying grammers client (for authentication)
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Get access to the session (for saving)
    pub fn session(&self) -> &SqliteSession {
        &self.session
    }

    /// Get the session file path
    pub fn session_path(&self) -> &Path {
        &self.session_path
    }

    /// Request login code for authentication
    pub async fn request_login_code(
        &self,
        phone: &str,
        api_hash: &str,
    ) -> Result<grammers_client::types::LoginToken, Error> {
        self.client
            .request_login_code(phone, api_hash)
            .await
            .map_err(|e| Error::Auth(format!("Failed to request login code: {}", e)))
    }

    /// Sign in with the received code
    pub async fn sign_in(
        &self,
        token: &grammers_client::types::LoginToken,
        code: &str,
    ) -> Result<(), Error> {
        match self.client.sign_in(token, code).await {
            Ok(_user) => {
                tracing::info!("Successfully signed in");
                Ok(())
            }
            Err(grammers_client::SignInError::PasswordRequired(password_token)) => {
                Err(Error::Auth(format!(
                    "2FA password required (hint: {:?})",
                    password_token.hint()
                )))
            }
            Err(e) => Err(Error::Auth(format!("Sign in failed: {}", e))),
        }
    }

    /// Sign in with 2FA password
    pub async fn check_password(
        &self,
        password_token: grammers_client::types::PasswordToken,
        password: &str,
    ) -> Result<(), Error> {
        self.client
            .check_password(password_token, password.as_bytes())
            .await
            .map_err(|e| Error::Auth(format!("2FA verification failed: {}", e)))?;
        tracing::info!("Successfully signed in with 2FA");
        Ok(())
    }
}

#[async_trait::async_trait]
impl TelegramClientTrait for TelegramClient {
    async fn is_connected(&self) -> bool {
        self.client.is_authorized().await.unwrap_or_default()
    }

    async fn get_subscribed_channels(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<crate::telegram::Channel>, Error> {
        let mut channels = Vec::new();
        let mut dialogs = self.client.iter_dialogs();
        let mut count = 0u32;

        while let Some(dialog) = dialogs
            .next()
            .await
            .map_err(|e| Error::TelegramApi(format!("Failed to iterate dialogs: {}", e)))?
        {
            let peer = dialog.peer();

            // Only include channels and groups
            if let Some(channel) = convert_peer_to_channel(peer) {
                if count >= offset {
                    channels.push(channel);
                    if channels.len() >= limit as usize {
                        break;
                    }
                }
                count += 1;
            }
        }

        tracing::debug!(
            "Retrieved {} channels (offset: {}, limit: {})",
            channels.len(),
            offset,
            limit
        );

        Ok(channels)
    }

    async fn get_channel_info(&self, identifier: &str) -> Result<crate::telegram::Channel, Error> {
        // Validate identifier
        if identifier.is_empty() {
            return Err(Error::InvalidInput(
                "Channel identifier cannot be empty".to_string(),
            ));
        }

        // Resolve the channel
        let peer = if let Some(username) = identifier.strip_prefix('@') {
            // Username lookup (@ prefix stripped)
            self.client
                .resolve_username(username)
                .await
                .map_err(|e| Error::TelegramApi(format!("Failed to resolve username: {}", e)))?
                .ok_or_else(|| Error::InvalidInput(format!("Channel not found: {}", identifier)))?
        } else if let Ok(id) = identifier.parse::<i64>() {
            // Numeric ID lookup - need to search through dialogs
            let mut dialogs = self.client.iter_dialogs();
            let mut found = None;

            while let Some(dialog) = dialogs
                .next()
                .await
                .map_err(|e| Error::TelegramApi(format!("Failed to iterate dialogs: {}", e)))?
            {
                if dialog.peer().id().bare_id() == id {
                    found = Some(dialog.peer().clone());
                    break;
                }
            }

            found
                .ok_or_else(|| Error::InvalidInput(format!("Channel not found: {}", identifier)))?
        } else {
            // Try as username without @ prefix
            self.client
                .resolve_username(identifier)
                .await
                .map_err(|e| Error::TelegramApi(format!("Failed to resolve username: {}", e)))?
                .ok_or_else(|| Error::InvalidInput(format!("Channel not found: {}", identifier)))?
        };

        convert_peer_to_channel(&peer)
            .ok_or_else(|| Error::InvalidInput("Not a channel or group".to_string()))
    }

    async fn search_messages(&self, params: &SearchParams) -> Result<SearchResult, Error> {
        // Validate parameters
        // Empty query is allowed when media_filter is set (search for media type only)
        if params.query.is_empty() && params.media_filter.is_none() {
            return Err(Error::InvalidInput(
                "Search query cannot be empty (unless media_filter is specified)".to_string(),
            ));
        }

        if params.limit == 0 {
            return Err(Error::InvalidInput(
                "Search limit must be greater than 0".to_string(),
            ));
        }

        let start_time = Instant::now();
        let cutoff_time = Utc::now() - Duration::hours(params.hours_back as i64);
        let mut messages = Vec::new();
        let mut channels_searched = 0u32;

        // If channel_id is specified, search only that channel
        if let Some(channel_id) = &params.channel_id {
            // Find the channel in our dialogs
            let mut dialogs = self.client.iter_dialogs();

            while let Some(dialog) = dialogs
                .next()
                .await
                .map_err(|e| Error::TelegramApi(format!("Failed to iterate dialogs: {}", e)))?
            {
                let peer = dialog.peer();
                if peer.id().bare_id() == channel_id.get() {
                    channels_searched += 1;

                    // Search in this specific channel
                    let mut search_iter = self.client.search_messages(peer).query(&params.query);

                    // Apply media filter if specified
                    if let Some(ref media_filter) = params.media_filter {
                        search_iter = search_iter.filter(convert_media_filter(media_filter));
                    }

                    while let Some(msg) = search_iter
                        .next()
                        .await
                        .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?
                    {
                        // Check time filter
                        let msg_time = msg.date();

                        if msg_time < cutoff_time {
                            break; // Messages are in reverse chronological order
                        }

                        if let Some(converted) = convert_message(&msg, peer) {
                            messages.push(converted);
                            if messages.len() >= params.limit as usize {
                                break;
                            }
                        }
                    }
                    break;
                }
            }
        } else {
            // Search all channels using global search
            let mut search_iter = self.client.search_all_messages().query(&params.query);

            // Apply media filter if specified
            if let Some(ref media_filter) = params.media_filter {
                search_iter = search_iter.filter(convert_media_filter(media_filter));
            }

            while let Some(msg) = search_iter
                .next()
                .await
                .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?
            {
                // Check time filter
                let msg_time = msg.date();

                if msg_time < cutoff_time {
                    continue; // Skip old messages but keep searching
                }

                // Get peer from message and convert
                if let Ok(peer) = msg.peer()
                    && let Some(converted) = convert_message(&msg, peer)
                {
                    messages.push(converted);
                    if messages.len() >= params.limit as usize {
                        break;
                    }
                }
            }

            // Count unique channels in results
            let unique_channels: std::collections::HashSet<_> =
                messages.iter().map(|m| m.channel_id.get()).collect();
            channels_searched = unique_channels.len() as u32;
        }

        // Sort by timestamp (newest first)
        messages.sort_by_key(|b| std::cmp::Reverse(b.timestamp));

        let search_time_ms = start_time.elapsed().as_millis() as u64;
        let total_found = messages.len() as u64;

        tracing::info!(
            query = %params.query,
            media_filter = ?params.media_filter,
            results = total_found,
            channels = channels_searched,
            duration_ms = search_time_ms,
            "Search completed"
        );

        Ok(SearchResult {
            messages,
            total_found,
            search_time_ms,
            query_metadata: QueryMetadata {
                query: params.query.clone(),
                hours_back: params.hours_back,
                channels_searched,
            },
        })
    }

    async fn get_recent_messages(&self, params: &HistoryParams) -> Result<SearchResult, Error> {
        // Validate limit
        if params.limit == 0 {
            return Err(Error::InvalidInput(
                "Limit must be greater than 0".to_string(),
            ));
        }

        let start_time = Instant::now();
        let cutoff_time = Utc::now() - Duration::hours(params.hours_back as i64);
        let mut messages = Vec::new();

        // Find the channel in our dialogs
        let mut dialogs = self.client.iter_dialogs();
        let mut found_channel = false;

        while let Some(dialog) = dialogs
            .next()
            .await
            .map_err(|e| Error::TelegramApi(format!("Failed to iterate dialogs: {}", e)))?
        {
            let peer = dialog.peer();
            if peer.id().bare_id() == params.channel_id.get() {
                found_channel = true;

                // Use iter_messages to get message history (no search query)
                let mut messages_iter = self.client.iter_messages(peer);

                while let Some(msg) = messages_iter
                    .next()
                    .await
                    .map_err(|e| Error::TelegramApi(format!("Failed to iterate messages: {}", e)))?
                {
                    // Check time filter - messages are in reverse chronological order
                    if msg.date() < cutoff_time {
                        break;
                    }

                    // Apply media filter client-side (iter_messages doesn't support server-side filtering)
                    if params
                        .media_filter
                        .as_ref()
                        .is_some_and(|filter| !matches_media_filter(&msg, filter))
                    {
                        continue;
                    }

                    // Convert and collect
                    if let Some(converted) = convert_message(&msg, peer) {
                        messages.push(converted);
                        if messages.len() >= params.limit as usize {
                            break;
                        }
                    }
                }
                break;
            }
        }

        if !found_channel {
            return Err(Error::InvalidInput(format!(
                "Channel not found: {}",
                params.channel_id
            )));
        }

        let search_time_ms = start_time.elapsed().as_millis() as u64;
        let total_found = messages.len() as u64;

        tracing::info!(
            channel_id = %params.channel_id,
            media_filter = ?params.media_filter,
            results = total_found,
            hours_back = params.hours_back,
            duration_ms = search_time_ms,
            "Get recent messages completed"
        );

        Ok(SearchResult {
            messages,
            total_found,
            search_time_ms,
            query_metadata: QueryMetadata {
                query: String::new(), // No query for history retrieval
                hours_back: params.hours_back,
                channels_searched: 1,
            },
        })
    }
}
