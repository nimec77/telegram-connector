//! Client construction, accessors, and connection/account status.
//!
//! Unit of `client` (LM-2).

use super::*;

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
            max_download_bytes: config.max_download_bytes,
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

    pub(super) async fn is_connected_impl(&self) -> bool {
        self.client.is_authorized().await.unwrap_or_default()
    }

    pub(super) async fn is_premium_impl(&self) -> Option<bool> {
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
}
