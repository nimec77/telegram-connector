//! Concrete canonical DTOs. Domain/legacy serializers remain unchanged.
use super::contract::{ToolError, WireData, legacy_json};
use super::responses;
use crate::error::Error;
use crate::telegram::types::{self, PeerNamespace};
use chrono::{DateTime, Utc};
use rmcp::model::{CallToolResult, ContentBlock};
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct ChannelId(#[schemars(regex(pattern = "^[1-9][0-9]*$"))] String);
impl From<types::ChannelId> for ChannelId {
    fn from(id: types::ChannelId) -> Self {
        Self(id.to_string())
    }
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct UserId(#[schemars(regex(pattern = "^[1-9][0-9]*$"))] String);
impl From<types::UserId> for UserId {
    fn from(id: types::UserId) -> Self {
        Self(id.to_string())
    }
}
use super::responses::{BufferedResponseEntry, MediaLimits, NextCursor, RateLimiterStatus};
use crate::telegram::types::{
    AlbumInfo, AudioInfo, ChannelName, ChatType, DocumentInfo, LinkPreview, MediaType, MessageId,
    MessageReaction, PollInfo, QueryMetadata, StatsSample, Username, VideoInfo,
};
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ChannelData {
    pub peer_namespace: PeerNamespace,
    pub id: ChannelId,
    pub name: ChannelName,
    pub username: Option<Username>,
    pub chat_type: ChatType,
    pub description: Option<String>,
    pub member_count: Option<u64>,
    pub is_verified: bool,
    pub is_public: bool,
    pub is_subscribed: bool,
    pub last_message_date: Option<DateTime<Utc>>,
}
impl TryFrom<types::Channel> for ChannelData {
    type Error = Error;
    fn try_from(value: types::Channel) -> Result<Self, Error> {
        Ok(Self {
            peer_namespace: value
                .peer_namespace
                .ok_or(Error::Internal("channel namespace unavailable"))?,
            id: value.id.into(),
            name: value.name,
            username: value.username,
            chat_type: value.chat_type,
            description: value.description,
            member_count: value.member_count,
            is_verified: value.is_verified,
            is_public: value.is_public,
            is_subscribed: value.is_subscribed,
            last_message_date: value.last_message_date,
        })
    }
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ForwardData {
    pub channel_id: Option<ChannelId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_name: Option<ChannelName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_username: Option<Username>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_date: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_message_id: Option<MessageId>,
}
impl TryFrom<types::ForwardInfo> for ForwardData {
    type Error = Error;
    fn try_from(value: types::ForwardInfo) -> Result<Self, Error> {
        Ok(Self {
            channel_id: value.channel_id.map(Into::into),
            channel_name: value.channel_name,
            channel_username: value.channel_username,
            sender_name: value.sender_name,
            post_author: value.post_author,
            original_date: value.original_date,
            original_message_id: value.original_message_id,
        })
    }
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MessageData {
    pub peer_namespace: PeerNamespace,
    pub id: MessageId,
    pub channel_id: ChannelId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_name: Option<ChannelName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_username: Option<Username>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_full_length: Option<u64>,
    pub timestamp: DateTime<Utc>,
    pub sender_id: Option<UserId>,
    pub sender_name: Option<String>,
    pub has_media: bool,
    pub media_type: MediaType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forwarded_from: Option<ForwardData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_preview: Option<LinkPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub views: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forwards: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<MessageId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_info: Option<VideoInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_info: Option<AudioInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_info: Option<DocumentInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poll_info: Option<PollInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grouped_id: Option<i64>,
    pub link: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reactions: Option<Vec<MessageReaction>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reactions_total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<AlbumInfo>,
}
impl TryFrom<responses::MessageResponse> for MessageData {
    type Error = Error;
    fn try_from(value: responses::MessageResponse) -> Result<Self, Error> {
        Ok(Self {
            peer_namespace: value
                .peer_namespace
                .ok_or(Error::Internal("message namespace unavailable"))?,
            id: value.id,
            channel_id: value
                .channel_id
                .ok_or(Error::Internal("message channel unavailable"))?
                .into(),
            channel_name: value.channel_name,
            channel_username: value.channel_username,
            text: value.text,
            text_truncated: value.text_truncated,
            text_full_length: value.text_full_length,
            timestamp: value.timestamp,
            sender_id: value.sender_id.map(Into::into),
            sender_name: value.sender_name,
            has_media: value.has_media,
            media_type: value.media_type,
            forwarded_from: value
                .forwarded_from
                .map(ForwardData::try_from)
                .transpose()?,
            link_preview: value.link_preview,
            views: value.views,
            forwards: value.forwards,
            reply_to_message_id: value.reply_to_message_id,
            video_info: value.video_info,
            audio_info: value.audio_info,
            document_info: value.document_info,
            poll_info: value.poll_info,
            grouped_id: value.grouped_id,
            link: value.link,
            reactions: value.reactions,
            reactions_total: value.reactions_total,
            album: value.album,
        })
    }
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HeaderData {
    pub peer_namespace: PeerNamespace,
    pub id: ChannelId,
    pub name: ChannelName,
    pub username: Option<Username>,
}
impl TryFrom<responses::ChannelHeader> for HeaderData {
    type Error = Error;
    fn try_from(value: responses::ChannelHeader) -> Result<Self, Error> {
        Ok(Self {
            peer_namespace: value
                .peer_namespace
                .ok_or(Error::Internal("channel namespace unavailable"))?,
            id: value.id.into(),
            name: value.name,
            username: value.username,
        })
    }
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StatsData {
    pub channel_id: ChannelId,
    pub post_count: u64,
    pub posts_per_day: f64,
    pub median_views: Option<u64>,
    pub media_share: f64,
    pub album_share: f64,
    pub sample: StatsSample,
}
impl TryFrom<types::ChannelStats> for StatsData {
    type Error = Error;
    fn try_from(value: types::ChannelStats) -> Result<Self, Error> {
        Ok(Self {
            channel_id: value.channel_id.into(),
            post_count: value.post_count,
            posts_per_day: value.posts_per_day,
            median_views: value.median_views,
            media_share: value.media_share,
            album_share: value.album_share,
            sample: value.sample,
        })
    }
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StatusData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sending: Option<responses::SendingStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_artifacts: Option<responses::MediaArtifactsStatus>,
    pub telegram_connected: bool,
    pub rate_limiter: RateLimiterStatus,
    pub media: MediaLimits,
    pub server_version: String,
    pub requests_received: u64,
    pub responses_written: u64,
    pub last_response_write_age_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_response_write_unavailable: Option<String>,
    pub session_started_at: DateTime<Utc>,
    pub session_uptime_secs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub premium: Option<bool>,
}
impl TryFrom<responses::StatusResponse> for StatusData {
    type Error = Error;
    fn try_from(value: responses::StatusResponse) -> Result<Self, Error> {
        Ok(Self {
            sending: value.sending,
            media_artifacts: value.media_artifacts,
            telegram_connected: value.telegram_connected,
            rate_limiter: value.rate_limiter,
            media: value.media,
            server_version: value.server_version,
            requests_received: value.requests_received,
            responses_written: value.responses_written,
            last_response_write_age_secs: value.last_response_write_age_secs,
            last_response_write_unavailable: value.last_response_write_unavailable,
            session_started_at: value
                .session_started_at
                .parse()
                .map_err(|_| Error::Internal("invalid session timestamp"))?,
            session_uptime_secs: value.session_uptime_secs,
            premium: value.premium,
        })
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ChannelsData {
    pub channels: Vec<ChannelData>,
    pub returned: usize,
    pub total: Option<usize>,
    pub has_more: Option<bool>,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct ChannelFailure {
    pub channel: String,
    pub error: ToolError,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<HeaderData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<std::collections::BTreeMap<String, HeaderData>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_errors: Option<Vec<ChannelFailure>>,
    pub messages: Vec<MessageData>,
    pub returned: u64,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<NextCursor>,
    pub search_time_ms: u64,
    pub query_metadata: QueryMetadata,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct MissingMessage {
    pub id: MessageId,
    pub error: ToolError,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct BatchData {
    pub channel_id: ChannelId,
    pub peer_namespace: PeerNamespace,
    pub messages: Vec<MessageData>,
    pub returned: usize,
    pub missing: Vec<MissingMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted_ids: Option<Vec<MessageId>>,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Resolution {
    Success {
        identifier: String,
        channel: ChannelData,
    },
    Failure {
        identifier: String,
        error: ToolError,
    },
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct ResolveData {
    pub resolutions: Vec<Resolution>,
    pub returned: usize,
    pub resolved: usize,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum RecoveryEntry {
    Response {
        #[serde(skip_serializing_if = "Option::is_none")]
        recovery_key: Option<String>,
        request_id: String,
        tool_name: String,
        written_at: DateTime<Utc>,
        size_bytes: usize,
        response: serde_json::Value,
    },
    Failure {
        recovery_key: String,
        error: ToolError,
    },
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RecoveryData {
    pub responses: Vec<RecoveryEntry>,
    pub buffered: usize,
}
impl WireData for types::Channel {
    type Canonical = ChannelData;
    fn canonical(self) -> Result<(ChannelData, Vec<ContentBlock>), Error> {
        Ok((ChannelData::try_from(self)?, Vec::new()))
    }
    fn legacy(self) -> Result<CallToolResult, Error> {
        legacy_json(&self)
    }
}
impl WireData for responses::MessageResponse {
    type Canonical = MessageData;
    fn canonical(self) -> Result<(MessageData, Vec<ContentBlock>), Error> {
        Ok((MessageData::try_from(self)?, Vec::new()))
    }
    fn legacy(self) -> Result<CallToolResult, Error> {
        legacy_json(&self)
    }
}
impl WireData for types::ChannelStats {
    type Canonical = StatsData;
    fn canonical(self) -> Result<(StatsData, Vec<ContentBlock>), Error> {
        Ok((StatsData::try_from(self)?, Vec::new()))
    }
    fn legacy(self) -> Result<CallToolResult, Error> {
        legacy_json(&self)
    }
}
impl WireData for responses::StatusResponse {
    type Canonical = StatusData;
    fn canonical(self) -> Result<(StatusData, Vec<ContentBlock>), Error> {
        Ok((StatusData::try_from(self)?, Vec::new()))
    }
    fn legacy(self) -> Result<CallToolResult, Error> {
        legacy_json(&self)
    }
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkData {
    pub channel_id: ChannelId,
    pub peer_namespace: PeerNamespace,
    pub message_id: i64,
    pub https_link: String,
    pub tg_protocol_link: Option<String>,
    pub internal_link: String,
    pub is_public: bool,
}
impl WireData for responses::MessageLinkResponse {
    type Canonical = LinkData;
    fn canonical(self) -> Result<(LinkData, Vec<ContentBlock>), Error> {
        let (id, peer_namespace) = self
            .resolved_channel
            .ok_or(Error::Internal("link identity unavailable"))?;
        Ok((
            LinkData {
                channel_id: id.into(),
                peer_namespace,
                message_id: self.message_id,
                https_link: self.https_link,
                tg_protocol_link: self.tg_protocol_link,
                internal_link: self.internal_link,
                is_public: self.is_public,
            },
            Vec::new(),
        ))
    }
    fn legacy(self) -> Result<CallToolResult, Error> {
        legacy_json(&self)
    }
}
impl WireData for responses::OpenMessageResponse {
    type Canonical = Self;
    fn canonical(self) -> Result<(Self, Vec<ContentBlock>), Error> {
        Ok((self, Vec::new()))
    }
    fn legacy(self) -> Result<CallToolResult, Error> {
        legacy_json(&self)
    }
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct TranscriptionData {
    pub channel_id: ChannelId,
    pub peer_namespace: PeerNamespace,
    pub message_id: MessageId,
    pub text: String,
    pub partial: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u32>,
    pub media_type: MediaType,
}
impl WireData for responses::TranscribeVoiceMessageResponse {
    type Canonical = TranscriptionData;
    fn canonical(self) -> Result<(TranscriptionData, Vec<ContentBlock>), Error> {
        let (id, peer_namespace, message_id) = self
            .target
            .ok_or(Error::Internal("transcription identity unavailable"))?;
        Ok((
            TranscriptionData {
                channel_id: id.into(),
                peer_namespace,
                message_id,
                text: self.text,
                partial: self.partial,
                duration_seconds: self.duration_seconds,
                media_type: self.media_type,
            },
            Vec::new(),
        ))
    }
    fn legacy(self) -> Result<CallToolResult, Error> {
        legacy_json(&self)
    }
}
impl WireData for responses::ChannelsResponse {
    type Canonical = ChannelsData;
    fn canonical(self) -> Result<(ChannelsData, Vec<ContentBlock>), Error> {
        Ok((
            ChannelsData {
                channels: self
                    .channels
                    .into_iter()
                    .map(ChannelData::try_from)
                    .collect::<Result<_, _>>()?,
                returned: self.returned,
                total: self.total,
                has_more: self.has_more,
            },
            Vec::new(),
        ))
    }
    fn legacy(self) -> Result<CallToolResult, Error> {
        legacy_json(&self)
    }
}
impl WireData for responses::SearchResponse<Error> {
    type Canonical = SearchData;
    fn canonical(mut self) -> Result<(SearchData, Vec<ContentBlock>), Error> {
        for message in &mut self.messages {
            if message.channel_id.is_none() {
                let header = self
                    .channel
                    .as_ref()
                    .ok_or(Error::Internal("compact channel unavailable"))?;
                message.channel_id = Some(header.id);
            }
            if let Some(headers) = &self.channels {
                let id = message
                    .channel_id
                    .ok_or(Error::Internal("channel unavailable"))?;
                let header = headers
                    .get(&id.to_string())
                    .ok_or(Error::Internal("compact header unavailable"))?;
                if header.peer_namespace != message.peer_namespace {
                    return Err(Error::AmbiguousChannel(id.to_string()));
                }
            }
        }
        self.query_metadata.channels_in_results = self
            .messages
            .iter()
            .map(|m| (m.peer_namespace, m.channel_id))
            .collect::<std::collections::HashSet<_>>()
            .len() as u32;
        let data = SearchData {
            channel: self.channel.map(HeaderData::try_from).transpose()?,
            channels: self
                .channels
                .map(|headers| {
                    headers
                        .into_iter()
                        .map(|(key, value)| Ok((key, HeaderData::try_from(value)?)))
                        .collect::<Result<_, Error>>()
                })
                .transpose()?,
            channel_errors: self.channel_errors.map(|errors| {
                errors
                    .into_iter()
                    .map(|e| ChannelFailure {
                        channel: e.channel,
                        error: e.error.into(),
                    })
                    .collect()
            }),
            messages: self
                .messages
                .into_iter()
                .map(MessageData::try_from)
                .collect::<Result<_, _>>()?,
            returned: self.returned,
            has_more: self.has_more,
            next_cursor: self.next_cursor,
            search_time_ms: self.search_time_ms,
            query_metadata: self.query_metadata,
        };
        Ok((data, Vec::new()))
    }
    fn legacy(self) -> Result<CallToolResult, Error> {
        legacy_json(&self)
    }
}
impl WireData for responses::MessagesBatchResponse {
    type Canonical = BatchData;
    fn canonical(self) -> Result<(BatchData, Vec<ContentBlock>), Error> {
        let (id, peer_namespace) = self
            .resolved_channel
            .ok_or(Error::Internal("batch identity unavailable"))?;
        let missing = self
            .missing
            .into_iter()
            .map(|m| MissingMessage {
                id: m.id,
                error: Error::MessageNotFound {
                    channel: id.to_string(),
                    message_id: m.id.get(),
                }
                .into(),
            })
            .collect();
        Ok((
            BatchData {
                channel_id: id.into(),
                peer_namespace,
                messages: self
                    .messages
                    .into_iter()
                    .map(MessageData::try_from)
                    .collect::<Result<_, _>>()?,
                returned: self.returned,
                missing,
                omitted_ids: self.omitted_ids,
            },
            Vec::new(),
        ))
    }
    fn legacy(self) -> Result<CallToolResult, Error> {
        legacy_json(&self)
    }
}
impl WireData for responses::ResolveChannelsResponse<Error> {
    type Canonical = ResolveData;
    fn canonical(self) -> Result<(ResolveData, Vec<ContentBlock>), Error> {
        let resolutions = self
            .resolutions
            .into_iter()
            .map(|r| match (r.channel, r.error) {
                (Some(channel), None) => Ok(Resolution::Success {
                    identifier: r.identifier,
                    channel: ChannelData::try_from(channel)?,
                }),
                (None, Some(error)) => Ok(Resolution::Failure {
                    identifier: r.identifier,
                    error: error.into(),
                }),
                _ => Err(Error::Internal("invalid resolution outcome")),
            })
            .collect::<Result<_, _>>()?;
        Ok((
            ResolveData {
                resolutions,
                returned: self.returned,
                resolved: self.resolved,
            },
            Vec::new(),
        ))
    }
    fn legacy(self) -> Result<CallToolResult, Error> {
        legacy_json(&self)
    }
}
impl WireData for responses::LastResponsesResponse {
    type Canonical = RecoveryData;
    fn canonical(self) -> Result<(RecoveryData, Vec<ContentBlock>), Error> {
        if let Some(responses) = self.keyed {
            return Ok((
                RecoveryData {
                    responses,
                    buffered: self.buffered,
                },
                Vec::new(),
            ));
        }
        let responses = self
            .responses
            .into_iter()
            .map(|e: BufferedResponseEntry| {
                Ok(RecoveryEntry::Response {
                    recovery_key: None,
                    request_id: e.request_id,
                    tool_name: e.tool_name,
                    written_at: e
                        .written_at
                        .parse()
                        .map_err(|_| Error::Internal("invalid replay timestamp"))?,
                    size_bytes: e.size_bytes,
                    response: e.response,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok((
            RecoveryData {
                responses,
                buffered: self.buffered,
            },
            Vec::new(),
        ))
    }
    fn legacy(self) -> Result<CallToolResult, Error> {
        if let Some(responses) = self.keyed {
            return legacy_json(&RecoveryData {
                responses,
                buffered: self.buffered,
            });
        }
        legacy_json(&self)
    }
}
