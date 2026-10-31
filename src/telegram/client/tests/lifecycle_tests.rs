//! Account RPC policy below the whole-client mock seam.
use super::*;
use std::future::{pending, ready};

#[tokio::test(start_paused = true)]
async fn startup_status_times_out() {
    let (client, _runner, _dir) = crate::telegram::client::client_fixture::stalled_client().await;
    let start = tokio::time::Instant::now();
    let result = tokio::time::timeout(
        StdDuration::from_secs(3),
        client.refresh_account_status_impl(),
    )
    .await;
    assert!(
        matches!(result, Ok(Err(Error::Timeout { secs: 2, .. }))),
        "{result:?}"
    );
    assert_eq!(start.elapsed(), StdDuration::from_secs(2));
}

#[tokio::test]
async fn transport_failure_is_not_reported_as_unauthenticated() {
    let (client, runner, _dir) = crate::telegram::client::client_fixture::stalled_client().await;
    drop(runner);
    let result = client.refresh_account_status_impl().await;
    assert!(matches!(result, Err(Error::TelegramApi(_))), "{result:?}");
    assert_eq!(
        *client.account_status.read().await,
        AccountStatus::default()
    );
}

#[tokio::test]
async fn failed_authorization_refresh_preserves_successful_observations() {
    let cache = tokio::sync::RwLock::new(AccountStatus::default());
    let observed = refresh_account_status_with(&cache, 2, ready(Ok(true)), ready(Ok(true)))
        .await
        .expect("initial observation");
    assert!(observed.authorization_checked_at.is_some());
    assert!(observed.premium_checked_at.is_some());
    let result = refresh_account_status_with(
        &cache,
        2,
        ready(Err(Error::TelegramApi("transport unavailable".into()))),
        pending(),
    )
    .await;
    assert!(matches!(result, Err(Error::TelegramApi(_))));
    assert_eq!(*cache.read().await, observed);
}

#[tokio::test(start_paused = true)]
async fn premium_warmup_is_bounded_and_nonfatal() {
    let cache = tokio::sync::RwLock::new(AccountStatus::default());
    let start = tokio::time::Instant::now();
    let observed = refresh_account_status_with(&cache, 2, ready(Ok(true)), pending())
        .await
        .expect("Premium failure must not fail authorization");
    assert!(observed.authorized);
    assert_eq!(observed.premium, None);
    assert!(observed.authorization_checked_at.is_some());
    assert_eq!(observed.premium_checked_at, None);
    assert_eq!(start.elapsed(), StdDuration::from_secs(2));
}

#[tokio::test]
async fn failed_premium_refresh_keeps_its_last_successful_timestamp() {
    let cache = tokio::sync::RwLock::new(AccountStatus::default());
    let before = refresh_account_status_with(&cache, 2, ready(Ok(true)), ready(Ok(true)))
        .await
        .expect("initial observation");
    let after = refresh_account_status_with(
        &cache,
        2,
        ready(Ok(true)),
        ready(Err(Error::TelegramApi("lookup unavailable".into()))),
    )
    .await
    .expect("authorization succeeded");
    assert!(after.authorized);
    assert_eq!(after.premium, Some(true));
    assert_eq!(after.premium_checked_at, before.premium_checked_at);
}

#[tokio::test(start_paused = true)]
async fn unauthenticated_observation_skips_premium_and_clears_old_account_data() {
    let cache = tokio::sync::RwLock::new(AccountStatus::default());
    refresh_account_status_with(&cache, 2, ready(Ok(true)), ready(Ok(true)))
        .await
        .expect("initial observation");
    let start = tokio::time::Instant::now();
    let after = refresh_account_status_with(&cache, 2, ready(Ok(false)), pending())
        .await
        .expect("definitive unauthenticated observation");
    assert!(!after.authorized);
    assert_eq!(after.premium, None);
    assert_eq!(after.premium_checked_at, None);
    assert_eq!(start.elapsed(), StdDuration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn cached_status_remains_readable_while_refresh_is_pending() {
    let cache = Arc::new(tokio::sync::RwLock::new(AccountStatus::default()));
    let refreshing = Arc::clone(&cache);
    let task = tokio::spawn(async move {
        refresh_account_status_with(&refreshing, 2, pending(), pending()).await
    });
    tokio::task::yield_now().await;
    let value = tokio::time::timeout(StdDuration::from_secs(1), cache.read())
        .await
        .expect("local read");
    assert_eq!(*value, AccountStatus::default());
    drop(value);
    task.abort();
}
