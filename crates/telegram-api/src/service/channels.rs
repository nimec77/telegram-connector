//! Hosted channel metadata, discovery and transcription routes.

use super::authorization::ReadAccess;
use super::http_error::from_telegram;
use super::{ApiError, ServiceState};
use crate::contracts::ErrorCode;
use crate::contracts::hosted_tools::{
    DiscoverChannels, ListChannels, ResolveChannels, Transcription,
};
use axum::Json;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use telegram_core::telegram::types::{Channel, ChannelId, MessageId};

type App = State<Arc<ServiceState>>;
type QueryInput<T> = Result<Query<T>, QueryRejection>;
type Body<T> = Result<Json<T>, JsonRejection>;
type Reply = Result<Json<Value>, ApiError>;

pub(super) async fn list(
    State(state): App,
    headers: HeaderMap,
    params: QueryInput<ListChannels>,
) -> Reply {
    let Query(params) = params.map_err(|_| invalid())?;
    let limit = limit(params.limit, 100)?;
    let access = ReadAccess::authorize(&state, &headers).await?;
    if !access.any_reads() {
        access.revalidate().await?;
        return Ok(Json(json!({"channels":[],"total":0})));
    }
    let lease = access.lease().await?;
    let page = lease
        .client()
        .operations
        .list_channels(
            limit,
            params.offset.unwrap_or(0),
            |channel| permitted(&access, channel),
            &access.context(&lease),
        )
        .await
        .map_err(from_telegram)?;
    access.revalidate().await?;
    Ok(Json(
        json!({"channels": page.channels.into_iter().map(channel_json).collect::<Vec<_>>(), "total":page.total}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InfoQuery {
    include_full: Option<bool>,
}

pub(super) async fn info(
    State(state): App,
    headers: HeaderMap,
    Path(id): Path<String>,
    params: QueryInput<InfoQuery>,
) -> Reply {
    let Query(params) = params.map_err(|_| invalid())?;
    let (access, channel) = channel_access(&state, &headers, &id).await?;
    let lease = access.lease().await?;
    let info = lease
        .client()
        .operations
        .channel_info(
            channel,
            params.include_full.unwrap_or(false),
            &access.context(&lease),
        )
        .await
        .map_err(from_telegram)?;
    access.revalidate().await?;
    Ok(Json(channel_json(info)))
}

pub(super) async fn resolve(
    State(state): App,
    headers: HeaderMap,
    body: Body<ResolveChannels>,
) -> Reply {
    let Json(body) = body.map_err(super::json_error)?;
    if !(1..=50).contains(&body.identifiers.len())
        || body
            .identifiers
            .iter()
            .any(|id| id.trim().is_empty() || id.len() > 256)
    {
        return Err(invalid());
    }
    let access = ReadAccess::authorize(&state, &headers).await?;
    let allowed = if access.any_reads() {
        let lease = access.lease().await?;
        lease
            .client()
            .operations
            .list_channels(
                10_000,
                0,
                |channel| permitted(&access, channel),
                &access.context(&lease),
            )
            .await
            .map_err(from_telegram)?
            .channels
    } else {
        Vec::new()
    };
    let items = body
        .identifiers
        .iter()
        .map(|reference| {
            let mut matches = allowed
                .iter()
                .filter(|channel| matches_ref(channel, reference));
            match (matches.next(), matches.next()) {
                (Some(channel), None) => {
                    json!({"identifier": reference, "channel": channel_json(channel.clone())})
                }
                _ => json!({"identifier": reference, "error": {"code":"unavailable_resource"}}),
            }
        })
        .collect::<Vec<_>>();
    access.revalidate().await?;
    Ok(Json(json!({"items":items})))
}

fn matches_ref(channel: &Channel, reference: &str) -> bool {
    if let Some(id) = super::accounts::positive_channel_id(reference) {
        return channel.id.get() == id;
    }
    let username = reference.strip_prefix('@').unwrap_or(reference);
    channel
        .username
        .as_ref()
        .is_some_and(|value| value.as_str().eq_ignore_ascii_case(username))
        || channel.name.as_str() == reference
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StatsQuery {
    days_back: Option<u32>,
}

pub(super) async fn stats(
    State(state): App,
    headers: HeaderMap,
    Path(id): Path<String>,
    params: QueryInput<StatsQuery>,
) -> Reply {
    let Query(params) = params.map_err(|_| invalid())?;
    let (access, channel) = channel_access(&state, &headers, &id).await?;
    let lease = access.lease().await?;
    let stats = lease
        .client()
        .operations
        .channel_stats(
            channel,
            params.days_back.unwrap_or(7),
            &access.context(&lease),
        )
        .await
        .map_err(from_telegram)?;
    access.revalidate().await?;
    let mut value =
        serde_json::to_value(stats).map_err(|_| ApiError::new(ErrorCode::Unavailable))?;
    value["channel_id"] = Value::String(id);
    Ok(Json(value))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LinkQuery {
    include_tg_protocol: Option<bool>,
}

pub(super) async fn link(
    State(state): App,
    headers: HeaderMap,
    Path((id, msg)): Path<(String, i64)>,
    params: QueryInput<LinkQuery>,
) -> Reply {
    let Query(params) = params.map_err(|_| invalid())?;
    let message = message_id(msg)?;
    let (access, channel) = channel_access(&state, &headers, &id).await?;
    let lease = access.lease().await?;
    let link = lease
        .client()
        .operations
        .message_link(channel, message, &access.context(&lease))
        .await
        .map_err(from_telegram)?;
    access.revalidate().await?;
    let mut reply = json!({"channel_id":id, "message_id":msg, "https_link":link.https_link});
    if params.include_tg_protocol.unwrap_or(false) {
        reply["tg_link"] = Value::String(link.tg_protocol_link);
    }
    Ok(Json(reply))
}

pub(super) async fn transcribe(
    State(state): App,
    headers: HeaderMap,
    Path((id, msg)): Path<(String, i64)>,
    body: Body<Transcription>,
) -> Reply {
    let Json(body) = body.map_err(super::json_error)?;
    let message = message_id(msg)?;
    let timeout = body.timeout_seconds.unwrap_or(30);
    if !(1..=120).contains(&timeout) {
        return Err(invalid());
    }
    let (access, channel) = channel_access(&state, &headers, &id).await?;
    let lease = access.lease().await?;
    let result = lease
        .client()
        .operations
        .transcribe(
            channel,
            message,
            timeout,
            access.hosted.config.rate_limiting.transcription_cost,
            &access.context(&lease),
        )
        .await
        .map_err(from_telegram)?;
    access.revalidate().await?;
    Ok(Json(json!({
        "channel_id":result.channel_id.get().to_string(), "peer_namespace":result.peer_namespace,
        "message_id":result.message_id, "text":result.text, "partial":result.partial,
        "media_type":result.media_type, "duration_seconds":result.duration_seconds
    })))
}

pub(super) async fn discover(
    State(state): App,
    headers: HeaderMap,
    body: Body<DiscoverChannels>,
) -> Reply {
    let Json(body) = body.map_err(super::json_error)?;
    let limit = limit(body.limit, 20)?;
    let access = ReadAccess::authorize(&state, &headers).await?;
    if !access.can_discover() {
        return Err(ApiError::new(ErrorCode::Forbidden));
    }
    let lease = access.lease().await?;
    let channels = lease
        .client()
        .operations
        .discover_channels(&body.query, limit, &access.context(&lease))
        .await
        .map_err(from_telegram)?;
    access.revalidate().await?;
    Ok(Json(
        json!({"channels":channels.into_iter().filter(|channel| access.allows_channel(&channel.id.get().to_string())).map(channel_json).collect::<Vec<_>>()}),
    ))
}

fn permitted(access: &ReadAccess, channel: &Channel) -> bool {
    channel.is_subscribed
        && channel.peer_namespace == Some(telegram_core::telegram::types::PeerNamespace::Channel)
        && access.allows_channel(&channel.id.get().to_string())
}

async fn channel_access(
    state: &Arc<ServiceState>,
    headers: &HeaderMap,
    id: &str,
) -> Result<(ReadAccess, ChannelId), ApiError> {
    let channel = super::accounts::positive_channel_id(id)
        .and_then(|value| ChannelId::new(value).ok())
        .ok_or_else(invalid)?;
    let access = ReadAccess::authorize(state, headers).await?;
    access.require_channel(id)?;
    Ok((access, channel))
}

fn message_id(raw: i64) -> Result<MessageId, ApiError> {
    MessageId::new(raw)
        .ok()
        .filter(|id| id.as_i32().is_some())
        .ok_or_else(invalid)
}

fn limit(value: Option<u32>, default: u32) -> Result<u32, ApiError> {
    let value = value.unwrap_or(default);
    (1..=100)
        .contains(&value)
        .then_some(value)
        .ok_or_else(invalid)
}

pub(super) fn channel_json(channel: Channel) -> Value {
    json!({
        "channel_id":channel.id.get().to_string(), "peer_namespace":channel.peer_namespace,
        "name":channel.name, "username":channel.username, "chat_type":channel.chat_type,
        "description":channel.description, "member_count":channel.member_count,
        "is_verified":channel.is_verified, "is_public":channel.is_public,
        "is_subscribed":channel.is_subscribed, "last_message_date":channel.last_message_date,
    })
}

fn invalid() -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest)
}

#[cfg(test)]
mod tests;
