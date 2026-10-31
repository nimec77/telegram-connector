//! Transcription deadlines through the production client, without network I/O.
use super::*;

#[tokio::test(start_paused = true)]
async fn resolution_uses_remaining_transcription_budget() {
    let (client, _runner, _dir) = crate::telegram::client::client_fixture::stalled_client().await;
    let start = tokio::time::Instant::now();
    let result = client
        .transcribe_audio_impl("@news", 42, TranscriptionDeadline::new(1))
        .await;
    assert!(
        matches!(result, Err(Error::Timeout { secs: 1, .. })),
        "{result:?}"
    );
    assert_eq!(start.elapsed(), StdDuration::from_secs(1));
}

#[tokio::test(start_paused = true)]
async fn configured_resolution_timeout_is_not_extended_by_operation_budget() {
    let (client, _runner, _dir) = crate::telegram::client::client_fixture::stalled_client().await;
    let start = tokio::time::Instant::now();
    let result = client
        .transcribe_audio_impl("@news", 42, TranscriptionDeadline::new(10))
        .await;
    assert!(
        matches!(result, Err(Error::Timeout { secs: 2, .. })),
        "{result:?}"
    );
    assert_eq!(start.elapsed(), StdDuration::from_secs(2));
}

#[tokio::test(start_paused = true)]
async fn initial_rpc_uses_shorter_of_history_cap_and_remaining_budget() {
    for (budget, expected) in [(1, 1), (10, 5)] {
        let (client, _runner, _dir) =
            crate::telegram::client::client_fixture::stalled_client().await;
        let start = tokio::time::Instant::now();
        let peer = tl::types::InputPeerSelf {}.into();
        let result = client
            .invoke_transcribe(peer, 42, TranscriptionDeadline::new(budget))
            .await;
        assert!(
            matches!(result, Err(Error::Timeout { secs, .. }) if secs == expected),
            "{result:?}"
        );
        assert_eq!(start.elapsed(), StdDuration::from_secs(expected));
    }
}
