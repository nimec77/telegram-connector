//! Telegram client implementation wrapping grammers-client

use crate::config::{TelegramConfig, TimeoutConfig};
use crate::error::Error;
use crate::telegram::converters::{
    convert_media_filter, convert_media_to_type, convert_message, convert_peer_to_channel,
    extract_audio_duration, extract_video_info, matches_media_filter, select_size_candidate,
    size_candidates,
};
use crate::telegram::trait_def::TelegramClientTrait;
use crate::telegram::types::{
    HistoryParams, MediaDownload, MediaType, QueryMetadata, SearchParams, SearchResult,
    TranscriptionOutcome, TranscriptionState,
};
use chrono::{Duration, Utc};
use grammers_client::Client;
use grammers_client::media::Media;
use grammers_client::tl;
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant};
use tokio::task::JoinHandle;

/// Wrap a fallible async operation in a hard wall-clock timeout.
///
/// If `fut` resolves within `secs`, its result is returned unchanged. Otherwise the
/// in-flight future is dropped and an [`Error::Timeout`] carrying `operation` and
/// `secs` is returned.
///
/// `operation` should be a short stable identifier (`"resolve_username"`,
/// `"iter_messages"`, etc.) so log searches can pivot on it.
pub(crate) async fn with_timeout<F, T>(operation: &str, secs: u64, fut: F) -> Result<T, Error>
where
    F: Future<Output = Result<T, Error>>,
{
    match tokio::time::timeout(StdDuration::from_secs(secs), fut).await {
        Ok(inner) => inner,
        Err(_) => Err(Error::Timeout {
            operation: operation.to_string(),
            secs,
        }),
    }
}

