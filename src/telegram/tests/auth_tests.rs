use super::*;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tracing::instrument::WithSubscriber as _;

#[tokio::test]
async fn password_reader_submits_exact_bytes() {
    for (case, input, expected) in [
        ("spaces", " password ", &b" password "[..]),
        ("tabs", "\tpassword\t", &b"\tpassword\t"[..]),
        (
            "unicode",
            "\u{2003}password\u{a0}",
            &b"\xe2\x80\x83password\xc2\xa0"[..],
        ),
        ("newline data", "\npassword\r\n", &b"\npassword\r\n"[..]),
    ] {
        let mut captured = Vec::new();
        read_and_submit_password(
            || Ok(input.into()),
            |password| {
                captured = password.into_bytes();
                std::future::ready(Ok(()))
            },
        )
        .await
        .expect("password submission succeeds");
        assert!(
            captured == expected,
            "password bytes changed for case {case}"
        );
    }
}

#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("diagnostic capture")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn password_failures_do_not_diagnose_credentials() {
    let capture = Capture(Arc::new(Mutex::new(Vec::new())));
    let output = Arc::clone(&capture.0);
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || capture.clone())
        .finish();
    async {
        let input_error = read_and_submit_password(
            || Err(std::io::Error::other("terminal unavailable").into()),
            |_| async { panic!("failed reader must prevent password submission") },
        )
        .await
        .expect_err("terminal failure propagates");
        let submission_error = read_and_submit_password(
            || Ok(" FAKE_PASSWORD_93\t".into()),
            |_| async {
                tokio::task::yield_now().await;
                Err(Error::Auth("2FA verification failed".into()))
            },
        )
        .await
        .expect_err("client failure propagates");
        for error in [input_error, submission_error] {
            assert!(matches!(error, Error::Auth(_)));
            tracing::error!(%error, ?error, "authentication failure");
        }
    }
    .with_subscriber(subscriber)
    .await;
    let bytes = output.lock().expect("diagnostic capture");
    let diagnostics = String::from_utf8_lossy(&bytes);
    assert!(diagnostics.contains("terminal unavailable"));
    assert!(diagnostics.contains("2FA verification failed"));
    assert!(!diagnostics.contains("Successfully authenticated"));
    assert!(!diagnostics.contains("FAKE_PASSWORD_93"));
}
