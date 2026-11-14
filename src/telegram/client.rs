//! Telegram client wrapping grammers-client.
//!
//! Holds the `TelegramClient` struct and the single `TelegramClientTrait` impl,
//! whose methods are thin delegators to inherent `*_impl` methods in the
//! `client/` submodules (LM-2). The struct stays here so every submodule (a
//! descendant) can reach its private fields.

use crate::config::TimeoutConfig;
use crate::error::Error;
use crate::telegram::trait_def::TelegramClientTrait;
use crate::telegram::transcription::TranscriptionDeadline;
use crate::telegram::types::{
    AccountStatus, HistoryParams, MediaDownload, SearchParams, SearchResult, TranscriptionOutcome,
};
use grammers_client::Client;
use grammers_session::storages::SqliteSession;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::JoinHandle;

mod auth;
mod channels;
mod cursor;
mod guard;
mod lifecycle;
mod ops_history;
mod ops_media;
mod ops_message;
mod ops_resolve;
mod ops_search;
mod ops_send;
mod ops_stats;
mod ops_transcribe;
mod raw_fetch;
mod raw_page;
mod raw_pager;
mod resolve;
mod resolve_recipient;
mod results;
mod search_budget;
mod send_diagnostics;
mod send_receipt;
mod send_targets;
mod send_transport;

#[cfg(test)]
#[path = "client/tests/recipient_tests.rs"]
mod recipient_tests;
#[cfg(test)]
#[path = "client/tests/send_target_tests.rs"]
mod send_target_tests;
#[cfg(test)]
#[path = "client/tests/send_text_tests.rs"]
mod send_text_tests;
mod walk;

#[cfg(test)]
#[path = "client/tests/send_transport_tests.rs"]
mod send_transport_tests;

#[cfg(test)]
#[path = "client/tests/helpers_tests.rs"]
mod helpers_tests;

#[cfg(test)]
#[path = "client/tests/client_fixture.rs"]
pub(crate) mod client_fixture;
#[cfg(test)]
pub(crate) use client_fixture::stalled_client;

#[cfg(test)]
#[path = "client/tests/session_upgrade_tests.rs"]
mod session_upgrade_tests;

pub(crate) use resolve::username_to_resolve;

/// Telegram client wrapping grammers-client
pub struct TelegramClient {
    client: Client,
    send_adapter: send_transport::FinalSendAdapter,
    session: Arc<SqliteSession>,
    session_path: PathBuf,
    timeouts: TimeoutConfig,
    /// Hard cap (bytes) on a single media download (`[telegram] max_download_bytes`, AD-6).
    max_download_bytes: u64,
    /// Wall-clock budget for a search accumulation loop (`[search] deadline_seconds`).
    search_deadline_secs: u64,
    /// Local observations; refreshes never hold this lock across network work.
    account_status: tokio::sync::RwLock<AccountStatus>,
    _runner_handle: JoinHandle<()>,
}

#[async_trait::async_trait]
impl TelegramClientTrait for TelegramClient {
    async fn send_message(
        &self,
        request: &crate::telegram::types::sending::SendText,
        deadline: crate::telegram::sending::SendDeadline,
    ) -> Result<
        crate::telegram::types::sending::SentMessage,
        crate::telegram::types::sending::SendFailure,
    > {
        self.send_message_impl(request, deadline).await
    }
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

    async fn cached_account_status(&self) -> AccountStatus {
        *self.account_status.read().await
    }

    async fn refresh_account_status(&self) -> Result<AccountStatus, Error> {
        self.refresh_account_status_impl().await
    }

    async fn get_message_by_id(
        &self,
        channel_ref: &str,
        message_id: i32,
    ) -> Result<crate::telegram::Message, Error> {
        self.get_message_by_id_impl(channel_ref, message_id).await
    }

    async fn get_messages_batch(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
    ) -> Result<crate::telegram::MessageBatch, Error> {
        self.get_messages_batch_impl(channel_ref, message_ids).await
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

    async fn download_messages_media(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
        max_dimension: u32,
    ) -> Result<crate::telegram::types::MediaBatch, Error> {
        self.download_messages_media_impl(channel_ref, message_ids, max_dimension)
            .await
    }

    async fn download_messages_media_pipeline(
        &self,
        channel_ref: &str,
        message_ids: &[i32],
        max_dimension: u32,
    ) -> Result<crate::telegram::types::MediaBatch, Error> {
        self.download_messages_media_pipeline_impl(channel_ref, message_ids, max_dimension)
            .await
    }

    async fn transcribe_audio(
        &self,
        channel_ref: &str,
        message_id: i32,
        deadline: TranscriptionDeadline,
    ) -> Result<TranscriptionOutcome, Error> {
        self.transcribe_audio_impl(channel_ref, message_id, deadline)
            .await
    }

    async fn resolve_channels(
        &self,
        identifiers: &[String],
    ) -> Result<Vec<crate::telegram::ChannelResolution<Error>>, Error> {
        self.resolve_channels_impl(identifiers).await
    }

    async fn get_channel_stats(
        &self,
        channel_ref: &str,
        days_back: u32,
    ) -> Result<crate::telegram::ChannelStats, Error> {
        self.get_channel_stats_impl(channel_ref, days_back).await
    }
}
