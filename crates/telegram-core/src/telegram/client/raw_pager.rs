//! Raw-TL history/search pagers.
//!
//! Replicates the pagination of grammers' `MessageIter` / `SearchIter` /
//! `GlobalSearchIter` (pinned rev, `client/messages.rs`) while keeping the
//! response envelope's `chats`+`users`, which the high-level iterators
//! discard behind a crate-private `PeerMap`. Same requests, same order, same
//! single-peer stop conditions; global slices continue across peer IDs. The
//! envelope feeds forward attribution (see `telegram/envelope.rs`).

use super::raw_page::{
    RawPage, chat_peer_for_message, fill_buffer, input_peer_for_message, unpack_page,
};
use crate::UtcTimestamp;
use crate::telegram::envelope::EntityLookup;
use grammers_client::{Client, tl};
use grammers_mtsender::InvocationError;
use grammers_session::types::PeerRef;
use std::collections::VecDeque;
use std::sync::Arc;

/// grammers' MAX_LIMIT: server pages cap at 100 messages.
pub(super) const PAGE_LIMIT: i32 = 100;

/// Raw date of a message (0 for Empty — mirrors grammers `date_timestamp`).
fn raw_date(raw: &tl::enums::Message) -> i32 {
    match raw {
        tl::enums::Message::Message(m) => m.date,
        tl::enums::Message::Service(m) => m.date,
        tl::enums::Message::Empty(_) => 0,
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

/// Map a time window onto `messages.SearchGlobal`'s `min_date`/`max_date`,
/// widened by one second at each end.
///
/// The TL schema types both as `int` (i32 unix seconds) and treats `0` as
/// "unbounded". Out-of-range instants clamp rather than error: a degraded
/// bound costs a slower search, a rejected one costs the caller their result.
///
/// Neither the TL schema nor grammers documents whether the bounds are
/// inclusive or exclusive, and the ±1 makes the answer stop mattering: the
/// server window is a strict superset of the requested one under *either*
/// semantics, so no message can be dropped server-side that the client-side
/// window checks in `ops_search` would have kept. Without it, an exclusive
/// server silently drops a message posted on the exact second of an explicit
/// `from_date`/`to_date` (the README's own example is that shape), and the
/// retained client-side guard never sees it to recover it. Cost is at most two
/// extra seconds of index — unmeasurable against a sub-second search.
fn window_bounds(from: UtcTimestamp, to: Option<UtcTimestamp>) -> (i32, i32) {
    let clamp = |ts: i64| ts.clamp(0, i32::MAX as i64) as i32;
    (
        clamp(from.unix_seconds().saturating_sub(1)),
        to.map_or(0, |t| clamp(t.unix_seconds().saturating_add(1))),
    )
}

/// Write a time window onto a `SearchGlobal` request.
///
/// A transposition here (`min_date` ← the upper bound) would silently
/// reinstate the unbounded global walk.
fn apply_window(
    request: &mut tl::functions::messages::SearchGlobal,
    from: UtcTimestamp,
    to: Option<UtcTimestamp>,
) {
    let (min_date, max_date) = window_bounds(from, to);
    request.min_date = min_date;
    request.max_date = max_date;
}

/// The network boundary for the production history pager and offline page scripts.
pub(super) trait HistoryFetch {
    fn fetch(
        &mut self,
        request: &tl::functions::messages::GetHistory,
    ) -> impl std::future::Future<Output = Result<tl::enums::messages::Messages, InvocationError>> + Send;
}

impl HistoryFetch for Client {
    async fn fetch(
        &mut self,
        request: &tl::functions::messages::GetHistory,
    ) -> Result<tl::enums::messages::Messages, InvocationError> {
        self.invoke(request).await
    }
}

enum HistoryOffsets {
    IdAndDate,
    IdOnly,
}

/// Raw `messages.GetHistory` pager for recent reads and explicit ID ranges.
pub(super) struct RawHistoryPager<F = Client> {
    fetch: F,
    offsets: HistoryOffsets,
    request: tl::functions::messages::GetHistory,
    buffer: VecDeque<(tl::enums::Message, Arc<EntityLookup>)>,
    last_chunk: bool,
    /// Messages in the page just fetched, taken exactly once by the caller so a
    /// round trip is counted once rather than once per yielded message.
    last_page_size: Option<usize>,
}

impl RawHistoryPager {
    pub(super) fn new(client: &Client, peer: PeerRef) -> Self {
        Self::with_offsets(client.clone(), peer.into(), 0, HistoryOffsets::IdAndDate)
    }
}

impl<F: HistoryFetch> RawHistoryPager<F> {
    fn with_offsets(
        fetch: F,
        peer: tl::enums::InputPeer,
        offset: i32,
        offsets: HistoryOffsets,
    ) -> Self {
        Self {
            fetch,
            offsets,
            request: history_request(peer, offset),
            buffer: VecDeque::new(),
            last_chunk: false,
            last_page_size: None,
        }
    }

    pub(super) fn for_id_range(fetch: F, peer: tl::enums::InputPeer, upper_offset: i32) -> Self {
        Self::with_offsets(fetch, peer, upper_offset, HistoryOffsets::IdOnly)
    }

    pub(super) fn offset_id(mut self, offset: i32) -> Self {
        self.request.offset_id = offset;
        self
    }

    pub(super) fn take_last_page_size(&mut self) -> Option<usize> {
        self.last_page_size.take()
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
        let page = unpack_page(self.fetch.fetch(&self.request).await?);
        self.last_chunk = match self.offsets {
            HistoryOffsets::IdAndDate => {
                advance_history_offsets(&mut self.request, &page);
                page.single_peer_exhausted(self.request.limit)
            }
            HistoryOffsets::IdOnly => {
                // Match recovery by ID alone. A short Telegram slice may still
                // have older rows, even at low IDs.
                let previous = self.request.offset_id;
                let advance = page
                    .messages
                    .last()
                    .map(|last| last.id())
                    .filter(|&id| id > 0 && (previous == 0 || id < previous));
                if let Some(id) = advance {
                    self.request.offset_id = id;
                }
                page.global_exhausted() || advance.is_none()
            }
        };
        // Measured before buffering so the caller counts each raw page once.
        let page_size = page.messages.len();
        fill_buffer(&mut self.buffer, page);
        self.last_page_size = Some(page_size);
        Ok(self.buffer.pop_front())
    }
}

/// Raw `messages.Search` pager (search_messages single-channel path).
pub(super) struct RawChannelSearchPager {
    client: Client,
    request: tl::functions::messages::Search,
    buffer: VecDeque<(tl::enums::Message, Arc<EntityLookup>)>,
    last_chunk: bool,
    /// Messages in the page just fetched, taken exactly once by the caller so a
    /// round trip is counted once rather than once per yielded message.
    last_page_size: Option<usize>,
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
            last_page_size: None,
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

    pub(super) fn take_last_page_size(&mut self) -> Option<usize> {
        self.last_page_size.take()
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
        let page = unpack_page(self.client.invoke(&self.request).await?);
        self.last_chunk = page.single_peer_exhausted(self.request.limit);
        // Measured on the page, not the buffer — see `RawHistoryPager::next`.
        let page_size = page.messages.len();
        advance_search_offsets(&mut self.request, &page);
        fill_buffer(&mut self.buffer, page);
        self.last_page_size = Some(page_size);
        Ok(self.buffer.pop_front())
    }
}

/// The network boundary used by the production pager and offline page scripts.
pub(super) trait GlobalSearchFetch {
    fn fetch(
        &self,
        request: &tl::functions::messages::SearchGlobal,
    ) -> impl std::future::Future<Output = Result<tl::enums::messages::Messages, InvocationError>> + Send;
}

impl GlobalSearchFetch for Client {
    async fn fetch(
        &self,
        request: &tl::functions::messages::SearchGlobal,
    ) -> Result<tl::enums::messages::Messages, InvocationError> {
        self.invoke(request).await
    }
}

/// Raw global-index pager, retaining each result's own peer and envelope.
pub(super) struct RawGlobalSearchPager<F = Client> {
    client: Client,
    fetch: F,
    request: tl::functions::messages::SearchGlobal,
    buffer: VecDeque<(
        tl::enums::Message,
        Arc<EntityLookup>,
        Option<grammers_client::peer::Peer>,
    )>,
    last_chunk: bool,
    /// Messages in the page just fetched, taken exactly once by the caller so a
    /// round trip is counted once rather than once per yielded message.
    last_page_size: Option<usize>,
}

impl RawGlobalSearchPager {
    pub(super) fn new(client: &Client, from: UtcTimestamp, to: Option<UtcTimestamp>) -> Self {
        Self::with_fetch(client, from, to, client.clone())
    }
}

impl<F: GlobalSearchFetch> RawGlobalSearchPager<F> {
    /// The time window is a constructor argument, not an optional builder step:
    /// without server-side bounds the pager walks the entire global index
    /// backwards discarding out-of-window results — measured at 44.86 s for a
    /// rare media filter over a 24 h window. An omitted builder call would
    /// reinstate that silently, so the unbounded request is not constructible.
    fn with_fetch(client: &Client, from: UtcTimestamp, to: Option<UtcTimestamp>, fetch: F) -> Self {
        let mut request = tl::functions::messages::SearchGlobal {
            folder_id: None,
            q: String::new(),
            filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
            // Overwritten by `apply_window` below; the literal cannot call it.
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
        };
        apply_window(&mut request, from, to);
        Self {
            client: client.clone(),
            fetch,
            request,
            buffer: VecDeque::new(),
            last_chunk: false,
            last_page_size: None,
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

    pub(super) fn take_last_page_size(&mut self) -> Option<usize> {
        self.last_page_size.take()
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
        let page = unpack_page(self.fetch.fetch(&self.request).await?);
        self.last_chunk = page.global_exhausted();
        // Measured on the page, not the buffer — see `RawHistoryPager::next`.
        let page_size = page.messages.len();
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
        self.last_page_size = Some(page_size);
        Ok(self.buffer.pop_front())
    }
}

#[cfg(test)]
#[path = "tests/raw_pager_tests.rs"]
mod tests;
