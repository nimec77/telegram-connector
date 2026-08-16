//! Channel listing and single-channel lookup.
//!
//! Unit of `client` (LM-2).

use super::*;

impl TelegramClient {
    pub(super) async fn get_subscribed_channels_impl(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<crate::telegram::ChannelPage, Error> {
        let mut builder = ChannelPageBuilder::new(offset, limit);
        let mut dialogs = self.client.iter_dialogs();

        while let Some(dialog) = dialogs.next().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to iterate dialogs in get_subscribed_channels");
            Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
        })? {
            if let Some(mut channel) = convert_peer_to_channel(dialog.peer()) {
                // Free enrichment: the dialog already carries its top message (B8).
                channel.last_message_date =
                    dialog.last_message.as_ref().and_then(message_timestamp);
                builder.admit(channel);
            }
        }

        let page = builder.finish();
        tracing::debug!(
            returned = page.channels.len(),
            total = page.total,
            offset,
            limit,
            "get_subscribed_channels completed"
        );
        Ok(page)
    }

    pub(super) async fn get_channel_info_impl(
        &self,
        identifier: &str,
    ) -> Result<crate::telegram::Channel, Error> {
        validate_channel_identifier(identifier)?;

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
        // Same guard as the basic path: an empty identifier is a caller error, not
        // something to hand to peer resolution.
        validate_channel_identifier(identifier)?;

        let peer = self.resolve_peer(identifier).await?;
        let mut channel = convert_peer_to_channel(&peer)
            .ok_or_else(|| Error::InvalidInput("Not a channel or group".to_string()))?;

        // channels.GetFullChannel only exists for channel-kind peers
        // (broadcasts + megagroups). Others keep basic info.
        if supports_full_channel_rpc(&peer) {
            // Enrichment is strictly opt-in-for-more: a failure here (private or
            // forbidden channel, timeout, transport error) must never turn the
            // basic info the caller would otherwise have got into an error.
            match self.fetch_channel_full(&peer).await {
                Ok((description, member_count)) => {
                    if let Some(about) = description {
                        channel.description = Some(about);
                    }
                    channel.member_count = member_count;
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        channel_id = channel.id.get(),
                        "GetFullChannel enrichment failed; returning basic channel info"
                    );
                }
            }
        }

        // include_full already means "extra RPC accepted": peek the newest
        // message for last_message_date. Degrade, never fail (same policy as
        // the GetFullChannel enrichment above).
        match self.fetch_last_message_date(&peer).await {
            Ok(date) => channel.last_message_date = date,
            Err(e) => {
                tracing::warn!(error = %e, channel_id = channel.id.get(),
                    "last-message peek failed; leaving last_message_date null");
            }
        }

        Ok(channel)
    }

    /// Invoke `channels.GetFullChannel` and pull out the two enrichment fields.
    ///
    /// Split from [`Self::get_full_channel_info_impl`] so the failure of the
    /// extra RPC is a value the caller can degrade on rather than a `?` that
    /// discards the already-built basic `Channel`.
    async fn fetch_channel_full(
        &self,
        peer: &grammers_client::peer::Peer,
    ) -> Result<(Option<String>, Option<u64>), Error> {
        let peer_ref = peer_to_ref(peer).await?;
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
            let description = (!cf.about.is_empty()).then_some(cf.about);
            let member_count = cf.participants_count.and_then(|c| u64::try_from(c).ok());
            Ok((description, member_count))
        } else {
            Ok((None, None))
        }
    }

    /// Newest message's timestamp, via a single-message history peek.
    async fn fetch_last_message_date(
        &self,
        peer: &grammers_client::peer::Peer,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, Error> {
        let peer_ref = peer_to_ref(peer).await?;
        with_timeout("last_message_peek", self.timeouts.history_secs, async {
            let mut iter = self.client.iter_messages(peer_ref);
            match iter.next().await {
                Ok(Some(msg)) => Ok(message_timestamp(&msg)),
                Ok(None) => Ok(None),
                Err(e) => Err(Error::TelegramApi(format!("last-message peek failed: {e}"))),
            }
        })
        .await
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

        let clamped_limit = limit.clamp(1, 50);
        let request = tl::functions::contacts::Search {
            // No restriction to broadcasts-only or bots-only; a general keyword
            // search over the whole public directory.
            broadcasts: false,
            bots: false,
            q: query.to_string(),
            limit: clamped_limit as i32,
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
        let subscribed_keys = subscribed_peer_keys(&found.my_results);

        // `contacts.Search.limit` bounds only the global `results` set; `chats`
        // additionally carries the `Chat` objects behind `my_results`, so the
        // converted list can overshoot the caller's limit. Truncate to what was
        // asked for.
        let channels = found
            .chats
            .into_iter()
            .filter_map(|chat| {
                // `Chat::Empty` carries no usable identity; skip it rather than
                // surfacing a placeholder "Unknown" channel.
                if matches!(&chat, tl::enums::Chat::Empty(_)) {
                    return None;
                }
                let is_subscribed = subscribed_keys.contains(&chat_subscription_key(&chat));
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
            .take(clamped_limit as usize)
            .collect();
        Ok(channels)
    }
}

/// Accumulates a `ChannelPage` while the dialog walk runs to completion.
///
/// The walk covers the WHOLE dialog list: the page is cut out in passing and
/// iteration continues, so `total` is the genuine subscription count (B6).
/// Iteration always started from the beginning anyway — offset pages already
/// paid the full walk.
struct ChannelPageBuilder {
    offset: usize,
    limit: usize,
    page: Vec<crate::telegram::Channel>,
    total: usize,
}

impl ChannelPageBuilder {
    fn new(offset: u32, limit: u32) -> Self {
        Self {
            offset: offset as usize,
            limit: limit as usize,
            page: Vec::new(),
            total: 0,
        }
    }

    fn admit(&mut self, channel: crate::telegram::Channel) {
        if self.total >= self.offset && self.page.len() < self.limit {
            self.page.push(channel);
        }
        self.total += 1;
    }

    fn finish(self) -> crate::telegram::ChannelPage {
        crate::telegram::ChannelPage {
            channels: self.page,
            total: self.total,
        }
    }
}

/// Whether `channels.GetFullChannel` applies to this peer.
///
/// "Channel-kind" in TL terms covers broadcasts *and* megagroups. grammers routes
/// a non-broadcast `Chat::Channel` to `Peer::Group` (`Peer::from_raw`), so a
/// megagroup arrives as a `Group` and has to be recognised via `is_megagroup()`.
/// Small groups (`Chat::Chat`) are chat-kind and must be excluded: their `PeerRef`
/// converts to `InputChannel::Empty`, which the RPC would reject.
fn supports_full_channel_rpc(peer: &grammers_client::peer::Peer) -> bool {
    use grammers_client::peer::Peer;

    match peer {
        Peer::Channel(_) => true,
        Peer::Group(g) => g.is_megagroup(),
        Peer::Community(_) | Peer::User(_) => false,
    }
}

/// Identity of a `contacts.search` result within its own id namespace.
///
/// `PeerChat.chat_id` and `PeerChannel.channel_id` are independent namespaces, so
/// a bare `i64` cannot distinguish them — keying membership by kind + id prevents
/// a numeric collision from marking a never-joined channel as subscribed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SubscriptionKey {
    Chat(i64),
    Channel(i64),
}

/// Namespace-qualified keys for `contacts.search`'s `my_results` — matches from
/// the caller's own dialogs, as opposed to the wider `results` set. Pure function
/// over plain TL data (no client/session needed), so it's unit-tested directly
/// rather than through the network-bound `*_impl` method. `Peer::User` entries
/// are irrelevant here: only `Chat`-shaped results are ever converted.
fn subscribed_peer_keys(
    my_results: &[tl::enums::Peer],
) -> std::collections::HashSet<SubscriptionKey> {
    my_results
        .iter()
        .filter_map(|peer| match peer {
            tl::enums::Peer::Chat(p) => Some(SubscriptionKey::Chat(p.chat_id)),
            tl::enums::Peer::Channel(p) => Some(SubscriptionKey::Channel(p.channel_id)),
            tl::enums::Peer::User(_) => None,
        })
        .collect()
}

/// The [`SubscriptionKey`] a `Chat` result is probed with — same namespace split
/// grammers itself applies when deriving a `PeerId` from a `Chat`.
fn chat_subscription_key(chat: &tl::enums::Chat) -> SubscriptionKey {
    use tl::enums::Chat as C;

    match chat {
        C::Empty(c) => SubscriptionKey::Chat(c.id),
        C::Chat(c) => SubscriptionKey::Chat(c.id),
        C::Forbidden(c) => SubscriptionKey::Chat(c.id),
        C::Channel(c) => SubscriptionKey::Channel(c.id),
        C::ChannelForbidden(c) => SubscriptionKey::Channel(c.id),
        C::Community(c) => SubscriptionKey::Channel(c.id),
        C::CommunityForbidden(c) => SubscriptionKey::Channel(c.id),
    }
}

#[cfg(test)]
#[path = "tests/channels_tests.rs"]
mod tests;
