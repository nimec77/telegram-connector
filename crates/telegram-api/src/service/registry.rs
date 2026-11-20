//! Bounded account clients, leases and rate-limit retention.

use super::storage::{AccountGeneration, StorageError};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use telegram_core::config::RateLimitConfig;
use telegram_core::operations::{CancellationToken, OperationContext};
use telegram_core::rate_limiter::RateLimiter;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

pub(super) struct AccountRegistry<C> {
    state: Mutex<RegistryState<C>>,
    max_clients: usize,
    idle_timeout: Duration,
    rates: RateLimitConfig,
    started_at: std::time::Instant,
}

struct RegistryState<C> {
    stopped: bool,
    entries: HashMap<AccountGeneration, Arc<Entry<C>>>,
    budgets: HashMap<AccountGeneration, Arc<RateLimiter>>,
    closing: HashSet<AccountGeneration>,
}

struct Entry<C> {
    client: OnceLock<C>,
    initializing: tokio::sync::Mutex<()>,
    limiter: Arc<RateLimiter>,
    cancellation: CancellationToken,
    revocation: CancellationToken,
    last_used: Mutex<Instant>,
    admitted: Arc<Semaphore>,
}

pub(super) struct Reservation<C> {
    key: AccountGeneration,
    entry: Arc<Entry<C>>,
    permit: OwnedSemaphorePermit,
}

pub(super) struct AccountLease<C> {
    entry: Arc<Entry<C>>,
    _permit: OwnedSemaphorePermit,
}

