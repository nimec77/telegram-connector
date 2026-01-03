//! Request parameters and result types.

use super::entities::Message;
use super::ids::ChannelId;
use super::media::MediaFilter;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Search parameters for message search.
#[derive(Debug, Clone)]
pub struct SearchParams {
    pub query: String,
    pub channel_id: Option<ChannelId>,
    pub hours_back: u32,
    pub limit: u32,
    /// Optional media filter for server-side filtering by attachment type.
    /// When set, only messages with the specified media type are returned.
    /// The text query still applies to message text/caption.
    pub media_filter: Option<MediaFilter>,
}

impl SearchParams {
    pub const DEFAULT_HOURS_BACK: u32 = 48;
    pub const MAX_HOURS_BACK: u32 = 72;
    pub const DEFAULT_LIMIT: u32 = 20;
    pub const MAX_LIMIT: u32 = 100;

    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            channel_id: None,
            hours_back: Self::DEFAULT_HOURS_BACK,
            limit: Self::DEFAULT_LIMIT,
            media_filter: None,
        }
    }
}

impl Default for SearchParams {
    fn default() -> Self {
        Self::new("")
    }
}

/// Parameters for retrieving message history from a channel.
///
/// Unlike `SearchParams`, this does not require a search query.
/// Uses grammers' `iter_messages()` to iterate message history.
#[derive(Debug, Clone)]
pub struct HistoryParams {
    /// Channel to retrieve messages from (required)
    pub channel_id: ChannelId,
    /// How many hours back to retrieve messages
    pub hours_back: u32,
    /// Maximum number of messages to return
    pub limit: u32,
    /// Optional media filter (applied client-side since iter_messages doesn't support server-side filtering)
    pub media_filter: Option<MediaFilter>,
}

impl HistoryParams {
    pub const DEFAULT_HOURS_BACK: u32 = 48;
    pub const MAX_HOURS_BACK: u32 = 168; // 7 days
    pub const DEFAULT_LIMIT: u32 = 20;
    pub const MAX_LIMIT: u32 = 100;

    pub fn new(channel_id: ChannelId) -> Self {
        Self {
            channel_id,
            hours_back: Self::DEFAULT_HOURS_BACK,
            limit: Self::DEFAULT_LIMIT,
            media_filter: None,
        }
    }

    /// Builder method to set hours_back
    pub fn hours_back(mut self, hours: u32) -> Self {
        self.hours_back = hours.min(Self::MAX_HOURS_BACK);
        self
    }

    /// Builder method to set limit
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = limit.min(Self::MAX_LIMIT);
        self
    }

    /// Builder method to set media filter
    pub fn media_filter(mut self, filter: MediaFilter) -> Self {
        self.media_filter = Some(filter);
        self
    }
}

/// Search result aggregate.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchResult {
    pub messages: Vec<Message>,
    pub total_found: u64,
    pub search_time_ms: u64,
    pub query_metadata: QueryMetadata,
}

/// Query metadata for search results.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryMetadata {
    pub query: String,
    pub hours_back: u32,
    pub channels_searched: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // SearchParams Tests
    // =========================================================================

    #[test]
    fn search_params_default() {
        let params = SearchParams::default();
        assert_eq!(params.query, "");
        assert_eq!(params.hours_back, SearchParams::DEFAULT_HOURS_BACK);
        assert_eq!(params.limit, SearchParams::DEFAULT_LIMIT);
        assert!(params.channel_id.is_none());
        assert!(params.media_filter.is_none());
    }

    #[test]
    fn search_params_new() {
        let params = SearchParams::new("AI news");
        assert_eq!(params.query, "AI news");
        assert_eq!(params.hours_back, 48);
        assert_eq!(params.limit, 20);
        assert!(params.media_filter.is_none());
    }

    #[test]
    fn search_params_constants() {
        assert_eq!(SearchParams::DEFAULT_HOURS_BACK, 48);
        assert_eq!(SearchParams::MAX_HOURS_BACK, 72);
        assert_eq!(SearchParams::DEFAULT_LIMIT, 20);
        assert_eq!(SearchParams::MAX_LIMIT, 100);
    }

    #[test]
    fn search_params_with_media_filter() {
        let params = SearchParams {
            query: "test".to_string(),
            channel_id: None,
            hours_back: 48,
            limit: 20,
            media_filter: Some(MediaFilter::Photo),
        };
        assert_eq!(params.media_filter, Some(MediaFilter::Photo));
    }

    // =========================================================================
    // HistoryParams Tests
    // =========================================================================

    #[test]
    fn history_params_new() {
        let channel_id = ChannelId::new(123456).unwrap();
        let params = HistoryParams::new(channel_id);

        assert_eq!(params.channel_id.get(), 123456);
        assert_eq!(params.hours_back, HistoryParams::DEFAULT_HOURS_BACK);
        assert_eq!(params.limit, HistoryParams::DEFAULT_LIMIT);
        assert!(params.media_filter.is_none());
    }

    #[test]
    fn history_params_constants() {
        assert_eq!(HistoryParams::DEFAULT_HOURS_BACK, 48);
        assert_eq!(HistoryParams::MAX_HOURS_BACK, 168); // 7 days
        assert_eq!(HistoryParams::DEFAULT_LIMIT, 20);
        assert_eq!(HistoryParams::MAX_LIMIT, 100);
    }

    #[test]
    fn history_params_builder_methods() {
        let channel_id = ChannelId::new(123456).unwrap();
        let params = HistoryParams::new(channel_id)
            .hours_back(72)
            .limit(50)
            .media_filter(MediaFilter::Photo);

        assert_eq!(params.hours_back, 72);
        assert_eq!(params.limit, 50);
        assert_eq!(params.media_filter, Some(MediaFilter::Photo));
    }

    #[test]
    fn history_params_hours_back_capped_at_max() {
        let channel_id = ChannelId::new(123456).unwrap();
        let params = HistoryParams::new(channel_id).hours_back(500); // Exceeds max

        assert_eq!(params.hours_back, HistoryParams::MAX_HOURS_BACK);
    }

    #[test]
    fn history_params_limit_capped_at_max() {
        let channel_id = ChannelId::new(123456).unwrap();
        let params = HistoryParams::new(channel_id).limit(500); // Exceeds max

        assert_eq!(params.limit, HistoryParams::MAX_LIMIT);
    }

    // =========================================================================
    // SearchResult Tests
    // =========================================================================

    #[test]
    fn search_result_serialization() {
        let result = SearchResult {
            messages: vec![],
            total_found: 42,
            search_time_ms: 150,
            query_metadata: QueryMetadata {
                query: "test".to_string(),
                hours_back: 48,
                channels_searched: 5,
            },
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: SearchResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.total_found, 42);
        assert_eq!(deserialized.search_time_ms, 150);
        assert_eq!(deserialized.query_metadata.query, "test");
    }
}
