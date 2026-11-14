use super::ops_send::prepare_text_request;
use super::resolve_recipient::from_peer;
use super::send_diagnostics::private_rpc;
use super::send_targets::ValidatedTargets;
use crate::telegram::types::sending::{RecipientSpec, SendOptions, SendText};
use crate::test_helpers::raw_tl_channel;
use grammers_client::peer::Peer;
use std::io::Write;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn sending_text_request_preserves_content_and_omits_all_unrequested_mutation_flags() {
    let (client, _runner, _dir) = super::stalled_client().await;
    let mut raw = raw_tl_channel(91, "private", Some("news"));
    raw.date = 1_700_000_000;
    let recipient = from_peer(&Peer::from_raw(client.client(), raw.into()))
        .await
        .expect("recipient");
    let input = SendText {
        options: SendOptions {
            recipient: RecipientSpec::Peer(recipient.identity),
            silent: true,
            reply_to_message_id: None,
            topic_id: None,
        },
        text: " literal *text* ".into(),
        link_preview: false,
    };
    let r = prepare_text_request(
        &input,
        &recipient,
        &ValidatedTargets { reply_to: None },
        739,
    );
    assert_eq!(r.random_id, 739);
    assert_eq!(r.message, " literal *text* ");
    assert!(r.no_webpage);
    assert!(r.silent);
    assert!(!r.clear_draft);
    assert!(!r.allow_paid_floodskip);
    assert!(r.allow_paid_stars.is_none());
    assert!(r.send_as.is_none());
    assert!(r.schedule_date.is_none());
    assert!(r.schedule_repeat_period.is_none());
    assert!(r.entities.is_none());
    assert!(r.rich_message.is_none());
    assert!(r.reply_markup.is_none());
}
#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("capture").extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[tokio::test]
async fn sending_text_private_rpc_suppresses_raw_upstream_diagnostics_across_polls() {
    use tracing::instrument::WithSubscriber as _;
    let capture = Capture(Arc::new(Mutex::new(Vec::new())));
    let output = capture.0.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || capture.clone())
        .finish();
    async {
        private_rpc(async {
            tracing::error!(target:"grammers_mtsender::sender_pool","secret provider detail");
            tokio::task::yield_now().await;
            tracing::info!(target:"grammers_client","secret outgoing body");
        })
        .await;
        tracing::info!(stage = "resolve", "safe send outcome");
    }
    .with_subscriber(subscriber)
    .await;
    let b = output.lock().expect("capture");
    let text = String::from_utf8_lossy(&b);
    assert!(!text.contains("secret"));
    assert!(text.contains("safe send outcome"));
}
