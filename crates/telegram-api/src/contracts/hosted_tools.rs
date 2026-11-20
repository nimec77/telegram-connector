//! Hosted channel, media, and immediate-send request contracts.

use serde::{Deserialize, Serialize};

/// Qualified destination selected for this send.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Recipient {
    SavedMessages {},
    Username {
        username: String,
        expected_namespace: UsernameNamespace,
    },
    Peer {
        namespace: PeerNamespace,
        id: String,
    },
}

#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerNamespace {
    User,
    Chat,
    Channel,
}

#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsernameNamespace {
    User,
    Channel,
}

/// Caller-supplied metadata for an already staged upload.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MediaSpec {
    Image {},
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

/// Immediately send a plain-text message to the chosen account peer.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SendMessage {
    pub recipient: Recipient,
    pub text: String,
    #[serde(default)]
    pub silent: bool,
    #[serde(default)]
    pub link_preview: bool,
    pub reply_to_message_id: Option<i64>,
    pub topic_id: Option<i64>,
}

/// Immediately send a caller-owned staged file.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SendMedia {
    pub recipient: Recipient,
    pub upload_handle: String,
    pub media: MediaSpec,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub silent: bool,
    pub reply_to_message_id: Option<i64>,
    pub topic_id: Option<i64>,
}

/// Page over the current grant's eligible channels.
#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ListChannels {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelInfo {
    pub channel_id: String,
    pub include_full: Option<bool>,
}

#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveChannels {
    pub identifiers: Vec<String>,
}

#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelStats {
    pub channel_id: String,
    pub days_back: Option<u32>,
}

#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MessageLink {
    pub channel_id: String,
    pub message_id: u32,
    pub include_tg_protocol: Option<bool>,
}

#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Transcription {
    pub timeout_seconds: Option<u32>,
}

#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverChannels {
    pub query: String,
    pub limit: Option<u32>,
}

#[cfg_attr(feature = "schemas", derive(schemars::JsonSchema), schemars(inline))]
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Previews {
    pub channel_id: String,
    pub message_ids: Vec<u32>,
    pub max_dimension: Option<u32>,
}
