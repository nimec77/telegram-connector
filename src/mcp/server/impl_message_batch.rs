//! `McpServer` inherent `*_impl` method: get_messages_batch (work-order A1).

use super::*;

/// Hard cap on ids per batch call (one `channels.GetMessages` RPC).
pub(super) const MAX_BATCH_IDS: usize = 50;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn get_messages_batch_impl(
        &self,
        request: GetMessagesBatchRequest,
    ) -> Result<String, String> {
        if request.channel_id.trim().is_empty() {
            return Err("channel_id is required".to_string());
        }
        if request.message_ids.is_empty() {
            return Err("message_ids must contain at least one id".to_string());
        }

        let (unique, wire_ids) = dedupe_and_validate_ids(&request.message_ids, MAX_BATCH_IDS)?;

        let max_text_length = request
            .max_text_length
            .unwrap_or(shaping::DEFAULT_MAX_TEXT_LENGTH);
        if max_text_length == 0 {
            return Err("max_text_length must be greater than 0".to_string());
        }

        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        let batch = self
            .telegram_client
            .get_messages_batch(&request.channel_id, &wire_ids)
            .await
            .map_err(|e| e.to_string())?;

        tracing::info!(
            channel = %request.channel_id,
            requested = unique.len(),
            found = batch.messages.len(),
            missing = batch.missing_ids.len(),
            "Messages batch results"
        );

        let mut messages: Vec<MessageResponse> = batch
            .messages
            .into_iter()
            .map(MessageResponse::from)
            .collect();
        for msg in &mut messages {
            shaping::truncate_text(msg, max_text_length);
        }
        let missing = batch
            .missing_ids
            .iter()
            .filter_map(|id| MessageId::new(*id).ok())
            .map(|id| MissingMessageEntry {
                id,
                error: "not found or deleted".to_string(),
            })
            .collect();

        let returned = messages.len();
        let mut response = MessagesBatchResponse {
            channel_id: request.channel_id,
            messages,
            returned,
            missing,
            omitted_ids: None,
        };
        shaping::fit_batch_to_budget(&mut response, self.response_byte_budget)?;
        json_response(&response)
    }
}
