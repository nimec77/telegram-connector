//! Native receiver ownership and authoritative channel heads for live setup.

use super::TelegramClient;
use super::raw_page::raw_peer_id;
use crate::error::Error;
use crate::streams::{ChannelHead, ChannelHeadLookup, SubscribeError, broadcast_membership};
use crate::telegram::converters::channel_identity;
use crate::telegram::types::{ChannelId, ChannelIdentity, MessageId};
use grammers_client::peer::Peer;
use grammers_client::{InvocationError, tl};
use grammers_session::types::{PeerKind, PeerRef};
use grammers_session::updates::UpdatesLike;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Clone, Copy)]
pub(super) enum JoinedChannelKind {
    Broadcast,
    BroadcastOrSupergroup,
}

pub(crate) struct JoinedChannel {
    pub(super) peer: Peer,
    pub(super) reference: PeerRef,
}

impl JoinedChannel {
    fn bind(peer: Peer, reference: PeerRef) -> Result<Self, Error> {
        if reference.id.kind() != PeerKind::Channel || reference.id != peer.id() {
            return Err(Error::UnsupportedPeer);
        }
        Ok(Self { peer, reference })
    }

    pub(crate) fn identity(&self) -> Result<ChannelIdentity, Error> {
        channel_identity(&self.peer).ok_or(Error::UnsupportedPeer)
    }
}

impl TelegramClient {
    pub(crate) fn take_stream_updates(
        &self,
    ) -> Result<mpsc::Receiver<UpdatesLike>, SubscribeError> {
        self.stream_updates
            .as_ref()
            .and_then(|updates| updates.lock().ok()?.take())
            .ok_or(SubscribeError::ReaderUnavailable)
    }

    pub(crate) fn stream_maintenance_timeout(&self) -> Duration {
        Duration::from_secs(self.timeouts.resolve_secs)
    }

    pub(crate) async fn require_joined_channel(
        &self,
        id: ChannelId,
    ) -> Result<JoinedChannel, Error> {
        self.joined_channel_peers(&[id], JoinedChannelKind::BroadcastOrSupergroup)
            .await?
            .remove(&id)
            .ok_or_else(|| ineligible(id))
    }

    pub(super) async fn joined_channel_peers(
        &self,
        channels: &[ChannelId],
        kind: JoinedChannelKind,
    ) -> Result<HashMap<ChannelId, JoinedChannel>, Error> {
        let requested: HashSet<_> = channels.iter().map(|id| id.get()).collect();
        let mut dialogs = self.client.iter_dialogs();
        let mut peers = HashMap::with_capacity(channels.len());
        while let Some(dialog) = dialogs.next().await.map_err(stream_rpc_error)? {
            let peer = dialog.peer();
            if peer.id().kind() == PeerKind::Channel
                && let Some(id) = peer.id().bare_id()
                && requested.contains(&id)
                && let Some(reference) = peer.to_ref().await.map_err(|_| head_unavailable())?
            {
                peers.insert(
                    ChannelId::new(id)?,
                    JoinedChannel::bind(peer.clone(), reference)?,
                );
                if peers.len() == channels.len() {
                    break;
                }
            }
        }
        if let Some(missing) = channels.iter().find(|channel| !peers.contains_key(channel)) {
            return Err(ineligible(*missing));
        }
        let confirmed = self
            .current_joined_channel_ids(
                peers.values().map(|channel| channel.reference).collect(),
                kind,
            )
            .await?;
        if let Some(&missing) = channels.iter().find(|id| !confirmed.contains(id)) {
            return Err(ineligible(missing));
        }
        Ok(peers)
    }

    pub(super) async fn current_joined_channel_ids(
        &self,
        references: Vec<PeerRef>,
        kind: JoinedChannelKind,
    ) -> Result<HashSet<ChannelId>, Error> {
        if references.is_empty() {
            return Ok(HashSet::new());
        }
        // Dialog peer normalization loses forbidden status. Confirm fresh raw flags
        // from the channel namespace before using these references for content.
        let tl::enums::messages::Chats::Chats(result) = self
            .client
            .invoke(&tl::functions::channels::GetChannels {
                id: references.into_iter().map(Into::into).collect(),
            })
            .await
            .map_err(stream_rpc_error)?
        else {
            return Err(head_unavailable());
        };
        confirmed_memberships(&result.chats, kind)
    }
}

#[async_trait::async_trait]
impl ChannelHeadLookup for TelegramClient {
    async fn channel_heads(&self, channels: &[ChannelId]) -> Result<Vec<ChannelHead>, Error> {
        let peers = self
            .joined_channel_peers(channels, JoinedChannelKind::Broadcast)
            .await?;
        let mut heads = Vec::with_capacity(channels.len());
        for &channel_id in channels {
            let result = self
                .client
                .invoke(&tl::functions::messages::GetHistory {
                    peer: peers
                        .get(&channel_id)
                        .ok_or_else(|| ineligible(channel_id))?
                        .reference
                        .into(),
                    offset_id: 0,
                    offset_date: 0,
                    add_offset: 0,
                    limit: 1,
                    max_id: 0,
                    min_id: 0,
                    hash: 0,
                })
                .await
                .map_err(|error| {
                    if error.is("CHANNEL_PRIVATE") || error.is("CHANNEL_INVALID") {
                        ineligible(channel_id)
                    } else {
                        stream_rpc_error(error)
                    }
                })?;
            heads.push(ChannelHead {
                channel_id,
                head_message_id: decode_head(channel_id, result)?,
            });
        }
        Ok(heads)
    }
}

