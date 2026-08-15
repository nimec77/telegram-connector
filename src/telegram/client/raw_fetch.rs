//! Raw-TL by-id message fetch (`getMessages`) preserving the response
//! envelope, so converters can enrich from the same call's chats/users.

use super::raw_page::{raw_peer_id, unpack_page};
use super::raw_pager::PAGE_LIMIT;
use crate::telegram::envelope::EntityLookup;
use grammers_client::Client;
use grammers_client::tl;
use grammers_mtsender::InvocationError;
use grammers_session::types::{PeerId, PeerKind, PeerRef};
use std::collections::HashMap;
use std::sync::Arc;

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

#[cfg(test)]
#[path = "tests/raw_fetch_tests.rs"]
mod tests;
