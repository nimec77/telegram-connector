//! ID-bounded history traversal with one deadline for resolution and pagination.

use super::TelegramClient;
use super::raw_page::raw_peer_id;
use super::raw_pager::{HistoryFetch, RawHistoryPager};
use super::stream_source::{JoinedChannelKind, stream_rpc_error};
use crate::error::Error;
use crate::operations::{BoundedOperation, HistoryIdBounds, HistoryIdPage};
use crate::telegram::converters::convert_raw_message;
use crate::telegram::types::ChannelId;
use grammers_client::peer::Peer;
use grammers_session::types::PeerId;

impl TelegramClient {
    pub(crate) async fn read_history_id_range(
        &self,
        channel_id: ChannelId,
        bounds: HistoryIdBounds,
        timing: &BoundedOperation<'_>,
    ) -> Result<HistoryIdPage, Error> {
        let channel = timing
            .run(self.joined_channel_peers(&[channel_id], JoinedChannelKind::BroadcastOrSupergroup))
            .await?
            .remove(&channel_id)
            .ok_or_else(|| Error::ChannelNotFound(channel_id.to_string()))?;
        let pager = RawHistoryPager::for_id_range(
            self.client.clone(),
            channel.reference.into(),
            bounds.upper_offset,
        );
        collect_history_range(pager, &channel.peer, bounds, timing).await
    }
}

pub(super) async fn collect_history_range(
    mut pager: RawHistoryPager<impl HistoryFetch>,
    peer: &Peer,
    bounds: HistoryIdBounds,
    timing: &BoundedOperation<'_>,
) -> Result<HistoryIdPage, Error> {
    let mut page = HistoryIdPage::default();
    let mut upper_offset = bounds.upper_offset;
    let has_more = loop {
        let next = timing
            .run(async { pager.next().await.map_err(stream_rpc_error) })
            .await;
        let (raw, entities) = match next {
            Ok(Some(row)) => row,
            Ok(None) => break false,
            Err(Error::Timeout { .. }) if !page.messages.is_empty() => {
                page.partial = true;
                page.timed_out = true;
                break true;
            }
            Err(error) => return Err(error),
        };

        let id = raw.id();
        if id <= bounds.after_id {
            break false;
        }
        if id > bounds.through_id || (upper_offset != 0 && id >= upper_offset) {
            continue;
        }
        if !raw_peer_id(&raw).is_some_and(|source| PeerId::from(source.clone()) == peer.id()) {
            continue;
        }
        if let Some(message) = convert_raw_message(&raw, peer, &entities) {
            if page.messages.len() == bounds.limit {
                break true;
            }
            page.messages.push(message);
            upper_offset = id;
            if id - 1 == bounds.after_id {
                break false;
            }
        }
    };
    if has_more {
        page.next_before_id = page.messages.last().map(|row| row.id);
    }
    Ok(page)
}
