//! Entity map built from a raw MTProto response envelope.
//!
//! History/search responses (`messages.Messages` family) carry `chats` and
//! `users` arrays with every entity their messages reference — including
//! forward sources the account is not subscribed to. This module turns those
//! arrays into a pure, client-free lookup used to attribute forwards (and
//! resolve senders) with zero extra network calls. Required because the
//! pinned grammers rev keeps `Message.peers` crate-private.

// Wired into the converters in the very next task of this plan; the allow
// only bridges the strict per-task clippy gate.
#![allow(dead_code)]

use grammers_client::tl;
use grammers_session::types::PeerId;
use std::collections::HashMap;

/// Display data for one peer out of a response envelope.
#[derive(Debug, Clone)]
pub(crate) struct EntityInfo {
    /// Title for chats/channels; "first last" for users.
    pub display_name: Option<String>,
    /// Users only — message-level `sender_name` has always been first-name-only.
    pub first_name: Option<String>,
    /// Public @username, without the prefix.
    pub username: Option<String>,
}

impl EntityInfo {
    /// Name for message-level `sender_name` (first name for users, title otherwise).
    pub(crate) fn sender_name(&self) -> Option<String> {
        self.first_name
            .clone()
            .or_else(|| self.display_name.clone())
    }
}

/// "first last" from the two optional name parts, however many are present.
fn join_names(first: Option<&str>, last: Option<&str>) -> Option<String> {
    match (first, last) {
        (Some(f), Some(l)) => Some(format!("{f} {l}")),
        (Some(f), None) => Some(f.to_string()),
        (None, Some(l)) => Some(l.to_string()),
        (None, None) => None,
    }
}

/// Pure lookup from a raw peer to its envelope entity.
///
/// Keyed by [`PeerId`], which bit-packs the peer namespace — a user and a
/// channel sharing a bare id never collide.
#[derive(Debug, Default)]
pub(crate) struct EntityLookup {
    map: HashMap<PeerId, EntityInfo>,
}

impl EntityLookup {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    /// Build from a response envelope's `chats` + `users` arrays.
    pub(crate) fn from_envelope(chats: &[tl::enums::Chat], users: &[tl::enums::User]) -> Self {
        let mut map = HashMap::new();
        for chat in chats {
            // Titles come from whichever variant carries them; forbidden
            // variants still name the source, which is all attribution needs.
            // Communities live in the channel namespace (Community::id() uses
            // channel_unchecked in the pinned grammers rev).
            let (key, title, username) = match chat {
                tl::enums::Chat::Channel(c) => (
                    PeerId::channel_unchecked(c.id),
                    c.title.clone(),
                    c.username.clone(),
                ),
                tl::enums::Chat::ChannelForbidden(c) => {
                    (PeerId::channel_unchecked(c.id), c.title.clone(), None)
                }
                tl::enums::Chat::Community(c) => {
                    (PeerId::channel_unchecked(c.id), c.title.clone(), None)
                }
                tl::enums::Chat::CommunityForbidden(c) => {
                    (PeerId::channel_unchecked(c.id), c.title.clone(), None)
                }
                tl::enums::Chat::Chat(c) => (PeerId::chat_unchecked(c.id), c.title.clone(), None),
                tl::enums::Chat::Forbidden(c) => {
                    (PeerId::chat_unchecked(c.id), c.title.clone(), None)
                }
                tl::enums::Chat::Empty(_) => continue,
            };
            map.insert(
                key,
                EntityInfo {
                    display_name: Some(title),
                    first_name: None,
                    username,
                },
            );
        }
        for user in users {
            if let tl::enums::User::User(u) = user {
                map.insert(
                    PeerId::user_unchecked(u.id),
                    EntityInfo {
                        display_name: join_names(u.first_name.as_deref(), u.last_name.as_deref()),
                        first_name: u.first_name.clone(),
                        username: u.username.clone(),
                    },
                );
            }
        }
        Self { map }
    }

    /// Resolve a raw `Peer` reference (e.g. a fwd header's `from_id`).
    pub(crate) fn get(&self, peer: &tl::enums::Peer) -> Option<&EntityInfo> {
        self.map.get(&PeerId::from(peer.clone()))
    }

