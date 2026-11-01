//! Single and ordered streaming media downloads, sharing one operation deadline.
use super::TelegramClient;
use super::guard::require_found;
use super::resolve::{peer_to_ref, validate_channel_identifier};
use crate::error::Error;
use crate::telegram::converters::{
    convert_media_to_type, extract_video_info, select_size_candidate, size_candidates,
};
use crate::telegram::media_pipeline::{self, MediaDeadline};
use crate::telegram::timeout::with_timeout;
use crate::telegram::types::{MediaBatch, MediaDownload, MediaFetchError, MediaType};
use futures::StreamExt as _;
use grammers_client::media::Media;

impl TelegramClient {
    pub(super) async fn download_message_media_impl(
        &self,
        channel_ref: &str,
        message_id: i32,
        max_dimension: u32,
    ) -> Result<MediaDownload, Error> {
        validate_channel_identifier(channel_ref)?;
        let deadline = MediaDeadline::try_new(self.timeouts.download_secs)?;
        deadline
            .run(async {
                let peer = self.resolve_peer(channel_ref).await?;
                let peer_ref = peer_to_ref(&peer).await?;
                let messages =
                    with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
                        self.client
                            .get_messages_by_id(peer_ref, &[message_id])
                            .await
                            .map_err(|error| {
                                Error::TelegramApi(format!("Failed to get media messages: {error}"))
                            })
                    })
                    .await?;
                let message = require_found(
                    messages.into_iter().next().flatten(),
                    channel_ref,
                    message_id,
                )?;
                let lease = media_pipeline::acquire_raw(self.max_download_bytes, deadline).await?;
                let mut image = download_visual_media(
                    &self.client,
                    self.timeouts.download_secs,
                    self.max_download_bytes,
                    message,
                    channel_ref,
                    message_id,
                    max_dimension,
                )
                .await?;
                if image.bytes.capacity() as u64 > self.max_download_bytes {
                    return Err(Error::MediaTooLarge {
                        size_bytes: image.bytes.capacity() as u64,
                        max_bytes: self.max_download_bytes,
                    });
                }
                lease.record_retained(image.bytes.capacity());
                image.lease = Some(lease);
                Ok(image)
            })
            .await?
    }

    // Compatibility collection for Rust consumers. The caller owns retained batch
    // memory; production MCP uses the lazy pipeline below and keeps every lease.
    pub(super) async fn download_messages_media_impl(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
        max_dimension: u32,
    ) -> Result<MediaBatch, Error> {
        let batch = self
            .download_messages_media_pipeline_impl(channel_ref, message_ids, max_dimension)
            .await?;
        let channel_id = batch.channel_id;
        let peer_namespace = batch.peer_namespace;
        let deadline = batch.deadline;
        let outcomes = batch
            .into_stream()
            .map(|mut outcome| {
                if let Ok(image) = &mut outcome.result {
                    image.lease = None;
                }
                outcome
            })
            .collect()
            .await;
        Ok(MediaBatch {
            channel_id,
            peer_namespace,
            outcomes,
            stream: None,
            deadline,
        })
    }

    pub(super) async fn download_messages_media_pipeline_impl(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
        max_dimension: u32,
    ) -> Result<MediaBatch, Error> {
        validate_channel_identifier(channel_ref)?;
        let deadline = MediaDeadline::try_new(self.timeouts.download_secs)?;
        deadline
            .run(async {
                // One resolution and one shared fetch; the lazy stream never repeats them.
                let peer = self.resolve_peer(channel_ref).await?;
                let peer_ref = peer_to_ref(&peer).await?;
                let messages =
                    with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
                        self.client
                            .get_messages_by_id(peer_ref, message_ids)
                            .await
                            .map_err(|error| {
                                Error::TelegramApi(format!("Failed to get media messages: {error}"))
                            })
                    })
                    .await?;
                debug_assert_eq!(
                    messages.len(),
                    message_ids.len(),
                    "grammers must return one slot per requested id"
                );
                let jobs = message_ids
                    .iter()
                    .copied()
                    .zip(messages.into_iter().chain(std::iter::repeat_with(|| None)))
                    .map(|(id, message)| (id, (id, message)))
                    .collect();
                let client = self.client.clone();
                let channel_reference = channel_ref.to_owned();
                let download_secs = self.timeouts.download_secs;
                let max_bytes = self.max_download_bytes;
                let stream =
                    media_pipeline::media_stream(jobs, deadline, max_bytes, move |(id, slot)| {
                        let client = client.clone();
                        let channel_reference = channel_reference.clone();
                        async move {
                            let message = require_found(slot, &channel_reference, id)
                                .map_err(|_| MediaFetchError::NotFound)?;
                            download_visual_media(
                                &client,
                                download_secs,
                                max_bytes,
                                message,
                                &channel_reference,
                                id,
                                max_dimension,
                            )
                            .await
                            .map_err(|error| match error {
                                Error::NoVisualMedia { media_type } => {
                                    MediaFetchError::NoVisualMedia { media_type }
                                }
                                other => MediaFetchError::Failed(other),
                            })
                        }
                    });
                Ok(MediaBatch {
                    channel_id: crate::telegram::converters::channel_identity(&peer)
                        .ok_or(Error::UnsupportedPeer)?
                        .id,
                    peer_namespace: crate::telegram::converters::peer_namespace(&peer),
                    outcomes: Vec::new(),
                    stream: Some(stream),
                    deadline: Some(deadline),
                })
            })
            .await?
    }
}

async fn download_visual_media(
    client: &grammers_client::Client,
    download_secs: u64,
    max_download_bytes: u64,
    msg: grammers_client::message::Message,
    channel_ref: &str,
    message_id: i32,
    max_dimension: u32,
) -> Result<MediaDownload, Error> {
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
        .ok_or_else(|| Error::DownloadFailed("selected size variant disappeared".to_string()))?;

    let bytes = with_timeout("download_media", download_secs, async {
        let mut data: Vec<u8> = Vec::new();
        let mut download = client.iter_download(photo_size);
        loop {
            match download.next().await {
                Ok(Some(chunk)) => {
                    // Refuse the chunk before extending, including Vec capacity growth.
                    let observed = data.len().saturating_add(chunk.len()) as u64;
                    if observed > max_download_bytes {
                        return Err(Error::MediaTooLarge {
                            size_bytes: observed,
                            max_bytes: max_download_bytes,
                        });
                    }
                    data.try_reserve_exact(chunk.len())
                        .map_err(|_| Error::DownloadFailed("raw image allocation failed".into()))?;
                    data.extend_from_slice(&chunk);
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
        lease: None,
        channel_id: crate::telegram::types::ChannelId::new(
            msg.peer_id().bare_id().ok_or(Error::UnsupportedPeer)?,
        )?,
        peer_namespace: match msg.peer_id().kind() {
            grammers_session::types::PeerKind::User => return Err(Error::UnsupportedPeer),
            grammers_session::types::PeerKind::Chat => crate::telegram::types::PeerNamespace::Chat,
            grammers_session::types::PeerKind::Channel => {
                crate::telegram::types::PeerNamespace::Channel
            }
        },
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

#[cfg(test)]
#[path = "tests/media_pipeline_tests.rs"]
mod tests;
