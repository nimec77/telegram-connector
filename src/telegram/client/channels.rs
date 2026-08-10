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

    pub(super) async fn search_public_channels_impl(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<crate::telegram::Channel>, Error> {
        use grammers_client::peer::Peer;

        if query.trim().is_empty() {
            return Err(Error::InvalidInput(
                "Search query cannot be empty".to_string(),
            ));
        }

        let request = tl::functions::contacts::Search {
            // No restriction to broadcasts-only or bots-only; a general keyword
            // search over the whole public directory.
            broadcasts: false,
            bots: false,
            q: query.to_string(),
            limit: limit.clamp(1, 50) as i32,
        };
        let found = with_timeout("contacts_search", self.timeouts.search_secs, async {
            self.client
                .invoke(&request)
                .await
                .map_err(|e| Error::TelegramApi(format!("Public search failed: {e}")))
        })
        .await?;

        let tl::enums::contacts::Found::Found(found) = found;
        // `my_results` are matches from the caller's own dialogs (already
        // subscribed); `chats` carries the `Chat` objects for both `my_results`
        // and the wider `results` set with no per-entry subscribed flag, so
        // membership has to be recomputed by bare id.
        let subscribed_ids = subscribed_chat_ids(&found.my_results);

        let channels = found
            .chats
            .into_iter()
            .filter_map(|chat| {
                // `Chat::Empty` carries no usable identity; skip it rather than
                // surfacing a placeholder "Unknown" channel.
                if matches!(&chat, tl::enums::Chat::Empty(_)) {
                    return None;
                }
                let is_subscribed = subscribed_ids.contains(&chat.id());
                // `Peer::from_raw` already routes each `Chat` variant to the right
                // peer kind (including the broadcast-vs-megagroup distinction
                // inside `Chat::Channel`/`ChannelForbidden`, which the individual
                // `Channel`/`Group`/`Community::from_raw` constructors panic on if
                // mismatched) — reuse it instead of re-deriving that routing here.
                let peer = Peer::from_raw(&self.client, chat);
                if is_subscribed {
                    convert_peer_to_channel(&peer)
                } else {
                    convert_discovered_peer(&peer)
                }
            })
            .collect();
        Ok(channels)
    }
}

/// Bare `chat_id`/`channel_id`s present in `contacts.search`'s `my_results` —
/// matches from the caller's own dialogs, as opposed to the wider `results`
/// set. Pure function over plain TL data (no client/session needed), so it's
/// unit-tested directly rather than through the network-bound `*_impl` method.
/// `Peer::User` entries are irrelevant here: only `Chat`-shaped results are
/// ever converted to a `Channel`.
fn subscribed_chat_ids(my_results: &[tl::enums::Peer]) -> std::collections::HashSet<i64> {
    my_results
        .iter()
        .filter_map(|peer| match peer {
            tl::enums::Peer::Chat(p) => Some(p.chat_id),
            tl::enums::Peer::Channel(p) => Some(p.channel_id),
            tl::enums::Peer::User(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribed_chat_ids_collects_chat_and_channel_ids_only() {
        let my_results = vec![
            tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: 111 }),
            tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 222 }),
            tl::enums::Peer::User(tl::types::PeerUser { user_id: 333 }),
        ];

        let ids = subscribed_chat_ids(&my_results);

        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&111));
        assert!(ids.contains(&222));
        assert!(!ids.contains(&333));
    }

    #[test]
    fn subscribed_chat_ids_empty_when_no_matches() {
        assert!(subscribed_chat_ids(&[]).is_empty());
    }
}
