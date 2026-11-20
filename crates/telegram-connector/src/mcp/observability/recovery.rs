//! Explicit, bounded caller-selected recovery separate from diagnostic history.
use super::{BufferedResponse, OVERSIZED_PAYLOAD_STUB, ResponseBuffer};
use crate::error::{Error, StateUnavailable};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

type SelectedResponse = (String, Result<BufferedResponse, Error>);
pub(crate) struct KeyedSnapshot {
    pub responses: Vec<SelectedResponse>,
    pub retained: usize,
}

const RETENTION: Duration = Duration::from_secs(600);

pub(super) struct KeyedResponse {
    key: String,
    completed: Option<(Instant, Option<BufferedResponse>)>,
}

pub(crate) struct RecoveryReservation {
    buffer: Arc<ResponseBuffer>,
    key: String,
    finished: bool,
}

pub(crate) fn validate_key(key: &str) -> Result<(), Error> {
    if !(1..=128).contains(&key.len()) || !key.bytes().all(|byte| (b' '..=b'~').contains(&byte)) {
        return Err(Error::InvalidInput(
            "recovery_key must contain 1–128 printable ASCII characters".into(),
        ));
    }
    Ok(())
}

impl ResponseBuffer {
    pub(crate) fn reserve_key(self: &Arc<Self>, key: &str) -> Result<RecoveryReservation, Error> {
        validate_key(key)?;
        let mut entries = self
            .keyed
            .lock()
            .map_err(|_| StateUnavailable::ResponseBuffer)?;
        let now = Instant::now();
        entries.retain(|entry| {
            entry
                .completed
                .as_ref()
                .is_none_or(|(at, _)| now.duration_since(*at) < RETENTION)
        });
        if entries.iter().any(|entry| entry.key == key) {
            return Err(Error::DuplicateRecoveryKey);
        }
        if entries.len() >= self.capacity {
            let oldest = entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| entry.completed.as_ref().map(|(at, _)| (index, *at)))
                .min_by_key(|(_, at)| *at)
                .map(|(index, _)| index);
            if let Some(index) = oldest {
                entries.remove(index);
            } else {
                return Err(Error::RecoveryCapacityReached);
            }
        }
        entries.push_back(KeyedResponse {
            key: key.into(),
            completed: None,
        });
        Ok(RecoveryReservation {
            buffer: Arc::clone(self),
            key: key.into(),
            finished: false,
        })
    }

    pub(crate) fn select_keys(&self, keys: &[String]) -> Result<KeyedSnapshot, Error> {
        if keys.is_empty() || keys.len() > self.capacity.max(1) {
            return Err(Error::InvalidInput(
                "recovery_keys must be nonempty and fit the configured recovery capacity".into(),
            ));
        }
        let mut unique = std::collections::HashSet::new();
        for key in keys {
            validate_key(key)?;
            if !unique.insert(key) {
                return Err(Error::InvalidInput("recovery_keys must be unique".into()));
            }
        }
        let mut entries = self
            .keyed
            .lock()
            .map_err(|_| StateUnavailable::ResponseBuffer)?;
        let now = Instant::now();
        entries.retain(|entry| {
            entry
                .completed
                .as_ref()
                .is_none_or(|(at, _)| now.duration_since(*at) < RETENTION)
        });
        let responses = keys
            .iter()
            .map(|key| {
                let result = match entries.iter().find(|entry| &entry.key == key) {
                    Some(KeyedResponse {
                        completed: None, ..
                    }) => Err(Error::ResponsePending),
                    Some(KeyedResponse {
                        completed: Some((_, Some(response))),
                        ..
                    }) => Ok(response.clone()),
                    _ => Err(Error::ResponseUnavailable),
                };
                (key.clone(), result)
            })
            .collect();
        Ok(KeyedSnapshot {
            responses,
            retained: entries.len(),
        })
    }
}

impl RecoveryReservation {
    pub(crate) fn complete(mut self, mut response: BufferedResponse) -> Result<(), Error> {
        if response.payload.len() > self.buffer.max_payload_bytes {
            response.payload = OVERSIZED_PAYLOAD_STUB.into();
        }
        let mut entries = self
            .buffer
            .keyed
            .lock()
            .map_err(|_| StateUnavailable::ResponseBuffer)?;
        let entry = entries
            .iter_mut()
            .find(|entry| entry.key == self.key)
            .ok_or(Error::ResponseUnavailable)?;
        entry.completed = Some((Instant::now(), Some(response)));
        self.finished = true;
        Ok(())
    }
}
impl Drop for RecoveryReservation {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // Cancellation frees pending capacity but keeps an unavailable tombstone,
        // so a caller cannot confuse a reused key with the cancelled operation.
        if let Ok(mut entries) = self.buffer.keyed.lock()
            && let Some(entry) = entries.iter_mut().find(|entry| entry.key == self.key)
        {
            entry.completed = Some((Instant::now(), None));
        }
    }
}

#[cfg(test)]
#[path = "tests/recovery_tests.rs"]
mod tests;
