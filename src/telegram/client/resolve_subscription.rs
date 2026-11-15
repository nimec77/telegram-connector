//! Contextual username decoding keeps raw membership evidence and SDK cache effects together.

use super::TelegramClient;
use crate::error::Error;
use crate::telegram::converters::{
    SubscriptionEvidence, convert_peer_with_evidence, peer_namespace, subscription_evidence,
};
use crate::telegram::timeout::with_timeout;
use crate::telegram::types::{Channel, PeerNamespace, Username};
use grammers_client::peer::{Peer, User};
use grammers_client::{Client, InvocationError, tl};
use grammers_session::Session;
use grammers_session::types::{PeerId, PeerInfo};
use std::collections::HashMap;
use std::future::Future;

pub(super) struct ResolvedChannelPeer {
    pub(super) peer: Peer,
    pub(super) subscription: SubscriptionEvidence,
}

impl ResolvedChannelPeer {
    pub(super) fn channel(&self) -> Option<Channel> {
        convert_peer_with_evidence(&self.peer, self.subscription)
    }
}

pub(super) fn decode_resolved_peer(
    client: &Client,
    response: tl::types::contacts::ResolvedPeer,
) -> (Option<ResolvedChannelPeer>, Vec<PeerInfo>) {
    let mut peers = HashMap::new();
    for user in response.users {
        let peer = Peer::User(User::from_raw(client, user));
        peers.insert(
            peer.id(),
            ResolvedChannelPeer {
                peer,
                subscription: SubscriptionEvidence::Unconfirmed,
            },
        );
    }
    for chat in response.chats {
        let subscription = subscription_evidence(&chat);
        let peer = Peer::from_raw(client, chat);
        peers.insert(peer.id(), ResolvedChannelPeer { peer, subscription });
    }
    // Match SDK build_peer_map: typed identity, last duplicate wins, full authorized envelope.
    let cache = peers
        .values()
        .map(|resolved| PeerInfo::from(&resolved.peer))
        .filter(|info| info.auth().is_some())
        .collect();
    (peers.remove(&PeerId::from(response.peer)), cache)
}

pub(super) async fn resolve_username_with<F, C, CF, E>(
    client: &Client,
    reference: &str,
    timeout_secs: u64,
    invoke: F,
    cache: C,
) -> Result<Option<ResolvedChannelPeer>, Error>
where
    F: Future<Output = Result<tl::enums::contacts::ResolvedPeer, InvocationError>>,
    C: FnOnce(Vec<PeerInfo>) -> CF,
    CF: Future<Output = Result<(), E>>,
{
    let username = reference.strip_prefix('@').unwrap_or(reference);
    if !Username::is_valid_shape(username) {
        return Ok(None);
    }
    with_timeout("resolve_username", timeout_secs, async {
        let response = match invoke.await {
            Ok(tl::enums::contacts::ResolvedPeer::Peer(response)) => response,
            Err(error) if error.is("USERNAME_NOT_OCCUPIED") => return Ok(None),
            Err(_) => return Err(Error::TelegramApi("Failed to resolve username".into())),
        };
        let (resolved, peers) = decode_resolved_peer(client, response);
        // Client::new enables auto_cache_peers; this connector never overrides it.
        if cache(peers).await.is_err() {
            tracing::warn!("Could not cache resolved username peers");
        }
        Ok(resolved)
    })
    .await
}

pub(super) async fn resolve_channel_with<D, U>(
    reference: &str,
    dialog: D,
    username: U,
) -> Result<ResolvedChannelPeer, Error>
where
    D: Future<Output = Result<Option<Peer>, Error>>,
    U: Future<Output = Result<Option<ResolvedChannelPeer>, Error>>,
{
    let resolved = if reference.parse::<i64>().is_ok() {
        dialog.await?.map(|peer| ResolvedChannelPeer {
            peer,
            subscription: SubscriptionEvidence::Dialog,
        })
    } else {
        username.await?
    }
    .ok_or_else(|| Error::ChannelNotFound(reference.to_string()))?;
    if peer_namespace(&resolved.peer) == PeerNamespace::User {
        return Err(Error::UnsupportedPeer);
    }
    Ok(resolved)
}

impl TelegramClient {
    pub(super) async fn resolve_username_peer_with_evidence(
        &self,
        reference: &str,
    ) -> Result<Option<ResolvedChannelPeer>, Error> {
        let request = tl::functions::contacts::ResolveUsername {
            username: reference.strip_prefix('@').unwrap_or(reference).to_string(),
            referer: None,
        };
        resolve_username_with(
            &self.client,
            reference,
            self.timeouts.resolve_secs,
            self.client.invoke(&request),
            |peers| self.session.cache_peers(peers),
        )
        .await
    }

    pub(super) async fn resolve_channel_peer_with_evidence(
        &self,
        reference: &str,
    ) -> Result<ResolvedChannelPeer, Error> {
        resolve_channel_with(
            reference,
            async {
                self.find_dialog_peer(
                    reference
                        .parse::<i64>()
                        .map_err(|_| Error::InvalidInput("Invalid channel ID".into()))?,
                )
                .await
            },
            self.resolve_username_peer_with_evidence(reference),
        )
        .await
    }
}

#[cfg(test)]
#[path = "tests/subscription_tests.rs"]
mod tests;
