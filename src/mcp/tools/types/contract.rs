//! Versioned application envelope, safe errors and one canonical output boundary.
use crate::config::OutputFormat;
use crate::error::Error;
use rmcp::model::{CallToolResult, ContentBlock, JsonObject};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Exact application schema revision; independent from the MCP protocol revision.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub enum SchemaVersion {
    #[serde(rename = "1")]
    V1,
}

/// Exactly one non-null branch. All registered data types are non-null objects.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ToolResponse<T> {
    Success {
        schema_version: SchemaVersion,
        data: T,
        error: (),
    },
    Failure {
        schema_version: SchemaVersion,
        data: (),
        error: ToolError,
    },
}
impl<T> ToolResponse<T> {
    pub fn success(data: T) -> Self {
        Self::Success {
            schema_version: SchemaVersion::V1,
            data,
            error: (),
        }
    }
    pub fn failure(error: ToolError) -> Self {
        Self::Failure {
            schema_version: SchemaVersion::V1,
            data: (),
            error,
        }
    }
}

/// Stable machine-readable failure category.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    SendingDisabled,
    RecipientNotFound,
    RecipientMismatch,
    SendForbidden,
    InvalidReplyTarget,
    UploadSourceInvalid,
    UnsupportedMedia,
    UploadFailed,
    SendCapacityReached,
    DeliveryUnknown,
    DuplicateRecoveryKey,
    RecoveryCapacityReached,
    ResponsePending,
    ResponseUnavailable,
    ResponseTooLarge,
    InvalidInput,
    RateLimited,
    Timeout,
    Unauthenticated,
    Unavailable,
    MessageNotFound,
    ChannelNotFound,
    AmbiguousChannel,
    UnsupportedPeer,
    UnsupportedPlatform,
    DesktopOpenFailed,
    InternalError,
    AllChannelsFailed,
    TelegramError,
    DownloadFailed,
    NoVisualMedia,
    MediaTooLarge,
    PayloadCapReached,
    PremiumRequired,
    TranscriptionFailed,
    VoiceTooLong,
    NotTranscribable,
}

/// Safe client-facing failure; raw provider payloads are never serialized here.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ToolError {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_state: Option<crate::telegram::types::sending::FailureDeliveryState>,
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
}
impl From<Error> for ToolError {
    fn from(error: Error) -> Self {
        let (code, message, retryable, retry_after_seconds) = match error {
            Error::ArtifactUnavailable => (
                ErrorCode::Unavailable,
                "Media artifact storage is unavailable",
                false,
                None,
            ),
            Error::ArtifactTooLarge => (
                ErrorCode::MediaTooLarge,
                "Media artifact exceeds storage capacity",
                false,
                None,
            ),
            Error::ImageLimitExceeded => (
                ErrorCode::MediaTooLarge,
                "Decoded image exceeds dimension, pixel or allocation limits",
                false,
                None,
            ),
            Error::DuplicateRecoveryKey => (
                ErrorCode::DuplicateRecoveryKey,
                "Recovery key is already pending or retained",
                false,
                None,
            ),
            Error::RecoveryCapacityReached => (
                ErrorCode::RecoveryCapacityReached,
                "Recovery capacity is unavailable",
                true,
                None,
            ),
            Error::ResponsePending => (
                ErrorCode::ResponsePending,
                "Response is still pending",
                true,
                None,
            ),
            Error::ResponseUnavailable => (
                ErrorCode::ResponseUnavailable,
                "Response is unavailable",
                false,
                None,
            ),
            Error::ResponseTooLarge => (
                ErrorCode::ResponseTooLarge,
                "Response metadata exceeds the output budget",
                false,
                None,
            ),
            Error::InvalidInput(_) => (
                ErrorCode::InvalidInput,
                "Invalid arguments; check the tool input schema and documented bounds",
                false,
                None,
            ),
            Error::RateLimit {
                retry_after_seconds,
                ..
            } => (
                ErrorCode::RateLimited,
                "Rate limit exceeded",
                true,
                Some(retry_after_seconds),
            ),
            Error::Timeout { .. } => (ErrorCode::Timeout, "Operation timed out", true, None),
            Error::Auth(_) => (
                ErrorCode::Unauthenticated,
                "Telegram authentication is required",
                false,
                None,
            ),
            Error::Unavailable(_) => (
                ErrorCode::Unavailable,
                "Required server state is unavailable",
                false,
                None,
            ),
            Error::MessageNotFound { .. } => (
                ErrorCode::MessageNotFound,
                "Message not found or deleted",
                false,
                None,
            ),
            Error::ChannelNotFound(_) => {
                (ErrorCode::ChannelNotFound, "Channel not found", false, None)
            }
            Error::AmbiguousChannel(_) | Error::AmbiguousTitle { .. } => (
                ErrorCode::AmbiguousChannel,
                "Ambiguous channel identity; use an unambiguous username or full output where available",
                false,
                None,
            ),
            Error::UnsupportedPeer => (
                ErrorCode::UnsupportedPeer,
                "Expected a channel or group",
                false,
                None,
            ),
            Error::UnsupportedPlatform => (
                ErrorCode::UnsupportedPlatform,
                "Desktop opening is supported only on macOS",
                false,
                None,
            ),
            Error::DesktopOpenFailed { .. } => (
                ErrorCode::DesktopOpenFailed,
                "The desktop application could not be opened",
                false,
                None,
            ),
            Error::Internal(_) => (
                ErrorCode::InternalError,
                "Internal operation failed",
                false,
                None,
            ),
            Error::AllChannelsFailed(_) => (
                ErrorCode::AllChannelsFailed,
                "All requested channels failed",
                false,
                None,
            ),
            Error::TelegramApi(_) => (
                ErrorCode::TelegramError,
                "Telegram request failed",
                false,
                None,
            ),
            Error::DownloadFailed(_) => (
                ErrorCode::DownloadFailed,
                "Media transfer or processing failed",
                false,
                None,
            ),
            Error::NoVisualMedia { .. } => (
                ErrorCode::NoVisualMedia,
                "Message has no supported visual media",
                false,
                None,
            ),
            Error::MediaTooLarge { .. } => (
                ErrorCode::MediaTooLarge,
                "Media exceeds the download limit",
                false,
                None,
            ),
            Error::PayloadCapExceeded { .. } => (
                ErrorCode::PayloadCapReached,
                "Image cannot fit the payload budget",
                false,
                None,
            ),
            Error::PremiumRequired => (
                ErrorCode::PremiumRequired,
                "Transcription requires Telegram Premium",
                false,
                None,
            ),
            Error::TranscriptionFailed(_) => (
                ErrorCode::TranscriptionFailed,
                "Transcription failed",
                false,
                None,
            ),
            Error::VoiceTooLong => (
                ErrorCode::VoiceTooLong,
                "Audio exceeds Telegram's transcription length limit",
                false,
                None,
            ),
            Error::NotTranscribable { .. } => (
                ErrorCode::NotTranscribable,
                "Only voice and video_note messages can be transcribed",
                false,
                None,
            ),
        };
        Self {
            delivery_state: None,
            code,
            message: message.to_string(),
            retryable,
            retry_after_seconds,
        }
    }
}

