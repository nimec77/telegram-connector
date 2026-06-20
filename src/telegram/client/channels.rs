//! Channel listing and single-channel lookup.
//!
//! Unit of `client` (LM-2).

use super::*;

impl TelegramClient {
    pub(super) async fn get_subscribed_channels_impl(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<crate::telegram::Channel>, Error> {
        let mut channels = Vec::new();
        let mut dialogs = self.client.iter_dialogs();
        let mut count = 0u32;

        while let Some(dialog) = dialogs.next().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to iterate dialogs in get_subscribed_channels");
            Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
        })? {
            let peer = dialog.peer();

            // Only include channels and groups
            if let Some(channel) = convert_peer_to_channel(peer) {
                if count >= offset {
                    channels.push(channel);
                    if channels.len() >= limit as usize {
                        break;
                    }
                }
                count += 1;
            }
        }

        tracing::debug!(
            channels_found = channels.len(),
            offset,
            limit,
            "get_subscribed_channels completed"
        );

        Ok(channels)
    }

    pub(super) async fn get_channel_info_impl(
        &self,
        identifier: &str,
    ) -> Result<crate::telegram::Channel, Error> {
        // Validate identifier
        if identifier.is_empty() {
            return Err(Error::InvalidInput(
                "Channel identifier cannot be empty".to_string(),
            ));
        }

        // Resolve the channel reference (numeric id or username) to a peer. The
        // former inline @-prefix / numeric / bare-username branches are exactly
        // the cases `resolve_peer` already covers (AD-1).
        let peer = self.resolve_peer(identifier).await?;

        convert_peer_to_channel(&peer).ok_or_else(|| {
            tracing::warn!(
                peer_id = peer.id().bare_id(),
                "Resolved peer is not a channel or group"
            );
            Error::InvalidInput("Not a channel or group".to_string())
        })
    }
}
