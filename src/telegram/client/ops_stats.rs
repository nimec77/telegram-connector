//! get_channel_stats operation (work-order A5).
//!
//! Unit of `client` (LM-2).

use super::raw_pager::RawHistoryPager;
use super::*;
use crate::telegram::albums::collapse_albums;
use crate::telegram::types::stats::{ChannelStats, compute_stats};

impl TelegramClient {
    pub(super) async fn get_channel_stats_impl(
        &self,
        channel_ref: &str,
        days_back: u32,
    ) -> Result<ChannelStats, Error> {
        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }
        if days_back == 0 {
            return Err(Error::InvalidInput(
                "days_back must be greater than 0".to_string(),
            ));
        }

        let window_to = chrono::Utc::now();
        let cutoff = window_to - chrono::Duration::days(days_back as i64);

        let peer = self.resolve_peer(channel_ref).await?;
        let peer_ref = peer_to_ref(&peer).await?;

        let (messages, scanned, oldest, complete) =
            with_timeout("iter_messages", self.timeouts.history_secs, async {
                let mut messages = Vec::new();
                let mut scanned = 0u32;
                let mut oldest: Option<chrono::DateTime<chrono::Utc>> = None;
                let mut complete = true;
                // Raw GetHistory pager instead of grammers' iter_messages:
                // same request, but it keeps the response envelope, which is
                // what lets the envelope-less converter be deleted entirely.
                let mut pager = RawHistoryPager::new(&self.client, peer_ref);
                while let Some((raw_msg, entities)) = pager
                    .next()
                    .await
                    .map_err(|e| Error::TelegramApi(format!("Failed to iterate messages: {}", e)))?
                {
                    if timestamp_from_raw(&raw_msg).is_none_or(|t| t < cutoff) {
                        break; // reached the window edge: sweep is complete
                    }
                    if scanned >= ChannelStats::MAX_MESSAGES_SCANNED {
                        complete = false; // cap hit with in-window messages left
                        break;
                    }
                    scanned += 1;
                    if let Some(t) = timestamp_from_raw(&raw_msg) {
                        oldest =
                            Some(oldest.map_or(t, |o: chrono::DateTime<chrono::Utc>| o.min(t)));
                    }
                    if let Some(converted) = convert_raw_message(&raw_msg, &peer, &entities) {
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
