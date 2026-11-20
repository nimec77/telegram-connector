//! Explicitly enabled, prompt, non-retrying Telegram mutations.

use super::authorization::ReadAccess;
use super::{ApiError, ServiceState};
use crate::contracts::hosted_tools::{
    MediaSpec, PeerNamespace, Recipient, SendMedia, SendMessage, UsernameNamespace,
};
use crate::contracts::{ErrorCode, Scope};
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse as _, Response};
use std::sync::Arc;
use telegram_core::telegram::sending::{DispatchGate, SendDeadline};
use telegram_core::telegram::types::sending::{
    PreparedMediaSend, QualifiedRecipient, RecipientSpec, SendFailure, SendFailureKind,
    SendMediaKind, SendOptions, SendText, SentMessage,
};
use telegram_core::telegram::types::{MessageId, PeerNamespace as DomainNamespace, Username};

type Body<T> = Result<Json<T>, JsonRejection>;

pub(super) async fn text(
    State(state): State<Arc<ServiceState>>,
    headers: HeaderMap,
    body: Body<SendMessage>,
) -> Result<Response, ApiError> {
    let access = authorize(&state, &headers).await?;
    let Json(body) = body.map_err(super::json_error)?;
    let request = SendText {
        options: options(
            body.recipient,
            body.reply_to_message_id,
            body.topic_id,
            body.silent,
        )?,
        text: body.text,
        link_preview: body.link_preview,
    };
    if let Err(error) = request.validate() {
        return Ok(failure(error));
    }
    #[cfg(any(test, feature = "test-support"))]
    if let Some(fixture) = access.hosted.send_fixture.get() {
        return Ok(fixture
            .call(
                &access,
                &request.options,
                telegram_core::telegram::types::sending::ContentKind::Text,
            )
            .await);
    }
    let lease = access.lease().await?;
    access.revalidate().await?;
    let context = access.context(&lease);
    let gate = FinalGate(&access);
    Ok(
        match lease
            .client()
            .operations
            .send_text(
                &request,
                &context,
                &gate,
                access.hosted.config.rate_limiting.send_message_cost,
            )
            .await
        {
            Ok(receipt) => success(receipt),
            Err(error) => failure(error),
        },
    )
}

pub(super) async fn media(
    State(state): State<Arc<ServiceState>>,
    headers: HeaderMap,
    body: Body<SendMedia>,
) -> Result<Response, ApiError> {
    let access = authorize(&state, &headers).await?;
    let Json(body) = body.map_err(super::json_error)?;
    let options = options(
        body.recipient,
        body.reply_to_message_id,
        body.topic_id,
        body.silent,
    )?;
    let media = media_kind(body.media);
    if body.caption.encode_utf16().count() > 1024 {
        return Err(ApiError::new(ErrorCode::InvalidRequest));
    }
    if let Err(error) = options.validate().and_then(|()| media.validate()) {
        return Ok(failure(error));
    }
    let uploads = access
        .hosted
        .uploads
        .as_ref()
        .ok_or(ApiError::new(ErrorCode::Forbidden))?;
    let deadline = SendDeadline::at(access.opening_context().deadline());
    let staged = uploads
        .stage(&access, &body.upload_handle, &media, deadline)
        .await?;
    let request = PreparedMediaSend {
        options,
        caption: body.caption,
        media,
        staged,
    };
    #[cfg(any(test, feature = "test-support"))]
    if let Some(fixture) = access.hosted.send_fixture.get() {
        return Ok(fixture
            .call(&access, &request.options, request.media.content_kind())
            .await);
    }
    let lease = access.lease().await?;
    access.revalidate().await?;
    let context = access.context(&lease);
    let gate = FinalGate(&access);
    Ok(
        match lease
            .client()
            .operations
            .send_media(
                &request,
                &context,
                &gate,
                access.hosted.config.rate_limiting.send_media_cost,
            )
            .await
        {
            Ok(receipt) => success(receipt),
            Err(error) => failure(error),
        },
    )
}

async fn authorize(state: &Arc<ServiceState>, headers: &HeaderMap) -> Result<ReadAccess, ApiError> {
    if !headers.contains_key(header::AUTHORIZATION) {
        return Err(ApiError::new(ErrorCode::Unauthenticated));
    }
    let access = ReadAccess::for_scope(state, headers, Scope::Send).await?;
    if !access.can_send() {
        return Err(ApiError::new(ErrorCode::Forbidden));
    }
    Ok(access)
}

struct FinalGate<'a>(&'a ReadAccess);

