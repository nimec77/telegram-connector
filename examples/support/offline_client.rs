//! Credential-free client seam shared by development examples.
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use telegram_connector::telegram::sending::SendDeadline;
use telegram_connector::telegram::transcription::TranscriptionDeadline;
use telegram_connector::telegram::types::sending::{
    ContentKind, PreparedMediaSend, QualifiedRecipient, RecipientSpec, SendFailure,
    SendFailureKind, SendOptions, SendText, SentMessage, SubmittedMedia, UsernameNamespace,
};
use telegram_connector::telegram::types::{
    AccountStatus, Channel, ChannelId, ChannelIdentity, ChannelPage, ChannelResolution,
    ChannelStats, HistoryParams, MediaBatch, MediaDownload, MediaFetchError, MediaFetchOutcome,
    MediaType, Message, MessageBatch, PeerNamespace, SearchParams, SearchResult,
    TranscriptionOutcome,
};
use telegram_connector::{Error, TelegramClientTrait};

// An external implementation of the unsealed client seam. Any accidental RPC-like
// call fails instead of touching a network; status reads only the local observation.
pub(crate) struct OfflineClient {
    pub(crate) media: bool,
    pub(crate) gate: Option<Arc<SendGate>>,
    pub(crate) sent: AtomicUsize,
}
#[derive(Default)]
pub(crate) struct SendGate {
    pub(crate) started: tokio::sync::Notify,
    pub(crate) release: tokio::sync::Notify,
    pub(crate) dispatched: AtomicUsize,
    pub(crate) dropped: AtomicUsize,
}
struct Checkpoint(Arc<SendGate>);
impl Drop for Checkpoint {
    fn drop(&mut self) {
        self.0.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
impl OfflineClient {
    pub(crate) fn new(media: bool) -> Self {
        Self {
            media,
            gate: None,
            sent: AtomicUsize::new(0),
        }
    }
    async fn simulated_send(
        &self,
        options: &SendOptions,
        kind: ContentKind,
        media: Option<SubmittedMedia>,
        deadline: SendDeadline,
    ) -> Result<SentMessage, SendFailure> {
        if !self.media {
            return Err(SendFailure::not_sent(
                SendFailureKind::SendingDisabled,
                None,
            ));
        }
        if deadline.expired() {
            return Err(SendFailure::not_sent(SendFailureKind::Timeout, None));
        }
        // An irreversible simulated submission precedes its delayed acknowledgement.
        // This fixture never constructs a Telegram connection or sends to an account.
        let id = 42 + self.sent.fetch_add(1, Ordering::SeqCst) as i64;
        if let Some(gate) = &self.gate {
            let _checkpoint = Checkpoint(Arc::clone(gate));
            gate.dispatched.fetch_add(1, Ordering::SeqCst);
            gate.started.notify_one();
            tokio::time::timeout_at(deadline.instant(), gate.release.notified())
                .await
                .map_err(|_| SendFailure::unknown())?;
        }
        let (namespace, id_value, saved) = match &options.recipient {
            RecipientSpec::SavedMessages => (PeerNamespace::User, 91, true),
            RecipientSpec::Peer(id) => (id.namespace(), id.id(), false),
            RecipientSpec::Username {
                expected_namespace: UsernameNamespace::User,
                ..
            } => (PeerNamespace::User, 91, false),
            RecipientSpec::Username {
                expected_namespace: UsernameNamespace::Channel,
                ..
            } => (PeerNamespace::Channel, 92, false),
        };
        Ok(SentMessage {
            recipient: QualifiedRecipient::try_new(namespace, id_value)
                .map_err(|_| SendFailure::unknown())?,
            saved_messages: saved,
            message_id: telegram_connector::telegram::types::MessageId::new(id)
                .map_err(|_| SendFailure::unknown())?,
            date: chrono::DateTime::from_timestamp(1_700_000_000, 0)
                .ok_or_else(SendFailure::unknown)?,
            content_kind: kind,
            reply_to_message_id: options.reply_to_message_id,
            topic_id: options.topic_id,
            message_link: None,
            media,
        })
    }
}
fn unavailable<T>() -> Result<T, Error> {
    Err(Error::InvalidInput(
        "inert example has no Telegram connection".into(),
    ))
}

#[async_trait::async_trait]
impl TelegramClientTrait for OfflineClient {
    async fn send_message(
        &self,
        request: &SendText,
        deadline: SendDeadline,
    ) -> Result<SentMessage, SendFailure> {
        request.validate()?;
        self.simulated_send(&request.options, ContentKind::Text, None, deadline)
            .await
    }
    async fn send_media(
        &self,
        request: &PreparedMediaSend,
        deadline: SendDeadline,
    ) -> Result<SentMessage, SendFailure> {
        request.validate()?;
        let mut reader = request.staged.reader().await?;
        tokio::time::timeout_at(
            deadline.instant(),
            tokio::io::copy(&mut reader, &mut tokio::io::sink()),
        )
        .await
        .map_err(|_| SendFailure::not_sent(SendFailureKind::Timeout, None))?
        .map_err(|_| SendFailure::not_sent(SendFailureKind::UploadFailed, None))?;
        self.simulated_send(
            &request.options,
            request.media.content_kind(),
            Some(SubmittedMedia {
                mime_type: request.staged.mime_type().into(),
                size_bytes: request.staged.size_bytes(),
            }),
            deadline,
        )
        .await
    }
    async fn search_messages(&self, _params: &SearchParams) -> Result<SearchResult, Error> {
        unavailable()
    }
    async fn get_recent_messages(&self, _params: &HistoryParams) -> Result<SearchResult, Error> {
        unavailable()
    }
    async fn get_channel_info(&self, _identifier: &str) -> Result<Channel, Error> {
        unavailable()
    }
    async fn get_full_channel_info(&self, _identifier: &str) -> Result<Channel, Error> {
        unavailable()
    }
    async fn get_subscribed_channels(
        &self,
        _limit: u32,
        _offset: u32,
    ) -> Result<ChannelPage, Error> {
        unavailable()
    }
    async fn search_public_channels(
        &self,
        _query: &str,
        _limit: u32,
    ) -> Result<Vec<Channel>, Error> {
        unavailable()
    }
    async fn cached_account_status(&self) -> AccountStatus {
        AccountStatus {
            authorized: self.media,
            ..Default::default()
        }
    }
    async fn refresh_account_status(&self) -> Result<AccountStatus, Error> {
        unavailable()
    }
    async fn get_message_by_id(
        &self,
        _channel_ref: &str,
        _message_id: i32,
    ) -> Result<Message, Error> {
        unavailable()
    }
    async fn get_messages_batch(
        &self,
        _channel_ref: &str,
        _message_ids: &[i32],
    ) -> Result<MessageBatch, Error> {
        unavailable()
    }
    async fn resolve_channel_identity(&self, _channel_ref: &str) -> Result<ChannelIdentity, Error> {
        unavailable()
    }
    async fn download_message_media(
        &self,
        _channel_ref: &str,
        _message_id: i32,
        _max_dimension: u32,
    ) -> Result<MediaDownload, Error> {
        if !self.media {
            return unavailable();
        }
        let pixels = image::RgbImage::from_fn(64, 64, |x, y| {
            image::Rgb([(x * 4) as u8, (y * 4) as u8, 128])
        });
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 80)
            .encode_image(&pixels)
            .map_err(|_| Error::DownloadFailed("offline JPEG fixture failed".into()))?;
        Ok(MediaDownload {
            lease: None,
            channel_id: ChannelId::new(100)?,
            peer_namespace: PeerNamespace::Channel,
            source_size_bytes: bytes.len() as u64,
            bytes,
            media_type: MediaType::Photo,
            is_thumbnail: false,
            caption: Some("offline preview fixture".into()),
            width: Some(64),
            height: Some(64),
            video_info: None,
            largest_width: Some(64),
            largest_height: Some(64),
        })
    }
    async fn download_messages_media(
        &self,
        _channel_ref: &str,
        _message_ids: &[i32],
        _max_dimension: u32,
    ) -> Result<telegram_connector::telegram::types::MediaBatch, Error> {
        if !self.media {
            return unavailable();
        }
        let mut outcomes = Vec::new();
        for &id in _message_ids {
            let result = if id == 2 {
                Err(MediaFetchError::NotFound)
            } else {
                self.download_message_media(_channel_ref, id, _max_dimension)
                    .await
                    .map_err(MediaFetchError::Failed)
            };
            outcomes.push(MediaFetchOutcome {
                message_id: id,
                result,
            });
        }
        Ok(MediaBatch {
            channel_id: ChannelId::new(100)?,
            peer_namespace: PeerNamespace::Channel,
            outcomes,
            stream: None,
            deadline: None,
        })
    }
    async fn transcribe_audio(
        &self,
        _channel_ref: &str,
        _message_id: i32,
        _deadline: TranscriptionDeadline,
    ) -> Result<TranscriptionOutcome, Error> {
        unavailable()
    }
    async fn resolve_channels(
        &self,
        _identifiers: &[String],
    ) -> Result<Vec<ChannelResolution<Error>>, Error> {
        unavailable()
    }
    async fn get_channel_stats(
        &self,
        _channel_ref: &str,
        _days_back: u32,
    ) -> Result<ChannelStats, Error> {
        unavailable()
    }
}
