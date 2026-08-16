//! get_message_by_id operation.
//!
//! Unit of `client` (LM-2).

use super::guard::{is_empty_variant, require_found_raw};
use super::raw_fetch::fetch_messages_by_id;
use super::*;
use crate::telegram::envelope::EntityLookup;

impl TelegramClient {
    pub(super) async fn get_message_by_id_impl(
        &self,
        channel_ref: &str,
        message_id: i32,
    ) -> Result<crate::telegram::Message, Error> {
        validate_channel_identifier(channel_ref)?;

        // Resolve the channel peer (same pattern as get_channel_info). The resolve /
        // dialog-walk paths share the same hang exposure as the other tools, so they
        // are bounded by `resolve_secs` even though the plan only flags the message
        // fetch itself.
        let peer = self.resolve_peer(channel_ref).await?;

        // Get message by ID using grammers API
        let peer_ref = peer_to_ref(&peer).await?;

        // Raw getMessages instead of grammers' get_messages_by_id: same RPC,
        // but it keeps the response envelope so a forward from a channel we
        // do not subscribe to is still attributed (zero extra calls).
        let (mut by_id, entities) =
            with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
                fetch_messages_by_id(&self.client, peer_ref, &[message_id])
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

        // Deleted ids come back as a MessageEmpty placeholder, not as an
        // absent entry (work-order B1).
        let raw = require_found_raw(by_id.remove(&message_id), channel_ref, message_id)?;

        convert_raw_message(&raw, &peer, &entities).ok_or_else(|| {
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
        validate_channel_identifier(channel_ref)?;
        if message_ids.is_empty() {
            return Err(Error::InvalidInput(
                "message_ids cannot be empty".to_string(),
            ));
        }

        let peer = self.resolve_peer(channel_ref).await?;
        let peer_ref = peer_to_ref(&peer).await?;

        let (mut by_id, entities) =
            with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
                fetch_messages_by_id(&self.client, peer_ref, message_ids)
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

        Ok(partition_batch(
            message_ids,
            &mut by_id,
            &peer,
            &entities,
            channel_ref,
        ))
    }
}

/// Split a fetched id-keyed map into found messages and missing ids.
///
/// Single pass so every requested id lands in exactly one bucket — never
/// silently in neither. An absent entry and a `MessageEmpty` both mean the id
/// does not exist in this channel; a present, non-empty message that still
/// fails domain conversion is logged and reported as missing rather than
/// dropped.
fn partition_batch(
    message_ids: &[i32],
    by_id: &mut std::collections::HashMap<i32, tl::enums::Message>,
    peer: &grammers_client::peer::Peer,
    entities: &EntityLookup,
    channel_ref: &str,
) -> crate::telegram::MessageBatch {
    let mut messages = Vec::with_capacity(message_ids.len());
    let mut missing_ids = Vec::with_capacity(message_ids.len());
    for &message_id in message_ids {
        match by_id.remove(&message_id) {
            Some(raw) if !is_empty_variant(&raw) => {
                match convert_raw_message(&raw, peer, entities) {
                    Some(converted) => messages.push(converted),
                    None => {
                        tracing::warn!(
                            channel_ref = %channel_ref,
                            message_id,
                            "Failed to convert message in batch; reporting as missing"
                        );
                        missing_ids.push(i64::from(message_id));
                    }
                }
            }
            _ => missing_ids.push(i64::from(message_id)),
        }
    }
    crate::telegram::MessageBatch {
        messages,
        missing_ids,
    }
}

#[cfg(test)]
#[path = "tests/ops_message_tests.rs"]
mod tests;
