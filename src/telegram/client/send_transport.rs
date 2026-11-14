//! Final mutation submission bypasses pool application retries and discards raw failures.

use super::send_receipt::{ExpectedSend, map_ack};
use crate::telegram::sending::{SendDeadline, SendStage};
use crate::telegram::types::sending::{SendFailure, SendFailureKind, SentMessage};
use grammers_client::tl::{self, Deserializable as _};
use grammers_mtsender::{InvocationError, SenderPoolHandle};
use std::future::Future;

pub(super) struct FinalSendAdapter {
    handle: SenderPoolHandle,
}
impl FinalSendAdapter {
    pub(super) fn new(handle: SenderPoolHandle) -> Self {
        Self { handle }
    }
    pub(super) async fn submit(
        &self,
        dc_id: i32,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, InvocationError> {
        self.handle.raw_invoke_in_dc(dc_id, body).await
    }
}

pub(super) struct PreparedFinalSend {
    pub body: Vec<u8>,
    pub expected: ExpectedSend,
}

pub(super) fn random_id() -> Result<i64, SendFailure> {
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes)
        .map_err(|_| SendFailure::not_sent(SendFailureKind::InternalError, None))?;
    let id = i64::from_ne_bytes(bytes);
    if id == 0 {
        return Err(SendFailure::not_sent(SendFailureKind::InternalError, None));
    }
    Ok(id)
}

pub(super) async fn submit_final_with<F, Fut>(
    prepared: PreparedFinalSend,
    deadline: SendDeadline,
    submit: F,
) -> Result<SentMessage, SendFailure>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: Future<Output = Result<Vec<u8>, InvocationError>>,
{
    if deadline.expired() {
        return Err(SendFailure::not_sent(SendFailureKind::Timeout, None));
    }
    // Dispatch boundary: the one-shot closure can enqueue a mutation. Every
    // ambiguous failure from this point onwards must preserve unknown delivery.
    let response = deadline
        .run(SendStage::FinalSend, async {
            submit(prepared.body).await.map_err(classify_final_error)
        })
        .await?;
    deadline
        .run(SendStage::Receipt, async {
            let updates =
                tl::enums::Updates::from_bytes(&response).map_err(|_| SendFailure::unknown())?;
            map_ack(updates, &prepared.expected)
        })
        .await
}

pub(super) fn classify_final_error(error: InvocationError) -> SendFailure {
    let InvocationError::Rpc(rpc) = error else {
        return SendFailure::unknown();
    };
    // Match typed provider fields, never formatted errors. Unknown/provider-5xx
    // responses cannot establish that a prior mutation did not commit.
    let kind = match (rpc.code, rpc.name.as_str()) {
        (420, "FLOOD_WAIT" | "FLOOD_PREMIUM_WAIT" | "SLOWMODE_WAIT") => {
            return SendFailure::not_sent(SendFailureKind::RateLimited, rpc.value.map(u64::from));
        }
        (
            401,
            "AUTH_KEY_UNREGISTERED" | "SESSION_REVOKED" | "SESSION_EXPIRED" | "USER_DEACTIVATED",
        ) => SendFailureKind::Unauthenticated,
        (
            400..=499,
            "CHAT_WRITE_FORBIDDEN"
            | "USER_IS_BLOCKED"
            | "YOU_BLOCKED_USER"
            | "CHAT_SEND_MEDIA_FORBIDDEN"
            | "CHAT_SEND_PHOTOS_FORBIDDEN"
            | "CHAT_SEND_AUDIOS_FORBIDDEN"
            | "CHAT_SEND_VOICES_FORBIDDEN"
            | "CHAT_SEND_VIDEOS_FORBIDDEN"
            | "CHAT_SEND_PLAIN_FORBIDDEN"
            | "CHAT_RESTRICTED"
            | "CHANNEL_PRIVATE"
            | "USER_BANNED_IN_CHANNEL"
            | "CHAT_ADMIN_REQUIRED"
            | "ALLOW_PAYMENT_REQUIRED"
            | "PAYMENT_REQUIRED"
            | "PRIVACY_PREMIUM_REQUIRED"
            | "PREMIUM_ACCOUNT_REQUIRED"
            | "BROADCAST_FORBIDDEN",
        ) => SendFailureKind::SendForbidden,
        (
            400..=499,
            "REPLY_MESSAGE_ID_INVALID"
            | "TOPIC_CLOSED"
            | "TOPIC_DELETED"
            | "TOPIC_ID_INVALID"
            | "REPLY_TO_INVALID"
            | "MSG_ID_INVALID",
        ) => SendFailureKind::InvalidReplyTarget,
        (400..=499, "MESSAGE_EMPTY" | "MESSAGE_TOO_LONG" | "MEDIA_CAPTION_TOO_LONG") => {
            SendFailureKind::InvalidInput
        }
        _ => return SendFailure::unknown(),
    };
    SendFailure::not_sent(kind, None)
}
