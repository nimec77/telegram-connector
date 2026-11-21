//! Durable account generations and per-client grants under one private root.

use crate::contracts::groups::GroupRef;
use crate::contracts::{GrantSnapshot, GrantUpdate, ReadGrant};
use libsql::{Builder, Connection, params};
use serde::Serialize;
use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};
use telegram_core::telegram::session_storage::prepare_private_database;
use tokio::sync::Mutex;

const LINK_RETENTION_SECONDS: u64 = 86_400;
const SESSION_FILE: &str = "session.db";
const SCHEMA: &str = "
BEGIN IMMEDIATE;
CREATE TABLE IF NOT EXISTS generations (
    id TEXT PRIMARY KEY,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('starting', 'verifying', 'active', 'pending', 'logged_out', 'manual')),
    telegram_id INTEGER,
    retired_at INTEGER
) STRICT;
CREATE UNIQUE INDEX IF NOT EXISTS one_local_owner ON generations(issuer, subject)
    WHERE state IN ('starting', 'verifying', 'active');
CREATE UNIQUE INDEX IF NOT EXISTS one_telegram_owner ON generations(telegram_id)
    WHERE state = 'active';
CREATE TABLE IF NOT EXISTS grants (
    generation TEXT NOT NULL REFERENCES generations(id),
    client TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    read_scope TEXT NOT NULL,
    send_enabled INTEGER NOT NULL CHECK (send_enabled IN (0, 1)),
    PRIMARY KEY (generation, client)
) STRICT;
CREATE TABLE IF NOT EXISTS eligible_content (
    channel_id TEXT PRIMARY KEY
) STRICT;
PRAGMA user_version = 1;
COMMIT;
";

/// Account metadata or lifecycle failure with no secret-bearing source payload.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("account storage unavailable")]
    Unavailable,
    #[error("account state conflict")]
    Conflict,
    #[error("linked account unavailable")]
    Missing,
    #[error("invalid grant")]
    InvalidGrant,
    #[error("data root is already locked")]
    RootLocked,
}

/// Immutable generation key and its server-assigned session path.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct AccountGeneration {
    id: String,
    path: PathBuf,
}

impl AccountGeneration {
    pub(super) fn id(&self) -> &str {
        &self.id
    }

    /// Path assigned to this generation exclusively.
    pub fn session_file(&self) -> &Path {
        &self.path
    }
}

/// Safe account-page view of local linking and retired session cleanup.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    Unlinked,
    Linking,
    Linked,
    RevocationPending,
    ManualRevocation,
    LinkedWithRevocationPending,
    LinkedWithManualRevocation,
}

/// A retired generation eligible for cleanup, irrespective of newer links.
#[derive(Clone)]
pub struct RetiredGeneration {
    generation: AccountGeneration,
    retired_at: u64,
}

impl RetiredGeneration {
    pub(super) fn generation(&self) -> &AccountGeneration {
        &self.generation
    }

    /// Only this session may be opened for remote logout.
    pub fn session_file(&self) -> &Path {
        self.generation.session_file()
    }

    /// Whether the local erasure deadline has elapsed.
    pub fn expired_at(&self, now: u64) -> bool {
        now >= self.retired_at.saturating_add(LINK_RETENTION_SECONDS)
    }
}

/// Result of attempting remote logout on a retired session.
pub enum RetirementOutcome {
    LoggedOut,
    ManualRevocation,
}

/// Private service metadata database and its exclusive data-root lock.
pub struct AccountStore {
    db: Mutex<Connection>,
    root: PathBuf,
    _lock: File,
}