impl From<crate::telegram::types::sending::SendFailure> for ToolError {
    fn from(error: crate::telegram::types::sending::SendFailure) -> Self {
        use crate::telegram::types::sending::SendFailureKind as Kind;
        let code = match error.kind() {
            Kind::SendingDisabled => ErrorCode::SendingDisabled,
            Kind::RecipientNotFound => ErrorCode::RecipientNotFound,
            Kind::RecipientMismatch => ErrorCode::RecipientMismatch,
            Kind::SendForbidden => ErrorCode::SendForbidden,
            Kind::InvalidReplyTarget => ErrorCode::InvalidReplyTarget,
            Kind::UploadSourceInvalid => ErrorCode::UploadSourceInvalid,
            Kind::UnsupportedMedia => ErrorCode::UnsupportedMedia,
            Kind::UploadFailed => ErrorCode::UploadFailed,
            Kind::SendCapacityReached => ErrorCode::SendCapacityReached,
            Kind::DeliveryUnknown => ErrorCode::DeliveryUnknown,
            Kind::InvalidInput => ErrorCode::InvalidInput,
            Kind::MediaTooLarge => ErrorCode::MediaTooLarge,
            Kind::RateLimited => ErrorCode::RateLimited,
            Kind::Unauthenticated => ErrorCode::Unauthenticated,
            Kind::Timeout => ErrorCode::Timeout,
            Kind::ResponseTooLarge => ErrorCode::ResponseTooLarge,
            Kind::Unavailable => ErrorCode::Unavailable,
            Kind::InternalError => ErrorCode::InternalError,
        };
        Self {
            code,
            message: error.to_string(),
            retryable: error.retryable(),
            retry_after_seconds: error.retry_after_seconds(),
            delivery_state: Some(error.delivery_state()),
        }
    }
}

pub(crate) fn decode<P: for<'de> Deserialize<'de>>(arguments: JsonObject) -> Result<P, Error> {
    serde_json::from_value(serde_json::Value::Object(arguments))
        .map_err(|_| Error::InvalidInput("Arguments do not match the input schema".into()))
}

/// Retains typed data until rendering; Task 8 can trim and rebuild before wire encoding.
pub(crate) trait WireData: Sized {
    type Canonical: Serialize + JsonSchema + 'static;
    fn canonical(self) -> Result<(Self::Canonical, Vec<ContentBlock>), Error>;
    fn legacy(self) -> Result<CallToolResult, Error>;
}

