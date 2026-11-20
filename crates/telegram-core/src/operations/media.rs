//! Account charging, deadlines and ordered preview shaping for joined channels.

use super::preview::shape_jpeg;
use super::{AccountOperations, BoundedOperation, OperationContext};
use crate::error::Error;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::{
    ChannelId, MediaBatch, MediaDownload, MediaFetchError, MediaType, MessageId, PeerNamespace,
};
use futures::StreamExt as _;
use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

const MAX_IDS: usize = 10;
/// Maximum size of one hosted JPEG preview.
pub const MAX_PREVIEW_JPEG_BYTES: usize = 1_179_648;
const MAX_BATCH_BYTES: usize = 6 * 1024 * 1024;
static IMAGE_JOBS: Semaphore = Semaphore::const_new(2);

/// An explicit joined channel and an ordered set of requested preview messages.
pub struct MediaPreviewRequest {
    channel_id: ChannelId,
    ids: Vec<i32>,
    max_dimension: u32,
}

impl MediaPreviewRequest {
    /// Validate, deduplicate and keep IDs in request order, and bound dimensions.
    ///
    /// # Errors
    /// Rejects an empty/oversized batch, or a message ID outside Telegram's range.
    pub fn try_new(
        channel_id: ChannelId,
        message_ids: Vec<MessageId>,
        max_dimension: Option<u32>,
    ) -> Result<Self, Error> {
        let mut seen = HashSet::new();
        let mut ids = Vec::new();
        for message in message_ids {
            if seen.insert(message) {
                ids.push(super::validation::wire_message_id(message)?);
            }
            if ids.len() > MAX_IDS {
                return Err(Error::InvalidInput(
                    "Too many requested media previews".into(),
                ));
            }
        }
        if ids.is_empty() {
            return Err(Error::InvalidInput(
                "At least one media preview is required".into(),
            ));
        }
        Ok(Self {
            channel_id,
            ids,
            max_dimension: max_dimension.unwrap_or(1280).clamp(64, 2048),
        })
    }
}

/// Retrieval boundary for positively identified joined channels.
#[doc(hidden)]
#[async_trait::async_trait]
pub trait JoinedMediaSource: Send + Sync {
    /// Check that the account can currently read a qualified joined channel.
    ///
    /// # Errors
    /// Returns channel-unavailable or a typed provider failure.
    async fn verify_media_channel(&self, channel_id: ChannelId) -> Result<(), Error>;

    /// Prepare raw media from only the requested joined channel.
    ///
    /// # Errors
    /// Returns the whole-operation error when membership or raw fetching fails.
    async fn joined_media(
        &self,
        channel_id: ChannelId,
        message_ids: &[i32],
        max_dimension: u32,
    ) -> Result<MediaBatch, Error>;
}

/// Processed JPEG and bounded scalar source metadata.
pub struct MediaPreview {
    /// JPEG bytes subject to the image and batch caps.
    pub jpeg: Vec<u8>,
    /// Preview width in pixels.
    pub width: u32,
    /// Preview height in pixels.
    pub height: u32,
    /// Original Telegram media type.
    pub media_type: MediaType,
    /// Whether the preview was rendered from a Telegram thumbnail.
    pub is_thumbnail: bool,
    /// Size of the downloaded source variant in bytes.
    pub source_size_bytes: u64,
}

/// One successful preview or typed failure for a requested message.
pub struct MediaPreviewItem {
    /// Positive Telegram message ID in the requested channel.
    pub message_id: i32,
    /// Bounded JPEG or a safe typed media failure.
    pub result: Result<MediaPreview, Error>,
}

/// Ordered batch with a charge reservation until delivery has been accepted.
///
/// Dropping a batch before delivery refunds the preview charges once.
pub struct MediaPreviewBatch<R: RateLimiterTrait> {
    /// One outcome per distinct message ID.
    pub items: Vec<MediaPreviewItem>,
    charge: Charge<R>,
    cost: u32,
    previews: usize,
}

impl<R: RateLimiterTrait> MediaPreviewBatch<R> {
    /// Settle the charge for previews that received transport handles or output.
    pub fn deliver(mut self, delivered: usize) {
        self.charge.retained = self
            .cost
            .saturating_mul(delivered.min(self.previews) as u32);
    }
}

struct Charge<R: RateLimiterTrait> {
    limiter: Arc<R>,
    reserved: u32,
    retained: u32,
}

