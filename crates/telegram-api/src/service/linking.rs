//! Bounded, in-memory Telegram login challenges.

use super::http_error::ApiError;
use super::storage::{AccountGeneration, AccountStore};
use crate::contracts::ErrorCode;
use crate::identity::Principal;
use grammers_client::client::{LoginToken, PasswordToken};
use secrecy::{ExposeSecret as _, SecretString};
use serde::Serialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use telegram_core::TelegramClient;
use telegram_core::config::{TelegramConfig, defaults};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::Instant;

const ATTEMPT_TTL: Duration = Duration::from_secs(300);
const REQUEST_WINDOW: Duration = Duration::from_secs(300);
const MAX_PENDING: usize = 128;
const MAX_STARTS_PER_SOURCE: u8 = 12;
const MAX_STARTS_PER_OWNER: u8 = 4;
const MAX_TRACKED_SOURCES: usize = 4096;

#[derive(Default)]
pub(super) struct Linking {
    attempts: Mutex<Attempts>,
}

#[derive(Default)]
struct Attempts {
    owners: HashMap<Principal, Arc<AsyncMutex<Option<Attempt>>>>,
    sources: HashMap<IpAddr, RateWindow>,
    owner_starts: HashMap<Principal, RateWindow>,
}

struct RateWindow {
    expires: Instant,
    starts: u8,
}

struct Attempt {
    challenge: String,
    generation: AccountGeneration,
    client: Box<dyn LoginChallenge>,
    step: Step,
    expires: Instant,
}

enum Step {
    Code,
    Password,
}

#[async_trait::async_trait]
trait LoginChallenge: Send + Sync {
    async fn code_requires_password(&mut self, code: &str) -> Result<bool, ApiError>;
    async fn password(&mut self, password: &str) -> Result<(), ApiError>;
    async fn account_id(&self) -> Result<i64, ApiError>;
    async fn close(&self);
}

struct TelegramLogin {
    client: TelegramClient,
    code: Option<LoginToken>,
    password: Option<PasswordToken>,
}

#[async_trait::async_trait]
impl LoginChallenge for TelegramLogin {
    async fn code_requires_password(&mut self, code: &str) -> Result<bool, ApiError> {
        let token = self.code.take().ok_or(ApiError::new(ErrorCode::Conflict))?;
        self.password = self
            .client
            .sign_in(&token, code)
            .await
            .map_err(super::http_error::from_telegram)?;
        Ok(self.password.is_some())
    }

    async fn password(&mut self, password: &str) -> Result<(), ApiError> {
        let token = self
            .password
            .take()
            .ok_or(ApiError::new(ErrorCode::Conflict))?;
        self.client
            .check_password(token, password)
            .await
            .map_err(super::http_error::from_telegram)
    }

    async fn account_id(&self) -> Result<i64, ApiError> {
        self.client
            .client()
            .get_me()
            .await
            .ok()
            .and_then(|user| user.id().bare_id())
            .ok_or(ApiError::new(ErrorCode::Unavailable))
    }

    async fn close(&self) {
        self.client.close().await;
    }
}

#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum NextStep {
    Code,
    Password,
    Linked,
}

#[derive(Debug, Serialize)]
pub(super) struct LinkReply {
    step: NextStep,
    #[serde(skip_serializing_if = "Option::is_none")]
    challenge: Option<String>,
}

impl Linking {
    pub(super) async fn start(
        &self,
        store: &Arc<AccountStore>,
        owner: &Principal,
        source: IpAddr,
        config: &TelegramConfig,
        phone: SecretString,
    ) -> Result<LinkReply, ApiError> {
        self.start_using(store, owner, source, config, phone, request_code)
            .await
    }

