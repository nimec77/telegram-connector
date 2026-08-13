//! Raw-TL history/search pagers.
//!
//! Replicates the pagination of grammers' `MessageIter` / `SearchIter` /
//! `GlobalSearchIter` (pinned rev, `client/messages.rs`) while keeping the
//! response envelope's `chats`+`users`, which the high-level iterators
//! discard behind a crate-private `PeerMap`. Same requests, same order, same
//! stop conditions — zero additional network calls; the envelope feeds
//! forward attribution (see `telegram/envelope.rs`).

use crate::telegram::envelope::EntityLookup;
use chrono::{DateTime, Utc};
use grammers_client::Client;
use grammers_client::tl;
use grammers_mtsender::InvocationError;
use grammers_session::types::{PeerId, PeerKind, PeerRef};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;

/// grammers' MAX_LIMIT: server pages cap at 100 messages.
const PAGE_LIMIT: i32 = 100;

/// One decoded page: raw messages plus the envelope entities they came with.
struct RawPage {
    messages: Vec<tl::enums::Message>,
    chats: Vec<tl::enums::Chat>,
    users: Vec<tl::enums::User>,
    next_rate: Option<i32>,
    last_chunk: bool,
}

/// Raw date of a message (0 for Empty — mirrors grammers `date_timestamp`).
fn raw_date(raw: &tl::enums::Message) -> i32 {
    match raw {
        tl::enums::Message::Message(m) => m.date,
        tl::enums::Message::Service(m) => m.date,
        tl::enums::Message::Empty(_) => 0,
    }
}

