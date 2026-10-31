//! Deadline, cancellation, media guard and RPC-error mapping regressions.
use super::*;

fn rpc(name: &str, value: Option<u32>) -> InvocationError {
    InvocationError::Rpc(grammers_mtsender::RpcError {
        code: 400,
        name: name.to_string(),
        value,
        caused_by: None,
    })
}

fn state(text: &str, pending: bool) -> TranscriptionState {
    TranscriptionState {
        transcription_id: 1,
        text: text.to_string(),
        pending,
    }
}

// ---- ensure_transcribable ----

#[test]
fn voice_is_transcribable() {
    assert!(ensure_transcribable(MediaType::Voice).is_ok());
}

#[test]
fn video_note_is_transcribable() {
    assert!(ensure_transcribable(MediaType::VideoNote).is_ok());
}

#[test]
fn photo_is_rejected_naming_type() {
    let err = ensure_transcribable(MediaType::Photo).unwrap_err();
    assert!(matches!(err, Error::NotTranscribable { .. }));
    assert!(err.to_string().contains("photo"));
}

// ---- map_transcribe_rpc_error ----

#[test]
fn maps_premium_required() {
    assert!(matches!(
        map_transcribe_rpc_error(rpc("PREMIUM_ACCOUNT_REQUIRED", None)),
        Error::PremiumRequired
    ));
}

#[test]
fn maps_transcription_failed() {
    assert!(matches!(
        map_transcribe_rpc_error(rpc("TRANSCRIPTION_FAILED", None)),
        Error::TranscriptionFailed(_)
    ));
}

#[test]
fn maps_voice_too_long() {
    assert!(matches!(
        map_transcribe_rpc_error(rpc("MSG_VOICE_TOO_LONG", None)),
        Error::VoiceTooLong
    ));
}

#[test]
fn maps_flood_wait_to_rate_limit_with_seconds() {
    match map_transcribe_rpc_error(rpc("FLOOD_WAIT", Some(31))) {
        Error::RateLimit {
            retry_after_seconds,
            detail,
        } => {
            assert_eq!(retry_after_seconds, 31);
            assert_eq!(detail, "");
        }
        other => panic!("expected RateLimit, got {other:?}"),
    }
}

#[test]
fn maps_unknown_to_telegram_api() {
    assert!(matches!(
        map_transcribe_rpc_error(rpc("SOMETHING_ELSE", None)),
        Error::TelegramApi(_)
    ));
}

// ---- poll_until_complete ----

#[tokio::test]
async fn immediate_non_pending_returns_without_polling() {
    let mut calls = 0u32;
    let (final_state, partial) = poll_until_complete(
        state("done", false),
        TranscriptionDeadline::new(30),
        Duration::from_secs(2),
        || {
            calls += 1;
            async { Ok(state("unused", false)) }
        },
    )
    .await;
    assert_eq!(
        calls, 0,
        "poll must not run for an already-complete initial state"
    );
    assert!(!partial);
    assert_eq!(final_state.text, "done");
}

#[tokio::test(start_paused = true)]
async fn pending_then_completed_returns_full_text() {
    let mut calls = 0u32;
    let (final_state, partial) = poll_until_complete(
        state("partial", true),
        TranscriptionDeadline::new(30),
        Duration::from_secs(2),
        || {
            calls += 1;
            let done = calls >= 2;
            async move {
                Ok(TranscriptionState {
                    transcription_id: 1,
                    text: if done {
                        "full text".to_string()
                    } else {
                        "partial".to_string()
                    },
                    pending: !done,
                })
            }
        },
    )
    .await;
    assert!(!partial);
    assert_eq!(final_state.text, "full text");
    assert_eq!(calls, 2);
}

#[tokio::test(start_paused = true)]
async fn pending_until_timeout_returns_partial() {
    let (final_state, partial) = poll_until_complete(
        state("so far", true),
        TranscriptionDeadline::new(6),
        Duration::from_secs(2),
        || async { Ok(state("so far", true)) },
    )
    .await;
    assert!(partial);
    assert_eq!(final_state.text, "so far");
}

#[tokio::test(start_paused = true)]
async fn timeout_shorter_than_poll_interval_does_not_poll() {
    let start = Instant::now();
    let mut calls = 0;
    let (latest, partial) = poll_until_complete(
        state("initial", true),
        TranscriptionDeadline::new(1),
        Duration::from_secs(2),
        || {
            calls += 1;
            async { Ok(state("late", false)) }
        },
    )
    .await;
    assert!(partial);
    assert_eq!(latest.text, "initial");
    assert_eq!(calls, 0);
    assert_eq!(start.elapsed(), Duration::from_secs(1));
}

