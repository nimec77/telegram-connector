//! Authenticated, charged reads with hosted response budgets.

use super::accounts::{ActiveAccount, positive_channel_id};
use super::authorization::ReadAccess;
use super::http_error::from_telegram;
use super::registry::AccountLease;
use super::{ApiError, ServiceState, json_error};
use crate::contracts::{
    BatchRequest, BatchResponse, ErrorCode, HistoryQuery, HistoryResponse, ReadAlbumInfo,
    ReadMediaKind, ReadMessage, ReadPeerNamespace, SearchChannelError, SearchMediaFilter,
    SearchRequest, SearchResponse,
};
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;
use telegram_core::UtcTimestamp;
use telegram_core::error::Error;
use telegram_core::operations::fanout::MAX_FANOUT_CHANNELS;
use telegram_core::operations::{
    HistoryIdPage, HistoryIdRange, OperationContext, SearchOperation, SearchPage, SearchScope,
    validate_date_window,
};
use telegram_core::telegram::types::{
    ChannelId, MediaFilter, MediaType, Message, MessageBatch, MessageId, PeerNamespace,
    SearchParams,
};

#[cfg(test)]
#[path = "reads/fixture.rs"]
pub(super) mod fixture;
#[cfg(test)]
#[path = "reads/tests.rs"]
mod tests;

const RESULT_BYTE_BUDGET: usize = 40_000;
const MAX_BATCH_IDS: usize = 50;

pub(super) async fn search(
    State(state): State<Arc<ServiceState>>,
    headers: HeaderMap,
    request: Result<Json<SearchRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let access = ReadAccess::authorize(&state, &headers).await?;
    let Json(request) = request.map_err(json_error)?;
    let mut seen = HashSet::new();
    let channels: Vec<_> = request
        .channel_ids
        .into_iter()
        .filter(|id| seen.insert(id.clone()))
        .collect();
    if channels.is_empty() || channels.len() > MAX_FANOUT_CHANNELS {
        return Err(invalid_request());
    }
    for channel in &channels {
        access.require_channel(channel)?;
    }
    let single = channels.len() == 1;
    let scope = if single {
        SearchScope::Channel(channels.into_iter().next().ok_or_else(invalid_request)?)
    } else {
        SearchScope::Channels(channels)
    };
    let before_id = request.before_id.map(wire_id).transpose()?;
    let after_id = request.after_id.map(wire_id).transpose()?;
    if request.limit == 0
        || (request.query.trim().is_empty() && request.media_filter.is_none())
        || (!single && (before_id.is_some() || after_id.is_some()))
        || matches!((before_id, after_id), (Some(upper), Some(lower)) if upper.get() <= lower.get())
    {
        return Err(invalid_request());
    }
    let hours_back = request
        .hours_back
        .unwrap_or(SearchParams::DEFAULT_HOURS_BACK);
    let from_date = parse_date(request.from_date.as_deref())?;
    let to_date = parse_date(request.to_date.as_deref())?;
    // Use the same effective window as the charged core operation.
    if from_date.is_some() || to_date.is_some() {
        let window = SearchParams {
            channel_namespace: None,
            channel_id: None,
            query: String::new(),
            hours_back: hours_back.min(SearchParams::MAX_HOURS_BACK),
            limit: request.limit,
            media_filter: None,
            from_date,
            to_date,
            collapse_albums: false,
            before_id,
            after_id,
        };
        validate_date_window(from_date, to_date, window.window_start(), window.hours_back)
            .map_err(|_| invalid_request())?;
    }
    let operation = SearchOperation {
        scope,
        query: request.query,
        hours_back,
        limit: request.limit,
        media_filter: request.media_filter.map(media_filter),
        from_date,
        to_date,
        collapse_albums: request.collapse_albums,
        before_id,
        after_id,
    };

    let lease = ReadLease::open(&access).await?;
    let page = lease.search(operation).await.map_err(search_error)?;
    let mut failed_channels = Vec::with_capacity(page.failures.len());
    for (channel_id, error) in page.failures {
        let code = from_telegram(error).0;
        if code == ErrorCode::UnavailableResource {
            return Err(ApiError::new(code));
        }
        failed_channels.push(SearchChannelError { channel_id, code });
    }
    let messages = render_messages(page.result.messages)?;
    let timed_out = page.result.query_metadata.timed_out
        || failed_channels
            .iter()
            .any(|failure| failure.code == ErrorCode::Timeout);
    let next_before_id = if single && (page.result.has_more || timed_out) {
        messages.last().map(|row| row.id)
    } else {
        None
    };
    let response = search_response(
        SearchResponse {
            messages,
            has_more: page.result.has_more,
            next_before_id,
            partial: page.result.query_metadata.partial || !failed_channels.is_empty(),
            timed_out,
            failed_channels,
        },
        single,
    )?;
    access.revalidate().await?;
    Ok(response)
}

