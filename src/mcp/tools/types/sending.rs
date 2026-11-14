//! Strict write requests and canonical receipts. Local paths never enter receipts.

use super::serde_helpers;
use crate::telegram::types::sending::{
    ContentKind, QualifiedRecipient, RecipientSpec, SendFailure, SendFailureKind, SendMediaKind,
    SendOptions, SendText, SentMessage, UsernameNamespace,
};
use crate::telegram::types::{MessageId, PeerNamespace, Username};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Component, PathBuf};

fn invalid() -> SendFailure {
    SendFailure::not_sent(SendFailureKind::InvalidInput, None)
}

/// Explicit destination identity; unknown fields are rejected in every variant.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(inline)]
pub enum SendRecipientRequest {
    SavedMessages {},
    Username {
        username: String,
        expected_namespace: UsernameNamespace,
    },
    Peer {
        namespace: PeerNamespace,
        #[serde(deserialize_with = "serde_helpers::flexible_string")]
        #[schemars(regex(pattern = "^[0-9]+$"))]
        id: String,
    },
}

impl SendRecipientRequest {
    fn into_domain(self) -> Result<RecipientSpec, SendFailure> {
        Ok(match self {
            Self::SavedMessages {} => RecipientSpec::SavedMessages,
            Self::Username {
                username,
                expected_namespace,
            } => {
                let raw = username.strip_prefix('@').unwrap_or(&username);
                if !Username::is_valid_shape(raw) {
                    return Err(invalid());
                }
                RecipientSpec::Username {
                    username: Username::new(raw).map_err(|_| invalid())?,
                    expected_namespace,
                }
            }
            Self::Peer { namespace, id } => {
                if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(invalid());
                }
                RecipientSpec::Peer(QualifiedRecipient::try_new(
                    namespace,
                    id.parse().map_err(|_| invalid())?,
                )?)
            }
        })
    }
}

/// Send immediately as the authenticated account; no confirmation or implicit formatting.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SendMessageRequest {
    pub recipient: SendRecipientRequest,
    pub text: String,
    #[serde(default)]
    pub silent: bool,
    #[serde(default)]
    pub link_preview: bool,
    #[serde(default, deserialize_with = "serde_helpers::flexible_opt_int")]
    pub reply_to_message_id: Option<i64>,
    #[serde(default, deserialize_with = "serde_helpers::flexible_opt_int")]
    pub topic_id: Option<i64>,
}

fn options(
    recipient: SendRecipientRequest,
    reply: Option<i64>,
    topic: Option<i64>,
    silent: bool,
) -> Result<SendOptions, SendFailure> {
    let options = SendOptions {
        recipient: recipient.into_domain()?,
        reply_to_message_id: reply
            .map(MessageId::new)
            .transpose()
            .map_err(|_| invalid())?,
        topic_id: topic
            .map(MessageId::new)
            .transpose()
            .map_err(|_| invalid())?,
        silent,
    };
    options.validate()?;
    Ok(options)
}

impl SendMessageRequest {
    pub(crate) fn try_into_domain(self) -> Result<SendText, SendFailure> {
        let request = SendText {
            options: options(
                self.recipient,
                self.reply_to_message_id,
                self.topic_id,
                self.silent,
            )?,
            text: self.text,
            link_preview: self.link_preview,
        };
        request.validate()?;
        Ok(request)
    }
}

/// Alias and relative path on the connector host; never a remote URL or arbitrary URI.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
pub struct UploadSource {
    pub root: String,
    pub relative_path: PathBuf,
}

impl UploadSource {
    pub(crate) fn validate(&self) -> Result<(), SendFailure> {
        if self.root.is_empty()
            || self.root.len() > 64
            || !self
                .root
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || self.relative_path.as_os_str().is_empty()
            || !self
                .relative_path
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
            || self.relative_path.to_string_lossy().contains('\0')
        {
            return Err(SendFailure::not_sent(
                SendFailureKind::UploadSourceInvalid,
                None,
            ));
        }
        Ok(())
    }
}