    /// Seed from a high-level peer (used by the grammers-`Message` wrapper so
    /// call paths without an envelope keep their sender resolution).
    pub(crate) fn insert_peer(&mut self, peer: &grammers_client::peer::Peer) {
        use grammers_client::peer::Peer;
        let info = match peer {
            Peer::User(u) => EntityInfo {
                display_name: join_names(u.first_name(), u.last_name()),
                first_name: u.first_name().map(|s| s.to_string()),
                username: u.username().map(|s| s.to_string()),
            },
            Peer::Group(g) => EntityInfo {
                display_name: g.title().map(|s| s.to_string()),
                first_name: None,
                username: g.username().map(|s| s.to_string()),
            },
            Peer::Channel(c) => EntityInfo {
                display_name: Some(c.title().to_string()),
                first_name: None,
                username: c.username().map(|s| s.to_string()),
            },
            Peer::Community(c) => EntityInfo {
                display_name: Some(c.title().to_string()),
                first_name: None,
                username: None,
            },
        };
        self.map.insert(peer.id(), info);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{raw_tl_channel, raw_tl_user};
    use grammers_client::tl;

    fn channel_peer(id: i64) -> tl::enums::Peer {
        tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: id })
    }
    fn user_peer(id: i64) -> tl::enums::Peer {
        tl::enums::Peer::User(tl::types::PeerUser { user_id: id })
    }
    fn chat_peer(id: i64) -> tl::enums::Peer {
        tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: id })
    }

    #[test]
    fn resolves_channel_title_and_username_from_envelope() {
        let chats = vec![tl::enums::Chat::Channel(raw_tl_channel(
            1783384254,
            "Военкор",
            Some("voenkor_ru"),
        ))];
        let lookup = EntityLookup::from_envelope(&chats, &[]);
        let info = lookup.get(&channel_peer(1783384254)).expect("hit");
        assert_eq!(info.display_name.as_deref(), Some("Военкор"));
        assert_eq!(info.username.as_deref(), Some("voenkor_ru"));
        assert_eq!(info.first_name, None);
    }

    #[test]
    fn channel_without_username_resolves_title_only() {
        let chats = vec![tl::enums::Chat::Channel(raw_tl_channel(
            77,
            "Приватный",
            None,
        ))];
        let lookup = EntityLookup::from_envelope(&chats, &[]);
        let info = lookup.get(&channel_peer(77)).expect("hit");
        assert_eq!(info.display_name.as_deref(), Some("Приватный"));
        assert_eq!(info.username, None);
    }

    #[test]
    fn resolves_user_full_name_and_first_name() {
        let users = vec![tl::enums::User::User(raw_tl_user(
            42,
            Some("Иван"),
            Some("Петров"),
            Some("ivanp"),
        ))];
        let lookup = EntityLookup::from_envelope(&[], &users);
        let info = lookup.get(&user_peer(42)).expect("hit");
        assert_eq!(info.display_name.as_deref(), Some("Иван Петров"));
        assert_eq!(info.first_name.as_deref(), Some("Иван"));
        assert_eq!(info.username.as_deref(), Some("ivanp"));
        assert_eq!(info.sender_name().as_deref(), Some("Иван"));
    }

    #[test]
    fn user_without_last_name_uses_first_name_as_display() {
        let users = vec![tl::enums::User::User(raw_tl_user(
            7,
            Some("Анна"),
            None,
            None,
        ))];
        let lookup = EntityLookup::from_envelope(&[], &users);
        let info = lookup.get(&user_peer(7)).expect("hit");
        assert_eq!(info.display_name.as_deref(), Some("Анна"));
    }

    #[test]
    fn same_bare_id_in_different_namespaces_does_not_collide() {
        let chats = vec![tl::enums::Chat::Channel(raw_tl_channel(5, "Канал", None))];
        let users = vec![tl::enums::User::User(raw_tl_user(
            5,
            Some("Юзер"),
            None,
            None,
        ))];
        let lookup = EntityLookup::from_envelope(&chats, &users);
        assert_eq!(
            lookup
                .get(&channel_peer(5))
                .expect("channel")
                .display_name
                .as_deref(),
            Some("Канал")
        );
        assert_eq!(
            lookup
                .get(&user_peer(5))
                .expect("user")
                .first_name
                .as_deref(),
            Some("Юзер")
        );
    }

    #[test]
    fn miss_and_empty_variants_return_none() {
        let chats = vec![tl::enums::Chat::Empty(tl::types::ChatEmpty { id: 9 })];
        let users = vec![tl::enums::User::Empty(tl::types::UserEmpty { id: 9 })];
        let lookup = EntityLookup::from_envelope(&chats, &users);
        assert!(lookup.get(&chat_peer(9)).is_none());
        assert!(lookup.get(&user_peer(9)).is_none());
        assert!(EntityLookup::empty().get(&channel_peer(1)).is_none());
    }
}
