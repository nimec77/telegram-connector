//! Validated outgoing identities, content and safe delivery outcomes.

use crate::telegram::types::{MessageId, PeerNamespace, Username};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A positive recipient ID qualified by its Telegram namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualifiedRecipient {
    namespace: PeerNamespace,
    id: i64,
}

impl QualifiedRecipient {
    /// Validate an identity locally, without claiming the peer exists.
    ///
    /// # Errors
    /// Rejects zero and negative IDs, including Bot API notation.
    pub fn try_new(namespace: PeerNamespace, id: i64) -> Result<Self, SendFailure> {
        if id <= 0 {
            return Err(SendFailure::not_sent(SendFailureKind::InvalidInput, None));
        }
        Ok(Self { namespace, id })
    }
    /// Telegram namespace of this identity.
    pub fn namespace(self) -> PeerNamespace {
        self.namespace
    }
    /// Positive bare ID, unique only within the namespace.
    pub fn id(self) -> i64 {
        self.id
    }
}

/// Namespaces accepted for public-username resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub enum UsernameNamespace {
    User,
    Channel,
}

impl SendOptions {
    /// Validate recipient shape and positive wire-sized thread IDs without RPCs.
    ///
    /// # Errors
    /// Rejects malformed usernames and message/topic IDs outside Telegram's i32 range.
    pub fn validate(&self) -> Result<(), SendFailure> {
        if let RecipientSpec::Username { username, .. } = &self.recipient
            && !Username::is_valid_shape(username.as_str())
        {
            return Err(SendFailure::not_sent(SendFailureKind::InvalidInput, None));
        }
        if [self.reply_to_message_id, self.topic_id]
            .into_iter()
            .flatten()
            .any(|id| id.as_i32().is_none())
        {
            return Err(SendFailure::not_sent(SendFailureKind::InvalidInput, None));
        }
        Ok(())
    }
}

impl SendText {
    /// Preserve text verbatim while rejecting blank/overlong content and bad options.
    ///
    /// # Errors
    /// Rejects blank text, more than 4096 UTF-16 units or invalid local options.
    pub fn validate(&self) -> Result<(), SendFailure> {
        self.options.validate()?;
        if self.text.trim().is_empty() || self.text.encode_utf16().count() > 4096 {
            return Err(SendFailure::not_sent(SendFailureKind::InvalidInput, None));
        }
        Ok(())
    }
}

impl SendMediaKind {
    /// Validate explicit caller metadata; does not inspect codecs or create files.
    ///
    /// # Errors
    /// Rejects nonpositive/nonfinite durations, invalid dimensions and overlong tags.
    pub fn validate(&self) -> Result<(), SendFailure> {
        let valid = match self {
            Self::Image => true,
            Self::Audio {
                duration_seconds,
                title,
                performer,
            } => {
                *duration_seconds > 0
                    && [title, performer]
                        .into_iter()
                        .flatten()
                        .all(|s| s.encode_utf16().count() <= 256)
            }
            Self::Voice { duration_seconds } => *duration_seconds > 0,
            Self::Video {
                duration_seconds,
                width,
                height,
            } => {
                duration_seconds.is_finite()
                    && *duration_seconds > 0.0
                    && (1..=8192).contains(width)
                    && (1..=8192).contains(height)
            }
        };
        if !valid {
            return Err(SendFailure::not_sent(SendFailureKind::InvalidInput, None));
        }
        Ok(())
    }
}

/// A locally validated reference, resolved exactly once per send.
#[derive(Debug, Clone)]
pub enum RecipientSpec {
    SavedMessages,
    Username {
        username: Username,
        expected_namespace: UsernameNamespace,
    },
    Peer(QualifiedRecipient),
}

/// Recipient and thread/delivery options shared by all outgoing content.
#[derive(Debug, Clone)]
pub struct SendOptions {
    pub recipient: RecipientSpec,
    pub reply_to_message_id: Option<MessageId>,
    pub topic_id: Option<MessageId>,
    pub silent: bool,
}

/// A plain text send. Operation boundaries revalidate content and wire IDs.
#[derive(Clone)]
pub struct SendText {
    pub options: SendOptions,
    pub text: String,
    pub link_preview: bool,
}

impl std::fmt::Debug for SendText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SendText").finish_non_exhaustive()
    }
}

/// Caller-supplied media attributes; no codec discovery or transcoding.
#[derive(Debug, Clone)]
pub enum SendMediaKind {
    Image,
    Audio {
        duration_seconds: i32,
        title: Option<String>,
        performer: Option<String>,
    },
    Voice {
        duration_seconds: i32,
    },
    Video {
        duration_seconds: f64,
        width: u32,
        height: u32,
    },
}

impl SendMediaKind {
    /// Content label for the requested Telegram media representation.
    pub fn content_kind(&self) -> ContentKind {
        match self {
            Self::Image => ContentKind::Image,
            Self::Audio { .. } => ContentKind::Audio,
            Self::Voice { .. } => ContentKind::Voice,
            Self::Video { .. } => ContentKind::Video,
        }
    }
}

