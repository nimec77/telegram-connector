//! get_message_by_id operation.
//!
//! Unit of `client` (LM-2).

use super::guard::{is_empty_variant, require_found};
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
        let peer_ref = peer_to_ref(&peer).await?;

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

        // get_messages_by_id returns Vec<Option<Message>>; deleted ids come
        // back as a wrapped MessageEmpty, not None (work-order B1).
        let grammers_msg = require_found(
            messages.into_iter().next().flatten(),
            channel_ref,
            message_id,
        )?;

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

    pub(super) async fn get_messages_batch_impl(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
    ) -> Result<crate::telegram::MessageBatch, Error> {
        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }
        if message_ids.is_empty() {
            return Err(Error::InvalidInput(
                "message_ids cannot be empty".to_string(),
            ));
        }

        let peer = self.resolve_peer(channel_ref).await?;
        let peer_ref = peer_to_ref(&peer).await?;

        let slots = with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
            self.client
                .get_messages_by_id(peer_ref, message_ids)
                .await
                .map_err(|e| {
                    tracing::error!(
                        channel_ref = %channel_ref,
                        count = message_ids.len(),
                        error = %e,
                        "Failed to get messages batch"
                    );
                    Error::TelegramApi(format!("Failed to get messages: {}", e))
                })
        })
        .await?;

        // Slot i corresponds to message_ids[i]. Single pass so every requested id
        // lands in exactly one of `messages` / `missing_ids` — never silently in
        // neither: absent and MessageEmpty both mean the id does not exist in this
        // channel (work-order B1 guard); a present, non-empty slot that still fails
        // domain conversion (bad peer identity, invalid id) is logged and reported
        // as missing rather than dropped.
        let mut messages = Vec::with_capacity(message_ids.len());
        let mut missing_ids = Vec::with_capacity(message_ids.len());
        for (&message_id, slot) in message_ids.iter().zip(slots) {
            match slot {
                Some(msg) if !is_empty_variant(&msg.raw) => match convert_message(&msg, &peer) {
                    Some(converted) => messages.push(converted),
                    None => {
                        tracing::warn!(
                            channel_ref = %channel_ref,
                            message_id,
                            "Failed to convert message in batch; reporting as missing"
                        );
                        missing_ids.push(i64::from(message_id));
                    }
                },
                // Absent slot or MessageEmpty placeholder: the id does not exist
                // in this channel (deleted, or never existed) — no log, this is
                // the expected/common "missing" path (work-order B1 guard).
                _ => missing_ids.push(i64::from(message_id)),
            }
        }

        Ok(crate::telegram::MessageBatch {
            messages,
            missing_ids,
        })
    }
}
