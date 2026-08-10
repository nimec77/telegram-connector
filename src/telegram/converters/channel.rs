//! Peer -> `Channel` conversion.
//!
//! Sub-domain of `converters` (LM-4).

use crate::telegram::types::{Channel, ChannelId, ChannelName, Username};

/// Build the sentinel `Username` used when a peer exposes no public username.
///
/// The `kind` literal (`unknown`/`group`/`user`) is statically valid, so
/// construction is infallible — `expect` replaces the bare `.unwrap()` (CQ-1)
/// and gives the single fallback site shared by both converters (AD-4).
pub(crate) fn fallback_username(kind: &'static str) -> Username {
    Username::new(kind).expect("static fallback username is always valid")
}

/// Extract the `(id, display name, username-or-fallback)` triple shared by
/// `convert_peer_to_channel` and `convert_message` (AD-4).
///
/// Returns `None` only when the peer's id or display name cannot form a valid
/// newtype. Each peer kind keeps its own username fallback sentinel.
pub(crate) fn peer_identity(
    peer: &grammers_client::peer::Peer,
) -> Option<(ChannelId, ChannelName, Username)> {
    use grammers_client::peer::Peer;

    let triple = match peer {
        Peer::Channel(ch) => (
            ChannelId::new(ch.id().bare_id()?).ok()?,
            ChannelName::new(ch.title()).ok()?,
            ch.username()
                .and_then(|u| Username::new(u).ok())
                .unwrap_or_else(|| fallback_username("unknown")),
        ),
        Peer::Group(g) => (
            ChannelId::new(g.id().bare_id()?).ok()?,
            ChannelName::new(g.title().unwrap_or("Unknown")).ok()?,
            g.username()
                .and_then(|u| Username::new(u).ok())
                .unwrap_or_else(|| fallback_username("group")),
        ),
        // grammers 0.10 peer kind; exposes no username accessor, so it always
        // uses the group fallback sentinel.
        Peer::Community(c) => (
            ChannelId::new(c.id().bare_id()?).ok()?,
            ChannelName::new(c.title()).ok()?,
            fallback_username("group"),
        ),
        Peer::User(u) => (
            ChannelId::new(u.id().bare_id()?).ok()?,
            ChannelName::new(u.first_name().unwrap_or("User")).ok()?,
            u.username()
                .and_then(|un| Username::new(un).ok())
                // "user" is < 5 chars and fails Username validation, so a user
                // with no public username reuses the "unknown" sentinel (the
                // original `Username::new("user").unwrap()` panicked here).
                .unwrap_or_else(|| fallback_username("unknown")),
        ),
    };
    Some(triple)
}

/// Convert grammers Peer to our Channel type
pub fn convert_peer_to_channel(peer: &grammers_client::peer::Peer) -> Option<Channel> {
    use grammers_client::peer::Peer;

    // Only channels and groups convert; a User peer is not a channel. Capture the
    // variant-specific flags here, then share the identity triple via peer_identity.
    let (is_verified, is_public) = match peer {
        Peer::Channel(ch) => (ch.raw.verified, ch.username().is_some()),
        Peer::Group(g) => (false, g.username().is_some()),
        _ => {
            tracing::debug!(
                peer_id = peer.id().bare_id(),
                "Skipping non-channel/group peer in convert_peer_to_channel (likely a User)"
            );
            return None;
        }
    };

    let (id, name, username) = peer_identity(peer)?;

    Some(Channel {
        id,
        name,
        username,
        description: None,  // Not available from basic chat info
        member_count: None, // Not fetched from basic chat info; None ≠ a real zero (CQ-4)
        is_verified,
        is_public,
        is_subscribed: true, // We're iterating our dialogs, so we're subscribed
        last_message_date: None,
    })
}