#[tokio::test(start_paused = true)]
async fn pending_poll_returns_latest_partial_at_deadline() {
    let start = Instant::now();
    let mut calls = 0;
    let result = tokio::time::timeout(
        Duration::from_secs(4),
        poll_until_complete(
            state("initial", true),
            TranscriptionDeadline::new(3),
            Duration::from_secs(1),
            || {
                calls += 1;
                let first = calls == 1;
                async move {
                    if first {
                        Ok(state("updated", true))
                    } else {
                        std::future::pending().await
                    }
                }
            },
        ),
    )
    .await
    .expect("poll must finish at its deadline");
    assert!(result.1);
    assert_eq!(result.0.text, "updated");
    assert_eq!(calls, 2);
    assert_eq!(start.elapsed(), Duration::from_secs(3));
}

#[tokio::test(start_paused = true)]
async fn late_completion_does_not_override_deadline() {
    // The poll completes exactly at or after expiry; both must preserve partial.
    for delay in [2, 3] {
        let start = Instant::now();
        let (latest, partial) = poll_until_complete(
            state("initial", true),
            TranscriptionDeadline::new(3),
            Duration::from_secs(1),
            || async move {
                tokio::time::sleep(Duration::from_secs(delay)).await;
                Ok(state("too late", false))
            },
        )
        .await;
        assert!(partial);
        assert_eq!(latest.text, "initial");
        assert_eq!(start.elapsed(), Duration::from_secs(3));
    }
}

#[tokio::test(start_paused = true)]
async fn elapsed_setup_does_not_restart_the_poll_budget() {
    let start = Instant::now();
    let budget = TranscriptionDeadline::new(3);
    tokio::time::advance(Duration::from_secs(2)).await;
    let (latest, partial) = poll_until_complete(
        state("initial", true),
        budget,
        Duration::from_secs(2),
        || async { panic!("no time remains for another poll interval") },
    )
    .await;
    assert!(partial);
    assert_eq!(latest.text, "initial");
    assert_eq!(start.elapsed(), Duration::from_secs(3));
}

#[tokio::test(start_paused = true)]
async fn cancellation_stops_scheduling_polls_and_drops_inflight_work() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    struct Inflight(Arc<AtomicBool>);
    impl Drop for Inflight {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicBool::new(false));
    let task_calls = Arc::clone(&calls);
    let task_dropped = Arc::clone(&dropped);
    let task = tokio::spawn(async move {
        poll_until_complete(
            state("initial", true),
            TranscriptionDeadline::new(30),
            Duration::from_secs(2),
            move || {
                task_calls.fetch_add(1, Ordering::SeqCst);
                let guard = Inflight(Arc::clone(&task_dropped));
                async move {
                    let _guard = guard;
                    std::future::pending::<Result<TranscriptionState, Error>>().await
                }
            },
        )
        .await
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(2)).await;
    tokio::task::yield_now().await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!dropped.load(Ordering::SeqCst));
    task.abort();
    assert!(task.await.expect_err("cancelled operation").is_cancelled());
    assert!(dropped.load(Ordering::SeqCst));
    tokio::time::advance(Duration::from_secs(60)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn expired_deadline_does_not_start_a_stage() {
    let deadline = TranscriptionDeadline::new(1);
    tokio::time::advance(Duration::from_secs(1)).await;
    let started = std::cell::Cell::new(false);
    let result = deadline
        .run(async {
            started.set(true);
            Ok(())
        })
        .await;
    assert!(matches!(result, Err(Error::Timeout { secs: 1, .. })));
    assert!(
        !started.get(),
        "expired requests must not start another RPC stage"
    );
}

#[tokio::test(start_paused = true)]
async fn stage_resuming_at_expiry_cannot_start_its_next_rpc() {
    let deadline = TranscriptionDeadline::new(1);
    let next_rpc_started = std::cell::Cell::new(false);
    let result = deadline
        .run(async {
            tokio::time::sleep_until(deadline.instant()).await;
            next_rpc_started.set(true);
            std::future::pending::<Result<(), Error>>().await
        })
        .await;
    assert!(matches!(result, Err(Error::Timeout { secs: 1, .. })));
    assert!(
        !next_rpc_started.get(),
        "resumption at expiry must not enter the next RPC stage"
    );
}
