//! Charged bounded lookup of individual channel messages.

use super::{AccountOperations, OperationContext};
use crate::error::Error;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::{MessageBatch, MessageId};
use std::collections::HashSet;

/// One channel and the message IDs to look up.
pub struct BatchOperation {
    /// Explicit channel reference.
    pub channel: String,
    /// Positive message IDs in caller order.
    pub message_ids: Vec<MessageId>,
}

const MAX_BATCH_IDS: usize = 50;

impl<T: TelegramClientTrait, R: RateLimiterTrait> AccountOperations<T, R> {
    /// Validate and fetch one batch, with one account charge.
    ///
    /// # Errors
    /// Returns invalid input, refusal, cancellation or a whole-operation error.
    pub async fn batch(
        &self,
        request: BatchOperation,
        context: &OperationContext,
    ) -> Result<MessageBatch, Error> {
        let timing = context.bound(self.limits.history_timeout, "get_messages_batch");
        if request.channel.trim().is_empty() {
            return Err(Error::InvalidInput("channel_id is required".into()));
        }
        if request.message_ids.is_empty() {
            return Err(Error::InvalidInput(
                "message_ids must contain at least one id".into(),
            ));
        }
        let mut seen = HashSet::new();
        let unique: Vec<MessageId> = request
            .message_ids
            .into_iter()
            .filter(|id| seen.insert(*id))
            .collect();
        if unique.len() > MAX_BATCH_IDS {
            return Err(Error::InvalidInput(format!(
                "message_ids accepts at most {MAX_BATCH_IDS} ids per call, got {}",
                unique.len()
            )));
        }
        let wire = unique
            .into_iter()
            .map(super::validation::wire_message_id)
            .collect::<Result<Vec<_>, _>>()?;
        super::validation::parse_channel_reference(&request.channel)
            .map_err(Error::InvalidInput)?;
        timing.run(self.limiter.acquire(1)).await?;
        timing
            .run(self.client.get_messages_batch(&request.channel, &wire))
            .await
    }
}
