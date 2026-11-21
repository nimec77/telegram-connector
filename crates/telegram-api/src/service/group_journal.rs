//! Private durable streams, immutable entries, and retention boundaries.

use super::storage::StorageError;
use crate::contracts::groups::{Change, CursorExpired, Entry, GapReason, Page};
use libsql::{Builder, Connection, params};
use std::fs;
use std::path::Path;
use telegram_core::UtcTimestamp;
use telegram_core::telegram::session_storage::prepare_private_database;
use tokio::sync::Mutex;

mod cursor;

pub(super) use cursor::valid_cursor;
use cursor::{CursorKeys, new_stream_id};

const JOURNAL_DB: &str = "group-journal.db";
const MAX_PAGE_BYTES: usize = 256 * 1024;
const MAX_ENTRY_BYTES: usize = MAX_PAGE_BYTES - 256;
const INDEX_SEQUENCES: i64 = 65_536;
const ENTRY_FRAME_BYTES: usize = r#"{"cursor":"","seq":"","observed_at":"","change":}"#.len();

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS group_streams (
    id TEXT PRIMARY KEY,
    generation TEXT NOT NULL,
    group_ref TEXT NOT NULL,
    base_seq INTEGER NOT NULL DEFAULT 0,
    last_seq INTEGER NOT NULL DEFAULT 0,
    retained_bytes INTEGER NOT NULL DEFAULT 0,
    retention_time INTEGER NOT NULL DEFAULT 0,
    UNIQUE (generation, group_ref)
) STRICT;
CREATE TABLE IF NOT EXISTS group_entries (
    stream_id TEXT NOT NULL REFERENCES group_streams(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    retention_time INTEGER NOT NULL,
    observed_at TEXT NOT NULL,
    change TEXT NOT NULL,
    encoded_bytes INTEGER NOT NULL,
    PRIMARY KEY (stream_id, seq)
) WITHOUT ROWID, STRICT;
CREATE TABLE IF NOT EXISTS group_sources (
    stream_id TEXT NOT NULL REFERENCES group_streams(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    seq INTEGER NOT NULL,
    PRIMARY KEY (stream_id, source)
) WITHOUT ROWID, STRICT;
CREATE INDEX IF NOT EXISTS group_sources_seq ON group_sources(stream_id, seq);
CREATE TABLE IF NOT EXISTS group_text_ids (
    stream_id TEXT NOT NULL REFERENCES group_streams(id) ON DELETE CASCADE,
    message_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    PRIMARY KEY (stream_id, message_id)
) WITHOUT ROWID, STRICT;
CREATE INDEX IF NOT EXISTS group_text_ids_seq ON group_text_ids(stream_id, seq);
CREATE TABLE IF NOT EXISTS group_retired_generations (id TEXT PRIMARY KEY) WITHOUT ROWID, STRICT;
PRAGMA user_version = 1;
";

#[derive(Clone, Copy)]
pub(super) struct Retention {
    pub(super) seconds: u64,
    pub(super) bytes: usize,
}

pub(super) struct PendingChange {
    pub(super) source: Option<String>,
    pub(super) change: Change,
}

pub(super) enum PageResult {
    Page(Page),
    Expired(CursorExpired),
    InvalidCursor,
}

pub(super) struct GroupJournal {
    db: Mutex<Connection>,
    retention_seconds: i64,
    retention_bytes: i64,
    cursors: CursorKeys,
}

struct Stream {
    id: String,
    base_seq: i64,
    last_seq: i64,
    retained_bytes: i64,
    retention_time: i64,
}

impl GroupJournal {
    pub(super) fn exists(root: &Path) -> bool {
        // A broken or unreadable path still needs the normal open-time checks.
        !matches!(
            fs::symlink_metadata(root.join(JOURNAL_DB)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        )
    }

    pub(super) async fn open(root: &Path, limits: Retention) -> Result<Self, StorageError> {
        let path = prepare_private_database(&root.join(JOURNAL_DB))
            .await
            .map_err(unavailable)?;
        let cursors = CursorKeys::open(root).await?;
        let db = Builder::new_local(path)
            .build()
            .await
            .map_err(unavailable)?
            .connect()
            .map_err(unavailable)?;
        db.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL; PRAGMA secure_delete = ON;",
        )
        .await
        .map_err(unavailable)?;
        let mut rows = db
            .query("PRAGMA user_version", ())
            .await
            .map_err(unavailable)?;
        let version = rows
            .next()
            .await
            .map_err(unavailable)?
            .ok_or(StorageError::Unavailable)?
            .get::<i64>(0)
            .map_err(unavailable)?;
        drop(rows);
        match version {
            0 => {
                db.execute_transactional_batch(SCHEMA)
                    .await
                    .map_err(unavailable)?;
            }
            1 => {}
            _ => return Err(StorageError::Unavailable),
        }
        Ok(Self {
            db: Mutex::new(db),
            retention_seconds: i64::try_from(limits.seconds).map_err(unavailable)?,
            retention_bytes: i64::try_from(limits.bytes).map_err(unavailable)?,
            cursors,
        })
    }

    pub(super) async fn enable(&self, generation: &str, group: &str) -> Result<bool, StorageError> {
        let db = self.db.lock().await;
        let inserted = db
            .execute(
                "INSERT INTO group_streams(id, generation, group_ref)
                 SELECT ?3, ?1, ?2 WHERE NOT EXISTS
                     (SELECT 1 FROM group_retired_generations WHERE id = ?1)
                 ON CONFLICT(generation, group_ref) DO NOTHING",
                params![generation, group, new_stream_id()?],
            )
            .await
            .map_err(unavailable)?;
        if inserted == 0 {
            load_stream(&db, generation, group).await?;
        }
        Ok(inserted == 1)
    }

    pub(super) async fn append(
        &self,
        generation: &str,
        group: &str,
        changes: Vec<PendingChange>,
    ) -> Result<(), StorageError> {
        self.append_at(generation, group, changes, UtcTimestamp::now())
            .await
    }

    async fn append_at(
        &self,
        generation: &str,
        group: &str,
        changes: Vec<PendingChange>,
        observed_at: UtcTimestamp,
    ) -> Result<(), StorageError> {
        if changes.is_empty() {
            return Ok(());
        }
        let db = self.db.lock().await;
        let transaction = db.transaction().await.map_err(unavailable)?;
        let mut stream = load_stream(&transaction, generation, group).await?;
        // An adjusted wall clock must never create a hole in the retained prefix.
        stream.retention_time = stream.retention_time.max(observed_at.unix_seconds());
        let observation = observed_at.to_string();
        for change in changes {
            insert_entry(&transaction, &mut stream, change, &observation).await?;
        }
        self.prune(&transaction, &mut stream, observed_at.unix_seconds())
            .await?;
        save_stream(&transaction, &stream).await?;
        let index_base = stream.last_seq.saturating_sub(INDEX_SEQUENCES);
        for statement in [
            "DELETE FROM group_sources WHERE stream_id = ?1 AND seq <= ?2",
            "DELETE FROM group_text_ids WHERE stream_id = ?1 AND seq <= ?2",
        ] {
            transaction
                .execute(statement, params![stream.id.as_str(), index_base])
                .await
                .map_err(unavailable)?;
        }
        transaction.commit().await.map_err(unavailable)
    }

    pub(super) async fn known_message(
        &self,
        generation: &str,
        group: &str,
        message_id: &str,
    ) -> Result<bool, StorageError> {
        self.index_contains(
            "SELECT EXISTS(
                SELECT 1 FROM group_streams AS s JOIN group_text_ids AS m ON m.stream_id = s.id
                WHERE s.generation = ?1 AND s.group_ref = ?2 AND m.message_id = ?3
            )",
            generation,
            group,
            message_id,
        )
        .await
    }

    pub(super) async fn known_source(
        &self,
        generation: &str,
        group: &str,
        source: &str,
    ) -> Result<bool, StorageError> {
        self.index_contains(
            "SELECT EXISTS(
                SELECT 1 FROM group_streams AS s JOIN group_sources AS d ON d.stream_id = s.id
                WHERE s.generation = ?1 AND s.group_ref = ?2 AND d.source = ?3
            )",
            generation,
            group,
            source,
        )
        .await
    }

    async fn index_contains(
        &self,
        query: &str,
        generation: &str,
        group: &str,
        value: &str,
    ) -> Result<bool, StorageError> {
        let db = self.db.lock().await;
        let mut rows = db
            .query(query, params![generation, group, value])
            .await
            .map_err(unavailable)?;
        rows.next()
            .await
            .map_err(unavailable)?
            .ok_or(StorageError::Unavailable)?
            .get::<i64>(0)
            .map(|found| found != 0)
            .map_err(unavailable)
    }

    pub(super) async fn page(
        &self,
        generation: &str,
        owner_binding: &str,
        group: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<PageResult, StorageError> {
        let scope = self.cursors.scoped(owner_binding, group);
        let cursor = if let Some(after) = after {
            let Some(cursor) = scope.verify(after)? else {
                return Ok(PageResult::InvalidCursor);
            };
            Some(cursor)
        } else {
            None
        };
        let db = self.db.lock().await;
        let now = UtcTimestamp::now().unix_seconds();
        let stream = self
            .preserving_retention(&db, generation, group, now)
            .await?;
        if let Some(cursor) = &cursor {
            if cursor.stream_id != stream.id || cursor.seq < stream.base_seq {
                return Ok(PageResult::Expired(CursorExpired {
                    base_cursor: scope.issue(&stream.id, stream.base_seq)?,
                    base_seq: stream.base_seq.to_string(),
                    stream_id: stream.id,
                }));
            }
            if cursor.seq > stream.last_seq {
                return Ok(PageResult::InvalidCursor);
            }
        }
        let base_seq = cursor.as_ref().map_or(stream.base_seq, |cursor| cursor.seq);
        let base_cursor = match after {
            Some(value) => value.to_owned(),
            None => scope.issue(&stream.id, base_seq)?,
        };
        let mut page = Page {
            base_cursor,
            base_seq: base_seq.to_string(),
            stream_id: stream.id,
            entries: Vec::new(),
            has_more: false,
        };
        let mut encoded_bytes = serde_json::to_vec(&page).map_err(unavailable)?.len();
        let limit = limit.clamp(1, 100);
        let mut rows = db
            .query(
                "SELECT seq, observed_at, change, encoded_bytes FROM group_entries
                 WHERE stream_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
                params![
                    page.stream_id.as_str(),
                    base_seq,
                    i64::try_from(limit).map_err(unavailable)?
                ],
            )
            .await
            .map_err(unavailable)?;
        let mut last_seq = base_seq;
        while let Some(row) = rows.next().await.map_err(unavailable)? {
            let seq = row.get::<i64>(0).map_err(unavailable)?;
            if Some(seq) != last_seq.checked_add(1) {
                return Err(StorageError::Unavailable);
            }
            let bytes =
                usize::try_from(row.get::<i64>(3).map_err(unavailable)?).map_err(unavailable)?;
            let total = encoded_bytes
                .checked_add(bytes)
                .and_then(|total| total.checked_add(usize::from(!page.entries.is_empty())))
                .ok_or(StorageError::Unavailable)?;
            if total > MAX_PAGE_BYTES {
                break;
            }
            page.entries.push(Entry {
                cursor: scope.issue(&page.stream_id, seq)?,
                seq: seq.to_string(),
                observed_at: row.get(1).map_err(unavailable)?,
                change: serde_json::from_str(&row.get::<String>(2).map_err(unavailable)?)
                    .map_err(unavailable)?,
            });
            last_seq = seq;
            encoded_bytes = total;
        }
        page.has_more = last_seq < stream.last_seq;
        if page.entries.is_empty() && page.has_more {
            return Err(StorageError::Unavailable);
        }
        Ok(PageResult::Page(page))
    }

    async fn preserving_retention(
        &self,
        db: &Connection,
        generation: &str,
        group: &str,
        now: i64,
    ) -> Result<Stream, StorageError> {
        let mut stream = load_stream(db, generation, group).await?;
        if stream.base_seq == stream.last_seq {
            return Ok(stream);
        }
        let transaction = db.transaction().await.map_err(unavailable)?;
        if self.prune(&transaction, &mut stream, now).await? {
            save_stream(&transaction, &stream).await?;
            transaction.commit().await.map_err(unavailable)?;
        }
        Ok(stream)
    }

    async fn prune(
        &self,
        db: &Connection,
        stream: &mut Stream,
        now: i64,
    ) -> Result<bool, StorageError> {
        let cutoff = now.saturating_sub(self.retention_seconds);
        let previous = stream.base_seq;
        let mut rows = db
            .query(
                "SELECT seq, retention_time, encoded_bytes FROM group_entries
                 WHERE stream_id = ?1 ORDER BY seq",
                [stream.id.as_str()],
            )
            .await
            .map_err(unavailable)?;
        while let Some(row) = rows.next().await.map_err(unavailable)? {
            if row.get::<i64>(1).map_err(unavailable)? > cutoff
                && stream.retained_bytes <= self.retention_bytes
            {
                break;
            }
            stream.base_seq = row.get(0).map_err(unavailable)?;
            stream.retained_bytes = stream
                .retained_bytes
                .checked_sub(row.get(2).map_err(unavailable)?)
                .ok_or(StorageError::Unavailable)?;
        }
        drop(rows);
        if stream.base_seq == previous {
            return Ok(false);
        }
        db.execute(
            "DELETE FROM group_entries WHERE stream_id = ?1 AND seq <= ?2",
            params![stream.id.as_str(), stream.base_seq],
        )
        .await
        .map_err(unavailable)?;
        Ok(true)
    }

    pub(super) async fn reap(&self) -> Result<(), StorageError> {
        let now = UtcTimestamp::now().unix_seconds();
        let db = self.db.lock().await;
        let mut rows = db
            .query(
                "SELECT s.generation, s.group_ref FROM group_streams AS s
             JOIN group_entries AS e ON e.stream_id = s.id AND e.seq = s.base_seq + 1
             WHERE e.retention_time <= ?1 OR s.retained_bytes > ?2 LIMIT 64",
                params![
                    now.saturating_sub(self.retention_seconds),
                    self.retention_bytes
                ],
            )
            .await
            .map_err(unavailable)?;
        let mut groups = Vec::new();
        while let Some(row) = rows.next().await.map_err(unavailable)? {
            groups.push((
                row.get::<String>(0).map_err(unavailable)?,
                row.get::<String>(1).map_err(unavailable)?,
            ));
        }
        drop(rows);
        for (generation, group) in groups {
            self.preserving_retention(&db, &generation, &group, now)
                .await?;
        }
        Ok(())
    }

    pub(super) async fn retire(&self, generation: &str) -> Result<(), StorageError> {
        let db = self.db.lock().await;
        let transaction = db.transaction().await.map_err(unavailable)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO group_retired_generations(id) VALUES (?1)",
                [generation],
            )
            .await
            .map_err(unavailable)?;
        transaction
            .execute(
                "DELETE FROM group_streams WHERE generation = ?1",
                [generation],
            )
            .await
            .map_err(unavailable)?;
        transaction.commit().await.map_err(unavailable)
    }

    pub(super) async fn generations_after(&self, after: &str) -> Result<Vec<String>, StorageError> {
        let db = self.db.lock().await;
        let mut rows = db
            .query(
                "SELECT DISTINCT generation FROM group_streams WHERE generation > ?1
                 ORDER BY generation LIMIT 32",
                [after],
            )
            .await
            .map_err(unavailable)?;
        let mut generations = Vec::new();
        while let Some(row) = rows.next().await.map_err(unavailable)? {
            generations.push(row.get(0).map_err(unavailable)?);
        }
        Ok(generations)
    }
}