impl AccountStore {
    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    /// Acquire the data-root lock, then open and migrate its metadata database.
    ///
    /// # Errors
    /// Returns a conflict for an occupied root or fails if storage is insecure or unavailable.
    pub async fn open(root: &Path) -> Result<Self, StorageError> {
        let lock_path = prepare_private_database(&root.join("service.lock"))
            .await
            .map_err(|_| StorageError::Unavailable)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|_| StorageError::Unavailable)?;
        lock.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => StorageError::RootLocked,
            TryLockError::Error(_) => StorageError::Unavailable,
        })?;
        let root = lock_path
            .parent()
            .ok_or(StorageError::Unavailable)?
            .to_owned();
        let path = prepare_private_database(&root.join("accounts.db"))
            .await
            .map_err(|_| StorageError::Unavailable)?;
        let db = Builder::new_local(path)
            .build()
            .await
            .map_err(db_failure)?
            .connect()
            .map_err(db_failure)?;
        let mut rows = db
            .query("PRAGMA user_version", ())
            .await
            .map_err(db_failure)?;
        let version = rows
            .next()
            .await
            .map_err(db_failure)?
            .ok_or(StorageError::Unavailable)?
            .get::<i64>(0)
            .map_err(db_failure)?;
        drop(rows);
        if version > 2 {
            return Err(StorageError::Unavailable);
        }
        db.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL;")
            .await
            .map_err(db_failure)?;
        if version == 0 {
            db.execute_batch(SCHEMA).await.map_err(db_failure)?;
        }
        if version < 2 {
            db.execute_batch(
                "BEGIN IMMEDIATE;
                 ALTER TABLE grants ADD COLUMN group_refs TEXT NOT NULL DEFAULT '[]';
                 PRAGMA user_version = 2;
                 COMMIT;",
            )
            .await
            .map_err(db_failure)?;
        }
        let store = Self {
            db: Mutex::new(db),
            root,
            _lock: lock,
        };
        store.recover_starting().await?;
        Ok(store)
    }

    /// Reserve a distinct generation and create its private session directory.
    ///
    /// # Errors
    /// Returns a conflict if the principal already has a link or pending attempt.
    pub async fn start(
        &self,
        issuer: &str,
        subject: &str,
    ) -> Result<AccountGeneration, StorageError> {
        let mut entropy = [0; 16];
        getrandom::fill(&mut entropy).map_err(|_| StorageError::Unavailable)?;
        let id = entropy
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let generation = self.generation(id)?;
        let db = self.db.lock().await;
        let inserted = db.execute(
            "INSERT OR IGNORE INTO generations(id, issuer, subject, state) VALUES (?1, ?2, ?3, 'starting')",
            params![generation.id.as_str(), issuer, subject],
        ).await.map_err(db_failure)?;
        if inserted != 1 {
            return Err(StorageError::Conflict);
        }
        let directory = generation.path.parent().ok_or(StorageError::Unavailable)?;
        if private_dir(directory).is_err() {
            db.execute(
                "DELETE FROM generations WHERE id = ?1",
                [generation.id.as_str()],
            )
            .await
            .map_err(db_failure)?;
            return Err(StorageError::Unavailable);
        }
        Ok(generation)
    }

    /// Activate a generation after Telegram has verified the authorized user.
    ///
    /// # Errors
    /// Returns a conflict if either identity has already been linked.
    pub async fn activate(
        &self,
        generation: &AccountGeneration,
        telegram_id: i64,
    ) -> Result<(), StorageError> {
        if telegram_id <= 0 {
            return Err(StorageError::Conflict);
        }
        let db = self.db.lock().await;
        let updated = db
            .execute(
                "UPDATE OR IGNORE generations SET state = 'active', telegram_id = ?2, retired_at = NULL
             WHERE id = ?1 AND state IN ('starting', 'verifying')",
                params![generation.id.as_str(), telegram_id],
            )
            .await
            .map_err(db_failure)?;
        changed(updated)
    }

    /// Load the active generation of an authenticated principal.
    ///
    /// # Errors
    /// Returns a storage failure if the account mapping cannot be read.
    pub async fn active(
        &self,
        issuer: &str,
        subject: &str,
    ) -> Result<Option<AccountGeneration>, StorageError> {
        let db = self.db.lock().await;
        let mut rows = db.query(
            "SELECT id FROM generations WHERE issuer = ?1 AND subject = ?2 AND state = 'active'",
            params![issuer, subject],
        ).await.map_err(db_failure)?;
        rows.next()
            .await
            .map_err(db_failure)?
            .map(|row| self.generation(row.get::<String>(0).map_err(db_failure)?))
            .transpose()
    }

    pub(super) async fn active_batch(
        &self,
        after: &str,
    ) -> Result<Vec<(AccountGeneration, String, String)>, StorageError> {
        let db = self.db.lock().await;
        let mut rows = db
            .query(
                "SELECT id, issuer, subject FROM generations
                 WHERE state = 'active' AND id > ?1 ORDER BY id LIMIT 16",
                [after],
            )
            .await
            .map_err(db_failure)?;
        let mut accounts = Vec::new();
        while let Some(row) = rows.next().await.map_err(db_failure)? {
            accounts.push((
                self.generation(row.get::<String>(0).map_err(db_failure)?)?,
                row.get::<String>(1).map_err(db_failure)?,
                row.get::<String>(2).map_err(db_failure)?,
            ));
        }
        Ok(accounts)
    }

    pub(super) async fn is_active_generation(
        &self,
        generation: &AccountGeneration,
    ) -> Result<bool, StorageError> {
        let db = self.db.lock().await;
        let mut rows = db
            .query(
                "SELECT 1 FROM generations WHERE id = ?1 AND state = 'active'",
                [generation.id()],
            )
            .await
            .map_err(db_failure)?;
        Ok(rows.next().await.map_err(db_failure)?.is_some())
    }

    /// Durably disable the active generation before its session is touched.
    ///
    /// # Errors
    /// Returns a storage error if local retirement cannot be committed.
    pub async fn retire(
        &self,
        issuer: &str,
        subject: &str,
        now: u64,
    ) -> Result<Option<AccountGeneration>, StorageError> {
        let db = self.db.lock().await;
        let mut rows = db
            .query(
                "UPDATE generations SET state = 'pending', retired_at = ?3, telegram_id = NULL
             WHERE issuer = ?1 AND subject = ?2 AND state = 'active' RETURNING id",
                params![issuer, subject, integer(now)?],
            )
            .await
            .map_err(db_failure)?;
        let retired = rows
            .next()
            .await
            .map_err(db_failure)?
            .map(|row| self.generation(row.get::<String>(0).map_err(db_failure)?))
            .transpose()?;
        // Finish the statement so the RETURNING mutation is durably committed.
        if rows.next().await.map_err(db_failure)?.is_some() {
            return Err(StorageError::Unavailable);
        }
        Ok(retired)
    }

    /// List retired generations in bounded cleanup batches.
    ///
    /// # Errors
    /// Returns a storage failure if cleanup metadata cannot be read.
    pub async fn pending_retirements(&self) -> Result<Vec<RetiredGeneration>, StorageError> {
        self.retirements_after(None).await
    }

    pub(super) async fn retirements_after(
        &self,
        after: Option<&RetiredGeneration>,
    ) -> Result<Vec<RetiredGeneration>, StorageError> {
        let db = self.db.lock().await;
        let mut rows = db.query(
            "SELECT id, retired_at FROM generations WHERE state = 'pending' AND id > ?1 ORDER BY id LIMIT 64",
            [after.map_or("", |item| item.generation.id.as_str())],
        ).await.map_err(db_failure)?;
        let mut pending = Vec::new();
        while let Some(row) = rows.next().await.map_err(db_failure)? {
            pending.push(RetiredGeneration {
                generation: self.generation(row.get::<String>(0).map_err(db_failure)?)?,
                retired_at: row.get::<u64>(1).map_err(db_failure)?,
            });
        }
        Ok(pending)
    }

    /// Purge the retired files and preserve their safe remote-revocation status.
    ///
    /// # Errors
    /// Returns a storage failure if purging or committing the status fails.
    pub async fn finish_retirement(
        &self,
        retired: &RetiredGeneration,
        outcome: RetirementOutcome,
    ) -> Result<(), StorageError> {
        let db = self.db.lock().await;
        let id = retired.generation.id.as_str();
        let mut rows = db
            .query(
                "SELECT 1 FROM generations WHERE id = ?1 AND state = 'pending'",
                [id],
            )
            .await
            .map_err(db_failure)?;
        if rows.next().await.map_err(db_failure)?.is_none() {
            return Err(StorageError::Missing);
        }
        drop(rows);
        // Preserve the disposition before erasure so a restart can finish the purge
        // without treating a successfully logged-out session as unconfirmed.
        let state = match outcome {
            RetirementOutcome::LoggedOut => "logged_out",
            RetirementOutcome::ManualRevocation => "manual",
        };
        db.execute(
            "UPDATE generations SET state = ?2 WHERE id = ?1",
            params![id, state],
        )
        .await
        .map_err(db_failure)?;
        remove_generation(&self.generation(id.to_owned())?)?;
        db.execute("DELETE FROM grants WHERE generation = ?1", [id])
            .await
            .map_err(db_failure)?;
        if matches!(outcome, RetirementOutcome::LoggedOut) {
            db.execute("DELETE FROM generations WHERE id = ?1", [id])
                .await
                .map_err(db_failure)?;
        }
        Ok(())
    }

    /// Get a non-secret account state for the authenticated principal.
    ///
    /// # Errors
    /// Returns a storage failure if the local state cannot be read.
    pub async fn status(&self, issuer: &str, subject: &str) -> Result<LinkState, StorageError> {
        let db = self.db.lock().await;
        let mut rows = db
            .query(
                "SELECT coalesce(max(state = 'active'), 0), coalesce(max(state IN ('starting', 'verifying')), 0),
                 coalesce(max(state = 'pending'), 0), coalesce(max(state = 'manual'), 0)
             FROM generations WHERE issuer = ?1 AND subject = ?2",
                params![issuer, subject],
            )
            .await
            .map_err(db_failure)?;
        let row = rows
            .next()
            .await
            .map_err(db_failure)?
            .ok_or(StorageError::Unavailable)?;
        let active = row.get::<i64>(0).map_err(db_failure)? != 0;
        let starting = row.get::<i64>(1).map_err(db_failure)? != 0;
        let pending = row.get::<i64>(2).map_err(db_failure)? != 0;
        let manual = row.get::<i64>(3).map_err(db_failure)? != 0;
        Ok(match (active, pending, manual, starting) {
            (true, true, _, _) => LinkState::LinkedWithRevocationPending,
            (true, false, true, _) => LinkState::LinkedWithManualRevocation,
            (true, false, false, _) => LinkState::Linked,
            (false, true, _, _) => LinkState::RevocationPending,
            (false, false, true, _) => LinkState::ManualRevocation,
            (false, false, false, true) => LinkState::Linking,
            (false, false, false, false) => LinkState::Unlinked,
        })
    }

    /// Fetch the client grant, including the initial empty revision.
    ///
    /// # Errors
    /// Fails if the generation is no longer active or storage is unavailable.
    pub async fn grant(
        &self,
        generation: &AccountGeneration,
        client: &str,
    ) -> Result<GrantSnapshot, StorageError> {
        let db = self.db.lock().await;
        let mut rows = db.query(
            "SELECT grants.revision, grants.read_scope, grants.send_enabled, grants.group_refs
             FROM generations LEFT JOIN grants ON grants.generation = generations.id AND grants.client = ?2
             WHERE generations.id = ?1 AND generations.state = 'active'",
            params![generation.id.as_str(), client],
        ).await.map_err(db_failure)?;
        let row = rows
            .next()
            .await
            .map_err(db_failure)?
            .ok_or(StorageError::Missing)?;
        let Some(revision) = row.get::<Option<u64>>(0).map_err(db_failure)? else {
            return Ok(GrantSnapshot::default());
        };
        let read = serde_json::from_str(&row.get::<String>(1).map_err(db_failure)?)
            .map_err(|_| StorageError::Unavailable)?;
        let group_refs = serde_json::from_str(&row.get::<String>(3).map_err(db_failure)?)
            .map_err(|_| StorageError::Unavailable)?;
        Ok(GrantSnapshot {
            group_refs,
            revision,
            read,
            send_enabled: row.get::<i64>(2).map_err(db_failure)? != 0,
        })
    }

    /// Compare-and-swap a client grant on a still-active generation.
    ///
    /// # Errors
    /// Returns a conflict for a stale revision or retired generation, or an invalid grant.
    pub async fn update_grant(
        &self,
        generation: &AccountGeneration,
        client: &str,
        update: GrantUpdate,
    ) -> Result<GrantSnapshot, StorageError> {
        self.update_grant_with_groups(generation, client, update, None)
            .await
    }

    pub(super) async fn update_grant_with_groups(
        &self,
        generation: &AccountGeneration,
        client: &str,
        update: GrantUpdate,
        group_refs: Option<Vec<String>>,
    ) -> Result<GrantSnapshot, StorageError> {
        let read = normalize_grant(update.read)?;
        let group_refs = group_refs.map(normalize_group_grant).transpose()?;
        let groups = group_refs
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|_| StorageError::InvalidGrant)?;
        let revision = update
            .expected_revision
            .checked_add(1)
            .ok_or(StorageError::InvalidGrant)?;
        let encoded = serde_json::to_string(&read).map_err(|_| StorageError::InvalidGrant)?;
        let db = self.db.lock().await;
        let mut rows = db.query(
            "INSERT INTO grants(generation, client, revision, read_scope, send_enabled, group_refs)
             SELECT ?1, ?2, ?3, ?4, ?5, coalesce(?7, '[]') WHERE EXISTS
                 (SELECT 1 FROM generations WHERE id = ?1 AND state = 'active')
                 AND coalesce((SELECT revision FROM grants WHERE generation = ?1 AND client = ?2), 0) = ?6
             ON CONFLICT(generation, client) DO UPDATE SET
                 revision = excluded.revision, read_scope = excluded.read_scope,
                 send_enabled = excluded.send_enabled, group_refs = coalesce(?7, grants.group_refs)
             RETURNING group_refs",
            params![generation.id.as_str(), client, integer(revision)?, encoded, i64::from(update.send_enabled), integer(update.expected_revision)?, groups],
        ).await.map_err(db_failure)?;
        let row = rows
            .next()
            .await
            .map_err(db_failure)?
            .ok_or(StorageError::Conflict)?;
        let group_refs = serde_json::from_str(&row.get::<String>(0).map_err(db_failure)?)
            .map_err(|_| StorageError::Unavailable)?;
        while rows.next().await.map_err(db_failure)?.is_some() {}
        Ok(GrantSnapshot {
            group_refs,
            revision,
            read,
            send_enabled: update.send_enabled,
        })
    }

    pub(super) async fn begin_verification(
        &self,
        generation: &AccountGeneration,
        now: u64,
    ) -> Result<(), StorageError> {
        let updated = self.db.lock().await.execute(
            "UPDATE generations SET state = 'verifying', retired_at = ?2 WHERE id = ?1 AND state = 'starting'",
            params![generation.id.as_str(), integer(now)?],
        ).await.map_err(db_failure)?;
        changed(updated)
    }

    pub(super) async fn await_password(
        &self,
        generation: &AccountGeneration,
    ) -> Result<(), StorageError> {
        let updated = self.db.lock().await.execute(
            "UPDATE generations SET state = 'starting', retired_at = NULL WHERE id = ?1 AND state = 'verifying'",
            [generation.id.as_str()],
        ).await.map_err(db_failure)?;
        changed(updated)
    }

    pub(super) async fn retire_starting(
        &self,
        generation: &AccountGeneration,
        now: u64,
    ) -> Result<(), StorageError> {
        self.db
            .lock()
            .await
            .execute(
                "UPDATE generations SET state = 'pending', retired_at = coalesce(retired_at, ?2)
             WHERE id = ?1 AND state IN ('starting', 'verifying')",
                params![generation.id.as_str(), integer(now)?],
            )
            .await
            .map_err(db_failure)?;
        Ok(())
    }

    pub(super) async fn abandon(&self, generation: &AccountGeneration) -> Result<(), StorageError> {
        let db = self.db.lock().await;
        let mut rows = db
            .query(
                "SELECT 1 FROM generations WHERE id = ?1 AND state = 'starting'",
                [generation.id.as_str()],
            )
            .await
            .map_err(db_failure)?;
        if rows.next().await.map_err(db_failure)?.is_none() {
            return Ok(());
        }
        drop(rows);
        remove_generation(generation)?;
        db.execute(
            "DELETE FROM generations WHERE id = ?1 AND state = 'starting'",
            [generation.id.as_str()],
        )
        .await
        .map_err(db_failure)?;
        Ok(())
    }

    pub(super) async fn replace_eligible_channels(
        &self,
        channels: &std::collections::HashSet<String>,
    ) -> Result<(), StorageError> {
        let db = self.db.lock().await;
        let transaction = db.transaction().await.map_err(db_failure)?;
        transaction
            .execute("DELETE FROM eligible_content", ())
            .await
            .map_err(db_failure)?;
        for channel in channels {
            transaction
                .execute(
                    "INSERT INTO eligible_content VALUES (?1)",
                    [channel.as_str()],
                )
                .await
                .map_err(db_failure)?;
        }
        transaction.commit().await.map_err(db_failure)
    }

    async fn recover_starting(&self) -> Result<(), StorageError> {
        let db = self.db.lock().await;
        // The process may have died after Telegram accepted a code/password.
        // Preserve the only material that can log out that old session.
        db.execute(
            "UPDATE generations SET state = 'pending' WHERE state = 'verifying'",
            (),
        )
        .await
        .map_err(db_failure)?;
        let mut rows = db
            .query(
                "SELECT id FROM generations WHERE state IN ('starting', 'logged_out', 'manual')",
                (),
            )
            .await
            .map_err(db_failure)?;
        while let Some(row) = rows.next().await.map_err(db_failure)? {
            let generation = self.generation(row.get::<String>(0).map_err(db_failure)?)?;
            remove_generation(&generation)?;
        }
        drop(rows);
        db.execute_batch(
            "DELETE FROM grants WHERE generation IN (SELECT id FROM generations WHERE state IN ('logged_out', 'manual'));
             DELETE FROM generations WHERE state IN ('starting', 'logged_out');",
        ).await.map_err(db_failure)?;
        Ok(())
    }

    pub(super) fn generation(&self, id: String) -> Result<AccountGeneration, StorageError> {
        if id.len() != 32
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(StorageError::Unavailable);
        }
        let path = self.root.join(&id).join(SESSION_FILE);
        Ok(AccountGeneration { id, path })
    }
}