fn confirmed_memberships(
    chats: &[tl::enums::Chat],
    kind: JoinedChannelKind,
) -> Result<HashSet<ChannelId>, Error> {
    let mut observations = HashMap::new();
    for chat in chats {
        let membership = match kind {
            JoinedChannelKind::Broadcast => broadcast_membership(chat),
            JoinedChannelKind::BroadcastOrSupergroup => {
                if let tl::enums::Chat::Channel(peer) = chat
                    && !peer.min
                    && peer.megagroup
                    && !peer.monoforum
                {
                    Some((ChannelId::new(peer.id)?, !peer.left))
                } else {
                    broadcast_membership(chat)
                }
            }
        };
        if let Some((id, joined)) = membership {
            observations
                .entry(id)
                .and_modify(|confirmed| *confirmed &= joined)
                .or_insert(joined);
        }
    }
    Ok(observations
        .into_iter()
        .filter_map(|(id, joined)| joined.then_some(id))
        .collect())
}

fn decode_head(
    channel: ChannelId,
    response: tl::enums::messages::Messages,
) -> Result<Option<MessageId>, Error> {
    use tl::enums::messages::Messages;
    let (messages, count) = match response {
        Messages::Messages(page) => (page.messages, None),
        Messages::Slice(page) => (page.messages, Some(page.count)),
        Messages::ChannelMessages(page) => (page.messages, Some(page.count)),
        Messages::NotModified(_) => return Err(head_unavailable()),
    };
    let Some(latest) = messages.first() else {
        return if count.is_none_or(|count| count == 0) {
            Ok(None)
        } else {
            Err(head_unavailable())
        };
    };
    if matches!(latest, tl::enums::Message::Empty(_))
        || !matches!(raw_peer_id(latest), Some(tl::enums::Peer::Channel(peer)) if peer.channel_id == channel.get())
    {
        return Err(head_unavailable());
    }
    MessageId::new(i64::from(latest.id()))
        .map(Some)
        .map_err(|_| head_unavailable())
}

fn ineligible(channel: ChannelId) -> Error {
    Error::ChannelNotFound(channel.to_string())
}

fn head_unavailable() -> Error {
    Error::TelegramApi("Channel head unavailable".into())
}

pub(super) fn stream_rpc_error(error: InvocationError) -> Error {
    match error {
        InvocationError::Rpc(rpc) if rpc.code == 420 => Error::RateLimit {
            retry_after_seconds: u64::from(rpc.value.unwrap_or(0)),
            detail: String::new(),
        },
        InvocationError::Rpc(rpc) if rpc.code == 401 => Error::Auth("Session unavailable".into()),
        InvocationError::Rpc(rpc)
            if matches!(rpc.name.as_str(), "CHANNEL_PRIVATE" | "CHANNEL_INVALID") =>
        {
            Error::UnsupportedPeer
        }
        _ => head_unavailable(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_channel_binding_rejects_chat_namespace_and_different_ids() {
        use grammers_session::types::{PeerAuth, PeerId};
        let pool = grammers_mtsender::SenderPool::new(
            std::sync::Arc::new(grammers_session::storages::MemorySession::default()),
            1,
        );
        let client = grammers_client::Client::new(pool.handle);
        let channel = || {
            Peer::from_raw(
                &client,
                tl::enums::Chat::Channel(crate::test_helpers::raw_tl_channel(42, "Channel", None)),
            )
        };
        let reference = |id| PeerRef {
            id,
            auth: PeerAuth::default(),
        };
        for wrong in [PeerId::chat_unchecked(42), PeerId::channel_unchecked(41)] {
            assert!(matches!(
                JoinedChannel::bind(channel(), reference(wrong)),
                Err(Error::UnsupportedPeer)
            ));
        }
        assert!(JoinedChannel::bind(channel(), reference(PeerId::channel_unchecked(42))).is_ok());
    }

    #[test]
    fn joined_membership_subset_drops_left_and_conflicting_channel_observations() {
        let raw = |id, left| {
            let mut channel = crate::test_helpers::raw_tl_channel(id, "Channel", None);
            channel.left = left;
            tl::enums::Chat::Channel(channel)
        };
        let confirmed = confirmed_memberships(
            &[raw(40, false), raw(41, false), raw(41, true), raw(42, true)],
            JoinedChannelKind::BroadcastOrSupergroup,
        )
        .expect("decode observed membership");
        assert_eq!(confirmed, [ChannelId::new(40).expect("positive ID")].into());
    }

    #[test]
    fn joined_channel_membership_keeps_basic_chat_ids_in_their_namespace() {
        let id = ChannelId::new(42).expect("channel ID");
        let basic_chat = tl::enums::Chat::Forbidden(tl::types::ChatForbidden {
            id: 42,
            title: "Basic group".into(),
        });
        let mode = JoinedChannelKind::BroadcastOrSupergroup;
        assert!(
            !confirmed_memberships(std::slice::from_ref(&basic_chat), mode)
                .expect("decode membership")
                .contains(&id)
        );

        let mut supergroup = crate::test_helpers::raw_tl_channel(42, "Supergroup", None);
        supergroup.broadcast = false;
        supergroup.megagroup = true;
        let channel = tl::enums::Chat::Channel(supergroup);
        assert!(
            confirmed_memberships(&[basic_chat, channel], mode)
                .expect("decode membership")
                .contains(&id)
        );
    }
}