async fn load_stream(
    db: &Connection,
    generation: &str,
    group: &str,
) -> Result<Stream, StorageError> {
    let mut rows = db
        .query(
            "SELECT id, base_seq, last_seq, retained_bytes, retention_time FROM group_streams
             WHERE generation = ?1 AND group_ref = ?2",
            [generation, group],
        )
        .await
        .map_err(unavailable)?;
    let row = rows
        .next()
        .await
        .map_err(unavailable)?
        .ok_or(StorageError::Missing)?;
    Ok(Stream {
        id: row.get(0).map_err(unavailable)?,
        base_seq: row.get(1).map_err(unavailable)?,
        last_seq: row.get(2).map_err(unavailable)?,
        retained_bytes: row.get(3).map_err(unavailable)?,
        retention_time: row.get(4).map_err(unavailable)?,
    })
}

async fn save_stream(db: &Connection, stream: &Stream) -> Result<(), StorageError> {
    db.execute(
        "UPDATE group_streams SET base_seq = ?2, last_seq = ?3, retained_bytes = ?4, retention_time = ?5
         WHERE id = ?1",
        params![stream.id.as_str(), stream.base_seq, stream.last_seq, stream.retained_bytes, stream.retention_time],
    )
    .await
    .map_err(unavailable)?;
    Ok(())
}

