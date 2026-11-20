//! Plain-text send orchestration. All RPCs follow the caller's admission.
use super::TelegramClient;
use super::resolve_recipient::ResolvedRecipient;
use super::send_diagnostics::{private_rpc, stage_result};
use super::send_receipt::ExpectedSend;
use super::send_targets::ValidatedTargets;
use super::send_transport::{PreparedFinalSend, random_id, submit_final_with};
use crate::telegram::sending::{SendDeadline, SendStage};
use crate::telegram::types::sending::{
    ContentKind, SendFailure, SendFailureKind, SendText, SentMessage,
};
use crate::telegram::types::{ChannelId, PeerNamespace};
use grammers_client::tl::{self, Serializable as _};
use grammers_session::Session as _;

pub(super) fn prepare_text_request(
    request: &SendText,
    recipient: &ResolvedRecipient,
    targets: &ValidatedTargets,
    random_id: i64,
) -> tl::functions::messages::SendMessage {
    tl::functions::messages::SendMessage {
        no_webpage: !request.link_preview,
        silent: request.options.silent,
        background: false,
        clear_draft: false,
        noforwards: false,
        update_stickersets_order: false,
        invert_media: false,
        allow_paid_floodskip: false,
        peer: recipient.reference.into(),
        reply_to: targets.reply_to.clone(),
        message: request.text.clone(),
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
        rich_message: None,
    }
}
impl TelegramClient {
    pub(super) async fn send_message_impl(
        &self,
        request: &SendText,
        deadline: SendDeadline,
    ) -> Result<SentMessage, SendFailure> {
        request.validate()?;
        let recipient = stage_result(
            "resolve",
            private_rpc(self.resolve_recipient(&request.options.recipient, deadline)).await,
        )?;
        let targets = stage_result(
            "validate_targets",
            private_rpc(self.validate_send_targets(&recipient, &request.options, deadline)).await,
        )?;
        let random_id = random_id()?;
        let body = prepare_text_request(request, &recipient, &targets, random_id).to_bytes();
        let expected = ExpectedSend {
            random_id,
            recipient: recipient.identity,
            saved_messages: recipient.saved_messages,
            content_kind: ContentKind::Text,
            reply_to_message_id: request.options.reply_to_message_id,
            topic_id: request.options.topic_id,
            message_link: None,
            media: None,
        };
        let dc = self
            .session
            .home_dc_id()
            .map_err(|_| SendFailure::not_sent(SendFailureKind::Unavailable, None))?;
        let sent = private_rpc(submit_final_with(
            PreparedFinalSend { body, expected },
            deadline,
            |body| self.send_adapter.submit(dc, body),
        ))
        .await;
        let mut sent = stage_result("final_send", sent)?;
        deadline
            .run(SendStage::Receipt, async {
                if recipient.identity.namespace() == PeerNamespace::Channel {
                    let channel = ChannelId::new(recipient.identity.id())
                        .map_err(|_| SendFailure::unknown())?;
                    sent.message_link = Some(
                        crate::link::MessageLink::new(
                            channel,
                            sent.message_id,
                            recipient.link_username.as_ref().map(|name| name.as_str()),
                        )
                        .https_link,
                    );
                }
                Ok(sent)
            })
            .await
    }
}
