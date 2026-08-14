//! download_message_media operation.
//!
//! Unit of `client` (LM-2).

use super::guard::require_found;
use super::*;

/// Concurrent media downloads in flight within one batch call.
///
/// Deliberately owned by this layer rather than shared with the MCP fan-out
/// constant it currently equals: these are multi-hundred-KB binary transfers,
/// not small JSON round trips, and the two should be tunable apart.
pub(crate) const MEDIA_DOWNLOAD_CONCURRENCY: usize = 4;

impl TelegramClient {
    pub(super) async fn download_message_media_impl(
        &self,
        channel_ref: &str,
        message_id: i32,
        max_dimension: u32,
    ) -> Result<MediaDownload, Error> {
        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }

        let peer = self.resolve_peer(channel_ref).await?;
        let peer_ref = peer_to_ref(&peer).await?;

        let messages = with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
            self.client
                .get_messages_by_id(peer_ref, &[message_id])
                .await
                .map_err(|e| {
                    tracing::error!(
                        channel_ref = %channel_ref,
                        message_id,
                        error = %e,
                        "Failed to get message for media download"
                    );
                    Error::TelegramApi(format!("Failed to get message: {}", e))
                })
        })
        .await?;

        let msg = require_found(
            messages.into_iter().next().flatten(),
            channel_ref,
            message_id,
        )?;

        self.media_download_from_message(msg, channel_ref, message_id, max_dimension)
            .await
    }

    pub(super) async fn download_messages_media_impl(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
        max_dimension: u32,
    ) -> Result<Vec<MediaFetchOutcome>, Error> {
        use futures::StreamExt as _;

        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }

        // One resolve and one fetch for the whole batch — the point of this
        // method. A numeric channel_ref costs a full dialog walk, so doing it
        // per id is what made the naive loop slow.
        let peer = self.resolve_peer(channel_ref).await?;
        let peer_ref = peer_to_ref(&peer).await?;

        let messages = with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
            self.client
                .get_messages_by_id(peer_ref, message_ids)
                .await
                .map_err(|e| {
                    tracing::error!(
                        channel_ref = %channel_ref,
                        requested = message_ids.len(),
                        error = %e,
                        "Failed to get messages for batch media download"
                    );
                    Error::TelegramApi(format!("Failed to get messages: {}", e))
                })
        })
        .await?;

        // grammers returns exactly one slot per requested id, in request order
        // (pinned rev 9fef0ba, client/messages.rs:1145 collects
        // `message_ids.iter().map(|id| map.remove(id))`), so the lengths match
        // by construction. A None slot is a deleted or inaccessible message.
        debug_assert_eq!(
            messages.len(),
            message_ids.len(),
            "grammers must return one slot per requested id"
        );
        let slots: Vec<(i32, Option<_>)> = message_ids.iter().copied().zip(messages).collect();

        let outcomes =
            futures::stream::iter(slots.into_iter().map(|(message_id, slot)| async move {
                // require_found also rejects the MessageEmpty placeholder, so
                // both flavours of "deleted" collapse to NotFound here exactly
                // as they do on the single-message path.
                let result = match require_found(slot, channel_ref, message_id) {
                    Err(_) => Err(MediaFetchError::NotFound),
                    Ok(msg) => self
                        .media_download_from_message(msg, channel_ref, message_id, max_dimension)
                        .await
                        .map_err(|e| match e {
                            Error::NoVisualMedia { media_type } => {
                                MediaFetchError::NoVisualMedia { media_type }
                            }
                            other => MediaFetchError::Failed(other),
                        }),
                };
                MediaFetchOutcome { message_id, result }
            }))
            .buffered(MEDIA_DOWNLOAD_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;

        tracing::info!(
            channel_ref = %channel_ref,
            requested = outcomes.len(),
            succeeded = outcomes.iter().filter(|o| o.result.is_ok()).count(),
            "Batch media download complete"
        );

        Ok(outcomes)
    }

    /// Select and download a message's visual media: the photo itself, or the
    /// server-side thumbnail for video-like media.
    ///
    /// Shared by the single-message and batch entry points so the
    /// photo-vs-thumbnail rules, the size-variant selection and the
    /// `max_download_bytes` enforcement exist in exactly one place. Takes an
    /// already-fetched message, so it performs no resolve and no fetch.
    pub(super) async fn media_download_from_message(
        &self,
        msg: grammers_client::message::Message,
        channel_ref: &str,
        message_id: i32,
        max_dimension: u32,
    ) -> Result<MediaDownload, Error> {
        // Hard cap on a single download (`[telegram] max_download_bytes`, AD-6).
        // Hoisted to a Copy local so the streaming closure captures the value.
        let max_download_bytes = self.max_download_bytes;

        let media = msg.media().ok_or_else(|| Error::NoVisualMedia {
            media_type: "none".to_string(),
        })?;
        let media_type = convert_media_to_type(&media);

        // Photos are downloaded directly; video-like media contributes only its
        // server-side thumbnail (the spec forbids full video downloads).
        let (thumbs, is_thumbnail) = match &media {
            Media::Photo(photo) => (photo.thumbs(), false),
            Media::Document(doc)
                if matches!(
                    media_type,
                    MediaType::Video | MediaType::Animation | MediaType::VideoNote
                ) =>
            {
                (doc.thumbs(), true)
            }
            _ => {
                return Err(Error::NoVisualMedia {
                    media_type: format!("{:?}", media_type).to_lowercase(),
                });
            }
        };

        let candidates = size_candidates(&thumbs);
        let largest = candidates
            .iter()
            .max_by_key(|c| u64::from(c.width) * u64::from(c.height));
        let selected = select_size_candidate(&candidates, max_dimension).ok_or_else(|| {
            Error::DownloadFailed("no downloadable size variant available".to_string())
        })?;

        if selected.size_bytes > max_download_bytes {
            return Err(Error::MediaTooLarge {
                size_bytes: selected.size_bytes,
                max_bytes: max_download_bytes,
            });
        }

        let photo_size = thumbs
            .iter()
            .find(|t| t.photo_type() == selected.photo_type)
            .ok_or_else(|| {
                Error::DownloadFailed("selected size variant disappeared".to_string())
            })?;

        let bytes = with_timeout("download_media", self.timeouts.download_secs, async {
            let mut data: Vec<u8> = Vec::new();
            let mut download = self.client.iter_download(photo_size);
            loop {
                match download.next().await {
                    Ok(Some(chunk)) => {
                        data.extend_from_slice(&chunk);
                        // Reported sizes are untrusted input; re-check while streaming.
                        if data.len() as u64 > max_download_bytes {
                            return Err(Error::MediaTooLarge {
                                size_bytes: data.len() as u64,
                                max_bytes: max_download_bytes,
                            });
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!(
                            channel_ref = %channel_ref,
                            message_id,
                            error = %e,
                            "Media download failed"
                        );
                        return Err(Error::DownloadFailed(format!("download failed: {}", e)));
                    }
                }
            }
            Ok(data)
        })
        .await?;

        let caption = match msg.text() {
            "" => None,
            text => Some(text.to_string()),
        };

        tracing::info!(
            channel_ref = %channel_ref,
            message_id,
            media_type = ?media_type,
            is_thumbnail,
            selected_type = %selected.photo_type,
            bytes = bytes.len(),
            "Media downloaded"
        );

        Ok(MediaDownload {
            bytes,
            media_type,
            is_thumbnail,
            caption,
            width: Some(selected.width),
            height: Some(selected.height),
            source_size_bytes: selected.size_bytes,
            video_info: extract_video_info(&media),
            largest_width: largest.map(|c| c.width),
            largest_height: largest.map(|c| c.height),
        })
    }
}