async fn insert_entry(
    db: &Connection,
    stream: &mut Stream,
    PendingChange { source, mut change }: PendingChange,
    observed_at: &str,
) -> Result<(), StorageError> {
    let seq = stream
        .last_seq
        .checked_add(1)
        .ok_or(StorageError::Unavailable)?;
    if let Some(source) = source {
        if source.len() > 256 {
            return Err(StorageError::Unavailable);
        }
        let inserted = db
            .execute(
                "INSERT INTO group_sources(stream_id, source, seq) VALUES (?1, ?2, ?3)
             ON CONFLICT(stream_id, source) DO NOTHING",
                params![stream.id.as_str(), source, seq],
            )
            .await
            .map_err(unavailable)?;
        if inserted == 0 {
            return Ok(());
        }
    }
    if let Change::Deleted { deleted_at, .. } = &mut change
        && deleted_at.is_empty()
    {
        deleted_at.push_str(observed_at);
    }
    let seq_len = seq.to_string().len();
    let cursor_len = cursor::encoded_len(seq_len);
    let overhead = ENTRY_FRAME_BYTES + cursor_len + seq_len + observed_at.len();
    let mut encoded = serde_json::to_string(&change).map_err(unavailable)?;
    if encoded.len() + overhead > MAX_ENTRY_BYTES {
        change = Change::Gap {
            reason: GapReason::AmbiguousUpdate,
        };
        encoded = serde_json::to_string(&change).map_err(unavailable)?;
    }
    let bytes = i64::try_from(encoded.len() + overhead).map_err(unavailable)?;
    db.execute(
        "INSERT INTO group_entries(stream_id, seq, retention_time, observed_at, change, encoded_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![stream.id.as_str(), seq, stream.retention_time, observed_at, encoded, bytes],
    )
    .await
    .map_err(unavailable)?;
    match change {
        Change::Added { message_id, .. } | Change::Edited { message_id, .. } => {
            db.execute(
                "INSERT INTO group_text_ids(stream_id, message_id, seq) VALUES (?1, ?2, ?3)
                 ON CONFLICT(stream_id, message_id) DO UPDATE SET seq = excluded.seq",
                params![stream.id.as_str(), message_id, seq],
            )
            .await
            .map_err(unavailable)?;
        }
        Change::Deleted { message_id, .. } => {
            db.execute(
                "DELETE FROM group_text_ids WHERE stream_id = ?1 AND message_id = ?2",
                params![stream.id.as_str(), message_id],
            )
            .await
            .map_err(unavailable)?;
        }
        Change::Gap { .. } => {}
    }
    stream.last_seq = seq;
    stream.retained_bytes = stream
        .retained_bytes
        .checked_add(bytes)
        .ok_or(StorageError::Unavailable)?;
    Ok(())
}

fn unavailable<T>(_: T) -> StorageError {
    StorageError::Unavailable
}

#[cfg(test)]
mod tests;
