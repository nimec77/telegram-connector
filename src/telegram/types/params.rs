//! Request parameters and result types.

use super::entities::Message;
use super::ids::{ChannelId, MessageId};
use super::media::MediaFilter;
use chrono::{DateTime, Duration, Utc};
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
    /// Inclusive lower bound. When set, overrides `hours_back` as the window
    /// start (and is deliberately NOT clamped by `MAX_HOURS_BACK`).
    pub from_date: Option<DateTime<Utc>>,
    /// Inclusive upper bound. Messages newer than this are skipped.
    pub to_date: Option<DateTime<Utc>>,
    /// Collapse album siblings into one post-level result; limit counts posts (B5+A2).
    pub collapse_albums: bool,
    /// Exclusive upper message-id bound: only messages with id < before_id
    /// are returned. Rides MTProto's offset_id, so paging doesn't drift on
    /// active channels the way offset-based paging does (A8).
    pub before_id: Option<MessageId>,
    /// Exclusive lower message-id bound: iteration stops at the first
    /// message with id <= after_id (client-side; grammers exposes no
    /// min_id setter).
    pub after_id: Option<MessageId>,
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
            from_date: None,
            to_date: None,
            collapse_albums: true,
            before_id: None,
            after_id: None,
        }
    }

    /// Effective window start: `from_date` if set, else `now - hours_back`.
    pub fn window_start(&self) -> DateTime<Utc> {
        self.from_date
            .unwrap_or_else(|| Utc::now() - Duration::hours(self.hours_back as i64))
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
    /// Known numeric channel id, used to walk the dialog list. `None` for a
    /// username reference, where the client owns resolution via
    /// `channel_identifier` and derives the id from the resolved peer (AD-2).
    pub channel_id: Option<ChannelId>,
    /// Original channel identifier (username or ID string) for direct resolution.
    /// When set to a username, the client can resolve the channel via `resolve_username`
    /// instead of iterating dialogs, allowing access to non-subscribed channels.
    pub channel_identifier: Option<String>,
    /// How many hours back to retrieve messages
    pub hours_back: u32,
    /// Maximum number of messages to return
    pub limit: u32,
    /// Optional media filter (applied client-side since iter_messages doesn't support server-side filtering)
    pub media_filter: Option<MediaFilter>,
    /// Inclusive lower bound. When set, overrides `hours_back` as the window
    /// start (and is deliberately NOT clamped by `MAX_HOURS_BACK`).
    pub from_date: Option<DateTime<Utc>>,
    /// Inclusive upper bound. Messages newer than this are skipped.
    pub to_date: Option<DateTime<Utc>>,
    /// Collapse album siblings into one post-level result; limit counts posts (B5+A2).
    pub collapse_albums: bool,
    /// Exclusive upper message-id bound: only messages with id < before_id
    /// are returned. Rides MTProto's offset_id, so paging doesn't drift on
    /// active channels the way offset-based paging does (A8).
    pub before_id: Option<MessageId>,
    /// Exclusive lower message-id bound: iteration stops at the first
    /// message with id <= after_id (client-side; grammers exposes no
    /// min_id setter).
    pub after_id: Option<MessageId>,
}

impl HistoryParams {
    pub const DEFAULT_HOURS_BACK: u32 = 48;
    pub const MAX_HOURS_BACK: u32 = 168; // 7 days
    pub const DEFAULT_LIMIT: u32 = 20;
    pub const MAX_LIMIT: u32 = 100;

    pub fn new(channel_id: ChannelId) -> Self {
        Self {
            channel_id: Some(channel_id),
            channel_identifier: None,
            hours_back: Self::DEFAULT_HOURS_BACK,
            limit: Self::DEFAULT_LIMIT,
            media_filter: None,
            from_date: None,
            to_date: None,
            collapse_albums: true,
            before_id: None,
            after_id: None,
        }
    }

    /// Effective window start: `from_date` if set, else `now - hours_back`.
    pub fn window_start(&self) -> DateTime<Utc> {
        self.from_date
            .unwrap_or_else(|| Utc::now() - Duration::hours(self.hours_back as i64))
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

    /// Builder method to set channel identifier for direct resolution
    pub fn channel_identifier(mut self, identifier: impl Into<String>) -> Self {
        self.channel_identifier = Some(identifier.into());
        self
    }
}

/// Search result aggregate.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchResult {
    pub messages: Vec<Message>,
    /// Number of messages in this response (page size, not a match count — B6).
    pub returned: u64,
    /// A qualifying message was *proven* to exist in the window beyond this
    /// page — refused by the limit, or dropped by `[limits] response_byte_budget`
    /// — so paging on can find it (A8). Deadline truncation proves nothing: it
    /// reports `query_metadata.timed_out`/`partial` and leaves this false.
    pub has_more: bool,
    pub search_time_ms: u64,
    pub query_metadata: QueryMetadata,
}

/// The window and scope a query actually executed with (work-order B6/B7).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryMetadata {
    pub query: String,
    /// Effective window start actually applied (from_date, or now - hours_back).
    pub window_from: DateTime<Utc>,
    /// Effective upper bound; omitted when the window is open-ended.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub window_to: Option<DateTime<Utc>>,
    /// Channels the search actually scanned; `null` when unknowable
    /// (server-side global search).
    pub channels_scanned: Option<u32>,
    /// Distinct channels present in `messages`.
    pub channels_in_results: u32,
    /// The search hit `[search] deadline_seconds` and stopped early. Omitted
    /// when false, so unaffected responses are unchanged on the wire.
    #[serde(default, skip_serializing_if = "is_false")]
    pub timed_out: bool,
    /// The result set is known-incomplete. Today only the deadline sets this;
    /// it is distinct from `timed_out` so future truncation causes can report
    /// incompleteness without claiming a timeout.
    #[serde(default, skip_serializing_if = "is_false")]
    pub partial: bool,
    /// Round trips issued to Telegram for this search.
    #[serde(default)]
    pub pages_fetched: u32,
    /// Raw messages walked, including those filtered out. Together with
    /// `pages_fetched` this is what makes an expensive call legible: a caller
    /// who cannot see a cost cannot budget for it.
    #[serde(default)]
    pub messages_scanned: u64,
}