/// Media options plus an opaque validated local snapshot; never an arbitrary path.
#[derive(Clone)]
pub struct PreparedMediaSend {
    pub options: SendOptions,
    pub caption: String,
    pub media: SendMediaKind,
    pub staged: std::sync::Arc<crate::sending::StagedUpload>,
}
impl std::fmt::Debug for PreparedMediaSend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedMediaSend").finish_non_exhaustive()
    }
}
impl PreparedMediaSend {
    /// Revalidate public Rust fields before any recipient/upload RPC.
    ///
    /// # Errors
    /// Rejects invalid options, metadata, captions or media/snapshot type mismatch.
    pub fn validate(&self) -> Result<(), SendFailure> {
        self.options.validate()?;
        self.media.validate()?;
        let matches = match self.media {
            SendMediaKind::Image => matches!(self.staged.mime_type(), "image/jpeg" | "image/png"),
            SendMediaKind::Audio { .. } => matches!(
                self.staged.mime_type(),
                "audio/mpeg" | "audio/mp4" | "audio/aac"
            ),
            SendMediaKind::Voice { .. } => self.staged.mime_type() == "audio/ogg",
            SendMediaKind::Video { .. } => self.staged.mime_type() == "video/mp4",
        };
        if !matches || self.caption.encode_utf16().count() > 1024 {
            return Err(SendFailure::not_sent(SendFailureKind::InvalidInput, None));
        }
        Ok(())
    }
}

/// Content submitted in one acknowledged message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Text,
    Image,
    Audio,
    Voice,
    Video,
}

/// Facts about the submitted snapshot, not Telegram's converted attachment.
#[derive(Debug, Clone)]
pub struct SubmittedMedia {
    pub mime_type: String,
    pub size_bytes: u64,
}

/// A positively matched Telegram acknowledgement, without outgoing content.
#[derive(Debug, Clone)]
pub struct SentMessage {
    pub recipient: QualifiedRecipient,
    pub saved_messages: bool,
    pub message_id: MessageId,
    pub date: DateTime<Utc>,
    pub content_kind: ContentKind,
    pub reply_to_message_id: Option<MessageId>,
    pub topic_id: Option<MessageId>,
    pub message_link: Option<String>,
    pub media: Option<SubmittedMedia>,
}

/// Failure delivery truth for this invocation; never implies an earlier call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FailureDeliveryState {
    NotSent,
    Unknown,
}

/// Safe categories; raw provider text and errors are discarded at the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendFailureKind {
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
    InvalidInput,
    MediaTooLarge,
    RateLimited,
    Unauthenticated,
    Timeout,
    ResponseTooLarge,
    Unavailable,
    InternalError,
}

impl SendFailureKind {
    /// Fixed diagnostic identifier without provider details.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SendingDisabled => "sending_disabled",
            Self::RecipientNotFound => "recipient_not_found",
            Self::RecipientMismatch => "recipient_mismatch",
            Self::SendForbidden => "send_forbidden",
            Self::InvalidReplyTarget => "invalid_reply_target",
            Self::UploadSourceInvalid => "upload_source_invalid",
            Self::UnsupportedMedia => "unsupported_media",
            Self::UploadFailed => "upload_failed",
            Self::SendCapacityReached => "send_capacity_reached",
            Self::DeliveryUnknown => "delivery_unknown",
            Self::InvalidInput => "invalid_input",
            Self::MediaTooLarge => "media_too_large",
            Self::RateLimited => "rate_limited",
            Self::Unauthenticated => "unauthenticated",
            Self::Timeout => "timeout",
            Self::ResponseTooLarge => "response_too_large",
            Self::Unavailable => "unavailable",
            Self::InternalError => "internal_error",
        }
    }
}

/// Redacted failure whose delivery state cannot be made retryable when unknown.
#[derive(Debug, Clone, Copy)]
pub struct SendFailure {
    kind: SendFailureKind,
    retry_after_seconds: Option<u64>,
}

impl SendFailure {
    /// Construct a pre-dispatch failure or allowlisted definitive refusal.
    /// Unknown categories are canonicalized to unknown, discarding retry hints.
    pub fn not_sent(kind: SendFailureKind, retry_after_seconds: Option<u64>) -> Self {
        if kind == SendFailureKind::DeliveryUnknown {
            return Self::unknown();
        }
        Self {
            kind,
            retry_after_seconds: if kind == SendFailureKind::RateLimited {
                retry_after_seconds
            } else {
                None
            },
        }
    }
    /// Acceptance cannot be established or ruled out; never safe to resend automatically.
    pub fn unknown() -> Self {
        Self {
            kind: SendFailureKind::DeliveryUnknown,
            retry_after_seconds: None,
        }
    }
    /// Safe failure category.
    pub fn kind(self) -> SendFailureKind {
        self.kind
    }
    /// Whether this invocation might have committed.
    pub fn delivery_state(self) -> FailureDeliveryState {
        if self.kind == SendFailureKind::DeliveryUnknown {
            FailureDeliveryState::Unknown
        } else {
            FailureDeliveryState::NotSent
        }
    }
    /// Only known pre-dispatch transient categories permit caller retry.
    pub fn retryable(self) -> bool {
        matches!(
            self.kind,
            SendFailureKind::Timeout
                | SendFailureKind::RateLimited
                | SendFailureKind::SendCapacityReached
        )
    }
    /// Definitive provider/bucket wait hint, never a request to sleep and resend.
    pub fn retry_after_seconds(self) -> Option<u64> {
        self.retry_after_seconds
    }
}

impl std::fmt::Display for SendFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Telegram send failed ({})", self.kind.as_str())
    }
}
impl std::error::Error for SendFailure {}
