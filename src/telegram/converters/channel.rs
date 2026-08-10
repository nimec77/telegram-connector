//! Peer -> `Channel` conversion.
//!
//! Sub-domain of `converters` (LM-4).

use crate::telegram::types::{Channel, ChannelId, ChannelIdentity, ChannelName, Username};

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

/// Convert grammers Peer to our Channel type (dialog-list path: subscribed).
pub fn convert_peer_to_channel(peer: &grammers_client::peer::Peer) -> Option<Channel> {
    convert_peer_with_subscription(peer, true)
}

/// Same conversion for peers found via public search (`contacts.Search`) — the
/// caller isn't necessarily subscribed to these, so `is_subscribed` is false.
pub fn convert_discovered_peer(peer: &grammers_client::peer::Peer) -> Option<Channel> {
    convert_peer_with_subscription(peer, false)
}

/// Shared conversion body for [`convert_peer_to_channel`] and
/// [`convert_discovered_peer`] — the two entry points differ only in whether
/// the resulting `Channel` is marked as subscribed.
fn convert_peer_with_subscription(
    peer: &grammers_client::peer::Peer,
    is_subscribed: bool,
) -> Option<Channel> {
    use grammers_client::peer::Peer;

    // Channels, groups, and communities convert; a User peer is not a channel.
    // Capture the variant-specific flags here, then share the identity triple
    // via peer_identity.
    let (is_verified, is_public) = match peer {
        Peer::Channel(ch) => (ch.raw.verified, ch.username().is_some()),
        Peer::Group(g) => (false, g.username().is_some()),
        // Community (grammers 0.10) exposes neither a username nor a verified
        // flag, so both default to false — same treatment as a private group.
        Peer::Community(_) => (false, false),
        Peer::User(_) => {
            tracing::debug!(
                peer_id = peer.id().bare_id(),
                "Skipping User peer in convert_peer_to_channel"
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
        is_subscribed,
        last_message_date: None,
    })
}

/// Extract the numeric-ID + optional-username pair used for link generation.
///
/// Unlike [`peer_identity`], no username fallback sentinel is applied: `None`
/// means the peer has no public username (work-order B2).
pub(crate) fn channel_identity(peer: &grammers_client::peer::Peer) -> Option<ChannelIdentity> {
    use grammers_client::peer::Peer;

    let id = ChannelId::new(peer.id().bare_id()?).ok()?;
    let username = match peer {
        Peer::Channel(ch) => ch.username().map(str::to_string),
        Peer::Group(g) => g.username().map(str::to_string),
        Peer::Community(_) => None,
        // A user peer's username builds a profile link (t.me/<username>), not
        // a message link, when fed into the link builder — pre-existing behavior.
        Peer::User(u) => u.username().map(str::to_string),
    };
    Some(ChannelIdentity { id, username })
}

#[cfg(test)]
mod tests {
    use super::*;
    use grammers_client::peer::{Community, Peer};
    use grammers_client::{Client, tl};
    use grammers_mtsender::SenderPool;
    use grammers_session::storages::MemorySession;
    use std::sync::Arc;

    /// Offline `Peer::Channel` fixture, mirroring `community_peer` — grammers'
    /// `Channel::from_raw` is public and needs only an inert client.
    fn channel_peer(id: i64, title: &str, username: Option<&str>) -> Peer {
        let session = Arc::new(MemorySession::default());
        let SenderPool { handle, .. } = SenderPool::new(session, 1);
        let client = Client::new(handle);
        let raw = tl::types::Channel {
            creator: false,
            left: false,
            broadcast: true,
            verified: false,
            megagroup: false,
            restricted: false,
            signatures: false,
            min: false,
            scam: false,
            has_link: false,
            has_geo: false,
            slowmode_enabled: false,
            call_active: false,
            call_not_empty: false,
            fake: false,
            gigagroup: false,
            noforwards: false,
            join_to_send: false,
            join_request: false,
            forum: false,
            stories_hidden: false,
            stories_hidden_min: false,
            stories_unavailable: true,
            signature_profiles: false,
            autotranslation: false,
            broadcast_messages_allowed: false,
            monoforum: false,
            forum_tabs: false,
            id,
            access_hash: Some(0),
            title: title.to_string(),
            username: username.map(str::to_string),
            photo: tl::enums::ChatPhoto::Empty,
            date: 0,
            restriction_reason: None,
            admin_rights: None,
            banned_rights: None,
            default_banned_rights: None,
            participants_count: None,
            usernames: None,
            stories_max_id: None,
            color: None,
            profile_color: None,
            emoji_status: None,
            level: None,
            subscription_until_date: None,
            bot_verification_icon: None,
            send_paid_messages_stars: None,
            linked_monoforum_id: None,
            linked_community_id: None,
        };
        Peer::Channel(grammers_client::peer::Channel::from_raw(
            &client,
            tl::enums::Chat::Channel(raw),
        ))
    }

    /// Build a `Peer::Community` without any I/O: the `SenderPool` runner is
    /// never spawned, so the `Client` is inert plumbing for `from_raw`.
    /// (grammers 0.10 made this possible; older versions had no offline path
    /// to a `Peer`, which is why converter tests didn't exist before.)
    fn community_peer(id: i64, title: &str) -> Peer {
        let session = Arc::new(MemorySession::default());
        let SenderPool { handle, .. } = SenderPool::new(session, 1);
        let client = Client::new(handle);
        let raw = tl::types::Community {
            creator: false,
            left: false,
            min: false,
            collapsed_in_dialogs: false,
            id,
            access_hash: Some(0),
            title: title.to_string(),
            photo: tl::enums::ChatPhoto::Empty,
            date: 0,
            admin_rights: None,
            default_banned_rights: None,
        };
        Peer::Community(Community::from_raw(
            &client,
            tl::enums::Chat::Community(raw),
        ))
    }

    #[test]
    fn peer_identity_maps_community_with_group_fallback_username() {
        let peer = community_peer(1234, "Test Community");

        let (id, name, username) = peer_identity(&peer).expect("community must yield an identity");

        assert_eq!(Some(id.get()), peer.id().bare_id());
        assert_eq!(name.as_str(), "Test Community");
        assert_eq!(username.as_str(), "group");
    }

    #[test]
    fn convert_peer_to_channel_does_not_drop_community() {
        let peer = community_peer(1234, "Test Community");

        let channel = convert_peer_to_channel(&peer)
            .expect("community is a group-like dialog and must convert to a Channel");

        assert_eq!(Some(channel.id.get()), peer.id().bare_id());
        assert_eq!(channel.name.as_str(), "Test Community");
        // Community exposes no username or verified flag in grammers 0.10.
        assert!(!channel.is_public);
        assert!(!channel.is_verified);
        assert!(channel.is_subscribed);
    }

    #[test]
    fn discovered_peer_is_not_subscribed() {
        let peer = community_peer(555, "Discovered");

        let channel = convert_discovered_peer(&peer).expect("must convert");
        assert!(!channel.is_subscribed);

        // and the existing path still reports subscribed:
        assert!(
            convert_peer_to_channel(&peer)
                .expect("must convert")
                .is_subscribed
        );
    }

    #[test]
    fn channel_identity_public_channel_carries_username() {
        let peer = channel_peer(1144180066, "Сводки", Some("swodki"));

        let identity = channel_identity(&peer).expect("channel must yield an identity");

        assert_eq!(identity.id.get(), 1144180066);
        assert_eq!(identity.username.as_deref(), Some("swodki"));
    }

    #[test]
    fn channel_identity_private_peer_has_no_username_sentinel() {
        let peer = community_peer(521440428, "Семейный чатик");

        let identity = channel_identity(&peer).expect("community must yield an identity");

        assert_eq!(identity.id.get(), 521440428);
        assert_eq!(identity.username, None);
    }
}
