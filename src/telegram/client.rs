use crate::config::TelegramConfig;
use crate::error::Error;
use crate::telegram::types::{
    Channel, ChannelId, ChannelName, MediaType, Message, MessageId, QueryMetadata, SearchParams,
    SearchResult, UserId, Username,
};
use chrono::{Duration, Utc};
use grammers_client::Client;
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::task::JoinHandle;

/// Trait for Telegram client operations (allows mocking in tests)
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait TelegramClientTrait: Send + Sync {
    /// Search for messages matching the given parameters
    async fn search_messages(&self, params: &SearchParams) -> Result<SearchResult, Error>;

    /// Get information about a specific channel by username or ID
    async fn get_channel_info(&self, identifier: &str) -> Result<Channel, Error>;

    /// Get list of subscribed channels with pagination
    async fn get_subscribed_channels(&self, limit: u32, offset: u32)
    -> Result<Vec<Channel>, Error>;

    /// Check if client is connected and authorized
    async fn is_connected(&self) -> bool;
}

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

    /// Convert grammers Peer to our Channel type
    fn convert_peer_to_channel(peer: &grammers_client::types::Peer) -> Option<Channel> {
        use grammers_client::types::Peer;

        match peer {
            Peer::Channel(ch) => {
                let id = ChannelId::new(ch.bare_id()).ok()?;
                let name = ChannelName::new(ch.title()).ok()?;
                let username = ch
                    .username()
                    .and_then(|u| Username::new(u).ok())
                    .unwrap_or_else(|| Username::new("unknown").unwrap());

                Some(Channel {
                    id,
                    name,
                    username,
                    description: None, // Not available from basic chat info
                    member_count: 0,   // Would need additional API call
                    is_verified: ch.raw.verified,
                    is_public: ch.username().is_some(),
                    is_subscribed: true, // We're iterating our dialogs, so we're subscribed
                    last_message_date: None,
                })
            }
            Peer::Group(g) => {
                // Include groups as they behave like channels for our purposes
                let id = ChannelId::new(g.id().bare_id()).ok()?;
                let name = ChannelName::new(g.title().unwrap_or("Unknown")).ok()?;
                let username = g
                    .username()
                    .and_then(|u| Username::new(u).ok())
                    .unwrap_or_else(|| Username::new("group").unwrap());

                Some(Channel {
                    id,
                    name,
                    username,
                    description: None,
                    member_count: 0,
                    is_verified: false,
                    is_public: g.username().is_some(),
                    is_subscribed: true,
                    last_message_date: None,
                })
            }
            _ => None, // Skip users
        }
    }

    /// Convert grammers Message to our Message type
    fn convert_message(
        msg: &grammers_client::types::Message,
        peer: &grammers_client::types::Peer,
    ) -> Option<Message> {
        use grammers_client::types::Peer;

        let (channel_id, channel_name, channel_username) = match peer {
            Peer::Channel(ch) => (
                ChannelId::new(ch.bare_id()).ok()?,
                ChannelName::new(ch.title()).ok()?,
                ch.username()
                    .and_then(|u| Username::new(u).ok())
                    .unwrap_or_else(|| Username::new("unknown").unwrap()),
            ),
            Peer::Group(g) => (
                ChannelId::new(g.id().bare_id()).ok()?,
                ChannelName::new(g.title().unwrap_or("Unknown")).ok()?,
                g.username()
                    .and_then(|u| Username::new(u).ok())
                    .unwrap_or_else(|| Username::new("group").unwrap()),
            ),
            Peer::User(u) => (
                ChannelId::new(u.bare_id()).ok()?,
                ChannelName::new(u.first_name().unwrap_or("User")).ok()?,
                u.username()
                    .and_then(|un| Username::new(un).ok())
                    .unwrap_or_else(|| Username::new("user").unwrap()),
            ),
        };

        let message_id = MessageId::new(msg.id() as i64).ok()?;

        // Get sender info
        let (sender_id, sender_name) = if let Some(sender) = msg.sender() {
            let id = UserId::new(sender.id().bare_id()).ok();
            let name = sender.name().map(|s| s.to_string());
            (id, name)
        } else {
            (None, None)
        };

        // Check for media
        let (has_media, media_type) = if msg.media().is_some() {
            (true, MediaType::Document) // Default to document
        } else {
            (false, MediaType::None)
        };

        Some(Message {
            id: message_id,
            channel_id,
            channel_name,
            channel_username,
            text: msg.text().to_string(),
            timestamp: msg.date(),
            sender_id,
            sender_name,
            has_media,
            media_type,
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
    ) -> Result<Vec<Channel>, Error> {
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
            if let Some(channel) = Self::convert_peer_to_channel(peer) {
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

    async fn get_channel_info(&self, identifier: &str) -> Result<Channel, Error> {
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

        Self::convert_peer_to_channel(&peer)
            .ok_or_else(|| Error::InvalidInput("Not a channel or group".to_string()))
    }

    async fn search_messages(&self, params: &SearchParams) -> Result<SearchResult, Error> {
        // Validate parameters
        if params.query.is_empty() {
            return Err(Error::InvalidInput(
                "Search query cannot be empty".to_string(),
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

                        if let Some(converted) = Self::convert_message(&msg, peer) {
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
                    && let Some(converted) = Self::convert_message(&msg, peer)
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
        messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        let search_time_ms = start_time.elapsed().as_millis() as u64;
        let total_found = messages.len() as u64;

        tracing::info!(
            query = %params.query,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create test channel
    fn create_test_channel(id: i64, name: &str) -> Channel {
        Channel {
            id: ChannelId::new(id).unwrap(),
            name: ChannelName::new(name).unwrap(),
            username: Username::new("testchannel").unwrap(),
            description: Some("Test channel".to_string()),
            member_count: 1000,
            is_verified: false,
            is_public: true,
            is_subscribed: true,
            last_message_date: None,
        }
    }

    // Helper to create test message
    fn create_test_message(id: i32, text: &str, channel_id: i64) -> Message {
        Message {
            id: MessageId::new(id as i64).unwrap(),
            channel_id: ChannelId::new(channel_id).unwrap(),
            channel_name: ChannelName::new("TestChannel").unwrap(),
            channel_username: Username::new("testchannel").unwrap(),
            text: text.to_string(),
            timestamp: chrono::Utc::now(),
            sender_id: Some(UserId::new(123).unwrap()),
            sender_name: Some("Test User".to_string()),
            has_media: false,
            media_type: MediaType::None,
        }
    }

    // ========================================
    // Mock-based tests
    // ========================================

    #[tokio::test]
    async fn mock_is_connected_returns_true() {
        let mut mock = MockTelegramClientTrait::new();
        mock.expect_is_connected().times(1).returning(|| true);

        assert!(mock.is_connected().await);
    }

    #[tokio::test]
    async fn mock_is_connected_returns_false() {
        let mut mock = MockTelegramClientTrait::new();
        mock.expect_is_connected().times(1).returning(|| false);

        assert!(!mock.is_connected().await);
    }

    #[tokio::test]
    async fn mock_get_subscribed_channels_returns_list() {
        let mut mock = MockTelegramClientTrait::new();

        let expected_channels = vec![
            create_test_channel(1, "Channel1"),
            create_test_channel(2, "Channel2"),
        ];
        let expected_clone = expected_channels.clone();

        mock.expect_get_subscribed_channels()
            .with(mockall::predicate::eq(10), mockall::predicate::eq(0))
            .times(1)
            .returning(move |_, _| Ok(expected_clone.clone()));

        let result = mock.get_subscribed_channels(10, 0).await;
        assert!(result.is_ok());
        let channels = result.unwrap();
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name.as_str(), "Channel1");
    }

    #[tokio::test]
    async fn mock_get_subscribed_channels_respects_pagination() {
        let mut mock = MockTelegramClientTrait::new();

        // First page
        mock.expect_get_subscribed_channels()
            .with(mockall::predicate::eq(2), mockall::predicate::eq(0))
            .times(1)
            .returning(|_, _| {
                Ok(vec![
                    create_test_channel(1, "Channel1"),
                    create_test_channel(2, "Channel2"),
                ])
            });

        // Second page
        mock.expect_get_subscribed_channels()
            .with(mockall::predicate::eq(2), mockall::predicate::eq(2))
            .times(1)
            .returning(|_, _| Ok(vec![create_test_channel(3, "Channel3")]));

        let page1 = mock.get_subscribed_channels(2, 0).await.unwrap();
        assert_eq!(page1.len(), 2);

        let page2 = mock.get_subscribed_channels(2, 2).await.unwrap();
        assert_eq!(page2.len(), 1);
    }

    #[tokio::test]
    async fn mock_get_channel_info_by_username() {
        let mut mock = MockTelegramClientTrait::new();
        let expected_channel = create_test_channel(123, "TestChannel");
        let expected_clone = expected_channel.clone();

        mock.expect_get_channel_info()
            .with(mockall::predicate::eq("@testchannel"))
            .times(1)
            .returning(move |_| Ok(expected_clone.clone()));

        let result = mock.get_channel_info("@testchannel").await;
        assert!(result.is_ok());
        let channel = result.unwrap();
        assert_eq!(channel.name.as_str(), "TestChannel");
    }

    #[tokio::test]
    async fn mock_get_channel_info_by_id() {
        let mut mock = MockTelegramClientTrait::new();
        let expected_channel = create_test_channel(123, "TestChannel");
        let expected_clone = expected_channel.clone();

        mock.expect_get_channel_info()
            .with(mockall::predicate::eq("123"))
            .times(1)
            .returning(move |_| Ok(expected_clone.clone()));

        let result = mock.get_channel_info("123").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn mock_get_channel_info_empty_identifier_fails() {
        let mut mock = MockTelegramClientTrait::new();

        mock.expect_get_channel_info()
            .with(mockall::predicate::eq(""))
            .times(1)
            .returning(|_| {
                Err(Error::InvalidInput(
                    "Channel identifier cannot be empty".to_string(),
                ))
            });

        let result = mock.get_channel_info("").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn mock_search_messages_returns_results() {
        let mut mock = MockTelegramClientTrait::new();

        let expected_messages = vec![
            create_test_message(1, "Test message 1", 100),
            create_test_message(2, "Test message 2", 100),
        ];

        let expected_result = SearchResult {
            messages: expected_messages.clone(),
            total_found: 2,
            search_time_ms: 100,
            query_metadata: QueryMetadata {
                query: "test".to_string(),
                hours_back: 24,
                channels_searched: 1,
            },
        };
        let expected_clone = expected_result.clone();

        mock.expect_search_messages()
            .times(1)
            .returning(move |_| Ok(expected_clone.clone()));

        let params = SearchParams::new("test".to_string());
        let result = mock.search_messages(&params).await;

        assert!(result.is_ok());
        let search_result = result.unwrap();
        assert_eq!(search_result.messages.len(), 2);
        assert_eq!(search_result.total_found, 2);
    }

    #[tokio::test]
    async fn mock_search_messages_empty_query_fails() {
        let mut mock = MockTelegramClientTrait::new();

        mock.expect_search_messages().times(1).returning(|_| {
            Err(Error::InvalidInput(
                "Search query cannot be empty".to_string(),
            ))
        });

        let params = SearchParams::new("".to_string());
        let result = mock.search_messages(&params).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn mock_search_messages_respects_limit() {
        let mut mock = MockTelegramClientTrait::new();

        // Create 5 messages but limit to 3
        let all_messages = vec![
            create_test_message(1, "Message 1", 100),
            create_test_message(2, "Message 2", 100),
            create_test_message(3, "Message 3", 100),
        ];

        let expected_result = SearchResult {
            messages: all_messages.clone(),
            total_found: 3,
            search_time_ms: 100,
            query_metadata: QueryMetadata {
                query: "test".to_string(),
                hours_back: 24,
                channels_searched: 1,
            },
        };
        let expected_clone = expected_result.clone();

        mock.expect_search_messages()
            .times(1)
            .returning(move |_| Ok(expected_clone.clone()));

        let params = SearchParams {
            query: "test".to_string(),
            limit: 3,
            ..Default::default()
        };

        let result = mock.search_messages(&params).await;
        assert!(result.is_ok());
        let search_result = result.unwrap();
        assert_eq!(search_result.messages.len(), 3);
    }

    #[tokio::test]
    async fn mock_search_messages_with_channel_filter() {
        let mut mock = MockTelegramClientTrait::new();

        let expected_messages = vec![create_test_message(1, "Message from specific channel", 100)];

        let expected_result = SearchResult {
            messages: expected_messages.clone(),
            total_found: 1,
            search_time_ms: 100,
            query_metadata: QueryMetadata {
                query: "test".to_string(),
                hours_back: 24,
                channels_searched: 1,
            },
        };
        let expected_clone = expected_result.clone();

        mock.expect_search_messages()
            .times(1)
            .returning(move |_| Ok(expected_clone.clone()));

        let params = SearchParams {
            query: "test".to_string(),
            channel_id: Some(ChannelId::new(100).unwrap()),
            ..Default::default()
        };

        let result = mock.search_messages(&params).await;
        assert!(result.is_ok());
        let search_result = result.unwrap();
        assert_eq!(search_result.query_metadata.channels_searched, 1);
    }
}
