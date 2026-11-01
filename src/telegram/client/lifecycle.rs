//! Client construction, accessors, and connection/account status.
//!
//! Unit of `client` (LM-2).

use super::TelegramClient;
use crate::config::{SearchConfig, TelegramConfig};
use crate::error::Error;
use crate::telegram::timeout::with_timeout;
use crate::telegram::types::AccountStatus;
use grammers_client::{Client, tl};
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use std::path::Path;
use std::sync::Arc;

impl TelegramClient {
    /// Create a new Telegram client
    ///
    /// This handles both first-time setup (no session) and returning users (with session).
    /// If session exists, it will be loaded and used. Otherwise, a new session is created.
    ///
    /// After creation, call [`crate::TelegramClientTrait::refresh_account_status`]
    /// to determine if authentication is needed. Creates parent directories, opens
    /// or migrates the SQLite session, and spawns the sender-pool runner. Config
    /// values other than the raw-media cap are copied without validation; use the configuration validators.
    /// Construction itself has no aggregate timeout or account limiter charge.
    ///
    /// # Errors
    /// Returns [`Error::Auth`] for session directory/open/migration failures.
    /// Does not prompt or perform interactive login.
    pub async fn new(config: &TelegramConfig, search: &SearchConfig) -> Result<Self, Error> {
        config.validate_media_limits().map_err(|_| {
            Error::InvalidInput(
                "telegram.max_download_bytes must fit the 80 MiB raw-memory allowance".into(),
            )
        })?;
        tracing::debug!("Opening private session storage");
        let session =
            crate::telegram::session_storage::open_private_session(&config.session_file).await?;

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
            search_deadline_secs: search.deadline_seconds,
            account_status: tokio::sync::RwLock::new(AccountStatus::default()),
            _runner_handle: runner_handle,
        })
    }

    /// Borrow the upstream grammers client for compatibility/integration.
    /// Calls made through it bypass this crate's validation, charging, redaction
    /// and timeout wrappers; the embedder owns those policies.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Borrow the upstream SQLite session. Mutating it directly bypasses the
    /// connector's lifecycle policy; this signature tracks pinned grammers.
    pub fn session(&self) -> &SqliteSession {
        &self.session
    }

    /// Get the session file path
    pub fn session_path(&self) -> &Path {
        &self.session_path
    }

    pub(super) async fn refresh_account_status_impl(&self) -> Result<AccountStatus, Error> {
        refresh_account_status_with(
            &self.account_status,
            self.timeouts.resolve_secs,
            async {
                self.client
                    .is_authorized()
                    .await
                    .map_err(|e| Error::TelegramApi(format!("Authorization check failed: {e}")))
            },
            async {
                let me = self
                    .client
                    .get_me()
                    .await
                    .map_err(|e| Error::TelegramApi(format!("Account lookup failed: {e}")))?;
                match &me.raw {
                    tl::enums::User::User(user) => Ok(user.premium),
                    tl::enums::User::Empty(_) => {
                        Err(Error::TelegramApi("Account lookup returned no user".into()))
                    }
                }
            },
        )
        .await
    }
}

// The transport seam stays below TelegramClientTrait so timeout/cache policy is
// exercised without replacing the production client with a whole-client mock.
async fn refresh_account_status_with(
    cache: &tokio::sync::RwLock<AccountStatus>,
    timeout_secs: u64,
    authorization: impl std::future::Future<Output = Result<bool, Error>>,
    premium: impl std::future::Future<Output = Result<bool, Error>>,
) -> Result<AccountStatus, Error> {
    let authorized = with_timeout("account_authorization", timeout_secs, authorization).await?;
    {
        let mut status = cache.write().await;
        status.authorized = authorized;
        status.authorization_checked_at = Some(chrono::Utc::now());
        if !authorized {
            status.premium = None;
            status.premium_checked_at = None;
        }
    }
    if authorized {
        match with_timeout("account_premium", timeout_secs, premium).await {
            Ok(premium) => {
                let mut status = cache.write().await;
                // Do not restore account data after a concurrent successful logout observation.
                if status.authorized {
                    status.premium = Some(premium);
                    status.premium_checked_at = Some(chrono::Utc::now());
                }
            }
            Err(error) => {
                tracing::warn!(%error, "Premium refresh failed; keeping last successful observation");
            }
        }
    }
    Ok(*cache.read().await)
}

#[cfg(test)]
#[path = "tests/lifecycle_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "tests/session_permissions.rs"]
mod permission_tests;