/// Metadata is caller-supplied; no FFmpeg, recording or transcoding is performed.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(inline)]
pub enum SendMediaRequestKind {
    Image {},
    Audio {
        #[schemars(range(min = 1, max = 2147483647))]
        duration_seconds: i32,
        title: Option<String>,
        performer: Option<String>,
    },
    Voice {
        #[schemars(range(min = 1, max = 2147483647))]
        duration_seconds: i32,
    },
    Video {
        duration_seconds: f64,
        #[schemars(range(min = 1, max = 8192))]
        width: u32,
        #[schemars(range(min = 1, max = 8192))]
        height: u32,
    },
}

impl From<SendMediaRequestKind> for SendMediaKind {
    fn from(value: SendMediaRequestKind) -> Self {
        match value {
            SendMediaRequestKind::Image {} => Self::Image,
            SendMediaRequestKind::Audio {
                duration_seconds,
                title,
                performer,
            } => Self::Audio {
                duration_seconds,
                title,
                performer,
            },
            SendMediaRequestKind::Voice { duration_seconds } => Self::Voice { duration_seconds },
            SendMediaRequestKind::Video {
                duration_seconds,
                width,
                height,
            } => Self::Video {
                duration_seconds,
                width,
                height,
            },
        }
    }
}

/// Upload one validated local snapshot and send it as one attachment with its caption.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SendMediaRequest {
    pub recipient: SendRecipientRequest,
    pub source: UploadSource,
    pub media: SendMediaRequestKind,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub silent: bool,
    #[serde(default, deserialize_with = "serde_helpers::flexible_opt_int")]
    pub reply_to_message_id: Option<i64>,
    #[serde(default, deserialize_with = "serde_helpers::flexible_opt_int")]
    pub topic_id: Option<i64>,
}

#[expect(dead_code, reason = "Staged media consumers arrive in Task 6")]
pub(crate) struct ValidatedMediaSend {
    pub options: SendOptions,
    pub source: UploadSource,
    pub media: SendMediaKind,
    pub caption: String,
}

impl SendMediaRequest {
    pub(crate) fn try_into_domain(self) -> Result<ValidatedMediaSend, SendFailure> {
        let options = options(
            self.recipient,
            self.reply_to_message_id,
            self.topic_id,
            self.silent,
        )?;
        self.source.validate()?;
        let media: SendMediaKind = self.media.into();
        media.validate()?;
        if self.caption.encode_utf16().count() > 1024 {
            return Err(invalid());
        }
        Ok(ValidatedMediaSend {
            options,
            source: self.source,
            media,
            caption: self.caption,
        })
    }
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SentDeliveryState {
    Sent,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SentRecipient {
    pub peer_namespace: PeerNamespace,
    pub peer_id: String,
    pub saved_messages: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SubmittedMediaReceipt {
    pub mime_type: String,
    pub size_bytes: u64,
}

/// Versioned success data; contains only acknowledged identity and bounded metadata.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SendReceipt {
    pub delivery_state: SentDeliveryState,
    pub recipient: SentRecipient,
    pub message_id: i32,
    pub date: String,
    pub content_kind: ContentKind,
    pub reply_to_message_id: Option<i32>,
    pub topic_id: Option<i32>,
    pub message_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<SubmittedMediaReceipt>,
}

impl TryFrom<SentMessage> for SendReceipt {
    type Error = SendFailure;
    fn try_from(value: SentMessage) -> Result<Self, Self::Error> {
        let wire_id = |id: MessageId| id.as_i32().ok_or_else(SendFailure::unknown);
        if !(1..=i64::from(i32::MAX)).contains(&value.date.timestamp())
            || value
                .message_link
                .as_ref()
                .is_some_and(|link| link.len() > 256)
        {
            return Err(SendFailure::unknown());
        }
        Ok(Self {
            delivery_state: SentDeliveryState::Sent,
            recipient: SentRecipient {
                peer_namespace: value.recipient.namespace(),
                peer_id: value.recipient.id().to_string(),
                saved_messages: value.saved_messages,
            },
            message_id: wire_id(value.message_id)?,
            date: value
                .date
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            content_kind: value.content_kind,
            reply_to_message_id: value.reply_to_message_id.map(wire_id).transpose()?,
            topic_id: value.topic_id.map(wire_id).transpose()?,
            message_link: value.message_link,
            media: value.media.map(|m| SubmittedMediaReceipt {
                mime_type: m.mime_type,
                size_bytes: m.size_bytes,
            }),
        })
    }
}