fn normalize_grant(read: ReadGrant) -> Result<ReadGrant, StorageError> {
    let ReadGrant::Channels(mut channels) = read else {
        return Ok(read);
    };
    if channels.len() > 1_000
        || channels.iter().any(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .is_none_or(|id| id.to_string() != *value)
        })
    {
        return Err(StorageError::InvalidGrant);
    }
    channels.sort_unstable();
    channels.dedup();
    Ok(ReadGrant::Channels(channels))
}

fn normalize_group_grant(mut groups: Vec<String>) -> Result<Vec<String>, StorageError> {
    if groups.len() > 1_000
        || groups
            .iter()
            .any(|group| group.parse::<GroupRef>().is_err())
    {
        return Err(StorageError::InvalidGrant);
    }
    groups.sort_unstable();
    groups.dedup();
    Ok(groups)
}

fn private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(path)
}

fn remove_generation(generation: &AccountGeneration) -> Result<(), StorageError> {
    let directory = generation.path.parent().ok_or(StorageError::Unavailable)?;
    match fs::remove_dir_all(directory) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(StorageError::Unavailable),
    }
}

fn db_failure(_: libsql::Error) -> StorageError {
    StorageError::Unavailable
}

fn integer(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::InvalidGrant)
}

fn changed(count: u64) -> Result<(), StorageError> {
    if count == 1 {
        Ok(())
    } else {
        Err(StorageError::Conflict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn interrupted_verification_is_quarantined_on_restart() {
        let root = tempfile::tempdir().expect("root");
        let store = AccountStore::open(root.path()).await.expect("open");
        let old = store
            .start("issuer", "owner")
            .await
            .expect("start verification");
        store
            .begin_verification(&old, 1)
            .await
            .expect("intent committed");
        std::fs::write(old.session_file(), b"fixture authorization").expect("session");
        drop(store);
        let store = AccountStore::open(root.path()).await.expect("restart");
        assert_eq!(
            store.status("issuer", "owner").await.expect("status"),
            LinkState::RevocationPending
        );
        let pending = store.pending_retirements().await.expect("quarantine");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].session_file(), old.session_file());
        assert!(pending[0].expired_at(86_401));
        assert!(old.session_file().exists());
        let new = store.start("issuer", "owner").await.expect("fresh attempt");
        store.activate(&new, 123).await.expect("new link");
        store
            .finish_retirement(&pending[0], RetirementOutcome::ManualRevocation)
            .await
            .expect("purge");
        assert!(!old.session_file().exists());
        assert!(
            store
                .active("issuer", "owner")
                .await
                .expect("mapping")
                .as_ref()
                == Some(&new)
        );
    }
}

