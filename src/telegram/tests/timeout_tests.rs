//! Tests for the `with_timeout` helper used to bound grammers network calls.
//!
//! Tests use `tokio::time::pause()` so they are deterministic and never touch a
//! real network.

use crate::error::Error;
use crate::telegram::client::with_timeout;
use std::time::Duration;
use tokio::time::{self, sleep};

#[tokio::test(start_paused = true)]
async fn with_timeout_returns_ok_when_future_completes_in_budget() {
    let result: Result<u32, Error> = with_timeout("resolve_username", 30, async {
        sleep(Duration::from_secs(1)).await;
        Ok(42)
    })
    .await;
    assert_eq!(result.unwrap(), 42);
}

#[tokio::test(start_paused = true)]
async fn with_timeout_propagates_inner_error() {
    let result: Result<u32, Error> = with_timeout("resolve_username", 30, async {
        Err(Error::TelegramApi("boom".to_string()))
    })
    .await;
    let err = result.expect_err("inner error must propagate unchanged");
    match err {
        Error::TelegramApi(msg) => assert_eq!(msg, "boom"),
        other => panic!("expected TelegramApi, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn with_timeout_elapsed_yields_typed_timeout_error() {
    // A future that never completes — advance virtual time past the budget.
    let fut = async {
        std::future::pending::<()>().await;
        Ok::<u32, Error>(0)
    };

    let handle = tokio::spawn(with_timeout("search_messages", 120, fut));

    // Advance virtual time past the budget so tokio::time::timeout fires.
    time::advance(Duration::from_secs(121)).await;

    let result = handle.await.expect("task panicked");
    let err = result.expect_err("must time out");
    match err {
        Error::Timeout { operation, secs } => {
            assert_eq!(operation, "search_messages");
            assert_eq!(secs, 120);
        }
        other => panic!("expected Timeout, got {other:?}"),
    }
}
