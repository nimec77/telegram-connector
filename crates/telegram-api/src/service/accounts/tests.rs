use super::*;
use crate::service::AccountGeneration;
use serde_json::json;
use telegram_core::error::Error;
use telegram_core::rate_limiter::{RateLimiter, RateLimiterTrait as _};
use telegram_core::streams::{ChannelHead, ChannelHeadLookup, StreamLimits, test_support};
use telegram_core::telegram::types::ChannelId;
use tokio::time::Instant;

struct EmptyChannel;

#[async_trait::async_trait]
impl ChannelHeadLookup for EmptyChannel {
    async fn channel_heads(&self, channels: &[ChannelId]) -> Result<Vec<ChannelHead>, Error> {
        Ok(channels
            .iter()
            .map(|&channel_id| ChannelHead {
                channel_id,
                head_message_id: None,
            })
            .collect())
    }
}

async fn active_account(limiter: Arc<RateLimiter>) -> (ActiveAccount, tempfile::TempDir) {
    let (client, _runner, dir) = telegram_core::telegram::client::stalled_client().await;
    let client = Arc::new(client);
    let (streams, _) = test_support::account_streams(
        Arc::new(EmptyChannel),
        limiter.clone(),
        StreamLimits::default(),
        Arc::new(telegram_core::streams::StreamBudget::new(1024)),
    );
    let operations = AccountOperations::new(Arc::clone(&client), limiter, AccountLimits::default());
    (
        ActiveAccount {
            client,
            operations,
            streams,
            capture: None,
        },
        dir,
    )
}

async fn hosted_fixture() -> (Arc<Hosted>, tempfile::TempDir) {
    let root = tempfile::tempdir().expect("disposable root");
    let config = serde_json::from_value(json!({
        "data_root":root.path(),
        "telegram":{"api_id":1,"api_hash":"fixture"},
        "registered_clients":["fixture"]
    }))
    .expect("fixture configuration");
    let service = Hosted::open(config)
        .await
        .unwrap_or_else(|_| panic!("fixture service"));
    (service, root)
}

#[tokio::test]
async fn shutdown_closes_the_reader_even_while_a_request_holds_a_lease() {
    let (service, _root) = hosted_fixture().await;

    let generation: AccountGeneration = service
        .store
        .start("issuer", "subject")
        .await
        .expect("generation");
    let context = OperationContext::new(
        Instant::now() + Duration::from_secs(5),
        CancellationToken::new(),
    );
    let mut client_dir = None;
    let lease = service
        .registry
        .lease(generation.clone(), &context, async |limiter| {
            let (client, dir) = active_account(limiter).await;
            client_dir = Some(dir);
            Ok(client)
        })
        .await
        .expect("account lease");
    lease.limiter().refund(1);
    let reader_closed = lease.client().streams.reader_closed();
    let mut subscriber = lease
        .client()
        .streams
        .subscribe_channel_posts(&[ChannelId::new(42).expect("channel")], &context)
        .await
        .expect("subscription");
    assert!(matches!(
        subscriber.next().await,
        Some(telegram_core::streams::ChannelEvent::Ready(_))
    ));

    service.cancellation.cancel();
    tokio::time::timeout(Duration::from_millis(250), reader_closed.cancelled())
        .await
        .expect("shutdown must close a reader even with a live account lease");
    assert!(lease.cancellation().is_cancelled());
    assert!(!lease.revocation().is_cancelled());
    assert!(matches!(
        subscriber.next().await,
        Some(telegram_core::streams::ChannelEvent::Interrupted(
            telegram_core::streams::InterruptionReason::ServiceDraining,
        ))
    ));
    assert!(subscriber.next().await.is_none());
    assert!(matches!(
        service.registry.reserve(generation, &context),
        Err(super::super::registry::LeaseError::Cancelled)
    ));
}

#[tokio::test]
async fn restarted_shutdown_cannot_extend_the_first_deadline() {
    let (service, _root) = hosted_fixture().await;
    tokio::time::pause();

    let started = Instant::now();
    let deadline = service.begin_draining();
    assert_eq!(deadline, started + Duration::from_secs(10));
    tokio::time::advance(Duration::from_secs(4)).await;
    assert_eq!(service.begin_draining(), deadline);
    tokio::time::timeout_at(deadline, service.wait_drained())
        .await
        .expect("account work joined under the original deadline");
}