impl<R: RateLimiterTrait> Drop for Charge<R> {
    fn drop(&mut self) {
        let refund = self.reserved.saturating_sub(self.retained);
        if refund != 0 {
            self.limiter.refund(refund);
        }
    }
}

/// A stdio media charge pending transport shaping. Dropping the reservation
/// refunds the uncommitted items, including when the renderer is cancelled.
pub struct MediaBatchReservation<R: RateLimiterTrait> {
    charge: Charge<R>,
    item_cost: u32,
    items: usize,
}

impl<R: RateLimiterTrait> MediaBatchReservation<R> {
    /// Settle the original charge for the images that survived shaping.
    pub fn finish(mut self, returned: usize) {
        let retained = self
            .item_cost
            .saturating_mul(returned.min(self.items) as u32);
        let refund = self.charge.reserved.saturating_sub(retained);
        self.charge.retained = self.charge.reserved;
        self.charge.limiter.refund(refund);
    }
}

impl<T, R: RateLimiterTrait> AccountOperations<T, R> {
    async fn reserve_media(
        &self,
        amount: u32,
        timing: &BoundedOperation<'_>,
    ) -> Result<Charge<R>, Error> {
        timing.run(self.limiter.acquire(amount)).await?;
        Ok(Charge {
            limiter: Arc::clone(&self.limiter),
            reserved: amount,
            retained: 0,
        })
    }
}

impl<T: TelegramClientTrait, R: RateLimiterTrait> AccountOperations<T, R> {
    /// Resolve and download one local media preview under the caller deadline.
    ///
    /// # Errors
    /// Returns validation, account admission, deadline or upstream failures.
    pub async fn download_media_by_reference(
        &self,
        channel: &str,
        id: i32,
        max_dimension: u32,
        cost: u32,
        context: &OperationContext,
    ) -> Result<MediaDownload, Error> {
        validate_local_media(channel, &[id], max_dimension)?;
        let timing = context.bound(Duration::MAX, "download_media");
        let mut charge = self.reserve_media(cost, &timing).await?;
        timing
            .run(async {
                charge.retained = cost;
                self.client
                    .download_message_media(channel, id, max_dimension)
                    .await
            })
            .await
    }

    /// Fetch a raw media batch with one admission for its distinct requested IDs.
    ///
    /// A failed shared lookup retains one token. The caller settles successful
    /// images after applying its output budget and transport format.
    ///
    /// # Errors
    /// Returns validation, account admission, deadline or shared lookup failures.
    pub async fn download_media_batch_by_reference(
        &self,
        channel: &str,
        ids: &[i32],
        max_dimension: u32,
        cost: u32,
        context: &OperationContext,
    ) -> Result<(MediaBatch, MediaBatchReservation<R>), Error> {
        validate_local_media(channel, ids, max_dimension)?;
        let timing = context.bound(Duration::MAX, "download_media");
        let mut charge = self
            .reserve_media(cost.saturating_mul(ids.len() as u32), &timing)
            .await?;
        let batch = timing
            .run(async {
                charge.retained = 1;
                self.client
                    .download_messages_media_pipeline(channel, ids, max_dimension)
                    .await
            })
            .await?;
        charge.retained = 0;
        Ok((
            batch,
            MediaBatchReservation {
                charge,
                item_cost: cost,
                items: ids.len(),
            },
        ))
    }
}

fn validate_local_media(channel: &str, ids: &[i32], max_dimension: u32) -> Result<(), Error> {
    super::validation::parse_channel_reference(channel).map_err(Error::InvalidInput)?;
    if !(1..=MAX_IDS).contains(&ids.len())
        || ids.iter().any(|id| *id <= 0)
        || ids.iter().copied().collect::<HashSet<_>>().len() != ids.len()
        || !(64..=2048).contains(&max_dimension)
    {
        return Err(Error::InvalidInput("invalid media selection".into()));
    }
    Ok(())
}

impl<T: JoinedMediaSource, R: RateLimiterTrait> AccountOperations<T, R> {
    /// Recheck qualified channel membership before returning a stored hosted preview.
    ///
    /// # Errors
    /// Returns admission, cancellation, time limit, provider or membership failure.
    pub async fn authorize_media_channel(
        &self,
        channel_id: ChannelId,
        context: &OperationContext,
    ) -> Result<(), Error> {
        let timing = context.bound(self.limits.history_timeout, "authorize_media_channel");
        timing.run(self.limiter.acquire(1)).await?;
        timing
            .run(self.client.verify_media_channel(channel_id))
            .await
    }

