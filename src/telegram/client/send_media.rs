//! Stream prepared snapshots, then submit exactly one final media mutation.
use super::TelegramClient;
use super::resolve_recipient::{ResolvedRecipient, read_failure};
use super::send_diagnostics::{private_rpc, stage_result};
use super::send_receipt::ExpectedSend;
use super::send_targets::ValidatedTargets;
use super::send_transport::{PreparedFinalSend, random_id, submit_final_with};
use super::upload_pool::UploadPool;
use crate::sending::StagedUpload;
use crate::telegram::sending::{SendDeadline, SendStage};
use crate::telegram::types::sending::{
    PreparedMediaSend, SendFailure, SendFailureKind, SendMediaKind, SentMessage, SubmittedMedia,
};
use grammers_client::media::Uploaded;
use grammers_client::tl::{self, Serializable as _};
use grammers_session::Session as _;
use std::future::Future;
use std::sync::Arc;

pub(super) async fn validate_before_upload<V, U, UF>(
    deadline: SendDeadline,
    validation: V,
    upload: U,
) -> Result<(ValidatedTargets, Uploaded), SendFailure>
where
    V: Future<Output = Result<ValidatedTargets, SendFailure>>,
    U: FnOnce() -> UF,
    UF: Future<Output = Result<Uploaded, SendFailure>>,
{
    let targets = deadline.run(SendStage::ValidateTargets, validation).await?;
    let uploaded = deadline
        .run(SendStage::Upload, async { upload().await })
        .await?;
    Ok((targets, uploaded))
}

pub(super) fn prepare_media_request(
    uploaded: Uploaded,
    request: &PreparedMediaSend,
    recipient: &ResolvedRecipient,
    targets: &ValidatedTargets,
    random_id: i64,
) -> Result<tl::functions::messages::SendMedia, SendFailure> {
    request.validate()?;
    let media = match &request.media {
        SendMediaKind::Image => tl::types::InputMediaUploadedPhoto {
            spoiler: false,
            live_photo: false,
            file: uploaded.raw,
            stickers: None,
            ttl_seconds: None,
            video: None,
        }
        .into(),
        kind => {
            let attribute: tl::enums::DocumentAttribute = match kind {
                SendMediaKind::Audio {
                    duration_seconds,
                    title,
                    performer,
                } => tl::types::DocumentAttributeAudio {
                    voice: false,
                    duration: *duration_seconds,
                    title: title.clone(),
                    performer: performer.clone(),
                    waveform: None,
                }
                .into(),
                SendMediaKind::Voice { duration_seconds } => tl::types::DocumentAttributeAudio {
                    voice: true,
                    duration: *duration_seconds,
                    title: None,
                    performer: None,
                    waveform: None,
                }
                .into(),
                SendMediaKind::Video {
                    duration_seconds,
                    width,
                    height,
                } => tl::types::DocumentAttributeVideo {
                    round_message: false,
                    supports_streaming: false,
                    nosound: false,
                    duration: *duration_seconds,
                    w: *width as i32,
                    h: *height as i32,
                    preload_prefix_size: None,
                    video_start_ts: None,
                    video_codec: None,
                }
                .into(),
                SendMediaKind::Image => {
                    return Err(SendFailure::not_sent(SendFailureKind::InternalError, None));
                }
            };
            tl::types::InputMediaUploadedDocument {
                nosound_video: false,
                force_file: false,
                spoiler: false,
                file: uploaded.raw,
                thumb: None,
                mime_type: request.staged.mime_type().into(),
                attributes: vec![
                    attribute,
                    tl::types::DocumentAttributeFilename {
                        file_name: request.staged.upload_name().into(),
                    }
                    .into(),
                ],
                stickers: None,
                video_cover: None,
                video_timestamp: None,
                ttl_seconds: None,
            }
            .into()
        }
    };
    Ok(tl::functions::messages::SendMedia {
        silent: request.options.silent,
        background: false,
        clear_draft: false,
        noforwards: false,
        update_stickersets_order: false,
        invert_media: false,
        allow_paid_floodskip: false,
        peer: recipient.reference.into(),
        reply_to: targets.reply_to.clone(),
        media,
        message: request.caption.clone(),
        random_id,
        reply_markup: None,
        entities: None,
        schedule_date: None,
        schedule_repeat_period: None,
        send_as: None,
        quick_reply_shortcut: None,
        effect: None,
        allow_paid_stars: None,
        suggested_post: None,
    })
}