    async fn start_using(
        &self,
        store: &Arc<AccountStore>,
        owner: &Principal,
        source: IpAddr,
        config: &TelegramConfig,
        phone: SecretString,
        request: impl AsyncFnOnce(
            &TelegramConfig,
            &str,
            &str,
        ) -> Result<Box<dyn LoginChallenge>, ApiError>,
    ) -> Result<LinkReply, ApiError> {
        let phone = phone.expose_secret();
        if !(4..=17).contains(&phone.len())
            || !phone.starts_with('+')
            || !phone[1..].bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(ApiError::new(ErrorCode::InvalidRequest));
        }
        let hash = config
            .api_hash
            .as_ref()
            .ok_or(ApiError::new(ErrorCode::Unavailable))?;
        let slot = self.admit_start(owner, source)?;
        let mut slot = slot
            .try_lock()
            .map_err(|_| ApiError::new(ErrorCode::Conflict))?;
        if let Some(previous) = slot.take() {
            abandon(store, previous).await?;
        }
        let challenge = random_id()?;
        let generation = store.start(owner.issuer(), owner.subject()).await?;
        let mut reservation = LoginReservation::new(store, &generation);
        let config = session_config(config, &generation);
        let expires = Instant::now() + ATTEMPT_TTL;
        let login = tokio::time::timeout_at(
            super::request_deadline(),
            request(&config, phone, hash.expose_secret()),
        )
        .await
        .map_err(|_| ApiError::new(ErrorCode::Timeout))?;
        let client = match login {
            Ok(value) => value,
            Err(error) => {
                store.abandon(&generation).await?;
                reservation.disarm();
                return Err(error);
            }
        };
        reservation.disarm();
        *slot = Some(Attempt {
            challenge: challenge.clone(),
            generation,
            client,
            step: Step::Code,
            expires,
        });
        Ok(LinkReply {
            step: NextStep::Code,
            challenge: Some(challenge),
        })
    }

    pub(super) async fn code(
        &self,
        store: &Arc<AccountStore>,
        owner: &Principal,
        challenge: &str,
        code: SecretString,
    ) -> Result<LinkReply, ApiError> {
        let code = code.expose_secret();
        if !(1..=16).contains(&code.len()) || !code.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ApiError::new(ErrorCode::InvalidRequest));
        }
        let slot = self.slot(owner)?;
        let mut slot = slot
            .try_lock()
            .map_err(|_| ApiError::new(ErrorCode::Conflict))?;
        let attempt = checked_attempt(slot.as_mut(), challenge)?;
        if !matches!(attempt.step, Step::Code) {
            return Err(ApiError::new(ErrorCode::Conflict));
        }
        let mut attempt = slot.take().ok_or(ApiError::new(ErrorCode::Conflict))?;
        let mut reservation = LoginReservation::verifying(store, &attempt.generation).await?;
        let response = tokio::time::timeout_at(
            attempt.expires.min(super::request_deadline()),
            attempt.client.code_requires_password(code),
        )
        .await
        .map_err(|_| ApiError::new(ErrorCode::Timeout))?;
        match response {
            Ok(true) => {
                store.await_password(&attempt.generation).await?;
                attempt.step = Step::Password;
                *slot = Some(attempt);
                reservation.disarm();
                Ok(LinkReply {
                    step: NextStep::Password,
                    challenge: Some(challenge.to_owned()),
                })
            }
            Ok(false) => {
                let result = activate(store, attempt).await;
                if result.is_ok() {
                    reservation.disarm();
                }
                result
            }
            Err(error) => {
                attempt.client.close().await;
                drop(attempt.client);
                store
                    .retire_starting(&attempt.generation, crate::identity::unix_now()?)
                    .await?;
                reservation.disarm();
                Err(error)
            }
        }
    }

    pub(super) async fn password(
        &self,
        store: &Arc<AccountStore>,
        owner: &Principal,
        challenge: &str,
        password: SecretString,
    ) -> Result<LinkReply, ApiError> {
        if password.expose_secret().is_empty() || password.expose_secret().len() > 1024 {
            return Err(ApiError::new(ErrorCode::InvalidRequest));
        }
        let slot = self.slot(owner)?;
        let mut slot = slot
            .try_lock()
            .map_err(|_| ApiError::new(ErrorCode::Conflict))?;
        let attempt = checked_attempt(slot.as_mut(), challenge)?;
        if !matches!(attempt.step, Step::Password) {
            return Err(ApiError::new(ErrorCode::Conflict));
        }
        let mut attempt = slot.take().ok_or(ApiError::new(ErrorCode::Conflict))?;
        let mut reservation = LoginReservation::verifying(store, &attempt.generation).await?;
        let result = tokio::time::timeout_at(
            attempt.expires.min(super::request_deadline()),
            attempt.client.password(password.expose_secret()),
        )
        .await;
        if let Err(error) = result
            .map_err(|_| ApiError::new(ErrorCode::Timeout))
            .and_then(std::convert::identity)
        {
            attempt.client.close().await;
            drop(attempt.client);
            store
                .retire_starting(&attempt.generation, crate::identity::unix_now()?)
                .await?;
            reservation.disarm();
            return Err(error);
        }
        let result = activate(store, attempt).await;
        if result.is_ok() {
            reservation.disarm();
        }
        result
    }

    pub(super) fn stop_idle(&self) {
        let slots = if let Ok(mut attempts) = self.attempts.lock() {
            std::mem::take(&mut attempts.owners)
        } else {
            return;
        };
        // Idle attempts own their client in the slot. An in-flight step owns
        // its slot independently and is cancelled by the HTTP handler.
        drop(slots);
    }

    pub(super) async fn cancel(
        &self,
        store: &Arc<AccountStore>,
        owner: &Principal,
    ) -> Result<(), ApiError> {
        let Some(slot) = self
            .attempts
            .lock()
            .map_err(|_| ApiError::new(ErrorCode::Unavailable))?
            .owners
            .get(owner)
            .cloned()
        else {
            return Ok(());
        };
        let mut slot = slot
            .try_lock()
            .map_err(|_| ApiError::new(ErrorCode::Conflict))?;
        if let Some(attempt) = slot.take() {
            abandon(store, attempt).await?;
        }
        Ok(())
    }

    pub(super) async fn reap(&self, store: &AccountStore) -> Result<(), ApiError> {
        let slots = {
            let attempts = self
                .attempts
                .lock()
                .map_err(|_| ApiError::new(ErrorCode::Unavailable))?;
            attempts.owners.values().cloned().collect::<Vec<_>>()
        };
        for slot in slots {
            if let Ok(mut slot) = slot.try_lock()
                && slot
                    .as_ref()
                    .is_some_and(|attempt| Instant::now() >= attempt.expires)
                && let Some(attempt) = slot.take()
            {
                abandon(store, attempt).await?;
            }
        }
        self.attempts
            .lock()
            .map_err(|_| ApiError::new(ErrorCode::Unavailable))?
            .owners
            .retain(|_, slot| {
                Arc::strong_count(slot) > 1 || !slot.try_lock().is_ok_and(|slot| slot.is_none())
            });
        Ok(())
    }

    fn slot(&self, owner: &Principal) -> Result<Arc<AsyncMutex<Option<Attempt>>>, ApiError> {
        self.attempts
            .lock()
            .map_err(|_| ApiError::new(ErrorCode::Unavailable))?
            .owners
            .get(owner)
            .cloned()
            .ok_or(ApiError::new(ErrorCode::UnavailableResource))
    }

    fn admit_start(
        &self,
        owner: &Principal,
        source: IpAddr,
    ) -> Result<Arc<AsyncMutex<Option<Attempt>>>, ApiError> {
        let mut attempts = self
            .attempts
            .lock()
            .map_err(|_| ApiError::new(ErrorCode::Unavailable))?;
        let now = Instant::now();
        attempts.sources.retain(|_, window| window.expires > now);
        attempts
            .owner_starts
            .retain(|_, window| window.expires > now);
        if (attempts.sources.len() >= MAX_TRACKED_SOURCES
            && !attempts.sources.contains_key(&source))
            || (attempts.owner_starts.len() >= MAX_TRACKED_SOURCES
                && !attempts.owner_starts.contains_key(owner))
            || (attempts.owners.len() >= MAX_PENDING && !attempts.owners.contains_key(owner))
        {
            return Err(ApiError::new(ErrorCode::RateLimited));
        }
        record_start(
            attempts.sources.entry(source).or_insert(RateWindow {
                expires: now + REQUEST_WINDOW,
                starts: 0,
            }),
            MAX_STARTS_PER_SOURCE,
        )?;
        record_start(
            attempts
                .owner_starts
                .entry(owner.clone())
                .or_insert(RateWindow {
                    expires: now + REQUEST_WINDOW,
                    starts: 0,
                }),
            MAX_STARTS_PER_OWNER,
        )?;
        Ok(Arc::clone(
            attempts.owners.entry(owner.clone()).or_default(),
        ))
    }
}

