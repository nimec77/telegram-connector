//! HTTP conversion for safe identity and service errors.

use crate::contracts::{ErrorBody, ErrorCode, ErrorResponse};
use crate::identity::AuthError;
use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

pub(super) struct ApiError(pub(super) ErrorCode, Option<u64>);

impl ApiError {
    pub(super) fn new(code: ErrorCode) -> Self {
        Self(code, None)
    }
}

impl From<AuthError> for ApiError {
    fn from(error: AuthError) -> Self {
        Self::new(match error {
            AuthError::Credential => ErrorCode::Unauthenticated,
            AuthError::Forbidden => ErrorCode::Forbidden,
            AuthError::Configuration | AuthError::Unavailable => ErrorCode::Unavailable,
        })
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.0 {
            ErrorCode::InvalidRequest => StatusCode::BAD_REQUEST,
            ErrorCode::Unauthenticated => StatusCode::UNAUTHORIZED,
            ErrorCode::Forbidden => StatusCode::FORBIDDEN,
            ErrorCode::UnavailableResource => StatusCode::NOT_FOUND,
            ErrorCode::Conflict => StatusCode::CONFLICT,
            ErrorCode::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::Timeout => StatusCode::GATEWAY_TIMEOUT,
            ErrorCode::PremiumRequired => StatusCode::FORBIDDEN,
            ErrorCode::NotTranscribable => StatusCode::BAD_REQUEST,
            ErrorCode::VoiceTooLong => StatusCode::BAD_REQUEST,
        };
        let retry_after_seconds = self.1.or((self.0 == ErrorCode::RateLimited).then_some(1));
        let mut response = (
            status,
            Json(ErrorResponse {
                error: ErrorBody {
                    code: self.0,
                    retry_after_seconds,
                },
            }),
        )
            .into_response();
        if let Some(value) =
            retry_after_seconds.and_then(|seconds| HeaderValue::try_from(seconds.to_string()).ok())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}

impl From<super::storage::StorageError> for ApiError {
    fn from(error: super::storage::StorageError) -> Self {
        use super::storage::StorageError;
        Self::new(match error {
            StorageError::Conflict | StorageError::RootLocked => ErrorCode::Conflict,
            StorageError::Missing => ErrorCode::UnavailableResource,
            StorageError::InvalidGrant => ErrorCode::InvalidRequest,
            StorageError::Unavailable => ErrorCode::Unavailable,
        })
    }
}

impl From<super::registry::LeaseError> for ApiError {
    fn from(error: super::registry::LeaseError) -> Self {
        use super::registry::LeaseError;
        match error {
            LeaseError::Capacity => Self::new(ErrorCode::RateLimited),
            LeaseError::Cancelled => Self::new(ErrorCode::UnavailableResource),
            LeaseError::Deadline => Self::new(ErrorCode::Timeout),
            LeaseError::Storage(error) => error.into(),
        }
    }
}

pub(super) fn from_telegram(error: telegram_core::error::Error) -> ApiError {
    use telegram_core::error::Error;
    if let Error::RateLimit {
        retry_after_seconds,
        ..
    } = error
    {
        return ApiError(ErrorCode::RateLimited, Some(retry_after_seconds));
    }
    ApiError::new(match error {
        Error::InvalidInput(_) => ErrorCode::InvalidRequest,
        Error::MessageNotFound { .. }
        | Error::ChannelNotFound(_)
        | Error::AmbiguousChannel(_)
        | Error::UnsupportedPeer
        | Error::Cancelled => ErrorCode::UnavailableResource,
        Error::AllChannelsFailed(errors)
            if errors.iter().all(|(_, error)| {
                matches!(
                    from_telegram(error.clone()).0,
                    ErrorCode::UnavailableResource
                )
            }) =>
        {
            ErrorCode::UnavailableResource
        }
        Error::Auth(_) => ErrorCode::Forbidden,
        Error::Timeout { .. } => ErrorCode::Timeout,
        Error::PremiumRequired => ErrorCode::PremiumRequired,
        Error::NotTranscribable { .. } => ErrorCode::NotTranscribable,
        Error::VoiceTooLong => ErrorCode::VoiceTooLong,
        Error::MediaTooLarge { .. } | Error::ArtifactTooLarge => ErrorCode::PayloadTooLarge,
        _ => ErrorCode::Unavailable,
    })
}
