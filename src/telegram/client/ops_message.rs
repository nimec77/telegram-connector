//! get_message_by_id operation.
//!
//! Unit of `client` (LM-2).

use super::*;

impl TelegramClient {
    pub(super) async fn get_message_by_id_impl(
        &self,
        channel_ref: &str,
        message_id: i32,
    ) -> Result<crate::telegram::Message, Error> {
        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }

        // Resolve the channel peer (same pattern as get_channel_info). The resolve /
        // dialog-walk paths share the same hang exposure as the other tools, so they
        // are bounded by `resolve_secs` even though the plan only flags the message
        // fetch itself.
        let peer = self.resolve_peer(channel_ref).await?;

        // Get message by ID using grammers API
        let peer_ref = peer
            .to_ref()
            .await
            .ok_or_else(|| Error::TelegramApi("Failed to convert peer to PeerRef".to_string()))?;

        let messages = with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
            self.client
                .get_messages_by_id(peer_ref, &[message_id])
                .await
                .map_err(|e| {
                    tracing::error!(
                        channel_ref = %channel_ref,
                        message_id,
                        error = %e,
                        "Failed to get message by ID"
                    );
                    Error::TelegramApi(format!("Failed to get message: {}", e))
                })
        })
        .await?;

        // get_messages_by_id returns Vec<Option<Message>> — extract the single result
        let grammers_msg = messages.into_iter().next().flatten().ok_or_else(|| {
            tracing::warn!(
                channel_ref = %channel_ref,
                message_id,
                "Message not found"
            );
            Error::InvalidInput(format!(
                "Message {} not found in channel {}",
                message_id, channel_ref
            ))
        })?;

        // Convert to our domain type
        convert_message(&grammers_msg, &peer).ok_or_else(|| {
            tracing::error!(
                channel_ref = %channel_ref,
                message_id,
                "Failed to convert message to domain type"
            );
            Error::TelegramApi("Failed to convert message".to_string())
        })
    }
}
