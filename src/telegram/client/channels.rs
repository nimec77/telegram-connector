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

    pub(super) async fn get_full_channel_info_impl(
        &self,
        identifier: &str,
    ) -> Result<crate::telegram::Channel, Error> {
        let peer = self.resolve_peer(identifier).await?;
        let mut channel = convert_peer_to_channel(&peer)
            .ok_or_else(|| Error::InvalidInput("Not a channel or group".to_string()))?;

        // channels.GetFullChannel only exists for channel-kind peers
        // (broadcasts + megagroups). Others keep basic info.
        if matches!(peer, grammers_client::peer::Peer::Channel(_)) {
            let peer_ref = peer_to_ref(&peer).await?;
            let request = tl::functions::channels::GetFullChannel {
                channel: (&peer_ref).into(),
            };
            let full = with_timeout("get_full_channel", self.timeouts.resolve_secs, async {
                self.client
                    .invoke(&request)
                    .await
                    .map_err(|e| Error::TelegramApi(format!("GetFullChannel failed: {e}")))
            })
            .await?;

            let tl::enums::messages::ChatFull::Full(chat_full) = full;
            if let tl::enums::ChatFull::ChannelFull(cf) = chat_full.full_chat {
                if !cf.about.is_empty() {
                    channel.description = Some(cf.about);
                }
                channel.member_count = cf.participants_count.and_then(|c| u64::try_from(c).ok());
            }
        }

        Ok(channel)
    }
}