/// Telegram client wrapping grammers-client
pub struct TelegramClient {
    client: Client,
    session: Arc<SqliteSession>,
    session_path: PathBuf,
    timeouts: TimeoutConfig,
    /// Cached Premium flag for the connected account (None = not yet determined).
    premium: tokio::sync::RwLock<Option<bool>>,
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
                .await
                .map_err(|e| Error::Auth(format!("Failed to open session: {}", e)))?,
        );

        // Create sender pool
        tracing::info!("Creating sender pool...");
        let pool = SenderPool::new(Arc::clone(&session), config.api_id);

        // Create client before spawning runner (Client::new takes SenderPoolFatHandle)
        let client = Client::new(pool.handle);

        // Spawn the runner in the background
        let runner_handle = tokio::spawn(async move {
            pool.runner.run().await;
        });

        tracing::info!("Telegram client created successfully");

        Ok(Self {
            client,
            session,
            session_path: config.session_file.clone(),
            timeouts: config.timeouts.clone(),
            premium: tokio::sync::RwLock::new(None),
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
    ) -> Result<grammers_client::client::LoginToken, Error> {
        self.client
            .request_login_code(phone, api_hash)
            .await
            .map_err(|e| Error::Auth(format!("Failed to request login code: {}", e)))
    }

    /// Sign in with the received code
    pub async fn sign_in(
        &self,
        token: &grammers_client::client::LoginToken,
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
        password_token: grammers_client::client::PasswordToken,
        password: &str,
    ) -> Result<(), Error> {
        self.client
            .check_password(password_token, password.as_bytes())
            .await
            .map_err(|e| Error::Auth(format!("2FA verification failed: {}", e)))?;
        tracing::info!("Successfully signed in with 2FA");
        Ok(())
    }

    /// Resolve a channel reference (numeric ID via dialog walk, or username) to a Peer.
    ///
    /// Extracted from get_message_by_id so download_message_media shares the
    /// exact same resolution semantics and timeout budget.
    async fn resolve_peer(&self, channel_ref: &str) -> Result<grammers_client::peer::Peer, Error> {
        if let Ok(id) = channel_ref.parse::<i64>() {
            // Numeric ID — search through dialogs
            let found = with_timeout("iter_dialogs", self.timeouts.resolve_secs, async {
                let mut dialogs = self.client.iter_dialogs();
                while let Some(dialog) = dialogs.next().await.map_err(|e| {
                    tracing::error!(error = %e, "Failed to iterate dialogs in resolve_peer");
                    Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
                })? {
                    if dialog.peer().id().bare_id() == id {
                        return Ok(Some(dialog.peer().clone()));
                    }
                }
                Ok(None)
            })
            .await?;

            found.ok_or_else(|| {
                tracing::warn!(id, "Channel not found in dialogs by ID");
                Error::InvalidInput(format!("Channel not found: {}", channel_ref))
            })
        } else {
            // Username — resolve directly
            let username = channel_ref.strip_prefix('@').unwrap_or(channel_ref);
            with_timeout("resolve_username", self.timeouts.resolve_secs, async {
                self.client.resolve_username(username).await.map_err(|e| {
                    tracing::error!(username = %username, error = %e, "Failed to resolve username");
                    Error::TelegramApi(format!("Failed to resolve username: {}", e))
                })
            })
            .await?
            .ok_or_else(|| {
                tracing::warn!(username = %username, "Username not found");
                Error::InvalidInput(format!("Channel not found: {}", channel_ref))
            })
        }
    }

    /// Invoke `messages.transcribeAudio` once and parse the result into a
    /// [`TranscriptionState`]. Bounded by the history timeout budget.
    async fn invoke_transcribe(
        &self,
        peer: tl::enums::InputPeer,
        msg_id: i32,
    ) -> Result<TranscriptionState, Error> {
        use crate::telegram::transcription::map_transcribe_rpc_error;

        let request = tl::functions::messages::TranscribeAudio { peer, msg_id };
        let result = with_timeout("transcribe_audio", self.timeouts.history_secs, async {
            self.client
                .invoke(&request)
                .await
                .map_err(map_transcribe_rpc_error)
        })
        .await?;

        let tl::enums::messages::TranscribedAudio::Audio(t) = result;
        Ok(TranscriptionState {
            transcription_id: t.transcription_id,
            text: t.text,
            pending: t.pending,
        })
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

        while let Some(dialog) = dialogs.next().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to iterate dialogs in get_subscribed_channels");
            Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
        })? {
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
            channels_found = channels.len(),
            offset,
            limit,
            "get_subscribed_channels completed"
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
            with_timeout("resolve_username", self.timeouts.resolve_secs, async {
                self.client.resolve_username(username).await.map_err(|e| {
                    tracing::error!(username = %username, error = %e, "Failed to resolve username");
                    Error::TelegramApi(format!("Failed to resolve username: {}", e))
                })
            })
            .await?
            .ok_or_else(|| {
                tracing::warn!(username = %username, "Username not found");
                Error::InvalidInput(format!("Channel not found: {}", identifier))
            })?
        } else if let Ok(id) = identifier.parse::<i64>() {
            // Numeric ID lookup - need to search through dialogs
            let found = with_timeout("iter_dialogs", self.timeouts.resolve_secs, async {
                let mut dialogs = self.client.iter_dialogs();
                while let Some(dialog) = dialogs.next().await.map_err(|e| {
                    tracing::error!(error = %e, "Failed to iterate dialogs in get_channel_info");
                    Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
                })? {
                    if dialog.peer().id().bare_id() == id {
                        return Ok(Some(dialog.peer().clone()));
                    }
                }
                Ok(None)
            })
            .await?;

            found.ok_or_else(|| {
                tracing::warn!(id, "Channel not found in dialogs by ID");
                Error::InvalidInput(format!("Channel not found: {}", identifier))
            })?
        } else {
            // Try as username without @ prefix
            with_timeout("resolve_username", self.timeouts.resolve_secs, async {
                self.client.resolve_username(identifier).await.map_err(|e| {
                    tracing::error!(identifier = %identifier, error = %e, "Failed to resolve username");
                    Error::TelegramApi(format!("Failed to resolve username: {}", e))
                })
            })
            .await?
            .ok_or_else(|| {
                tracing::warn!(identifier = %identifier, "Username not found");
                Error::InvalidInput(format!("Channel not found: {}", identifier))
            })?
        };

        convert_peer_to_channel(&peer).ok_or_else(|| {
            tracing::warn!(
                peer_id = peer.id().bare_id(),
                "Resolved peer is not a channel or group"
            );
            Error::InvalidInput("Not a channel or group".to_string())
        })
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

        // If channel_id is specified, search only that channel
        let (mut messages, channels_searched) = if let Some(channel_id) = &params.channel_id {
            with_timeout(
                "search_messages_channel",
                self.timeouts.search_secs,
                async {
                    let mut messages = Vec::new();
                    let mut channels_searched = 0u32;
                    // Find the channel in our dialogs
                    let mut dialogs = self.client.iter_dialogs();

                    while let Some(dialog) = dialogs.next().await.map_err(|e| {
                        Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
                    })? {
                        let peer = dialog.peer();
                        if peer.id().bare_id() == channel_id.get() {
                            channels_searched += 1;

                            // Search in this specific channel
                            let peer_ref = peer.to_ref().await.ok_or_else(|| {
                                Error::TelegramApi("Failed to convert peer to PeerRef".to_string())
                            })?;
                            let mut search_iter =
                                self.client.search_messages(peer_ref).query(&params.query);

                            // Apply media filter if specified
                            if let Some(ref media_filter) = params.media_filter {
                                search_iter =
                                    search_iter.filter(convert_media_filter(media_filter));
                            }

                            while let Some(msg) = search_iter
                                .next()
                                .await
                                .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?
                            {
                                let msg_time = msg.date();
                                if msg_time < cutoff_time {
                                    break; // reverse chronological order
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
                    Ok((messages, channels_searched))
                },
            )
            .await?
        } else {
            // Search all channels using global search
            let collected = with_timeout("search_all_messages", self.timeouts.search_secs, async {
                let mut messages = Vec::new();
                let mut search_iter = self.client.search_all_messages().query(&params.query);

                if let Some(ref media_filter) = params.media_filter {
                    search_iter = search_iter.filter(convert_media_filter(media_filter));
                }

                while let Some(msg) = search_iter
                    .next()
                    .await
                    .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?
                {
                    let msg_time = msg.date();
                    if msg_time < cutoff_time {
                        continue; // Skip old messages but keep searching
                    }
                    if let Some(peer) = msg.peer()
                        && let Some(converted) = convert_message(&msg, peer)
                    {
                        messages.push(converted);
                        if messages.len() >= params.limit as usize {
                            break;
                        }
                    }
                }
                Ok(messages)
            })
            .await?;

            // Count unique channels in results
            let unique_channels: std::collections::HashSet<_> =
                collected.iter().map(|m| m.channel_id.get()).collect();
            (collected, unique_channels.len() as u32)
        };

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

        // Try to resolve the channel peer directly by username (doesn't require subscription).
        // resolve_username is bounded by `resolve_secs`; a None result silently falls back
        // to the dialog walk, preserving existing behaviour.
        let resolved_peer = if let Some(ref identifier) = params.channel_identifier {
            let username = identifier.strip_prefix('@').unwrap_or(identifier);
            if !username.chars().all(|c| c.is_ascii_digit()) {
                let lookup: Result<Option<_>, Error> =
                    with_timeout("resolve_username", self.timeouts.resolve_secs, async {
                        self.client
                            .resolve_username(username)
                            .await
                            .map_err(|e| Error::TelegramApi(format!("{}", e)))
                    })
                    .await;
                match lookup {
                    Ok(Some(peer)) => Some(peer),
                    Ok(None) => {
                        tracing::warn!(
                            identifier = %identifier,
                            "Username not found, falling back to dialog search"
                        );
                        None
                    }
                    Err(e) => {
                        tracing::warn!(
                            identifier = %identifier,
                            error = %e,
                            "Failed to resolve username, falling back to dialog search"
                        );
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        // Use resolved peer or fall back to dialog search
        let peer = if let Some(peer) = resolved_peer {
            peer
        } else {
            let found = with_timeout("iter_dialogs", self.timeouts.resolve_secs, async {
                let mut dialogs = self.client.iter_dialogs();
                while let Some(dialog) = dialogs.next().await.map_err(|e| {
                    tracing::error!(error = %e, "Failed to iterate dialogs in get_recent_messages");
                    Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
                })? {
                    if dialog.peer().id().bare_id() == params.channel_id.get() {
                        return Ok(Some(dialog.peer().clone()));
                    }
                }
                Ok(None)
            })
            .await?;

            found.ok_or_else(|| {
                tracing::warn!(
                    channel_id = %params.channel_id,
                    "Channel not found in dialogs"
                );
                Error::InvalidInput(format!("Channel not found: {}", params.channel_id))
            })?
        };

        // Use iter_messages to get message history (no search query)
        let peer_ref = peer
            .to_ref()
            .await
            .ok_or_else(|| Error::TelegramApi("Failed to convert peer to PeerRef".to_string()))?;

        let messages = with_timeout("iter_messages", self.timeouts.history_secs, async {
            let mut messages = Vec::new();
            let mut messages_iter = self.client.iter_messages(peer_ref);

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

                if let Some(converted) = convert_message(&msg, &peer) {
                    messages.push(converted);
                    if messages.len() >= params.limit as usize {
                        break;
                    }
                }
            }
            Ok(messages)
        })
        .await?;

        let search_time_ms = start_time.elapsed().as_millis() as u64;
        let total_found = messages.len() as u64;

        tracing::info!(
            channel_id = %params.channel_id,
            identifier = ?params.channel_identifier,
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

    async fn get_message_by_id(
        &self,
        channel_ref: &str,
        message_id: i32,
    ) -> Result<crate::telegram::Message, Error> {
        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }

        // Resolve the channel peer (same pattern as get_channel_info). The resolve /
        // dialog-walk paths share the same hang exposure as the other tools, so they
        // are bounded by `resolve_secs` even though the plan only flags the message
        // fetch itself.
        let peer = self.resolve_peer(channel_ref).await?;

        // Get message by ID using grammers API
        let peer_ref = peer
            .to_ref()
            .await
            .ok_or_else(|| Error::TelegramApi("Failed to convert peer to PeerRef".to_string()))?;

        let messages = with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
            self.client
                .get_messages_by_id(peer_ref, &[message_id])
                .await
                .map_err(|e| {
                    tracing::error!(
                        channel_ref = %channel_ref,
                        message_id,
                        error = %e,
                        "Failed to get message by ID"
                    );
                    Error::TelegramApi(format!("Failed to get message: {}", e))
                })
        })
        .await?;

        // get_messages_by_id returns Vec<Option<Message>> — extract the single result
        let grammers_msg = messages.into_iter().next().flatten().ok_or_else(|| {
            tracing::warn!(
                channel_ref = %channel_ref,
                message_id,
                "Message not found"
            );
            Error::InvalidInput(format!(
                "Message {} not found in channel {}",
                message_id, channel_ref
            ))
        })?;

        // Convert to our domain type
        convert_message(&grammers_msg, &peer).ok_or_else(|| {
            tracing::error!(
                channel_ref = %channel_ref,
                message_id,
                "Failed to convert message to domain type"
            );
            Error::TelegramApi("Failed to convert message".to_string())
        })
    }

    async fn download_message_media(
        &self,
        channel_ref: &str,
        message_id: i32,
        max_dimension: u32,
    ) -> Result<MediaDownload, Error> {
        // Spec limit: never pull more than 20 MB over the network.
        const MAX_DOWNLOAD_BYTES: u64 = 20 * 1024 * 1024;

        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }

        let peer = self.resolve_peer(channel_ref).await?;
        let peer_ref = peer
            .to_ref()
            .await
            .ok_or_else(|| Error::TelegramApi("Failed to convert peer to PeerRef".to_string()))?;

        let messages = with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
            self.client
                .get_messages_by_id(peer_ref, &[message_id])
                .await
                .map_err(|e| {
                    tracing::error!(
                        channel_ref = %channel_ref,
                        message_id,
                        error = %e,
                        "Failed to get message for media download"
                    );
                    Error::TelegramApi(format!("Failed to get message: {}", e))
                })
        })
        .await?;

        let msg = messages.into_iter().next().flatten().ok_or_else(|| {
            Error::InvalidInput(format!(
                "Message {} not found in channel {}",
                message_id, channel_ref
            ))
        })?;

        let media = msg.media().ok_or_else(|| Error::NoVisualMedia {
            media_type: "none".to_string(),
        })?;
        let media_type = convert_media_to_type(&media);

        // Photos are downloaded directly; video-like media contributes only its
        // server-side thumbnail (the spec forbids full video downloads).
        let (thumbs, is_thumbnail) = match &media {
            Media::Photo(photo) => (photo.thumbs(), false),
            Media::Document(doc)
                if matches!(
                    media_type,
                    MediaType::Video | MediaType::Animation | MediaType::VideoNote
                ) =>
            {
                (doc.thumbs(), true)
            }
            _ => {
                return Err(Error::NoVisualMedia {
                    media_type: format!("{:?}", media_type).to_lowercase(),
                });
            }
        };

        let candidates = size_candidates(&thumbs);
        let selected = select_size_candidate(&candidates, max_dimension).ok_or_else(|| {
            Error::DownloadFailed("no downloadable size variant available".to_string())
        })?;

        if selected.size_bytes > MAX_DOWNLOAD_BYTES {
            return Err(Error::MediaTooLarge {
                size_bytes: selected.size_bytes,
                max_bytes: MAX_DOWNLOAD_BYTES,
            });
        }

        let photo_size = thumbs
            .iter()
            .find(|t| t.photo_type() == selected.photo_type)
            .ok_or_else(|| {
                Error::DownloadFailed("selected size variant disappeared".to_string())
            })?;

        let bytes = with_timeout("download_media", self.timeouts.download_secs, async {
            let mut data: Vec<u8> = Vec::new();
            let mut download = self.client.iter_download(photo_size);
            loop {
                match download.next().await {
                    Ok(Some(chunk)) => {
                        data.extend_from_slice(&chunk);
                        // Reported sizes are untrusted input; re-check while streaming.
                        if data.len() as u64 > MAX_DOWNLOAD_BYTES {
                            return Err(Error::MediaTooLarge {
                                size_bytes: data.len() as u64,
                                max_bytes: MAX_DOWNLOAD_BYTES,
                            });
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!(
                            channel_ref = %channel_ref,
                            message_id,
                            error = %e,
                            "Media download failed"
                        );
                        return Err(Error::DownloadFailed(format!("download failed: {}", e)));
                    }
                }
            }
            Ok(data)
        })
        .await?;

        let caption = match msg.text() {
            "" => None,
            text => Some(text.to_string()),
        };

        tracing::info!(
            channel_ref = %channel_ref,
            message_id,
            media_type = ?media_type,
            is_thumbnail,
            selected_type = %selected.photo_type,
            bytes = bytes.len(),
            "Media downloaded"
        );

        Ok(MediaDownload {
            bytes,
            media_type,
            is_thumbnail,
            caption,
            width: Some(selected.width),
            height: Some(selected.height),
            source_size_bytes: selected.size_bytes,
            video_info: extract_video_info(&media),
        })
    }

    async fn is_premium(&self) -> Option<bool> {
        if let Some(cached) = *self.premium.read().await {
            return Some(cached);
        }
        match self.client.get_me().await {
            Ok(me) => {
                let premium = me.is_premium();
                *self.premium.write().await = Some(premium);
                Some(premium)
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to determine Premium status");
                None
            }
        }
    }

    async fn transcribe_audio(
        &self,
        channel_ref: &str,
        message_id: i32,
        timeout_secs: u32,
    ) -> Result<TranscriptionOutcome, Error> {
        use crate::telegram::transcription::{
            POLL_INTERVAL_SECS, ensure_transcribable, poll_until_complete,
        };

        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }

        // Resolve once; reuse the InputPeer for every poll (no repeated dialog walk).
        let peer = self.resolve_peer(channel_ref).await?;
        let peer_ref = peer
            .to_ref()
            .await
            .ok_or_else(|| Error::TelegramApi("Failed to convert peer to PeerRef".to_string()))?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        // Fetch the message to validate media type and read duration.
        let messages = with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
            self.client
                .get_messages_by_id(peer_ref, &[message_id])
                .await
                .map_err(|e| Error::TelegramApi(format!("Failed to get message: {}", e)))
        })
        .await?;
        let msg = messages.into_iter().next().flatten().ok_or_else(|| {
            Error::InvalidInput(format!(
                "Message {} not found in channel {}",
                message_id, channel_ref
            ))
        })?;
        let media = msg.media().ok_or_else(|| Error::NotTranscribable {
            media_type: "none".to_string(),
        })?;
        let media_type = convert_media_to_type(&media);
        ensure_transcribable(media_type)?;
        let duration_seconds = extract_audio_duration(&media);

        // Initial transcribeAudio call.
        let initial = self
            .invoke_transcribe(input_peer.clone(), message_id)
            .await?;

        // Poll (re-invoke) until complete or timeout.
        let (final_state, partial) = poll_until_complete(
            initial,
            StdDuration::from_secs(timeout_secs as u64),
            StdDuration::from_secs(POLL_INTERVAL_SECS),
            || {
                let peer = input_peer.clone();
                async move { self.invoke_transcribe(peer, message_id).await }
            },
        )
        .await;

        Ok(TranscriptionOutcome {
            text: final_state.text,
            partial,
            media_type,
            duration_seconds,
        })
    }
}