pub(super) async fn history(
    State(state): State<Arc<ServiceState>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    query: Result<Query<HistoryQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let access = ReadAccess::authorize(&state, &headers).await?;
    let Path(channel) = path.map_err(|_| invalid_request())?;
    let Query(query) = query.map_err(|_| invalid_request())?;
    access.require_channel(&channel)?;
    let range = HistoryIdRange {
        channel_id: channel_id(&channel)?,
        after_id: query
            .after_id
            .filter(|&id| id != 0)
            .map(wire_id)
            .transpose()?,
        through_id: wire_id(query.through_id.unwrap_or(i32::MAX as u32))?,
        before_id: query.before_id.map(wire_id).transpose()?,
        limit: query.limit,
    };
    range.validate().map_err(from_telegram)?;
    if range.is_empty() {
        access.revalidate().await?;
        return history_response(HistoryResponse::default());
    }

    let lease = ReadLease::open(&access).await?;
    let page = lease.history(range).await.map_err(from_telegram)?;
    let response = history_response(HistoryResponse {
        messages: render_messages(page.messages)?,
        next_before_id: page.next_before_id.map(output_id).transpose()?,
        partial: page.partial,
        timed_out: page.timed_out,
    })?;
    access.revalidate().await?;
    Ok(response)
}

pub(super) async fn batch(
    State(state): State<Arc<ServiceState>>,
    headers: HeaderMap,
    request: Result<Json<BatchRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let access = ReadAccess::authorize(&state, &headers).await?;
    let Json(request) = request.map_err(json_error)?;
    access.require_channel(&request.channel_id)?;
    let mut seen = HashSet::new();
    let message_ids: Vec<_> = request
        .message_ids
        .into_iter()
        .filter(|&id| seen.insert(id))
        .map(wire_id)
        .collect::<Result<_, _>>()?;
    if message_ids.is_empty() || message_ids.len() > MAX_BATCH_IDS {
        return Err(invalid_request());
    }

    let channel_id = channel_id(&request.channel_id)?;
    let lease = ReadLease::open(&access).await?;
    let result = lease
        .batch(channel_id, message_ids)
        .await
        .map_err(from_telegram)?;
    if result.channel_id != channel_id || result.peer_namespace != PeerNamespace::Channel {
        return Err(ApiError::new(ErrorCode::UnavailableResource));
    }
    let response = batch_response(BatchResponse {
        channel_id: result.channel_id.to_string(),
        messages: render_messages(result.messages)?,
        missing_ids: result
            .missing_ids
            .into_iter()
            .map(|id| u32::try_from(id).map_err(|_| unavailable()))
            .collect::<Result<_, _>>()?,
        omitted_ids: Vec::new(),
    })?;
    access.revalidate().await?;
    Ok(response)
}

enum ReadLease {
    Live(AccountLease<ActiveAccount>, OperationContext),
    #[cfg(test)]
    Fixture(fixture::FixtureLease),
}

impl ReadLease {
    async fn open(access: &ReadAccess) -> Result<Self, ApiError> {
        #[cfg(test)]
        if let Some(fixture) = access.hosted.reads_fixture.get() {
            return fixture.lease(access).await.map(Self::Fixture);
        }
        let lease = access.lease().await?;
        let context = access.context(&lease);
        Ok(Self::Live(lease, context))
    }

    async fn search(&self, operation: SearchOperation) -> Result<SearchPage, Error> {
        match self {
            Self::Live(lease, context) => {
                lease
                    .client()
                    .operations
                    .search_joined_channels(operation, context)
                    .await
            }
            #[cfg(test)]
            Self::Fixture(lease) => lease.search(operation).await,
        }
    }

    async fn history(&self, range: HistoryIdRange) -> Result<HistoryIdPage, Error> {
        match self {
            Self::Live(lease, context) => {
                lease
                    .client()
                    .operations
                    .history_range(range, context)
                    .await
            }
            #[cfg(test)]
            Self::Fixture(lease) => lease.history(range).await,
        }
    }

    async fn batch(
        &self,
        channel_id: ChannelId,
        ids: Vec<MessageId>,
    ) -> Result<MessageBatch, Error> {
        match self {
            Self::Live(lease, context) => {
                lease
                    .client()
                    .operations
                    .batch_joined_channel(channel_id, ids, context)
                    .await
            }
            #[cfg(test)]
            Self::Fixture(lease) => lease.batch(channel_id, ids).await,
        }
    }
}

fn invalid_request() -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest)
}

fn unavailable() -> ApiError {
    ApiError::new(ErrorCode::Unavailable)
}

fn search_error(error: Error) -> ApiError {
    if let Error::AllChannelsFailed(errors) = error {
        let mut codes = errors.into_iter().map(|(_, error)| from_telegram(error).0);
        let Some(first) = codes.next() else {
            return unavailable();
        };
        let mut same_code = true;
        let mut missing = first == ErrorCode::UnavailableResource;
        for code in codes {
            same_code &= code == first;
            missing |= code == ErrorCode::UnavailableResource;
        }
        return if missing {
            ApiError::new(ErrorCode::UnavailableResource)
        } else if same_code {
            ApiError::new(first)
        } else {
            unavailable()
        };
    }
    from_telegram(error)
}

fn channel_id(value: &str) -> Result<ChannelId, ApiError> {
    ChannelId::new(positive_channel_id(value).ok_or_else(invalid_request)?).map_err(from_telegram)
}

