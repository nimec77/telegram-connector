//! Offline Telegram send outcomes after the actual hosted final dispatch gate.

use super::{FinalGate, failure, success};
use crate::service::authorization::ReadAccess;
use axum::response::Response;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use telegram_core::telegram::sending::DispatchGate as _;
use telegram_core::telegram::types::MessageId;
use telegram_core::telegram::types::sending::{
    ContentKind, RecipientSpec, SendFailure, SendFailureKind, SendOptions, SentMessage,
};
use tokio::sync::Notify;

#[derive(Default)]
pub(in crate::service) struct SendFixture {
    dispatches: AtomicUsize,
    pub(super) pause: AtomicBool,
    outcome: Mutex<SendOutcome>,
    pub(super) entered: Notify,
    pub(super) resume: Notify,
}

/// Outcome observed after the hosted send gate permits dispatch.
#[derive(Clone, Copy, Default)]
pub enum SendOutcome {
    /// A receipt confirms delivery.
    #[default]
    Sent,
    /// Telegram refused to post to the selected destination.
    Refused,
    /// The dispatch was queued and its receipt was lost.
    Unknown,
}

impl SendFixture {
    pub(in crate::service) fn dispatches(&self) -> usize {
        self.dispatches.load(Ordering::Relaxed)
    }

    pub(in crate::service) fn set_outcome(&self, outcome: SendOutcome) {
        *self.outcome.lock().expect("fixture send outcome") = outcome;
    }

    pub(super) async fn call(
        &self,
        access: &ReadAccess,
        request: &SendOptions,
        content_kind: ContentKind,
    ) -> Response {
        if self.pause.load(Ordering::Acquire) {
            self.entered.notify_one();
            self.resume.notified().await;
        }
        if let Err(error) = FinalGate(access)
            .submit(&mut || {
                self.dispatches.fetch_add(1, Ordering::Relaxed);
            })
            .await
        {
            return failure(error);
        }
        match *self.outcome.lock().expect("fixture send outcome") {
            SendOutcome::Sent => {}
            SendOutcome::Refused => {
                return failure(SendFailure::not_sent(SendFailureKind::SendForbidden, None));
            }
            SendOutcome::Unknown => return failure(SendFailure::unknown()),
        }
        let RecipientSpec::Peer(recipient) = request.recipient else {
            return failure(SendFailure::not_sent(SendFailureKind::SendForbidden, None));
        };
        success(SentMessage {
            recipient,
            saved_messages: false,
            message_id: MessageId::new(105).expect("ID"),
            date: telegram_core::UtcTimestamp::from_unix_seconds(1_700_000_000).expect("date"),
            content_kind,
            reply_to_message_id: None,
            topic_id: None,
            message_link: None,
            media: None,
        })
    }
}
