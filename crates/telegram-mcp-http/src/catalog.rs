//! Tool descriptions and their independent HTTP operations.

use crate::delegation::{Failure, Gateway};
use reqwest::RequestBuilder;
use rmcp::handler::server::common::{schema_for_output, schema_for_type};
use rmcp::model::{Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::sync::Arc;
use telegram_api::contracts::hosted_tools::{
    ChannelInfo, ChannelStats, DiscoverChannels, ListChannels, MessageLink, Previews,
    ResolveChannels, SendMedia, SendMessage, Transcription,
};
use telegram_api::contracts::{
    BatchRequest, BatchResponse, HistoryQuery, HistoryResponse, Scope, SearchRequest,
    SearchResponse,
};

pub(crate) const TOOLS: &[&str] = &[
    "search_messages",
    "get_recent_messages",
    "get_messages_batch",
    "get_subscribed_channels",
    "get_channel_info",
    "resolve_channels",
    "get_channel_stats",
    "generate_message_link",
    "transcribe_voice_message",
    "get_message_media",
    "get_messages_media_batch",
    "search_public_channels",
    "send_message",
    "send_media",
];

pub(crate) fn scope(name: &str) -> Scope {
    if matches!(name, "send_message" | "send_media") {
        Scope::Send
    } else {
        Scope::Read
    }
}

pub(crate) fn is_preview(name: &str) -> bool {
    matches!(name, "get_message_media" | "get_messages_media_batch")
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct History {
    channel_id: String,
    #[serde(flatten)]
    query: HistoryQuery,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Preview {
    channel_id: String,
    message_id: u32,
    max_dimension: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Transcribe {
    channel_id: String,
    message_id: u32,
    timeout_seconds: Option<u32>,
}

pub(crate) fn tool(name: &str) -> Option<Tool> {
    let name = *TOOLS.iter().find(|candidate| **candidate == name)?;
    let (description, input) = match name {
        "search_messages" => (
            "Search messages in explicitly selected channels",
            schema_for_type::<SearchRequest>(),
        ),
        "get_recent_messages" => (
            "Read a descending page of messages from one channel",
            schema_for_type::<History>(),
        ),
        "get_messages_batch" => (
            "Read a batch of message IDs from one channel",
            schema_for_type::<BatchRequest>(),
        ),
        "get_subscribed_channels" => (
            "List eligible channels granted to this client",
            schema_for_type::<ListChannels>(),
        ),
        "get_channel_info" => ("Read channel information", schema_for_type::<ChannelInfo>()),
        "resolve_channels" => (
            "Resolve identifiers within the channel grant",
            schema_for_type::<ResolveChannels>(),
        ),
        "get_channel_stats" => (
            "Summarize activity within a channel",
            schema_for_type::<ChannelStats>(),
        ),
        "generate_message_link" => (
            "Generate links for a channel message",
            schema_for_type::<MessageLink>(),
        ),
        "transcribe_voice_message" => (
            "Transcribe a voice message or video note",
            schema_for_type::<Transcribe>(),
        ),
        "get_message_media" => (
            "Create a caller-bound JPEG preview",
            schema_for_type::<Preview>(),
        ),
        "get_messages_media_batch" => (
            "Create caller-bound message previews",
            schema_for_type::<Previews>(),
        ),
        "search_public_channels" => (
            "Search the public Telegram channel directory",
            schema_for_type::<DiscoverChannels>(),
        ),
        "send_message" => (
            "Immediately send plain text to the selected Telegram recipient",
            schema_for_type::<SendMessage>(),
        ),
        "send_media" => (
            "Immediately send a staged upload to the selected Telegram recipient",
            schema_for_type::<SendMedia>(),
        ),
        _ => return None,
    };
    let data = match name {
        "search_messages" => Value::Object((*schema_for_output::<SearchResponse>()).clone()),
        "get_recent_messages" => Value::Object((*schema_for_output::<HistoryResponse>()).clone()),
        "get_messages_batch" => Value::Object((*schema_for_output::<BatchResponse>()).clone()),
        _ => json!({"type":"object"}),
    };
    let schema = json!({
        "type":"object",
        "oneOf":[
            {"required":["schema_version","data","error"],"properties":{"schema_version":{"const":"1"},"data":data,"error":{"type":"null"}}},
            {"required":["schema_version","data","error"],"properties":{"schema_version":{"const":"1"},"data":{"type":"null"},"error":{
                "type":"object","required":["code","message","retryable"],
                "properties":{"code":{"type":"string"},"message":{"type":"string"},"retryable":{"type":"boolean"},
                    "delivery_state":{"enum":["unknown","not_sent"]},"retry_after_seconds":{"type":"integer","minimum":0}}
            }}}
        ]
    });
    let mut result = Tool::new(name, description, input);
    result.output_schema = Some(Arc::new(schema.as_object()?.clone()));
    let write = scope(name) == Scope::Send;
    result.annotations = Some(
        ToolAnnotations::default()
            .read_only(!write)
            .destructive(write)
            .idempotent(!write),
    );
    Some(result)
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, Failure> {
    serde_json::from_value(value).map_err(|_| Failure::INVALID)
}

fn channel(id: &str) -> Result<&str, Failure> {
    if !id.starts_with('0')
        && id.bytes().all(|b| b.is_ascii_digit())
        && id.parse::<i64>().is_ok_and(|i| i > 0)
    {
        Ok(id)
    } else {
        Err(Failure::INVALID)
    }
}

pub(crate) fn handle(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

impl Gateway {
    pub(crate) fn operation(&self, name: &str, args: Value) -> Result<RequestBuilder, Failure> {
        let endpoint = |path: &str| self.api.join(path).map_err(|_| Failure::UNAVAILABLE);
        let channel_url =
            |id: &str, suffix: &str| endpoint(&format!("v1/channels/{}{suffix}", channel(id)?));
        let message_url = |id: &str, message: u32, suffix: &str| {
            if message == 0 || message > i32::MAX as u32 {
                return Err(Failure::INVALID);
            }
            channel_url(id, &format!("/messages/{message}/{suffix}"))
        };
        let call = match name {
            "search_messages" => {
                let q: SearchRequest = decode(args)?;
                if q.channel_ids.is_empty()
                    || q.channel_ids.iter().any(|id| channel(id).is_err())
                    || (q.query.trim().is_empty() && q.media_filter.is_none())
                {
                    return Err(Failure::INVALID);
                }
                self.http.post(endpoint("v1/messages/search")?).json(&q)
            }
            "get_recent_messages" => {
                let q: History = decode(args)?;
                self.http
                    .get(channel_url(&q.channel_id, "/messages")?)
                    .query(&q.query)
            }
            "get_messages_batch" => {
                let q: BatchRequest = decode(args)?;
                channel(&q.channel_id)?;
                self.http.post(endpoint("v1/messages/batch")?).json(&q)
            }
            "get_subscribed_channels" => {
                self.http
                    .get(endpoint("v1/channels")?)
                    .query(&decode::<ListChannels>(args)?)
            }
            "get_channel_info" => {
                let q: ChannelInfo = decode(args)?;
                self.http
                    .get(channel_url(&q.channel_id, "")?)
                    .query(&[("include_full", q.include_full)])
            }
            "resolve_channels" => self
                .http
                .post(endpoint("v1/channels/resolve")?)
                .json(&decode::<ResolveChannels>(args)?),
            "get_channel_stats" => {
                let q: ChannelStats = decode(args)?;
                self.http
                    .get(channel_url(&q.channel_id, "/stats")?)
                    .query(&[("days_back", q.days_back)])
            }
            "generate_message_link" => {
                let q: MessageLink = decode(args)?;
                self.http
                    .get(message_url(&q.channel_id, q.message_id, "link")?)
                    .query(&[("include_tg_protocol", q.include_tg_protocol)])
            }
            "transcribe_voice_message" => {
                let q: Transcribe = decode(args)?;
                self.http
                    .post(message_url(&q.channel_id, q.message_id, "transcription")?)
                    .json(&Transcription {
                        timeout_seconds: q.timeout_seconds,
                    })
            }
            "search_public_channels" => self
                .http
                .post(endpoint("v1/channels/discover")?)
                .json(&decode::<DiscoverChannels>(args)?),
            "get_message_media" | "get_messages_media_batch" => {
                let q: Previews = if name == "get_message_media" {
                    let single: Preview = decode(args)?;
                    Previews {
                        channel_id: single.channel_id,
                        message_ids: vec![single.message_id],
                        max_dimension: single.max_dimension,
                    }
                } else {
                    decode(args)?
                };
                channel(&q.channel_id)?;
                if !(1..=10).contains(&q.message_ids.len()) {
                    return Err(Failure::INVALID);
                }
                self.http.post(endpoint("v1/media/previews")?).json(&q)
            }
            "send_message" => self
                .http
                .post(endpoint("v1/messages/send")?)
                .json(&decode::<SendMessage>(args)?),
            "send_media" => {
                let q: SendMedia = decode(args)?;
                if !handle(&q.upload_handle) {
                    return Err(Failure::INVALID);
                }
                self.http.post(endpoint("v1/messages/send-media")?).json(&q)
            }
            _ => return Err(Failure::INVALID),
        };
        Ok(call)
    }
}