fn wire_id(value: u32) -> Result<MessageId, ApiError> {
    if value > i32::MAX as u32 {
        return Err(invalid_request());
    }
    MessageId::new(i64::from(value)).map_err(from_telegram)
}

fn output_id(value: MessageId) -> Result<u32, ApiError> {
    u32::try_from(value.get()).map_err(|_| unavailable())
}

fn parse_date(value: Option<&str>) -> Result<Option<UtcTimestamp>, ApiError> {
    value
        .map(str::parse)
        .transpose()
        .map_err(|_| invalid_request())
}

fn media_filter(value: SearchMediaFilter) -> MediaFilter {
    match value {
        SearchMediaFilter::Photo => MediaFilter::Photo,
        SearchMediaFilter::Video => MediaFilter::Video,
        SearchMediaFilter::PhotoVideo => MediaFilter::PhotoVideo,
        SearchMediaFilter::Document => MediaFilter::Document,
        SearchMediaFilter::Audio => MediaFilter::Audio,
        SearchMediaFilter::Voice => MediaFilter::Voice,
        SearchMediaFilter::VideoNote => MediaFilter::VideoNote,
        SearchMediaFilter::Gif => MediaFilter::Gif,
        SearchMediaFilter::Url => MediaFilter::Url,
        SearchMediaFilter::Pinned => MediaFilter::Pinned,
    }
}

fn media_kind(value: MediaType) -> ReadMediaKind {
    match value {
        MediaType::None => ReadMediaKind::None,
        MediaType::Photo => ReadMediaKind::Photo,
        MediaType::Video => ReadMediaKind::Video,
        MediaType::Document => ReadMediaKind::Document,
        MediaType::Audio => ReadMediaKind::Audio,
        MediaType::Voice => ReadMediaKind::Voice,
        MediaType::VideoNote => ReadMediaKind::VideoNote,
        MediaType::Animation => ReadMediaKind::Animation,
        MediaType::Sticker => ReadMediaKind::Sticker,
        MediaType::Contact => ReadMediaKind::Contact,
        MediaType::Location => ReadMediaKind::Location,
        MediaType::Venue => ReadMediaKind::Venue,
        MediaType::Poll => ReadMediaKind::Poll,
        MediaType::Dice => ReadMediaKind::Dice,
    }
}

fn render_messages(messages: Vec<Message>) -> Result<Vec<ReadMessage>, ApiError> {
    messages
        .into_iter()
        .map(|message| {
            let peer_namespace = match message.peer_namespace {
                Some(PeerNamespace::Channel) => ReadPeerNamespace::Channel,
                _ => return Err(ApiError::new(ErrorCode::UnavailableResource)),
            };
            Ok(ReadMessage {
                id: output_id(message.id)?,
                channel_id: message.channel_id.to_string(),
                peer_namespace,
                channel_name: message.channel_name.to_string(),
                channel_username: message.channel_username.map(|value| value.to_string()),
                text: message.text,
                timestamp: message.timestamp.to_string(),
                sender_id: message.sender_id.map(|value| value.to_string()),
                sender_name: message.sender_name,
                has_media: message.has_media,
                media_type: media_kind(message.media_type),
                grouped_id: message.grouped_id,
                album: message
                    .album
                    .map(|album| -> Result<ReadAlbumInfo, ApiError> {
                        Ok(ReadAlbumInfo {
                            media_count: album.media_count,
                            media_types: album.media_types.into_iter().map(media_kind).collect(),
                            message_ids: album
                                .message_ids
                                .into_iter()
                                .map(output_id)
                                .collect::<Result<_, _>>()?,
                        })
                    })
                    .transpose()?,
                link: message.link,
            })
        })
        .collect()
}

fn fit<T: Serialize>(
    mut page: T,
    mut trim: impl FnMut(&mut T) -> bool,
) -> Result<Response, ApiError> {
    loop {
        let bytes = serde_json::to_vec(&page).map_err(|_| unavailable())?;
        if bytes.len() <= RESULT_BYTE_BUDGET {
            return Ok(([(header::CONTENT_TYPE, "application/json")], bytes).into_response());
        }
        if !trim(&mut page) {
            return Err(ApiError::new(ErrorCode::PayloadTooLarge));
        }
    }
}

fn search_response(page: SearchResponse, single_channel: bool) -> Result<Response, ApiError> {
    fit(page, |page| {
        if page.messages.len() <= 1 {
            return false;
        }
        page.messages.pop();
        page.has_more = true;
        page.next_before_id = if single_channel {
            page.messages.last().map(|row| row.id)
        } else {
            None
        };
        true
    })
}

fn history_response(page: HistoryResponse) -> Result<Response, ApiError> {
    fit(page, |page| {
        if page.messages.len() <= 1 {
            return false;
        }
        page.messages.pop();
        page.next_before_id = page.messages.last().map(|row| row.id);
        true
    })
}

fn batch_response(page: BatchResponse) -> Result<Response, ApiError> {
    fit(page, |page| {
        if let Some(message) = page.messages.pop() {
            page.omitted_ids.insert(0, message.id);
            true
        } else {
            false
        }
    })
}