#[async_trait::async_trait]
impl DispatchGate for FinalGate<'_> {
    async fn submit(&self, enqueue: &mut (dyn FnMut() + Send)) -> Result<(), SendFailure> {
        let access = self.0;
        // Grant updates and retirement share this local transition lock.
        // The callback synchronously queues one SDK submission; the network
        // receipt is awaited after this guard drops.
        let _transition = access.hosted.transition.lock().await;
        if !access.can_send() || access.revalidate().await.is_err() {
            return Err(SendFailure::not_sent(SendFailureKind::SendForbidden, None));
        }
        enqueue();
        Ok(())
    }
}

fn options(
    recipient: Recipient,
    reply_to: Option<i64>,
    topic: Option<i64>,
    silent: bool,
) -> Result<SendOptions, ApiError> {
    let invalid = || ApiError::new(ErrorCode::InvalidRequest);
    let recipient = match recipient {
        Recipient::SavedMessages {} => RecipientSpec::SavedMessages,
        Recipient::Username {
            username,
            expected_namespace,
        } => {
            let name = Username::new(username).map_err(|_| invalid())?;
            let expected_namespace = match expected_namespace {
                UsernameNamespace::User => {
                    telegram_core::telegram::types::sending::UsernameNamespace::User
                }
                UsernameNamespace::Channel => {
                    telegram_core::telegram::types::sending::UsernameNamespace::Channel
                }
            };
            RecipientSpec::Username {
                username: name,
                expected_namespace,
            }
        }
        Recipient::Peer { namespace, id } => {
            let namespace = match namespace {
                PeerNamespace::User => DomainNamespace::User,
                PeerNamespace::Chat => DomainNamespace::Chat,
                PeerNamespace::Channel => DomainNamespace::Channel,
            };
            let id = super::accounts::positive_channel_id(&id).ok_or_else(invalid)?;
            RecipientSpec::Peer(QualifiedRecipient::try_new(namespace, id).map_err(|_| invalid())?)
        }
    };
    let parse = |id| MessageId::new(id).map_err(|_| invalid());
    Ok(SendOptions {
        recipient,
        reply_to_message_id: reply_to.map(parse).transpose()?,
        topic_id: topic.map(parse).transpose()?,
        silent,
    })
}

fn media_kind(media: MediaSpec) -> SendMediaKind {
    match media {
        MediaSpec::Image {} => SendMediaKind::Image,
        MediaSpec::Audio {
            duration_seconds,
            title,
            performer,
        } => SendMediaKind::Audio {
            duration_seconds,
            title,
            performer,
        },
        MediaSpec::Voice { duration_seconds } => SendMediaKind::Voice { duration_seconds },
        MediaSpec::Video {
            duration_seconds,
            width,
            height,
        } => SendMediaKind::Video {
            duration_seconds,
            width,
            height,
        },
    }
}

fn success(sent: SentMessage) -> Response {
    Json(serde_json::json!({
        "recipient": {"namespace": sent.recipient.namespace(), "id": sent.recipient.id().to_string()},
        "saved_messages": sent.saved_messages,
        "message_id": sent.message_id,
        "date": sent.date,
        "content_kind": sent.content_kind,
        "reply_to_message_id": sent.reply_to_message_id,
        "topic_id": sent.topic_id,
        "message_link": sent.message_link,
        "media": sent.media.map(|media| serde_json::json!({"mime_type":media.mime_type, "size_bytes":media.size_bytes})),
    })).into_response()
}

fn failure(error: SendFailure) -> Response {
    use SendFailureKind as Kind;
    let status = match error.kind() {
        Kind::RecipientNotFound => StatusCode::NOT_FOUND,
        Kind::SendingDisabled | Kind::SendForbidden => StatusCode::FORBIDDEN,
        Kind::Unauthenticated => StatusCode::UNAUTHORIZED,
        Kind::RecipientMismatch
        | Kind::InvalidReplyTarget
        | Kind::InvalidInput
        | Kind::UploadSourceInvalid
        | Kind::UnsupportedMedia => StatusCode::BAD_REQUEST,
        Kind::MediaTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        Kind::RateLimited | Kind::SendCapacityReached => StatusCode::TOO_MANY_REQUESTS,
        Kind::Timeout => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    };
    let retry = error.retry_after_seconds();
    let mut response = (
        status,
        Json(serde_json::json!({
            "error": {
                "code": error.kind().as_str(),
                "delivery": error.delivery_state(),
                "retryable": error.retryable(),
                "retry_after_seconds": retry,
            }
        })),
    )
        .into_response();
    if let Some(value) = retry.and_then(|secs| HeaderValue::try_from(secs.to_string()).ok()) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}

#[cfg(any(test, feature = "test-support"))]
pub(super) mod fixture;
#[cfg(test)]
mod tests;
