//! Raw-TL response-envelope interpretation shared by the pagers and the
//! by-id fetch: page unpacking, peer/id extraction, and buffer fill.

use crate::telegram::envelope::EntityLookup;
use grammers_client::Client;
use grammers_client::tl;
use std::collections::VecDeque;
use std::sync::Arc;

/// One decoded page: raw messages plus the envelope entities they came with.
pub(super) struct RawPage {
    pub(super) messages: Vec<tl::enums::Message>,
    pub(super) chats: Vec<tl::enums::Chat>,
    pub(super) users: Vec<tl::enums::User>,
    pub(super) next_rate: Option<i32>,
    pub(super) last_chunk: bool,
}

/// Raw peer the message lives in (mirrors grammers `utils::peer_from_message`).
pub(super) fn raw_peer_id(raw: &tl::enums::Message) -> Option<&tl::enums::Peer> {
    match raw {
        tl::enums::Message::Message(m) => Some(&m.peer_id),
        tl::enums::Message::Service(m) => Some(&m.peer_id),
        tl::enums::Message::Empty(m) => m.peer_id.as_ref(),
    }
}

/// Decode a `messages.Messages` response, preserving the envelope and
/// computing grammers' last-chunk rule: `Messages` is always final;
/// `Slice`/`ChannelMessages` are final when empty or when the newest id in
/// the page is within `limit` of the absolute lowest message id (1).
pub(super) fn unpack_page(res: tl::enums::messages::Messages, limit: i32) -> RawPage {
    use tl::enums::messages::Messages;
    let (messages, chats, users, next_rate, last_chunk) = match res {
        Messages::Messages(m) => (m.messages, m.chats, m.users, None, true),
        Messages::Slice(m) => {
            let last = m.messages.is_empty() || m.messages[0].id() <= limit;
            (m.messages, m.chats, m.users, m.next_rate, last)
        }
        Messages::ChannelMessages(m) => {
            let last = m.messages.is_empty() || m.messages[0].id() <= limit;
            (m.messages, m.chats, m.users, None, last)
        }
        // hash is always 0 in our requests, so NotModified cannot occur.
        // Treat it as an empty final page rather than panicking (never unwrap).
        Messages::NotModified(_) => (vec![], vec![], vec![], None, true),
    };
    RawPage {
        messages,
        chats,
        users,
        next_rate,
        last_chunk,
    }
}

/// Access hash for a channel-namespace chat variant, if this envelope entry
/// is that chat. Communities and both Forbidden forms live in the channel
/// namespace of this grammers rev (`Community::id()` uses `channel_unchecked`)
/// and all carry an access hash usable for `InputPeerChannel`.
fn channel_access_hash(chat: &tl::enums::Chat, channel_id: i64) -> Option<i64> {
    match chat {
        tl::enums::Chat::Channel(c) if c.id == channel_id => Some(c.access_hash.unwrap_or(0)),
        tl::enums::Chat::ChannelForbidden(c) if c.id == channel_id => Some(c.access_hash),
        tl::enums::Chat::Community(c) if c.id == channel_id => Some(c.access_hash.unwrap_or(0)),
        tl::enums::Chat::CommunityForbidden(c) if c.id == channel_id => {
            Some(c.access_hash.unwrap_or(0))
        }
        _ => None,
    }
}

/// InputPeer for a message's chat, with the access hash taken from the same
/// page's envelope. Envelope miss → `InputPeer::Empty` (grammers reaches its
/// session cache first in `PeerMap::get_ref`, then falls back to `Empty`;
/// the envelope always names the chats of its own messages, so the extra
/// cache hop buys nothing here).
pub(super) fn input_peer_for_message(
    raw: &tl::enums::Message,
    page: &RawPage,
) -> tl::enums::InputPeer {
    let Some(peer) = raw_peer_id(raw) else {
        return tl::enums::InputPeer::Empty;
    };
    match peer {
        tl::enums::Peer::Channel(p) => page
            .chats
            .iter()
            .find_map(|chat| {
                channel_access_hash(chat, p.channel_id).map(|access_hash| {
                    tl::enums::InputPeer::Channel(tl::types::InputPeerChannel {
                        channel_id: p.channel_id,
                        access_hash,
                    })
                })
            })
            .unwrap_or(tl::enums::InputPeer::Empty),
        tl::enums::Peer::User(p) => page
            .users
            .iter()
            .find_map(|user| match user {
                tl::enums::User::User(u) if u.id == p.user_id => {
                    Some(tl::enums::InputPeer::User(tl::types::InputPeerUser {
                        user_id: u.id,
                        access_hash: u.access_hash.unwrap_or(0),
                    }))
                }
                _ => None,
            })
            .unwrap_or(tl::enums::InputPeer::Empty),
        tl::enums::Peer::Chat(p) => {
            tl::enums::InputPeer::Chat(tl::types::InputPeerChat { chat_id: p.chat_id })
        }
    }
}

/// Move a page's messages into a pager buffer, pairing each with the page's
/// (shared) entity lookup.
pub(super) fn fill_buffer(
    buffer: &mut VecDeque<(tl::enums::Message, Arc<EntityLookup>)>,
    page: RawPage,
) {
    let entities = Arc::new(EntityLookup::from_envelope(&page.chats, &page.users));
    buffer.extend(
        page.messages
            .into_iter()
            .map(|message| (message, Arc::clone(&entities))),
    );
}

/// The message's own chat as a high-level Peer, built from the envelope
/// (`Peer::from_raw` / `User::from_raw` are public; the crate-private
/// `PeerMap` is not — this is the raw-TL replacement for `msg.peer()`).
pub(super) fn chat_peer_for_message(
    client: &Client,
    raw: &tl::enums::Message,
    chats: &[tl::enums::Chat],
    users: &[tl::enums::User],
) -> Option<grammers_client::peer::Peer> {
    use grammers_client::peer::{Peer, User};
    let peer = raw_peer_id(raw)?;
    match peer {
        // Forbidden variants still identify the chat (grammers' own peer map
        // includes them), so a message whose chat lost access mid-iteration
        // converts instead of being dropped.
        tl::enums::Peer::Channel(p) => chats.iter().find_map(|chat| match chat {
            tl::enums::Chat::Channel(c) if c.id == p.channel_id => {
                Some(Peer::from_raw(client, chat.clone()))
            }
            tl::enums::Chat::ChannelForbidden(c) if c.id == p.channel_id => {
                Some(Peer::from_raw(client, chat.clone()))
            }
            tl::enums::Chat::Community(c) if c.id == p.channel_id => {
                Some(Peer::from_raw(client, chat.clone()))
            }
            tl::enums::Chat::CommunityForbidden(c) if c.id == p.channel_id => {
                Some(Peer::from_raw(client, chat.clone()))
            }
            _ => None,
        }),
        tl::enums::Peer::Chat(p) => chats.iter().find_map(|chat| match chat {
            tl::enums::Chat::Chat(c) if c.id == p.chat_id => {
                Some(Peer::from_raw(client, chat.clone()))
            }
            tl::enums::Chat::Forbidden(c) if c.id == p.chat_id => {
                Some(Peer::from_raw(client, chat.clone()))
            }
            _ => None,
        }),
        tl::enums::Peer::User(p) => users.iter().find_map(|user| match user {
            tl::enums::User::User(u) if u.id == p.user_id => {
                Some(Peer::User(User::from_raw(client, user.clone())))
            }
            _ => None,
        }),
    }
}

#[cfg(test)]
#[path = "tests/raw_page_tests.rs"]
mod tests;
