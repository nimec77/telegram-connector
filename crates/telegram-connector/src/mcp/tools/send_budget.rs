//! Reserve complete worst-case receipt/error wire size before charging or sending.
use super::types::contract;
use super::types::sending::{SendReceipt, SentDeliveryState, SentRecipient, SubmittedMediaReceipt};
use crate::mcp::runtime;
use crate::telegram::types::PeerNamespace;
use crate::telegram::types::sending::{
    ContentKind, FailureDeliveryState, SendFailure, SendFailureKind,
};
use rmcp::model::CallToolResult;

pub(crate) fn preflight(bytes: usize) -> Result<(), SendFailure> {
    let too_large = || SendFailure::not_sent(SendFailureKind::ResponseTooLarge, None);
    let receipt = SendReceipt {
        delivery_state: SentDeliveryState::Sent,
        recipient: SentRecipient {
            peer_namespace: PeerNamespace::Channel,
            peer_id: i64::MAX.to_string(),
            saved_messages: false,
        },
        message_id: i32::MAX,
        date: "2038-01-19T03:14:07Z".into(),
        content_kind: ContentKind::Image,
        reply_to_message_id: Some(i32::MAX),
        topic_id: Some(i32::MAX),
        message_link: Some("a".repeat(128)),
        media: Some(SubmittedMediaReceipt {
            mime_type: "image/jpeg".into(),
            size_bytes: u64::MAX,
        }),
    };
    let rendered = contract::render_send(Ok(receipt));
    if runtime::response_size(&rendered).map_err(|_| too_large())? > bytes {
        return Err(too_large());
    }
    for kind in [
        SendFailureKind::SendingDisabled,
        SendFailureKind::RecipientNotFound,
        SendFailureKind::RecipientMismatch,
        SendFailureKind::SendForbidden,
        SendFailureKind::InvalidReplyTarget,
        SendFailureKind::UploadSourceInvalid,
        SendFailureKind::UnsupportedMedia,
        SendFailureKind::UploadFailed,
        SendFailureKind::SendCapacityReached,
        SendFailureKind::DeliveryUnknown,
        SendFailureKind::InvalidInput,
        SendFailureKind::MediaTooLarge,
        SendFailureKind::RateLimited,
        SendFailureKind::Unauthenticated,
        SendFailureKind::Timeout,
        SendFailureKind::ResponseTooLarge,
        SendFailureKind::Unavailable,
        SendFailureKind::InternalError,
    ] {
        let rendered = contract::render_send(Err(SendFailure::not_sent(kind, Some(u64::MAX))));
        if runtime::response_size(&rendered).map_err(|_| too_large())? > bytes {
            return Err(too_large());
        }
    }
    Ok(())
}

pub(crate) fn finish(result: Result<SendReceipt, SendFailure>, bytes: usize) -> CallToolResult {
    let may_have_sent = match &result {
        Ok(_) => true,
        Err(error) => error.delivery_state() == FailureDeliveryState::Unknown,
    };
    let rendered = contract::render_send(result);
    if runtime::response_size(&rendered).is_ok_and(|size| size <= bytes) {
        return rendered;
    }
    contract::render_send(Err(if may_have_sent {
        SendFailure::unknown()
    } else {
        SendFailure::not_sent(SendFailureKind::ResponseTooLarge, None)
    }))
}
