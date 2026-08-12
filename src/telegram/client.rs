//! Telegram client wrapping grammers-client.
//!
//! Holds the `TelegramClient` struct and the single `TelegramClientTrait` impl,
//! whose methods are thin delegators to inherent `*_impl` methods in the
//! `client/` submodules (LM-2). The struct stays here so every submodule (a
//! descendant) can reach its private fields.

use crate::config::{TelegramConfig, TimeoutConfig};
use crate::error::Error;
use crate::telegram::converters::{
    channel_identity, convert_discovered_peer, convert_media_filter, convert_media_to_type,
    convert_message, convert_peer_to_channel, extract_audio_duration, extract_video_info,
    matches_media_filter, message_timestamp, select_size_candidate, size_candidates,
};
use crate::telegram::timeout::with_timeout;
use crate::telegram::trait_def::TelegramClientTrait;
use crate::telegram::types::{
    ChannelIdentity, HistoryParams, MediaDownload, MediaType, QueryMetadata, SearchParams,
    SearchResult, TranscriptionOutcome, TranscriptionState, Username,
};
use grammers_client::Client;
use grammers_client::media::Media;
use grammers_client::tl;
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant};
use tokio::task::JoinHandle;

mod auth;
mod channels;
mod guard;
mod lifecycle;
mod ops_history;
mod ops_media;
mod ops_message;
mod ops_search;
mod ops_transcribe;
mod resolve;

pub(crate) use resolve::username_to_resolve;

/// Convert a resolved peer into the `PeerRef` that grammers RPC calls take.
///
/// grammers 0.10 split two failure modes that used to be one `Option`; keep
/// them distinguishable in logs: `Err` is a session/storage error, `Ok(None)`
/// means the peer is not in the session cache (no usable access hash).
async fn peer_to_ref(
    peer: &grammers_client::peer::Peer,
) -> Result<grammers_session::types::PeerRef, Error> {
    peer.to_ref()
        .await
        .map_err(|e| Error::TelegramApi(format!("Failed to convert peer to PeerRef: {e}")))?
        .ok_or_else(|| {
            Error::TelegramApi("Peer has no PeerRef (not in the session cache)".to_string())
        })
}

/// Telegram client wrapping grammers-client
pub struct TelegramClient {
    client: Client,
    session: Arc<SqliteSession>,
    session_path: PathBuf,
    timeouts: TimeoutConfig,
    /// Hard cap (bytes) on a single media download (`[telegram] max_download_bytes`, AD-6).
    max_download_bytes: u64,
    /// Cached Premium flag for the connected account (None = not yet determined).
    premium: tokio::sync::RwLock<Option<bool>>,
    _runner_handle: JoinHandle<()>,
}

#[async_trait::async_trait]
impl TelegramClientTrait for TelegramClient {
    async fn search_messages(&self, params: &SearchParams) -> Result<SearchResult, Error> {
        self.search_messages_impl(params).await
    }

    async fn get_recent_messages(&self, params: &HistoryParams) -> Result<SearchResult, Error> {
        self.get_recent_messages_impl(params).await
    }

    async fn get_channel_info(&self, identifier: &str) -> Result<crate::telegram::Channel, Error> {
        self.get_channel_info_impl(identifier).await
    }

    async fn get_full_channel_info(
        &self,
        identifier: &str,
    ) -> Result<crate::telegram::Channel, Error> {
        self.get_full_channel_info_impl(identifier).await
    }

    async fn get_subscribed_channels(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<crate::telegram::ChannelPage, Error> {
        self.get_subscribed_channels_impl(limit, offset).await
    }

    async fn search_public_channels(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<crate::telegram::Channel>, Error> {
        self.search_public_channels_impl(query, limit).await
    }

    async fn is_connected(&self) -> bool {
        self.is_connected_impl().await
    }

    async fn get_message_by_id(
        &self,
        channel_ref: &str,
        message_id: i32,
    ) -> Result<crate::telegram::Message, Error> {
        self.get_message_by_id_impl(channel_ref, message_id).await
    }

    async fn resolve_channel_identity(
        &self,
        channel_ref: &str,
    ) -> Result<crate::telegram::ChannelIdentity, Error> {
        self.resolve_channel_identity_impl(channel_ref).await
    }

    async fn download_message_media(
        &self,
        channel_ref: &str,
        message_id: i32,
        max_dimension: u32,
    ) -> Result<MediaDownload, Error> {
        self.download_message_media_impl(channel_ref, message_id, max_dimension)
            .await
    }

    async fn transcribe_audio(
        &self,
        channel_ref: &str,
        message_id: i32,
        timeout_secs: u32,
    ) -> Result<TranscriptionOutcome, Error> {
        self.transcribe_audio_impl(channel_ref, message_id, timeout_secs)
            .await
    }

    async fn is_premium(&self) -> Option<bool> {
        self.is_premium_impl().await
    }
}
