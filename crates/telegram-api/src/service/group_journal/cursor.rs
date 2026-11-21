//! Stable cursor authentication bound to the owner and typed group.

use super::{StorageError, unavailable};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, crypto};
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::Path;
use telegram_core::telegram::session_storage::prepare_private_database;

const CURSOR_KEY_FILE: &str = "group-cursor.key";
const UUID_LEN: usize = 36;
const SIGNATURE_LEN: usize = 43;
const MAX_CURSOR_BYTES: usize = encoded_len(19);

pub(super) const fn encoded_len(seq_digits: usize) -> usize {
    UUID_LEN + seq_digits + SIGNATURE_LEN + 2
}

pub(super) fn new_stream_id() -> Result<String, StorageError> {
    let mut id = [0; 16];
    getrandom::fill(&mut id).map_err(unavailable)?;
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    let hex = format!("{:032x}", u128::from_be_bytes(id));
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

fn load_key(path: &Path) -> Result<[u8; 32], StorageError> {
    let mut output = Vec::new();
    File::open(path)
        .map_err(unavailable)?
        .take(33)
        .read_to_end(&mut output)
        .map_err(unavailable)?;
    if !output.is_empty() {
        return output.try_into().map_err(unavailable);
    }
    // The caller holds the data-root lock. Publish the key atomically in a
    // separate private file so replacing the journal doesn't invalidate signatures.
    let parent = path.parent().ok_or(StorageError::Unavailable)?;
    let mut key = [0; 32];
    getrandom::fill(&mut key).map_err(unavailable)?;
    let mut pending = tempfile::NamedTempFile::new_in(parent).map_err(unavailable)?;
    pending.write_all(&key).map_err(unavailable)?;
    pending.as_file().sync_all().map_err(unavailable)?;
    pending.persist(path).map_err(unavailable)?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(unavailable)?;
    Ok(key)
}

pub(super) struct CursorKeys {
    signing: EncodingKey,
    verification: DecodingKey,
}

pub(super) struct CursorScope<'a> {
    keys: &'a CursorKeys,
    owner_binding: &'a str,
    group: &'a str,
}

impl CursorKeys {
    pub(super) async fn open(root: &Path) -> Result<Self, StorageError> {
        let key_path = prepare_private_database(&root.join(CURSOR_KEY_FILE))
            .await
            .map_err(unavailable)?;
        let key = load_key(&key_path)?;
        Ok(Self {
            signing: EncodingKey::from_secret(&key).map_err(unavailable)?,
            verification: DecodingKey::from_secret(&key).map_err(unavailable)?,
        })
    }

    pub(super) fn scoped<'a>(&'a self, owner_binding: &'a str, group: &'a str) -> CursorScope<'a> {
        CursorScope {
            keys: self,
            owner_binding,
            group,
        }
    }
}

impl CursorScope<'_> {
    fn payload(&self, positions: &str) -> Result<Vec<u8>, StorageError> {
        serde_json::to_vec(&("group-cursor-v1", self.owner_binding, self.group, positions))
            .map_err(unavailable)
    }

    pub(super) fn issue(&self, stream_id: &str, seq: i64) -> Result<String, StorageError> {
        let positions = format!("{stream_id}.{seq}");
        let signature = crypto::sign(
            &self.payload(&positions)?,
            &self.keys.signing,
            Algorithm::HS256,
        )
        .map_err(unavailable)?;
        Ok(format!("{positions}.{signature}"))
    }
    pub(super) fn verify<'a>(&self, value: &'a str) -> Result<Option<Cursor<'a>>, StorageError> {
        let Some(cursor) = parse_cursor(value) else {
            return Ok(None);
        };
        let signature_valid = crypto::verify(
            cursor.signature,
            &self.payload(cursor.positions)?,
            &self.keys.verification,
            Algorithm::HS256,
        )
        .map_err(unavailable)?;
        Ok(signature_valid.then_some(cursor))
    }
}

pub(super) struct Cursor<'a> {
    positions: &'a str,
    pub(super) stream_id: &'a str,
    pub(super) seq: i64,
    signature: &'a str,
}

pub(in crate::service) fn valid_cursor(value: &str) -> bool {
    parse_cursor(value).is_some()
}

fn parse_cursor(value: &str) -> Option<Cursor<'_>> {
    if value.len() > MAX_CURSOR_BYTES {
        return None;
    }
    let (positions, signature) = value.rsplit_once('.')?;
    let (stream_id, seq) = positions.split_once('.')?;
    let number: i64 = seq.parse().ok()?;
    if number < 0
        || number.to_string() != seq
        || !valid_stream_id(stream_id)
        || signature.len() != SIGNATURE_LEN
        || URL_SAFE_NO_PAD.decode(signature).ok()?.len() != 32
    {
        return None;
    }
    Some(Cursor {
        positions,
        stream_id,
        seq: number,
        signature,
    })
}

fn valid_stream_id(value: &str) -> bool {
    value.len() == UUID_LEN
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            14 => byte == b'4',
            19 => matches!(byte, b'8' | b'9' | b'a' | b'b'),
            _ => matches!(byte, b'0'..=b'9' | b'a'..=b'f'),
        })
}