pub(super) fn upload_failure(error: std::io::Error) -> SendFailure {
    if let Some(inner) = error.into_inner()
        && let Ok(error) = inner.downcast::<grammers_mtsender::InvocationError>()
    {
        return read_failure(*error, SendFailureKind::UploadFailed);
    }
    SendFailure::not_sent(SendFailureKind::UploadFailed, None)
}

impl TelegramClient {
    pub(super) async fn upload_staged(
        &self,
        staged: &Arc<StagedUpload>,
        deadline: SendDeadline,
    ) -> Result<Uploaded, SendFailure> {
        let pool = UploadPool::start(Arc::clone(&self.session), self.api_id, Arc::clone(staged));
        let result = deadline
            .run(
                SendStage::Upload,
                private_rpc(async {
                    let mut reader = staged.reader().await?;
                    let size = usize::try_from(staged.size_bytes())
                        .map_err(|_| SendFailure::not_sent(SendFailureKind::MediaTooLarge, None))?;
                    pool.client
                        .upload_stream(&mut reader, size, staged.upload_name().to_string())
                        .await
                        .map_err(upload_failure)
                }),
            )
            .await;
        drop(pool);
        stage_result("upload", result)
    }
    pub(super) async fn send_media_impl(
        &self,
        request: &PreparedMediaSend,
        deadline: SendDeadline,
    ) -> Result<SentMessage, SendFailure> {
        request.validate()?;
        let recipient = stage_result(
            "resolve",
            private_rpc(self.resolve_recipient(&request.options.recipient, deadline)).await,
        )?;
        let (targets, uploaded) = validate_before_upload(
            deadline,
            async {
                stage_result(
                    "validate_targets",
                    private_rpc(self.validate_send_targets(&recipient, &request.options, deadline))
                        .await,
                )
            },
            || self.upload_staged(&request.staged, deadline),
        )
        .await?;
        let random_id = random_id()?;
        let body =
            prepare_media_request(uploaded, request, &recipient, &targets, random_id)?.to_bytes();
        let expected = ExpectedSend {
            random_id,
            recipient: recipient.identity,
            saved_messages: recipient.saved_messages,
            content_kind: request.media.content_kind(),
            reply_to_message_id: request.options.reply_to_message_id,
            topic_id: request.options.topic_id,
            message_link: None,
            media: Some(SubmittedMedia {
                mime_type: request.staged.mime_type().into(),
                size_bytes: request.staged.size_bytes(),
            }),
        };
        let dc = self
            .session
            .home_dc_id()
            .map_err(|_| SendFailure::not_sent(SendFailureKind::Unavailable, None))?;
        let result = private_rpc(submit_final_with(
            PreparedFinalSend { body, expected },
            deadline,
            |body| self.send_adapter.submit(dc, body),
        ))
        .await;
        let mut sent = stage_result("final_send", result)?;
        deadline
            .run(SendStage::Receipt, async {
                if recipient.identity.namespace() == crate::telegram::types::PeerNamespace::Channel
                {
                    let id = crate::telegram::types::ChannelId::new(recipient.identity.id())
                        .map_err(|_| SendFailure::unknown())?;
                    sent.message_link = Some(
                        crate::link::MessageLink::new(
                            id,
                            sent.message_id,
                            recipient.link_username.as_ref().map(|u| u.as_str()),
                        )
                        .https_link,
                    );
                }
                Ok(sent)
            })
            .await
    }
}
