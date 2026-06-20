//! Peer -> `Channel` conversion.
//!
//! Sub-domain of `converters` (LM-4).

use crate::telegram::types::{Channel, ChannelId, ChannelName, Username};

/// Convert grammers Peer to our Channel type
pub fn convert_peer_to_channel(peer: &grammers_client::peer::Peer) -> Option<Channel> {
    use grammers_client::peer::Peer;

    match peer {
        Peer::Channel(ch) => {
            let id = ChannelId::new(ch.id().bare_id()).ok()?;
            let name = ChannelName::new(ch.title()).ok()?;
            let username = ch
                .username()
                .and_then(|u| Username::new(u).ok())
                .unwrap_or_else(|| Username::new("unknown").unwrap());

            Some(Channel {
                id,
                name,
                username,
                description: None, // Not available from basic chat info
                member_count: 0,   // Would need additional API call
                is_verified: ch.raw.verified,
                is_public: ch.username().is_some(),
                is_subscribed: true, // We're iterating our dialogs, so we're subscribed
                last_message_date: None,
            })
        }
        Peer::Group(g) => {
            // Include groups as they behave like channels for our purposes
            let id = ChannelId::new(g.id().bare_id()).ok()?;
            let name = ChannelName::new(g.title().unwrap_or("Unknown")).ok()?;
            let username = g
                .username()
                .and_then(|u| Username::new(u).ok())
                .unwrap_or_else(|| Username::new("group").unwrap());

            Some(Channel {
                id,
                name,
                username,
                description: None,
                member_count: 0,
                is_verified: false,
                is_public: g.username().is_some(),
                is_subscribed: true,
                last_message_date: None,
            })
        }
        _ => {
            tracing::debug!(
                peer_id = peer.id().bare_id(),
                "Skipping non-channel/group peer in convert_peer_to_channel (likely a User)"
            );
            None
        }
    }
}
