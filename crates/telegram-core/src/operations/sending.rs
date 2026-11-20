//! Charged text and media dispatch for a hosted account.

use super::{AccountOperations, OperationContext};
use crate::error::Error;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::sending::{DispatchGate, SendDeadline, SendStage};
use crate::telegram::types::sending::{
    PreparedMediaSend, SendFailure, SendFailureKind, SendText, SentMessage,
};
use crate::telegram::{TelegramClient, TelegramClientTrait};
use std::sync::atomic::{AtomicBool, Ordering};

impl<R: RateLimiterTrait> AccountOperations<TelegramClient, R> {
    /// Validate and charge one outgoing message, then apply the final gate.
    ///
    /// # Errors
    /// Returns a safe failure carrying known or unknown delivery state.
    pub async fn send_text(
        &self,
        request: &SendText,
        context: &OperationContext,
        gate: &dyn DispatchGate,
        cost: u32,
    ) -> Result<SentMessage, SendFailure> {
        request.validate()?;
        let deadline = SendDeadline::at(context.deadline());
        let gate = TrackingGate {
            gate,
            submitted: AtomicBool::new(false),
        };
        let operation = async {
            self.charge_send(cost, deadline).await?;
            self.client
                .send_message_gated(request, deadline, Some(&gate))
                .await
        };
        until_cancelled(context, &gate, operation).await
    }

    /// Validate and charge media before recipient lookup and upload, then apply
    /// the final gate at sender submission.
    ///
    /// # Errors
    /// Returns a safe failure carrying known or unknown delivery state.
    pub async fn send_media(
        &self,
        request: &PreparedMediaSend,
        context: &OperationContext,
        gate: &dyn DispatchGate,
        cost: u32,
    ) -> Result<SentMessage, SendFailure> {
        request.validate()?;
        let deadline = SendDeadline::at(context.deadline());
        let gate = TrackingGate {
            gate,
            submitted: AtomicBool::new(false),
        };
        let operation = async {
            self.charge_send(cost, deadline).await?;
            self.client
                .send_media_gated(request, deadline, Some(&gate))
                .await
        };
        until_cancelled(context, &gate, operation).await
    }
}

impl<T: TelegramClientTrait, R: RateLimiterTrait> AccountOperations<T, R> {
    /// Validate, charge and submit a local text send through the low-level client.
    ///
    /// # Errors
    /// Returns a safe failure carrying known or unknown delivery state.
    pub async fn send_local_text(
        &self,
        request: &SendText,
        deadline: SendDeadline,
        cost: u32,
    ) -> Result<SentMessage, SendFailure> {
        request.validate()?;
        self.charge_send(cost, deadline).await?;
        self.client.send_message(request, deadline).await
    }

    /// Validate, charge and submit a local send from an owned staged snapshot.
    ///
    /// # Errors
    /// Returns a safe failure carrying known or unknown delivery state.
    pub async fn send_local_media(
        &self,
        request: &PreparedMediaSend,
        deadline: SendDeadline,
        cost: u32,
    ) -> Result<SentMessage, SendFailure> {
        request.validate()?;
        self.charge_send(cost, deadline).await?;
        self.client.send_media(request, deadline).await
    }

    async fn charge_send(&self, cost: u32, deadline: SendDeadline) -> Result<(), SendFailure> {
        deadline
            .run(SendStage::LocalPreparation, async {
                self.limiter
                    .acquire(cost)
                    .await
                    .map_err(|error| match error {
                        Error::RateLimit {
                            retry_after_seconds,
                            ..
                        } => SendFailure::not_sent(
                            SendFailureKind::RateLimited,
                            Some(retry_after_seconds),
                        ),
                        _ => SendFailure::not_sent(SendFailureKind::Unavailable, None),
                    })
            })
            .await
    }
}

struct TrackingGate<'a> {
    gate: &'a dyn DispatchGate,
    submitted: AtomicBool,
}

#[async_trait::async_trait]
impl DispatchGate for TrackingGate<'_> {
    async fn submit(&self, enqueue: &mut (dyn FnMut() + Send)) -> Result<(), SendFailure> {
        self.gate
            .submit(&mut || {
                enqueue();
                self.submitted.store(true, Ordering::Release);
            })
            .await
    }
}

async fn until_cancelled(
    context: &OperationContext,
    gate: &TrackingGate<'_>,
    work: impl Future<Output = Result<SentMessage, SendFailure>>,
) -> Result<SentMessage, SendFailure> {
    tokio::select! {
        biased;
        _ = context.cancellation().cancelled() => {
            if gate.submitted.load(Ordering::Acquire) {
                Err(SendFailure::unknown())
            } else {
                Err(SendFailure::not_sent(SendFailureKind::SendForbidden, None))
            }
        }
        result = work => result,
    }
}