/// Raw peer the message lives in (mirrors grammers `utils::peer_from_message`).
fn raw_peer_id(raw: &tl::enums::Message) -> Option<&tl::enums::Peer> {
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
fn unpack_page(res: tl::enums::messages::Messages, limit: i32) -> RawPage {
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

fn history_request(
    peer: tl::enums::InputPeer,
    offset_id: i32,
) -> tl::functions::messages::GetHistory {
    tl::functions::messages::GetHistory {
        peer,
        offset_id,
        offset_date: 0,
        add_offset: 0,
        limit: PAGE_LIMIT,
        max_id: 0,
        min_id: 0,
        hash: 0,
    }
}

fn advance_history_offsets(request: &mut tl::functions::messages::GetHistory, page: &RawPage) {
    if let Some(last) = page.messages.last() {
        request.offset_id = last.id();
        request.offset_date = raw_date(last);
    }
}

fn advance_search_offsets(request: &mut tl::functions::messages::Search, page: &RawPage) {
    if let Some(last) = page.messages.last() {
        request.offset_id = last.id();
        request.max_date = raw_date(last);
    }
}

/// Map a time window onto `messages.SearchGlobal`'s `min_date`/`max_date`.
///
/// The TL schema types both as `int` (i32 unix seconds) and treats `0` as
/// "unbounded". Out-of-range instants clamp rather than error: a degraded
/// bound costs a slower search, a rejected one costs the caller their result.
/// The client-side window filter in `ops_search` stays in place either way.
fn window_bounds(from: DateTime<Utc>, to: Option<DateTime<Utc>>) -> (i32, i32) {
    let clamp = |ts: i64| ts.clamp(0, i32::MAX as i64) as i32;
    (
        clamp(from.timestamp()),
        to.map_or(0, |t| clamp(t.timestamp())),
    )
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
fn input_peer_for_message(raw: &tl::enums::Message, page: &RawPage) -> tl::enums::InputPeer {
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
fn fill_buffer(buffer: &mut VecDeque<(tl::enums::Message, Arc<EntityLookup>)>, page: RawPage) {
    let entities = Arc::new(EntityLookup::from_envelope(&page.chats, &page.users));
    buffer.extend(
        page.messages
            .into_iter()
            .map(|message| (message, Arc::clone(&entities))),
    );
}

/// Which RPC a peer's messages must be fetched with. Channel-namespace peers
/// require `channels.GetMessages`; `messages.GetMessages` resolves bare ids
/// across the account's dialogs and would return the wrong chat's message.
/// Mirrors grammers `client/messages.rs::get_messages_by_id` in the pinned rev.
pub(super) enum GetMessagesRequest {
    Channel(tl::functions::channels::GetMessages),
    Plain(tl::functions::messages::GetMessages),
}

fn get_messages_request(peer: PeerRef, ids: &[i32]) -> GetMessagesRequest {
    let id = ids
        .iter()
        .map(|&id| tl::enums::InputMessage::Id(tl::types::InputMessageId { id }))
        .collect();
    if peer.id.kind() == PeerKind::Channel {
        GetMessagesRequest::Channel(tl::functions::channels::GetMessages {
            channel: peer.into(),
            id,
        })
    } else {
        GetMessagesRequest::Plain(tl::functions::messages::GetMessages { id })
    }
}

/// Key a response's messages by id, dropping any that belong to a different
/// peer (grammers applies the same guard). `MessageEmpty` placeholders are
/// kept: the caller distinguishes "deleted" from "wrong peer", and both map
/// to missing anyway (work-order B1 guard).
fn index_messages(
    messages: Vec<tl::enums::Message>,
    peer: PeerRef,
) -> HashMap<i32, tl::enums::Message> {
    messages
        .into_iter()
        .filter(|raw| raw_peer_id(raw).is_none_or(|p| PeerId::from(p.clone()) == peer.id))
        .map(|raw| (raw.id(), raw))
        .collect()
}

/// Raw `getMessages` preserving the response envelope (get_message_by_link /
/// get_messages_batch path).
///
/// Same request and same RPC count as grammers' `get_messages_by_id`, but it
/// keeps the `chats`+`users` arrays that forward attribution reads instead of
/// collapsing them into a crate-private `PeerMap` (see `telegram/envelope.rs`).
/// Zero additional network calls.
pub(super) async fn fetch_messages_by_id(
    client: &Client,
    peer: PeerRef,
    ids: &[i32],
) -> Result<(HashMap<i32, tl::enums::Message>, Arc<EntityLookup>), InvocationError> {
    let response = match get_messages_request(peer, ids) {
        GetMessagesRequest::Channel(request) => client.invoke(&request).await?,
        GetMessagesRequest::Plain(request) => client.invoke(&request).await?,
    };
    // `limit` only drives the pager's last-chunk rule, which getMessages has
    // no use for; PAGE_LIMIT keeps the single decode path.
    let page = unpack_page(response, PAGE_LIMIT);
    let entities = Arc::new(EntityLookup::from_envelope(&page.chats, &page.users));
    Ok((index_messages(page.messages, peer), entities))
}

/// Raw `messages.GetHistory` pager (get_recent_messages path).
pub(super) struct RawHistoryPager {
    client: Client,
    request: tl::functions::messages::GetHistory,
    buffer: VecDeque<(tl::enums::Message, Arc<EntityLookup>)>,
    last_chunk: bool,
}

impl RawHistoryPager {
    pub(super) fn new(client: &Client, peer: PeerRef) -> Self {
        Self {
            client: client.clone(),
            request: history_request(peer.into(), 0),
            buffer: VecDeque::new(),
            last_chunk: false,
        }
    }

    pub(super) fn offset_id(mut self, offset: i32) -> Self {
        self.request.offset_id = offset;
        self
    }

    pub(super) async fn next(
        &mut self,
    ) -> Result<Option<(tl::enums::Message, Arc<EntityLookup>)>, InvocationError> {
        if let Some(item) = self.buffer.pop_front() {
            return Ok(Some(item));
        }
        if self.last_chunk {
            return Ok(None);
        }
        let page = unpack_page(self.client.invoke(&self.request).await?, self.request.limit);
        self.last_chunk = page.last_chunk;
        advance_history_offsets(&mut self.request, &page);
        fill_buffer(&mut self.buffer, page);
        Ok(self.buffer.pop_front())
    }
}

/// Raw `messages.Search` pager (search_messages single-channel path).
pub(super) struct RawChannelSearchPager {
    client: Client,
    request: tl::functions::messages::Search,
    buffer: VecDeque<(tl::enums::Message, Arc<EntityLookup>)>,
    last_chunk: bool,
}

impl RawChannelSearchPager {
    pub(super) fn new(client: &Client, peer: PeerRef) -> Self {
        Self {
            client: client.clone(),
            request: tl::functions::messages::Search {
                peer: peer.into(),
                q: String::new(),
                from_id: None,
                saved_peer_id: None,
                saved_reaction: None,
                top_msg_id: None,
                filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
                min_date: 0,
                max_date: 0,
                offset_id: 0,
                add_offset: 0,
                limit: PAGE_LIMIT,
                max_id: 0,
                min_id: 0,
                hash: 0,
            },
            buffer: VecDeque::new(),
            last_chunk: false,
        }
    }

    pub(super) fn query(mut self, query: &str) -> Self {
        self.request.q = query.to_string();
        self
    }

    pub(super) fn filter(mut self, filter: tl::enums::MessagesFilter) -> Self {
        self.request.filter = filter;
        self
    }

    pub(super) fn offset_id(mut self, offset: i32) -> Self {
        self.request.offset_id = offset;
        self
    }

    pub(super) async fn next(
        &mut self,
    ) -> Result<Option<(tl::enums::Message, Arc<EntityLookup>)>, InvocationError> {
        if let Some(item) = self.buffer.pop_front() {
            return Ok(Some(item));
        }
        if self.last_chunk {
            return Ok(None);
        }
        let page = unpack_page(self.client.invoke(&self.request).await?, self.request.limit);
        self.last_chunk = page.last_chunk;
        advance_search_offsets(&mut self.request, &page);
        fill_buffer(&mut self.buffer, page);
        Ok(self.buffer.pop_front())
    }
}

/// Raw `messages.SearchGlobal` pager (search_messages all-channels path).
///
/// Yields each message with its envelope entities and its own chat as a
/// high-level `Peer` (built from the same envelope), which the ops layer
/// needs for identity and link building.
pub(super) struct RawGlobalSearchPager {
    client: Client,
    request: tl::functions::messages::SearchGlobal,
    buffer: VecDeque<(
        tl::enums::Message,
        Arc<EntityLookup>,
        Option<grammers_client::peer::Peer>,
    )>,
    last_chunk: bool,
}

impl RawGlobalSearchPager {
    pub(super) fn new(client: &Client) -> Self {
        Self {
            client: client.clone(),
            request: tl::functions::messages::SearchGlobal {
                folder_id: None,
                q: String::new(),
                filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
                min_date: 0,
                max_date: 0,
                offset_rate: 0,
                offset_peer: tl::enums::InputPeer::Empty,
                offset_id: 0,
                limit: PAGE_LIMIT,
                broadcasts_only: false,
                groups_only: false,
                users_only: false,
                community: None,
            },
            buffer: VecDeque::new(),
            last_chunk: false,
        }
    }

    pub(super) fn query(mut self, query: &str) -> Self {
        self.request.q = query.to_string();
        self
    }

    pub(super) fn filter(mut self, filter: tl::enums::MessagesFilter) -> Self {
        self.request.filter = filter;
        self
    }

    /// Bound the search server-side. Without this the pager walks the entire
    /// global index backwards discarding out-of-window results — measured at
    /// 44.86 s for a rare media filter over a 24 h window.
    pub(super) fn window(mut self, from: DateTime<Utc>, to: Option<DateTime<Utc>>) -> Self {
        let (min_date, max_date) = window_bounds(from, to);
        self.request.min_date = min_date;
        self.request.max_date = max_date;
        self
    }

    pub(super) async fn next(
        &mut self,
    ) -> Result<
        Option<(
            tl::enums::Message,
            Arc<EntityLookup>,
            Option<grammers_client::peer::Peer>,
        )>,
        InvocationError,
    > {
        if let Some(item) = self.buffer.pop_front() {
            return Ok(Some(item));
        }
        if self.last_chunk {
            return Ok(None);
        }
        // Order matters: advance offsets while `page` is whole, then
        // destructure — the message vec is consumed by the buffer fill while
        // chats/users are still needed for per-message chat peers.
        let page = unpack_page(self.client.invoke(&self.request).await?, self.request.limit);
        self.last_chunk = page.last_chunk;
        if let Some(last) = page.messages.last() {
            self.request.offset_rate = page.next_rate.unwrap_or(0);
            self.request.offset_id = last.id();
            self.request.offset_peer = input_peer_for_message(last, &page);
        }
        let RawPage {
            messages,
            chats,
            users,
            ..
        } = page;
        let entities = Arc::new(EntityLookup::from_envelope(&chats, &users));
        for message in messages {
            let chat = chat_peer_for_message(&self.client, &message, &chats, &users);
            self.buffer
                .push_back((message, Arc::clone(&entities), chat));
        }
        Ok(self.buffer.pop_front())
    }
}

/// The message's own chat as a high-level Peer, built from the envelope
/// (`Peer::from_raw` / `User::from_raw` are public; the crate-private
/// `PeerMap` is not — this is the raw-TL replacement for `msg.peer()`).
fn chat_peer_for_message(
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
mod tests {
    use super::*;
    use crate::test_helpers::raw_tl_channel;
    use chrono::DateTime;
    use grammers_client::tl;
    use grammers_session::types::PeerAuth;

    fn raw_msg(id: i32, date: i32, channel_id: i64) -> tl::enums::Message {
        tl::enums::Message::Service(tl::types::MessageService {
            out: false,
            mentioned: false,
            media_unread: false,
            reactions_are_possible: false,
            silent: false,
            post: true,
            legacy: false,
            id,
            from_id: None,
            peer_id: tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id }),
            saved_peer_id: None,
            reply_to: None,
            date,
            action: tl::enums::MessageAction::Empty,
            reactions: None,
            ttl_period: None,
        })
    }

    fn slice(
        messages: Vec<tl::enums::Message>,
        next_rate: Option<i32>,
    ) -> tl::enums::messages::Messages {
        tl::enums::messages::Messages::Slice(tl::types::messages::MessagesSlice {
            inexact: false,
            count: 1000,
            next_rate,
            offset_id_offset: None,
            search_flood: None,
            messages,
            topics: vec![],
            chats: vec![tl::enums::Chat::Channel(raw_tl_channel(11, "Канал", None))],
            users: vec![],
        })
    }

    #[test]
    fn unpack_slice_computes_last_chunk_and_keeps_envelope() {
        let page = unpack_page(slice(vec![raw_msg(500, 1_700_000_000, 11)], Some(7)), 100);
        assert!(
            !page.last_chunk,
            "id 500 > limit 100 → more pages may exist"
        );
        assert_eq!(page.next_rate, Some(7));
        assert_eq!(page.messages.len(), 1);
        assert_eq!(page.chats.len(), 1);

        let tail = unpack_page(slice(vec![raw_msg(90, 1_700_000_000, 11)], None), 100);
        assert!(
            tail.last_chunk,
            "highest id <= limit → nothing older can exist"
        );

        let empty = unpack_page(slice(vec![], None), 100);
        assert!(empty.last_chunk);
    }

    #[test]
    fn unpack_messages_variant_is_always_last_chunk() {
        let res = tl::enums::messages::Messages::Messages(tl::types::messages::Messages {
            messages: vec![raw_msg(3, 1_700_000_000, 11)],
            topics: vec![],
            chats: vec![],
            users: vec![],
        });
        assert!(unpack_page(res, 100).last_chunk);
    }

    #[test]
    fn history_offsets_advance_from_last_message() {
        let mut request = history_request(tl::enums::InputPeer::Empty, 0);
        let page = unpack_page(
            slice(
                vec![
                    raw_msg(500, 1_700_000_500, 11),
                    raw_msg(499, 1_700_000_400, 11),
                ],
                None,
            ),
            100,
        );
        advance_history_offsets(&mut request, &page);
        assert_eq!(request.offset_id, 499);
        assert_eq!(request.offset_date, 1_700_000_400);
    }

    #[test]
    fn global_offset_peer_resolves_access_hash_from_envelope() {
        let page = unpack_page(slice(vec![raw_msg(500, 1_700_000_000, 11)], Some(9)), 100);
        let input = input_peer_for_message(&page.messages[0], &page);
        match input {
            tl::enums::InputPeer::Channel(c) => assert_eq!(c.channel_id, 11),
            other => panic!("expected InputPeer::Channel, got {other:?}"),
        }

        // Envelope miss → Empty, mirroring grammers' unwrap_or fallback.
        let missing = raw_msg(501, 1_700_000_000, 999);
        assert!(matches!(
            input_peer_for_message(&missing, &page),
            tl::enums::InputPeer::Empty
        ));
    }

    #[test]
    fn global_offset_peer_resolves_community_and_forbidden_chats() {
        // Communities and Forbidden channels are channel-namespace too; a
        // page ending in one must still produce a real offset peer, not
        // Empty (would drift SearchGlobal pagination).
        let community_page = RawPage {
            messages: vec![raw_msg(500, 1_700_000_000, 21)],
            chats: vec![tl::enums::Chat::Community(
                crate::test_helpers::raw_tl_community(21, "Группа"),
            )],
            users: vec![],
            next_rate: None,
            last_chunk: false,
        };
        match input_peer_for_message(&community_page.messages[0], &community_page) {
            tl::enums::InputPeer::Channel(c) => assert_eq!(c.channel_id, 21),
            other => panic!("expected InputPeer::Channel for community, got {other:?}"),
        }

        let forbidden_page = RawPage {
            messages: vec![raw_msg(501, 1_700_000_000, 22)],
            chats: vec![tl::enums::Chat::ChannelForbidden(
                tl::types::ChannelForbidden {
                    broadcast: true,
                    megagroup: false,
                    monoforum: false,
                    id: 22,
                    access_hash: 77,
                    title: "Закрытый".to_string(),
                    until_date: None,
                },
            )],
            users: vec![],
            next_rate: None,
            last_chunk: false,
        };
        match input_peer_for_message(&forbidden_page.messages[0], &forbidden_page) {
            tl::enums::InputPeer::Channel(c) => {
                assert_eq!(c.channel_id, 22);
                assert_eq!(c.access_hash, 77, "forbidden variant's hash is carried");
            }
            other => panic!("expected InputPeer::Channel for forbidden, got {other:?}"),
        }
    }

    #[test]
    fn search_offsets_advance_from_last_message() {
        let mut request = tl::functions::messages::Search {
            peer: tl::enums::InputPeer::Empty,
            q: String::new(),
            from_id: None,
            saved_peer_id: None,
            saved_reaction: None,
            top_msg_id: None,
            filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
            min_date: 0,
            max_date: 0,
            offset_id: 0,
            add_offset: 0,
            limit: PAGE_LIMIT,
            max_id: 0,
            min_id: 0,
            hash: 0,
        };
        let page = unpack_page(
            slice(
                vec![
                    raw_msg(500, 1_700_000_500, 11),
                    raw_msg(499, 1_700_000_400, 11),
                ],
                None,
            ),
            100,
        );
        advance_search_offsets(&mut request, &page);
        assert_eq!(request.offset_id, 499);
        assert_eq!(request.max_date, 1_700_000_400);
    }

    #[test]
    fn window_bounds_maps_both_ends() {
        let from = DateTime::from_timestamp(1_700_000_000, 0).expect("valid ts");
        let to = DateTime::from_timestamp(1_700_086_400, 0).expect("valid ts");
        let (min_date, max_date) = window_bounds(from, Some(to));
        assert_eq!(min_date, 1_700_000_000);
        assert_eq!(max_date, 1_700_086_400);
    }

    #[test]
    fn window_bounds_open_upper_end_is_unbounded_sentinel() {
        let from = DateTime::from_timestamp(1_700_000_000, 0).expect("valid ts");
        let (min_date, max_date) = window_bounds(from, None);
        assert_eq!(min_date, 1_700_000_000);
        // 0 is the protocol's "no upper bound", not "the epoch".
        assert_eq!(max_date, 0);
    }

    #[test]
    fn window_bounds_clamps_pre_epoch_lower_end_to_unbounded() {
        let from = DateTime::from_timestamp(-86_400, 0).expect("valid ts");
        let (min_date, max_date) = window_bounds(from, None);
        // A degraded bound costs latency; a rejected search costs the caller
        // their result. Degrade.
        assert_eq!(min_date, 0);
        assert_eq!(max_date, 0);
    }

    #[test]
    fn window_bounds_clamps_beyond_i32_range() {
        // Past 2038: saturates instead of wrapping into a negative i32, which
        // would silently widen the window to everything.
        let from = DateTime::from_timestamp(i32::MAX as i64 + 1_000, 0).expect("valid ts");
        let (min_date, _) = window_bounds(from, None);
        assert_eq!(min_date, i32::MAX);
    }

    fn channel_ref(id: i64) -> PeerRef {
        PeerRef {
            id: PeerId::channel_unchecked(id),
            auth: PeerAuth::from_hash(0),
        }
    }

    fn chat_ref(id: i64) -> PeerRef {
        PeerRef {
            id: PeerId::chat_unchecked(id),
            auth: PeerAuth::default(),
        }
    }

    #[test]
    fn channel_peer_routes_to_channels_get_messages() {
        let request = get_messages_request(channel_ref(1144180066), &[610121, 610122]);

        match request {
            GetMessagesRequest::Channel(r) => assert_eq!(r.id.len(), 2),
            GetMessagesRequest::Plain(_) => panic!("channel peer must use channels.GetMessages"),
        }
    }

    #[test]
    fn non_channel_peer_routes_to_messages_get_messages() {
        let request = get_messages_request(chat_ref(521440428), &[7]);

        match request {
            GetMessagesRequest::Plain(r) => assert_eq!(r.id.len(), 1),
            GetMessagesRequest::Channel(_) => panic!("chat peer must use messages.GetMessages"),
        }
    }

    #[test]
    fn index_messages_keys_by_id_regardless_of_response_order() {
        let messages = vec![
            raw_msg(610122, 1_700_000_100, 1144180066),
            raw_msg(610121, 1_700_000_000, 1144180066),
        ];

        let indexed = index_messages(messages, channel_ref(1144180066));

        assert_eq!(indexed.len(), 2);
        assert_eq!(indexed[&610121].id(), 610121);
        assert_eq!(indexed[&610122].id(), 610122);
    }

    #[test]
    fn index_messages_drops_a_message_from_a_different_peer() {
        // messages.GetMessages resolves bare ids across every dialog, so a
        // response can name a chat we did not ask about.
        let messages = vec![
            raw_msg(610121, 1_700_000_000, 1144180066),
            raw_msg(610122, 1_700_000_100, 999_999),
        ];

        let indexed = index_messages(messages, channel_ref(1144180066));

        assert_eq!(indexed.len(), 1);
        assert!(indexed.contains_key(&610121));
        assert!(!indexed.contains_key(&610122));
    }

    #[test]
    fn index_messages_keeps_empty_placeholders_for_the_caller_to_classify() {
        let messages = vec![tl::enums::Message::Empty(tl::types::MessageEmpty {
            id: 609784,
            peer_id: None,
        })];

        let indexed = index_messages(messages, channel_ref(1144180066));

        assert!(indexed.contains_key(&609784));
    }

    #[test]
    fn fetch_decode_builds_an_entity_map_from_the_response_envelope() {
        // THE load-bearing test for work order A. The bug was that
        // getMessages responses had their chats/users discarded, leaving
        // forwards ids-only. This asserts the decode keeps them, so a forward
        // source the account does not subscribe to is still attributable.
        let res =
            tl::enums::messages::Messages::ChannelMessages(tl::types::messages::ChannelMessages {
                inexact: false,
                pts: 1,
                count: 1,
                offset_id_offset: None,
                messages: vec![raw_msg(298716, 1_700_000_000, 1912881684)],
                topics: vec![],
                // The forward SOURCE — a channel we never asked about, present
                // only because the envelope names every entity its messages
                // reference.
                chats: vec![tl::enums::Chat::Channel(raw_tl_channel(
                    1783384254,
                    "Pavel Zloi",
                    Some("evilfreelancer"),
                ))],
                users: vec![],
            });

        let page = unpack_page(res, PAGE_LIMIT);
        let entities = EntityLookup::from_envelope(&page.chats, &page.users);

        let source = tl::enums::Peer::Channel(tl::types::PeerChannel {
            channel_id: 1783384254,
        });
        let info = entities
            .get(&source)
            .expect("envelope must name the forward source");
        assert_eq!(info.display_name.as_deref(), Some("Pavel Zloi"));
        assert_eq!(info.username.as_deref(), Some("evilfreelancer"));
    }

    #[test]
    fn unpack_page_treats_not_modified_as_an_empty_final_page() {
        let page = unpack_page(
            tl::enums::messages::Messages::NotModified(tl::types::messages::MessagesNotModified {
                count: 0,
            }),
            PAGE_LIMIT,
        );

        assert!(page.messages.is_empty());
        assert!(page.last_chunk);
    }
}
