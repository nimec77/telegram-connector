//! download_message_media operation.
//!
//! Unit of `client` (LM-2).

use super::*;

impl TelegramClient {
    pub(super) async fn download_message_media_impl(
        &self,
        channel_ref: &str,
        message_id: i32,
        max_dimension: u32,
    ) -> Result<MediaDownload, Error> {
        // Hard cap on a single download (`[telegram] max_download_bytes`, AD-6).
        // Hoisted to a Copy local so the streaming closure captures the value.
        let max_download_bytes = self.max_download_bytes;

        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }

        let peer = self.resolve_peer(channel_ref).await?;
        let peer_ref = peer
            .to_ref()
            .await
            .ok_or_else(|| Error::TelegramApi("Failed to convert peer to PeerRef".to_string()))?;

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

        let msg = messages.into_iter().next().flatten().ok_or_else(|| {
            Error::InvalidInput(format!(
                "Message {} not found in channel {}",
                message_id, channel_ref
            ))
        })?;

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
        })
    }
}