impl<C> AccountLease<C> {
    pub(super) fn client(&self) -> &C {
        self.entry
            .client
            .get()
            .expect("a lease requires initialized client")
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn limiter(&self) -> &Arc<RateLimiter> {
        &self.entry.limiter
    }

    pub(super) fn cancellation(&self) -> &CancellationToken {
        &self.entry.cancellation
    }

    pub(super) fn revocation(&self) -> &CancellationToken {
        &self.entry.revocation
    }
}

impl<C> Drop for AccountLease<C> {
    fn drop(&mut self) {
        if let Ok(mut last_used) = self.entry.last_used.lock() {
            *last_used = Instant::now();
        }
    }
}

#[derive(Debug)]
pub(super) enum LeaseError {
    Capacity,
    Cancelled,
    Deadline,
    Storage(StorageError),
}

impl<C> AccountRegistry<C> {
    pub(super) fn new(max_clients: usize, idle_timeout: Duration, rates: RateLimitConfig) -> Self {
        Self {
            state: Mutex::new(RegistryState {
                stopped: false,
                entries: HashMap::new(),
                budgets: HashMap::new(),
                closing: HashSet::new(),
            }),
            max_clients,
            idle_timeout,
            rates,
            started_at: std::time::Instant::now(),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) async fn lease(
        &self,
        key: AccountGeneration,
        context: &OperationContext,
        start: impl AsyncFnOnce(Arc<RateLimiter>) -> Result<C, StorageError>,
    ) -> Result<AccountLease<C>, LeaseError> {
        let reservation = self.reserve(key, context)?;
        self.open(reservation, context, start).await
    }

    pub(super) fn reserve(
        &self,
        key: AccountGeneration,
        context: &OperationContext,
    ) -> Result<Reservation<C>, LeaseError> {
        if context.cancellation().is_cancelled() {
            return Err(LeaseError::Cancelled);
        }
        if Instant::now() >= context.deadline() {
            return Err(LeaseError::Deadline);
        }
        let entry = {
            let mut state = self.state.lock().map_err(|_| unavailable())?;
            if state.stopped {
                return Err(LeaseError::Cancelled);
            }
            if let Some(entry) = state.entries.get(&key) {
                Arc::clone(entry)
            } else {
                if state.closing.contains(&key)
                    || state.entries.len() + state.closing.len() >= self.max_clients
                    || (!state.budgets.contains_key(&key)
                        && state.budgets.len() >= self.max_clients * 16)
                {
                    return Err(LeaseError::Capacity);
                }
                let limiter = state.budgets.entry(key.clone()).or_insert_with(|| {
                    let recovered =
                        self.started_at.elapsed().as_secs_f64() * self.rates.refill_rate;
                    Arc::new(if recovered >= f64::from(self.rates.max_tokens) {
                        RateLimiter::new(&self.rates)
                    } else {
                        RateLimiter::new_cold(&self.rates)
                    })
                });
                let entry = Arc::new(Entry {
                    client: OnceLock::new(),
                    initializing: tokio::sync::Mutex::new(()),
                    limiter: Arc::clone(limiter),
                    cancellation: CancellationToken::new(),
                    revocation: CancellationToken::new(),
                    last_used: Mutex::new(Instant::now()),
                    admitted: Arc::new(Semaphore::new(32)),
                });
                state.entries.insert(key.clone(), Arc::clone(&entry));
                entry
            }
        };
        let permit = Arc::clone(&entry.admitted)
            .try_acquire_owned()
            .map_err(|_| LeaseError::Capacity)?;
        Ok(Reservation { key, entry, permit })
    }

    pub(super) async fn open(
        &self,
        reservation: Reservation<C>,
        context: &OperationContext,
        start: impl AsyncFnOnce(Arc<RateLimiter>) -> Result<C, StorageError>,
    ) -> Result<AccountLease<C>, LeaseError> {
        let Reservation { key, entry, permit } = reservation;
        let initialize = async {
            if entry.client.get().is_some() {
                return Ok(());
            }
            let _initializer = entry.initializing.lock().await;
            if entry.client.get().is_none() {
                let client = start(Arc::clone(&entry.limiter))
                    .await
                    .map_err(LeaseError::Storage)?;
                // Publish under the same lock used to stop admission and revoke
                // generations so the supervisor cannot miss a late sender.
                let state = self.state.lock().map_err(|_| unavailable())?;
                if state.stopped || entry.cancellation.is_cancelled() {
                    return Err(LeaseError::Cancelled);
                }
                entry.client.set(client).map_err(|_| unavailable())?;
            }
            Ok(())
        };
        let started = tokio::select! {
            biased;
            _ = entry.cancellation.cancelled() => Err(LeaseError::Cancelled),
            _ = context.cancellation().cancelled() => Err(LeaseError::Cancelled),
            _ = tokio::time::sleep_until(context.deadline()) => Err(LeaseError::Deadline),
            result = initialize => result,
        };
        if let Err(error) = started {
            // Serialize waiter-drop with entry removal. Keeping two cancelled
            // waiters in the slot would otherwise strand an uninitialized entry.
            let mut state = self.state.lock().map_err(|_| unavailable())?;
            let sole_waiter = Arc::strong_count(&entry) == 2;
            let remove = sole_waiter && entry.client.get().is_none();
            drop(entry);
            if remove {
                state.entries.remove(&key);
            }
            return Err(error);
        }
        if entry.cancellation.is_cancelled() {
            return Err(LeaseError::Cancelled);
        }
        Ok(AccountLease {
            entry,
            _permit: permit,
        })
    }

    pub(super) fn inspect<T>(
        &self,
        key: &AccountGeneration,
        inspect: impl FnOnce(&C) -> T,
    ) -> Option<T> {
        let state = self.state.lock().ok()?;
        let entry = state.entries.get(key)?;
        if entry.cancellation.is_cancelled() {
            return None;
        }
        entry.client.get().map(inspect)
    }

    pub(super) fn invalidate(&self, key: &AccountGeneration) -> Result<(), StorageError> {
        let mut state = self.state.lock().map_err(|_| StorageError::Unavailable)?;
        if let Some(entry) = state.entries.get(key) {
            entry.revocation.cancel();
            entry.cancellation.cancel();
        }
        state.budgets.remove(key);
        Ok(())
    }

    pub(super) fn busy(&self, key: &AccountGeneration) -> bool {
        self.state.lock().map_or(true, |state| {
            state.entries.contains_key(key) || state.closing.contains(key)
        })
    }

    pub(super) fn invalidate_failed(
        &self,
        should_replace: impl Fn(&C) -> bool,
    ) -> Result<(), StorageError> {
        let state = self.state.lock().map_err(|_| StorageError::Unavailable)?;
        for entry in state.entries.values() {
            if entry.client.get().is_some_and(&should_replace) {
                entry.cancellation.cancel();
            }
        }
        Ok(())
    }

    pub(super) fn finish_close(&self, key: &AccountGeneration) -> Result<(), StorageError> {
        self.state
            .lock()
            .map_err(|_| StorageError::Unavailable)?
            .closing
            .remove(key);
        Ok(())
    }

    // Take an ownership snapshot before touching any client. Leases retain
    // the entry for the caller, but cannot keep its sender or reader running.
    pub(super) fn stop(&self, abort: impl Fn(&C)) {
        let entries = if let Ok(mut state) = self.state.lock() {
            state.stopped = true;
            state.entries.values().cloned().collect::<Vec<_>>()
        } else {
            return;
        };
        for entry in entries {
            entry.cancellation.cancel();
            if let Some(client) = entry.client.get() {
                abort(client);
            }
        }
    }

    pub(super) async fn join_stopped(&self, close: impl AsyncFn(&C)) {
        use futures::StreamExt as _;

        let entries = self.state.lock().map_or_else(
            |_| Vec::new(),
            |state| state.entries.values().cloned().collect(),
        );
        let close = &close;
        futures::stream::iter(entries)
            .for_each_concurrent(None, |entry| async move {
                if let Some(client) = entry.client.get() {
                    close(client).await;
                }
            })
            .await;
    }

    pub(super) fn release_stopped(&self) {
        if let Ok(mut state) = self.state.lock() {
            let entries = std::mem::take(&mut state.entries);
            state.budgets.clear();
            state.closing.clear();
            drop(state);
            drop(entries);
        }
    }

    pub(super) fn evict(&self) -> Result<Vec<(AccountGeneration, C)>, StorageError> {
        let mut state = self.state.lock().map_err(|_| StorageError::Unavailable)?;
        let now = Instant::now();
        let mut clients = Vec::new();
        let expired: Vec<_> = state
            .entries
            .iter()
            .filter(|(_, entry)| {
                Arc::strong_count(entry) == 1
                    && (entry.client.get().is_none()
                        || entry.cancellation.is_cancelled()
                        || entry.last_used.lock().is_ok_and(|last| {
                            now.saturating_duration_since(*last) >= self.idle_timeout
                        }))
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in expired {
            if let Some(entry) = state.entries.remove(&key)
                && let Ok(entry) = Arc::try_unwrap(entry)
                && let Some(client) = entry.client.into_inner()
            {
                state.closing.insert(key.clone());
                clients.push((key, client));
            }
        }
        state.budgets.retain(|_, limiter| {
            Arc::strong_count(limiter) > 1
                || limiter.available_tokens() < f64::from(self.rates.max_tokens)
        });
        Ok(clients)
    }
}

fn unavailable() -> LeaseError {
    LeaseError::Storage(StorageError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::AccountStore;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use telegram_core::rate_limiter::RateLimiterTrait as _;

    #[tokio::test]
    async fn concurrent_first_requests_share_one_client_per_generation() {
        let root = tempfile::tempdir().expect("root");
        let store = AccountStore::open(root.path()).await.expect("store");
        let generation = store.start("issuer", "owner").await.expect("generation");
        tokio::time::pause();
        let registry = AccountRegistry::new(1, Duration::from_secs(3), RateLimitConfig::default());
        let starts = AtomicUsize::new(0);
        let run = async |_: Arc<RateLimiter>| {
            starts.fetch_add(1, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok(())
        };
        let context = OperationContext::new(
            Instant::now() + Duration::from_secs(5),
            CancellationToken::new(),
        );
        let (a, b) = tokio::join!(
            registry.lease(generation.clone(), &context, run),
            registry.lease(generation.clone(), &context, run)
        );
        let first = a.expect("first lease");
        let second = b.expect("second lease");
        assert_eq!(starts.load(Ordering::Relaxed), 1);
        assert!(Arc::ptr_eq(first.limiter(), second.limiter()));
        first.limiter().refund(2);
        first
            .limiter()
            .acquire(1)
            .await
            .expect("charge shared bucket");
        let limiter = Arc::clone(first.limiter());
        tokio::time::advance(Duration::from_secs(4)).await;
        assert!(registry.evict().expect("with lease").is_empty());
        drop(first);
        drop(second);
        tokio::time::advance(Duration::from_secs(4)).await;
        assert_eq!(registry.evict().expect("idle").len(), 1);
        registry.finish_close(&generation).expect("closed");
        let reopened = registry
            .lease(
                generation,
                &OperationContext::new(
                    Instant::now() + Duration::from_secs(5),
                    CancellationToken::new(),
                ),
                run,
            )
            .await
            .expect("reopen");
        assert!(Arc::ptr_eq(&limiter, reopened.limiter()));
    }

    #[tokio::test]
    async fn cancelled_startup_releases_registry_slot() {
        let root = tempfile::tempdir().expect("root");
        let store = AccountStore::open(root.path()).await.expect("store");
        let stalled = store
            .start("issuer", "stalled")
            .await
            .expect("stalled generation");
        let next = store.start("issuer", "next").await.expect("new generation");
        let registry = AccountRegistry::new(1, Duration::from_secs(3), RateLimitConfig::default());
        let cancellation = CancellationToken::new();
        let context = OperationContext::new(
            Instant::now() + Duration::from_secs(5),
            cancellation.clone(),
        );
        let entered = tokio::sync::Notify::new();
        let start = async |_| {
            entered.notify_one();
            std::future::pending::<Result<(), StorageError>>().await
        };
        let first = registry.lease(stalled, &context, start);
        let abort = async {
            entered.notified().await;
            cancellation.cancel();
        };
        let (first, ()) = tokio::join!(first, abort);
        assert!(matches!(first, Err(LeaseError::Cancelled)));
        let context = OperationContext::new(
            Instant::now() + Duration::from_secs(5),
            CancellationToken::new(),
        );
        assert!(
            registry
                .lease(next, &context, async |_| Ok(()))
                .await
                .is_ok()
        );
    }
    #[tokio::test]
    async fn client_finishing_startup_after_stop_is_dropped_with_a_reservation_alive() {
        let root = tempfile::tempdir().expect("root");
        let store = AccountStore::open(root.path()).await.expect("store");
        let generation = store.start("issuer", "owner").await.expect("generation");
        let registry = AccountRegistry::new(1, Duration::from_secs(1), RateLimitConfig::default());
        let context = OperationContext::new(
            Instant::now() + Duration::from_secs(5),
            CancellationToken::new(),
        );
        let builder = registry
            .reserve(generation.clone(), &context)
            .expect("startup reservation");
        let waiting = registry
            .reserve(generation, &context)
            .expect("second reservation");
        let client = Arc::new(());
        let result = registry
            .open(builder, &context, async |_| {
                registry.stop(|_: &Arc<()>| panic!("client has not finished startup"));
                Ok(Arc::clone(&client))
            })
            .await;
        assert!(matches!(result, Err(LeaseError::Cancelled)));
        assert_eq!(
            Arc::strong_count(&client),
            1,
            "late client left in the reserved entry"
        );
        drop(waiting);
    }
}