pub(crate) fn legacy_json(value: &impl Serialize) -> Result<CallToolResult, Error> {
    let text = serde_json::to_string(value)
        .map_err(|_| Error::Internal("response serialization failed"))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

/// Serialize one typed envelope to matching structured and textual representations.
///
/// # Errors
/// Returns a safe internal error if the supplied data serializer fails or produces
/// null success data (which would violate the exclusive non-null branch contract). The registered
/// tool boundary catches that failure and emits a minimal canonical error envelope.
pub fn tool_result<T: Serialize + JsonSchema>(
    response: ToolResponse<T>,
) -> Result<CallToolResult, Error> {
    tool_result_ref(&response)
}

fn tool_result_ref<T: Serialize + JsonSchema>(
    response: &ToolResponse<T>,
) -> Result<CallToolResult, Error> {
    let failed = matches!(response, ToolResponse::Failure { .. });
    let value = serde_json::to_value(response)
        .map_err(|_| Error::Internal("response serialization failed"))?;
    if !failed && value["data"].is_null() {
        return Err(Error::Internal("success data must not be null"));
    }
    let text = serde_json::to_string(&value)
        .map_err(|_| Error::Internal("response serialization failed"))?;
    let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
    result.structured_content = Some(value);
    result.is_error = Some(failed);
    Ok(result)
}

pub(crate) fn render<T: WireData>(
    format: OutputFormat,
    result: Result<T, Error>,
) -> CallToolResult {
    if format == OutputFormat::Legacy {
        return match result.and_then(WireData::legacy) {
            Ok(result) => result,
            Err(Error::DesktopOpenFailed { message, link }) => {
                legacy_json(&super::responses::OpenMessageResponse {
                    success: false,
                    message,
                    link_used: link,
                    app_opened: false,
                })
                .unwrap_or_else(|_| {
                    CallToolResult::error(vec![ContentBlock::text("Response serialization failed")])
                })
            }
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.to_string())]),
        };
    }
    let (response, images) = match result.and_then(WireData::canonical) {
        Ok((data, images)) => (ToolResponse::<T::Canonical>::success(data), images),
        Err(error) => (
            ToolResponse::<T::Canonical>::failure(error.into()),
            Vec::new(),
        ),
    };
    // Borrow until all blocks have been appended, retaining any canonical data's
    // private delivery guards through complete CallToolResult assembly.
    match tool_result_ref(&response) {
        Ok(mut result) => {
            result.content.extend(images);
            result
        }
        Err(_) => {
            // This fixed scalar-only envelope cannot contain a failing user serializer.
            let value = serde_json::json!({"schema_version":"1", "data":null, "error":{
                "code":"internal_error", "message":"Response serialization failed", "retryable":false}});
            CallToolResult::structured_error(value)
        }
    }
}

pub(crate) fn output_schema<T: WireData>() -> Arc<JsonObject> {
    let mut schema =
        (*rmcp::handler::server::common::schema_for_output::<ToolResponse<T::Canonical>>()).clone();
    schema.insert("type".into(), serde_json::json!("object"));
    Arc::new(schema)
}

pub(crate) fn send_output_schema() -> Arc<JsonObject> {
    let mut schema = (*rmcp::handler::server::common::schema_for_output::<
        ToolResponse<super::sending::SendReceipt>,
    >())
    .clone();
    schema.insert("type".into(), serde_json::json!("object"));
    Arc::new(schema)
}

pub(crate) fn render_send(
    result: Result<super::sending::SendReceipt, crate::telegram::types::sending::SendFailure>,
) -> CallToolResult {
    render_send_data(result)
}

pub(crate) fn render_send_data<T: Serialize + JsonSchema>(
    result: Result<T, crate::telegram::types::sending::SendFailure>,
) -> CallToolResult {
    let response = match result {
        Ok(data) => ToolResponse::success(data),
        Err(error) => ToolResponse::failure(error.into()),
    };
    tool_result(response).unwrap_or_else(|_| {
        tracing::warn!(stage="receipt",category="delivery_unknown","Send receipt serialization failed");
        CallToolResult::structured_error(serde_json::json!({
        "schema_version":"1", "data":null, "error":{"code":"delivery_unknown",
        "message":"Telegram send failed (delivery_unknown)","retryable":false,"delivery_state":"unknown"}
    }))})
}

pub(crate) fn render_send_early(error: Error) -> CallToolResult {
    let mut error: ToolError = error.into();
    error.delivery_state = Some(crate::telegram::types::sending::FailureDeliveryState::NotSent);
    tool_result(ToolResponse::<super::sending::SendReceipt>::failure(error)).unwrap_or_else(|_| {
        render_send(Err(crate::telegram::types::sending::SendFailure::not_sent(
            crate::telegram::types::sending::SendFailureKind::InternalError,
            None,
        )))
    })
}

/// Add the shared wire-only recovery argument without changing Rust request APIs.
pub(crate) fn recoverable_schema(schema: Arc<JsonObject>) -> Arc<JsonObject> {
    let mut schema = (*schema).clone();
    let properties = schema
        .entry("properties")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(properties) = properties.as_object_mut() {
        properties.insert("recovery_key".into(), serde_json::json!({
            "description": "Optional unique caller key for targeted recovery; retained for 10 minutes after completion. Prevents accidental mixing, not authenticated isolation.",
            "anyOf": [{"type":"string", "minLength":1, "maxLength":128, "pattern":"^[ -~]+$"}, {"type":"null"}]
        }));
    }
    Arc::new(schema)
}
