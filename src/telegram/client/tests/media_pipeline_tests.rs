use super::*;

#[tokio::test(start_paused = true)]
async fn task8_media_deadline_includes_resolution_before_any_raw_download() {
    for batch in [false, true] {
        let (mut client, _runner, _directory) =
            super::super::client_fixture::stalled_client().await;
        client.timeouts.download_secs = 1;
        let started = tokio::time::Instant::now();
        let result = if batch {
            client
                .download_messages_media_impl("100", &[1, 2], 1280)
                .await
                .map(|_| ())
        } else {
            client
                .download_message_media_impl("100", 1, 1280)
                .await
                .map(|_| ())
        };
        assert!(
            matches!(result, Err(Error::Timeout { secs: 1, .. })),
            "{result:?}"
        );
        assert_eq!(started.elapsed(), std::time::Duration::from_secs(1));
    }
}
