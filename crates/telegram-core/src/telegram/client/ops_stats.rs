//! get_channel_stats operation (work-order A5).
//!
//! Unit of `client` (LM-2).

use super::TelegramClient;
use super::raw_page::raw_peer_id;
use super::raw_pager::RawHistoryPager;
use super::resolve::{peer_to_ref, validate_channel_identifier};
use super::stream_source::{JoinedChannel, stream_rpc_error};
use crate::error::Error;
use crate::telegram::albums::collapse_albums;
use crate::telegram::converters::{convert_raw_message, timestamp_from_raw};
use crate::telegram::timeout::with_timeout;
use crate::telegram::types::ChannelId;
use crate::telegram::types::stats::{ChannelStats, compute_stats};
use grammers_client::peer::Peer;
use grammers_session::types::{PeerId, PeerRef};

impl TelegramClient {
    pub(super) async fn get_channel_stats_impl(
        &self,
        channel_ref: &str,
        days_back: u32,
    ) -> Result<ChannelStats, Error> {
        validate_channel_identifier(channel_ref)?;
        if days_back == 0 {
            return Err(Error::InvalidInput(
                "days_back must be greater than 0".to_string(),
            ));
        }

        let window_to = crate::UtcTimestamp::now();
        let peer = self.resolve_peer(channel_ref).await?;
        let peer_ref = peer_to_ref(&peer).await?;
        self.stats_from_peer(&peer, peer_ref, days_back, window_to, None, channel_ref)
            .await
    }

    pub(crate) async fn read_joined_channel_stats(
        &self,
        joined: &JoinedChannel,
        days_back: u32,
    ) -> Result<ChannelStats, Error> {
        let identity = joined.identity()?;
        self.stats_from_peer(
            &joined.peer,
            joined.reference,
            days_back,
            crate::UtcTimestamp::now(),
            Some(joined.reference.id),
            &identity.id.to_string(),
        )
        .await
    }

    async fn stats_from_peer(
        &self,
        peer: &Peer,
        peer_ref: PeerRef,
        days_back: u32,
        window_to: crate::UtcTimestamp,
        require_peer: Option<PeerId>,
        channel_ref: &str,
    ) -> Result<ChannelStats, Error> {
        let cutoff = window_to - jiff::SignedDuration::from_hours(24 * i64::from(days_back));

        let (messages, scanned, oldest, complete) =
            with_timeout("iter_messages", self.timeouts.history_secs, async {
                let mut messages = Vec::new();
                let mut scanned = 0u32;
                let mut oldest: Option<crate::UtcTimestamp> = None;
                let mut complete = true;
                // Raw GetHistory pager instead of grammers' iter_messages:
                // same request, but it keeps the response envelope, which is
                // what lets the envelope-less converter be deleted entirely.
                #[cfg(not(any(test, feature = "test-support")))]
                let mut pager = RawHistoryPager::new(&self.client, peer_ref);
                #[cfg(any(test, feature = "test-support"))]
                let mut pager = RawHistoryPager::fixture_history(self, peer_ref);
                while let Some((raw_msg, entities)) = pager.next().await.map_err(|e| {
                    if require_peer.is_some() {
                        stream_rpc_error(e)
                    } else {
                        Error::TelegramApi(format!("Failed to iterate messages: {}", e))
                    }
                })? {
                    if let Some(expected) = require_peer
                        && raw_peer_id(&raw_msg)
                            .is_some_and(|raw| PeerId::from(raw.clone()) != expected)
                    {
                        return Err(Error::UnsupportedPeer);
                    }
                    if timestamp_from_raw(&raw_msg).is_none_or(|t| t < cutoff) {
                        break; // reached the window edge: sweep is complete
                    }
                    if scanned >= ChannelStats::MAX_MESSAGES_SCANNED {
                        complete = false; // cap hit with in-window messages left
                        break;
                    }
                    scanned += 1;
                    if let Some(t) = timestamp_from_raw(&raw_msg) {
                        oldest = Some(oldest.map_or(t, |o: crate::UtcTimestamp| o.min(t)));
                    }
                    if let Some(converted) = convert_raw_message(&raw_msg, peer, &entities) {
                        messages.push(converted);
                    }
                }
                Ok((messages, scanned, oldest, complete))
            })
            .await?;

        let channel_id = peer
            .id()
            .bare_id()
            .and_then(|id| ChannelId::new(id).ok())
            .ok_or_else(|| {
                Error::TelegramApi(format!("Failed to read channel id for {}", channel_ref))
            })?;
        let posts = collapse_albums(messages);
        // Incomplete sweep: the sample covers only what was scanned.
        let window_from = if complete {
            cutoff
        } else {
            oldest.unwrap_or(cutoff)
        };

        tracing::info!(
            channel_ref = %channel_ref,
            scanned,
            posts = posts.len(),
            complete,
            "Channel stats sweep completed"
        );
        Ok(compute_stats(
            channel_id,
            &posts,
            scanned,
            window_from,
            window_to,
            complete,
        ))
    }
}
