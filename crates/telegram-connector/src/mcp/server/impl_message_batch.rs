//! `McpServer` inherent `*_impl` method: get_messages_batch (work-order A1).

use super::McpServer;
use crate::error::Error;
use crate::mcp::tools::validation::parse_message_id;
use crate::mcp::tools::{
    GetMessagesBatchRequest, MessageResponse, MessagesBatchResponse, MissingMessageEntry, shaping,
};
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use crate::telegram::types::MessageId;
use telegram_core::operations::{BatchOperation, CancellationToken};

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn get_messages_batch_data(
        &self,
        request: GetMessagesBatchRequest,
        budget_mode: shaping::BudgetMode,
        cancellation: CancellationToken,
    ) -> Result<MessagesBatchResponse, Error> {
        let message_ids = request
            .message_ids
            .into_iter()
            .map(parse_message_id)
            .collect::<Result<Vec<_>, _>>()?;

        let max_text_length = request
            .max_text_length
            .unwrap_or(shaping::DEFAULT_MAX_TEXT_LENGTH);
        if max_text_length == 0 {
            return Err(Error::InvalidInput(
                "max_text_length must be greater than 0".to_string(),
            ));
        }

        let operations = self.account_operations();
        let context = operations.history_context(cancellation)?;
        let batch = operations
            .batch(
                BatchOperation {
                    channel: request.channel_id.clone(),
                    message_ids,
                },
                &context,
            )
            .await?;

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
            resolved_channel: Some((batch.channel_id, batch.peer_namespace)),
            channel_id: request.channel_id,
            messages,
            returned,
            missing,
            omitted_ids: None,
        };
        if budget_mode == shaping::BudgetMode::Direct {
            shaping::fit_batch_to_budget(&mut response, self.response_byte_budget)?;
        }
        Ok(response)
    }
}
