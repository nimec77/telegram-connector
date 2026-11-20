//! Compatibility exports for the core Telegram client and domain data.

pub mod auth;

pub use telegram_core::telegram::{
    AccountStatus, Channel, ChannelId, ChannelIdentity, ChannelName, ChannelPage,
    ChannelResolution, ChannelStats, ChatType, HistoryParams, MediaFetchError, MediaFetchOutcome,
    MediaFilter, MediaType, Message, MessageBatch, MessageId, QueryMetadata, SearchParams,
    SearchResult, StatsSample, TelegramClient, TelegramClientTrait, TranscriptionOutcome,
    TranscriptionState, UserId, Username, client, converters, media_pipeline, sending, timeout,
    trait_def, transcription, types,
};

#[cfg(test)]
pub(crate) use telegram_core::telegram::{MockTelegramClientTrait, albums, envelope};