    /// Download and shape a bounded preview batch, reserving one cost per distinct item.
    ///
    /// The timeout includes admission, all raw I/O, waiting for a CPU worker and shaping.
    /// A failed shared fetch retains the base lookup charge. Item and delivery failures
    /// refund their costs when the caller settles or drops the returned batch.
    ///
    /// # Errors
    /// Returns admission, membership, initial fetching or cancellation failure.
    pub async fn media_previews(
        &self,
        request: MediaPreviewRequest,
        cost: u32,
        timeout: Duration,
        context: &OperationContext,
    ) -> Result<MediaPreviewBatch<R>, Error> {
        let timing = context.bound(timeout, "media_previews");
        let reserved = cost.saturating_mul(request.ids.len() as u32);
        let mut charge = self.reserve_media(reserved, &timing).await?;
        let batch = timing
            .run(async {
                charge.retained = 1;
                self.client
                    .joined_media(request.channel_id, &request.ids, request.max_dimension)
                    .await
            })
            .await?;
        if batch.channel_id != request.channel_id || batch.peer_namespace != PeerNamespace::Channel
        {
            return Err(Error::UnsupportedPeer);
        }
        charge.retained = 0;

        let deadline = batch.deadline;
        let mut stream = batch.into_stream();
        let mut bytes = 0;
        let mut previews = 0;
        let mut items = Vec::with_capacity(request.ids.len());
        for message_id in request.ids {
            let cap = MAX_PREVIEW_JPEG_BYTES.min(MAX_BATCH_BYTES - bytes);
            let work = async {
                let outcome = stream
                    .next()
                    .await
                    .filter(|outcome| outcome.message_id == message_id)
                    .ok_or(Error::Internal("media outcome identity unavailable"))?;
                let download = outcome
                    .result
                    .map_err(|error| item_error(request.channel_id, message_id, error))?;
                if download.channel_id != request.channel_id
                    || download.peer_namespace != PeerNamespace::Channel
                {
                    return Err(Error::UnsupportedPeer);
                }
                shape(download, request.max_dimension, cap).await
            };
            let result = timing
                .run(async {
                    match deadline {
                        Some(deadline) => deadline.run(work).await?,
                        None => work.await,
                    }
                })
                .await;
            if matches!(result, Err(Error::Cancelled)) {
                return Err(Error::Cancelled);
            }
            if let Ok(preview) = &result {
                previews += 1;
                bytes += preview.jpeg.len();
            }
            items.push(MediaPreviewItem { message_id, result });
        }
        Ok(MediaPreviewBatch {
            items,
            charge,
            cost,
            previews,
        })
    }
}

fn item_error(channel: ChannelId, id: i32, error: MediaFetchError) -> Error {
    match error {
        MediaFetchError::NotFound => Error::MessageNotFound {
            channel: channel.to_string(),
            message_id: i64::from(id),
        },
        MediaFetchError::NoVisualMedia { media_type } => Error::NoVisualMedia { media_type },
        MediaFetchError::Failed(error) => error,
    }
}

async fn shape(
    download: MediaDownload,
    max_dimension: u32,
    cap: usize,
) -> Result<MediaPreview, Error> {
    if cap == 0 {
        return Err(Error::PayloadCapExceeded { limit: cap });
    }
    let permit = IMAGE_JOBS
        .acquire()
        .await
        .map_err(|_| Error::Internal("preview worker unavailable"))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _raw = download.lease;
        let encoded = shape_jpeg(&download.bytes, max_dimension, cap, Some)?;
        let (width, height) = (encoded.width, encoded.height);
        let jpeg = match encoded.jpeg {
            Cow::Borrowed(_) => download.bytes,
            Cow::Owned(jpeg) => jpeg,
        };
        Ok(MediaPreview {
            jpeg,
            width,
            height,
            media_type: download.media_type,
            is_thumbnail: download.is_thumbnail,
            source_size_bytes: download.source_size_bytes,
        })
    })
    .await
    .map_err(|_| Error::Internal("preview worker failed"))?
}

#[cfg(test)]
#[path = "tests/media.rs"]
mod tests;