/// serde `skip_serializing_if` helper. `std::ops::Not::not` cannot be used
/// here — serde hands the predicate a `&bool`.
fn is_false(b: &bool) -> bool {
    !*b
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
            from_date: None,
            to_date: None,
            collapse_albums: true,
            before_id: None,
            after_id: None,
        };
        assert_eq!(params.media_filter, Some(MediaFilter::Photo));
    }

    // =========================================================================
    // HistoryParams Tests
    // =========================================================================

    #[test]
    fn params_default_to_no_cursors() {
        let history = HistoryParams::new(ChannelId::new(1).unwrap());
        assert!(history.before_id.is_none());
        assert!(history.after_id.is_none());
        let search = SearchParams::new("query");
        assert!(search.before_id.is_none());
        assert!(search.after_id.is_none());
    }

    #[test]
    fn history_params_new() {
        let channel_id = ChannelId::new(123456).unwrap();
        let params = HistoryParams::new(channel_id);

        assert_eq!(params.channel_id.map(|c| c.get()), Some(123456));
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
    fn window_start_defaults_to_hours_back() {
        let params = SearchParams::new("q"); // hours_back = 48 default
        let expected = Utc::now() - Duration::hours(48);
        let diff = (params.window_start() - expected).num_seconds().abs();
        assert!(diff <= 1, "window_start should be ~now - hours_back");
    }

    #[test]
    fn window_start_prefers_from_date() {
        let mut params = SearchParams::new("q");
        let from = Utc::now() - Duration::days(30);
        params.from_date = Some(from);
        assert_eq!(params.window_start(), from);
    }

    #[test]
    fn search_result_serialization() {
        let window_from = "2026-08-01T00:00:00Z".parse().unwrap();
        let result = SearchResult {
            messages: vec![],
            returned: 42,
            has_more: false,
            search_time_ms: 150,
            query_metadata: QueryMetadata {
                query: "test".to_string(),
                window_from,
                window_to: None,
                channels_scanned: Some(5),
                channels_in_results: 5,
                timed_out: false,
                partial: false,
                pages_fetched: 0,
                messages_scanned: 0,
            },
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: SearchResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.returned, 42);
        assert_eq!(deserialized.search_time_ms, 150);
        assert_eq!(deserialized.query_metadata.query, "test");
        assert_eq!(deserialized.query_metadata.window_from, window_from);
        assert_eq!(deserialized.query_metadata.window_to, None);
        assert_eq!(deserialized.query_metadata.channels_scanned, Some(5));
        assert_eq!(deserialized.query_metadata.channels_in_results, 5);
        assert!(
            !json.contains("window_to"),
            "window_to must be omitted when None"
        );
    }

    #[test]
    fn search_result_serializes_has_more() {
        let result = SearchResult {
            messages: vec![],
            returned: 0,
            has_more: true,
            search_time_ms: 1,
            query_metadata: QueryMetadata {
                query: String::new(),
                window_from: Utc::now(),
                window_to: None,
                channels_scanned: Some(1),
                channels_in_results: 0,
                timed_out: false,
                partial: false,
                pages_fetched: 0,
                messages_scanned: 0,
            },
        };
        let json = serde_json::to_value(&result).expect("serialize");
        assert_eq!(json["has_more"], serde_json::Value::Bool(true));
    }

    #[test]
    fn query_metadata_omits_false_flags_from_json() {
        let meta = QueryMetadata {
            query: "test".to_string(),
            window_from: Utc::now(),
            window_to: None,
            channels_scanned: None,
            channels_in_results: 0,
            timed_out: false,
            partial: false,
            pages_fetched: 3,
            messages_scanned: 300,
        };
        let json = serde_json::to_string(&meta).expect("serializes");
        assert!(!json.contains("timed_out"), "false flags stay off the wire");
        assert!(!json.contains("partial"), "false flags stay off the wire");
        assert!(json.contains("\"pages_fetched\":3"));
        assert!(json.contains("\"messages_scanned\":300"));
    }

    #[test]
    fn query_metadata_emits_true_flags() {
        let meta = QueryMetadata {
            query: "test".to_string(),
            window_from: Utc::now(),
            window_to: None,
            channels_scanned: None,
            channels_in_results: 0,
            timed_out: true,
            partial: true,
            pages_fetched: 9,
            messages_scanned: 900,
        };
        let json = serde_json::to_string(&meta).expect("serializes");
        assert!(json.contains("\"timed_out\":true"));
        assert!(json.contains("\"partial\":true"));
    }

    #[test]
    fn query_metadata_deserializes_without_the_new_fields() {
        // A payload written by an older server must still parse.
        let json = r#"{"query":"q","window_from":"2026-08-13T00:00:00Z",
                       "channels_scanned":null,"channels_in_results":2}"#;
        let meta: QueryMetadata = serde_json::from_str(json).expect("parses");
        assert!(!meta.timed_out);
        assert!(!meta.partial);
        assert_eq!(meta.pages_fetched, 0);
        assert_eq!(meta.messages_scanned, 0);
    }
}
