//! HTTP conversion for safe identity and service errors.

use crate::contracts::{ErrorBody, ErrorCode, ErrorResponse};
use crate::identity::AuthError;
use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

pub(super) struct ApiError(pub(super) ErrorCode);

impl From<AuthError> for ApiError {
    fn from(error: AuthError) -> Self {
        Self(match error {
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
        };
        let retry_after_seconds = (status == StatusCode::TOO_MANY_REQUESTS).then_some(1);
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
        if retry_after_seconds.is_some() {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        }
        response
    }
}