#[cfg(test)]
mod group_grant_tests {
    use super::{AccountStore, GrantUpdate, ReadGrant};

    #[tokio::test]
    async fn explicit_group_grant_is_revisioned_and_preserved_by_legacy_updates() {
        let root = tempfile::tempdir().expect("private root");
        let store = AccountStore::open(root.path())
            .await
            .expect("account store");
        let account = store
            .start("issuer", "owner")
            .await
            .expect("reserve account");
        store
            .begin_verification(&account, 0)
            .await
            .expect("begin verification");
        store
            .activate(&account, 777)
            .await
            .expect("activate account");
        let update = |expected_revision| GrantUpdate {
            expected_revision,
            read: ReadGrant::Channels(vec!["42".into()]),
            send_enabled: false,
        };
        let first = store
            .update_grant_with_groups(
                &account,
                "client",
                update(0),
                Some(vec![
                    "supergroup:42".into(),
                    "chat:42".into(),
                    "chat:42".into(),
                ]),
            )
            .await
            .expect("group grant");
        assert_eq!(first.group_refs, ["chat:42", "supergroup:42"]);
        assert!(
            store
                .update_grant_with_groups(&account, "client", update(0), Some(vec![]))
                .await
                .is_err()
        );
        assert!(
            store
                .update_grant_with_groups(
                    &account,
                    "client",
                    update(1),
                    Some(vec!["chat:042".into()])
                )
                .await
                .is_err()
        );
        let second = store
            .update_grant(&account, "client", update(1))
            .await
            .expect("legacy update");
        assert_eq!(second.group_refs, first.group_refs);
        let revoked = store
            .update_grant_with_groups(&account, "client", update(2), Some(vec![]))
            .await
            .expect("revoke groups");
        assert!(revoked.group_refs.is_empty());
        assert_eq!(
            store
                .grant(&account, "client")
                .await
                .expect("current grant"),
            revoked
        );
    }
}