struct LoginReservation {
    cleanup: Option<(Arc<AccountStore>, AccountGeneration)>,
    possibly_authorized: bool,
}

impl LoginReservation {
    fn new(store: &Arc<AccountStore>, generation: &AccountGeneration) -> Self {
        Self {
            cleanup: Some((Arc::clone(store), generation.clone())),
            possibly_authorized: false,
        }
    }

    async fn verifying(
        store: &Arc<AccountStore>,
        generation: &AccountGeneration,
    ) -> Result<Self, ApiError> {
        let mut reservation = Self::new(store, generation);
        reservation.possibly_authorized = true;
        store
            .begin_verification(generation, crate::identity::unix_now()?)
            .await?;
        Ok(reservation)
    }

    fn disarm(&mut self) {
        self.cleanup = None;
    }
}

impl Drop for LoginReservation {
    fn drop(&mut self) {
        if let Some((store, generation)) = self.cleanup.take() {
            let possibly_authorized = self.possibly_authorized;
            tokio::spawn(async move {
                if possibly_authorized {
                    if let Ok(now) = crate::identity::unix_now() {
                        let _ = store.retire_starting(&generation, now).await;
                    }
                } else {
                    let _ = store.abandon(&generation).await;
                }
            });
        }
    }
}

fn record_start(window: &mut RateWindow, maximum: u8) -> Result<(), ApiError> {
    if window.starts >= maximum {
        return Err(ApiError::new(ErrorCode::RateLimited));
    }
    window.starts += 1;
    Ok(())
}

