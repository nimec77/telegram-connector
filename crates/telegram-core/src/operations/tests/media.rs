use super::super::{AccountLimits, AccountOperations, CancellationToken, OperationContext};
use super::{JoinedMediaSource, MediaPreviewRequest};
use crate::config::RateLimitConfig;
use crate::error::Error;
use crate::rate_limiter::RateLimiter;
use crate::telegram::types::{
    ChannelId, MediaBatch, MediaDownload, MediaFetchOutcome, MediaType, MessageId, PeerNamespace,
};
use crate::test_helpers::create_test_jpeg;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

struct Images;

#[async_trait::async_trait]
impl JoinedMediaSource for Images {
    async fn verify_media_channel(&self, _: ChannelId) -> Result<(), Error> {
        Ok(())
    }

    async fn joined_media(
        &self,
        channel_id: ChannelId,
        message_ids: &[i32],
        _: u32,
    ) -> Result<MediaBatch, Error> {
        let outcomes = message_ids
            .iter()
            .map(|&message_id| MediaFetchOutcome {
                message_id,
                result: Ok(MediaDownload {
                    lease: None,
                    channel_id,
                    peer_namespace: PeerNamespace::Channel,
                    bytes: if message_id == 2 {
                        b"corrupt image".to_vec()
                    } else {
                        create_test_jpeg(80, 30)
                    },
                    media_type: MediaType::Photo,
                    is_thumbnail: false,
                    caption: None,
                    width: Some(80),
                    height: Some(30),
                    source_size_bytes: 100,
                    video_info: None,
                    largest_width: None,
                    largest_height: None,
                }),
            })
            .collect();
        Ok(MediaBatch {
            channel_id,
            peer_namespace: PeerNamespace::Channel,
            stream: None,
            deadline: None,
            outcomes,
        })
    }
}

#[tokio::test]
async fn shaped_and_delivered_previews_spend_only_their_distinct_item_charge() {
    let limiter = Arc::new(RateLimiter::new(&RateLimitConfig {
        max_tokens: 30,
        refill_rate: 0.0,
        ..RateLimitConfig::default()
    }));
    let operations = AccountOperations {
        client: Arc::new(Images),
        limiter: limiter.clone(),
        limits: AccountLimits::default(),
    };
    let request = MediaPreviewRequest::try_new(
        ChannelId::new(42).unwrap(),
        [1, 2, 3, 1]
            .into_iter()
            .map(|id| MessageId::new(id).unwrap())
            .collect(),
        Some(64),
    )
    .unwrap();
    let context = OperationContext::new(
        Instant::now() + Duration::from_secs(10),
        CancellationToken::new(),
    );
    let result = operations
        .media_previews(request, 3, Duration::from_secs(5), &context)
        .await
        .unwrap();
    assert_eq!(
        limiter.available_tokens(),
        21.0,
        "three distinct requests reserved"
    );
    assert_eq!(result.items.len(), 3);
    let preview = result.items[0].result.as_ref().unwrap();
    assert_eq!((preview.width, preview.height), (64, 24));
    assert_eq!(&preview.jpeg[..2], &[255, 216]);
    assert!(matches!(
        result.items[1].result,
        Err(Error::DownloadFailed(_))
    ));
    assert!(result.items[2].result.is_ok());
    result.deliver(1);
    assert_eq!(
        limiter.available_tokens(),
        27.0,
        "processing and delivery failures refunded once"
    );
}