fn checked_attempt<'a>(
    attempt: Option<&'a mut Attempt>,
    challenge: &str,
) -> Result<&'a mut Attempt, ApiError> {
    attempt
        .filter(|attempt| attempt.challenge == challenge && Instant::now() < attempt.expires)
        .ok_or(ApiError::new(ErrorCode::UnavailableResource))
}

async fn request_code(
    config: &TelegramConfig,
    phone: &str,
    hash: &str,
) -> Result<Box<dyn LoginChallenge>, ApiError> {
    let client = TelegramClient::new_with_streams(config, &defaults::default_search_config())
        .await
        .map_err(super::http_error::from_telegram)?;
    let token = client
        .request_login_code(phone, hash)
        .await
        .map_err(super::http_error::from_telegram)?;
    Ok(Box::new(TelegramLogin {
        client,
        code: Some(token),
        password: None,
    }))
}

async fn activate(store: &AccountStore, attempt: Attempt) -> Result<LinkReply, ApiError> {
    let Attempt {
        generation,
        client,
        expires,
        ..
    } = attempt;
    let verified =
        tokio::time::timeout_at(expires.min(super::request_deadline()), client.account_id())
            .await
            .map_err(|_| ApiError::new(ErrorCode::Timeout))
            .and_then(std::convert::identity);
    client.close().await;
    drop(client);
    let result = match verified {
        Ok(id) => store
            .activate(&generation, id)
            .await
            .map_err(ApiError::from),
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        store
            .retire_starting(&generation, crate::identity::unix_now()?)
            .await?;
        return Err(error);
    }
    Ok(LinkReply {
        step: NextStep::Linked,
        challenge: None,
    })
}

async fn abandon(store: &AccountStore, attempt: Attempt) -> Result<(), ApiError> {
    attempt.client.close().await;
    drop(attempt.client);
    store.abandon(&attempt.generation).await?;
    Ok(())
}

pub(super) fn session_config(
    config: &TelegramConfig,
    generation: &AccountGeneration,
) -> TelegramConfig {
    let mut config = config.clone();
    config.session_file = generation.session_file().to_owned();
    config
}

fn random_id() -> Result<String, ApiError> {
    let mut entropy = [0; 24];
    getrandom::fill(&mut entropy).map_err(|_| ApiError::new(ErrorCode::Unavailable))?;
    use base64::Engine as _;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(entropy))
}

#[cfg(test)]
mod tests;
