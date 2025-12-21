use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::error::Error;

// =============================================================================
// ID Value Objects (with validation)
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelId(i64);

impl ChannelId {
    pub fn new(id: i64) -> Result<Self, Error> {
        if id <= 0 {
            return Err(Error::InvalidInput(format!(
                "Channel ID must be positive, got {}",
                id
            )));
        }
        Ok(Self(id))
    }

    pub fn get(&self) -> i64 {
        self.0
    }
}

impl fmt::Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(i64);

impl MessageId {
    pub fn new(id: i64) -> Result<Self, Error> {
        if id <= 0 {
            return Err(Error::InvalidInput(format!(
                "Message ID must be positive, got {}",
                id
            )));
        }
        Ok(Self(id))
    }

    pub fn get(&self) -> i64 {
        self.0
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(i64);

impl UserId {
    pub fn new(id: i64) -> Result<Self, Error> {
        if id <= 0 {
            return Err(Error::InvalidInput(format!(
                "User ID must be positive, got {}",
                id
            )));
        }
        Ok(Self(id))
    }

    pub fn get(&self) -> i64 {
        self.0
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// =============================================================================
// String Value Objects (with validation)
// =============================================================================

/// Telegram username (alphanumeric + underscore, 5-32 chars)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Username(String);

impl Username {
    pub fn new(username: impl Into<String>) -> Result<Self, Error> {
        let username = username.into();

        if username.len() < 5 || username.len() > 32 {
            return Err(Error::InvalidInput(format!(
                "Username must be 5-32 characters, got {}",
                username.len()
            )));
        }

        if !username.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(Error::InvalidInput(
                "Username must contain only alphanumeric characters and underscores".into(),
            ));
        }

        Ok(Self(username))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Non-empty channel/chat name
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelName(String);

impl ChannelName {
    pub fn new(name: impl Into<String>) -> Result<Self, Error> {
        let name = name.into();
        let trimmed = name.trim();

        if trimmed.is_empty() {
            return Err(Error::InvalidInput("Channel name cannot be empty".into()));
        }

        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ChannelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// =============================================================================
// Media Types (comprehensive coverage)
// =============================================================================

/// All Telegram media types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MediaType {
    #[default]
    None, // Text-only message
    Photo,     // Image
    Video,     // Video file
    Document,  // Generic file
    Audio,     // Audio file (music)
    Voice,     // Voice message
    VideoNote, // Round video message
    Animation, // GIF
    Sticker,   // Sticker
    Contact,   // Shared contact
    Location,  // GPS location
    Venue,     // Location with venue info
    Poll,      // Poll/quiz
    Dice,      // Dice/dart/etc game
}

// =============================================================================
// Domain Entities
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub channel_id: ChannelId,
    pub channel_name: ChannelName,
    pub channel_username: Username,
    pub text: String,
    pub timestamp: DateTime<Utc>,
    pub sender_id: Option<UserId>,
    pub sender_name: Option<String>,
    pub has_media: bool,
    pub media_type: MediaType,
}

impl Message {
    /// Check if message is within specified hours from now
    pub fn is_recent(&self, hours: u32) -> bool {
        let threshold = Utc::now() - chrono::Duration::hours(hours as i64);
        self.timestamp > threshold
    }

    /// Check if message is text-only (no media)
    pub fn is_text_only(&self) -> bool {
        self.media_type == MediaType::None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub name: ChannelName,
    pub username: Username,
    pub description: Option<String>,
    pub member_count: u64,
    pub is_verified: bool,
    pub is_public: bool,
    pub is_subscribed: bool,
    pub last_message_date: Option<DateTime<Utc>>,
}

// =============================================================================
// Request/Response Types
// =============================================================================

#[derive(Debug, Clone)]
pub struct SearchParams {
    pub query: String,
    pub channel_id: Option<ChannelId>,
    pub hours_back: u32,
    pub limit: u32,
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
        }
    }
}

impl Default for SearchParams {
    fn default() -> Self {
        Self::new("")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub messages: Vec<Message>,
    pub total_found: u64,
    pub search_time_ms: u64,
    pub query_metadata: QueryMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryMetadata {
    pub query: String,
    pub hours_back: u32,
    pub channels_searched: u32,
}

// =============================================================================
// Tests (TDD - written first)
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // ID Type Tests
    // =========================================================================

    #[test]
    fn channel_id_rejects_negative() {
        let result = ChannelId::new(-1);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("positive"));
    }

    #[test]
    fn channel_id_rejects_zero() {
        let result = ChannelId::new(0);
        assert!(result.is_err());
    }

    #[test]
    fn channel_id_accepts_positive() {
        let result = ChannelId::new(123);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().get(), 123);
    }

    #[test]
    fn channel_id_display() {
        let id = ChannelId::new(123456).unwrap();
        assert_eq!(format!("{}", id), "123456");
    }

    #[test]
    fn channel_id_serde_transparent() {
        let id = ChannelId::new(123456).unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "123456"); // No wrapping object
    }

    #[test]
    fn message_id_rejects_negative() {
        assert!(MessageId::new(-1).is_err());
    }

    #[test]
    fn message_id_rejects_zero() {
        assert!(MessageId::new(0).is_err());
    }

    #[test]
    fn message_id_accepts_positive() {
        let result = MessageId::new(456);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().get(), 456);
    }

    #[test]
    fn message_id_display() {
        let id = MessageId::new(789).unwrap();
        assert_eq!(format!("{}", id), "789");
    }

    #[test]
    fn user_id_rejects_negative() {
        assert!(UserId::new(-1).is_err());
    }

    #[test]
    fn user_id_rejects_zero() {
        assert!(UserId::new(0).is_err());
    }

    #[test]
    fn user_id_accepts_positive() {
        let result = UserId::new(999);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().get(), 999);
    }

    #[test]
    fn user_id_display() {
        let id = UserId::new(111).unwrap();
        assert_eq!(format!("{}", id), "111");
    }

    // =========================================================================
    // Username Tests
    // =========================================================================

    #[test]
    fn username_rejects_too_short() {
        let result = Username::new("abc");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("5-32"));
    }

    #[test]
    fn username_rejects_too_long() {
        let result = Username::new("a".repeat(33));
        assert!(result.is_err());
    }

    #[test]
    fn username_rejects_special_chars() {
        let result = Username::new("user@name");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("alphanumeric"));
    }

    #[test]
    fn username_accepts_underscore() {
        let result = Username::new("valid_user");
        assert!(result.is_ok());
    }

    #[test]
    fn username_accepts_numbers() {
        let result = Username::new("user123");
        assert!(result.is_ok());
    }

    #[test]
    fn username_accepts_valid() {
        let result = Username::new("valid_user123");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_str(), "valid_user123");
    }

    #[test]
    fn username_display() {
        let username = Username::new("telegram_user").unwrap();
        assert_eq!(format!("{}", username), "telegram_user");
    }

    #[test]
    fn username_serde_transparent() {
        let username = Username::new("testuser").unwrap();
        let json = serde_json::to_string(&username).unwrap();
        assert_eq!(json, "\"testuser\"");
    }

    // =========================================================================
    // ChannelName Tests
    // =========================================================================

    #[test]
    fn channel_name_rejects_empty() {
        let result = ChannelName::new("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn channel_name_rejects_whitespace_only() {
        let result = ChannelName::new("   ");
        assert!(result.is_err());
    }

    #[test]
    fn channel_name_trims_whitespace() {
        let result = ChannelName::new("  Tech News  ");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_str(), "Tech News");
    }

    #[test]
    fn channel_name_accepts_valid() {
        let result = ChannelName::new("AI Updates");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_str(), "AI Updates");
    }

    #[test]
    fn channel_name_display() {
        let name = ChannelName::new("News Channel").unwrap();
        assert_eq!(format!("{}", name), "News Channel");
    }

    // =========================================================================
    // MediaType Tests
    // =========================================================================

    #[test]
    fn media_type_default_is_none() {
        assert_eq!(MediaType::default(), MediaType::None);
    }

    #[test]
    fn media_type_serde_lowercase() {
        let json = serde_json::to_string(&MediaType::VideoNote).unwrap();
        assert_eq!(json, "\"videonote\"");
    }

    #[test]
    fn media_type_all_variants_serialize() {
        let variants = vec![
            MediaType::None,
            MediaType::Photo,
            MediaType::Video,
            MediaType::Document,
            MediaType::Audio,
            MediaType::Voice,
            MediaType::VideoNote,
            MediaType::Animation,
            MediaType::Sticker,
            MediaType::Contact,
            MediaType::Location,
            MediaType::Venue,
            MediaType::Poll,
            MediaType::Dice,
        ];

        for variant in variants {
            let json = serde_json::to_string(&variant);
            assert!(json.is_ok());
        }
    }

    // =========================================================================
    // Message Tests
    // =========================================================================

    #[test]
    fn message_is_recent_within_window() {
        let msg = Message {
            id: MessageId::new(1).unwrap(),
            channel_id: ChannelId::new(100).unwrap(),
            channel_name: ChannelName::new("Test").unwrap(),
            channel_username: Username::new("testchan").unwrap(),
            text: "test".to_string(),
            timestamp: Utc::now() - chrono::Duration::hours(24),
            sender_id: None,
            sender_name: None,
            has_media: false,
            media_type: MediaType::None,
        };

        assert!(msg.is_recent(48));
        assert!(!msg.is_recent(12));
    }

    #[test]
    fn message_is_text_only() {
        let msg = Message {
            id: MessageId::new(1).unwrap(),
            channel_id: ChannelId::new(100).unwrap(),
            channel_name: ChannelName::new("Test").unwrap(),
            channel_username: Username::new("testchan").unwrap(),
            text: "test".to_string(),
            timestamp: Utc::now(),
            sender_id: None,
            sender_name: None,
            has_media: false,
            media_type: MediaType::None,
        };

        assert!(msg.is_text_only());
    }

    #[test]
    fn message_with_photo_not_text_only() {
        let msg = Message {
            id: MessageId::new(1).unwrap(),
            channel_id: ChannelId::new(100).unwrap(),
            channel_name: ChannelName::new("Test").unwrap(),
            channel_username: Username::new("testchan").unwrap(),
            text: "".to_string(),
            timestamp: Utc::now(),
            sender_id: None,
            sender_name: None,
            has_media: true,
            media_type: MediaType::Photo,
        };

        assert!(!msg.is_text_only());
    }

    #[test]
    fn message_serialization() {
        let msg = Message {
            id: MessageId::new(1).unwrap(),
            channel_id: ChannelId::new(100).unwrap(),
            channel_name: ChannelName::new("Test").unwrap(),
            channel_username: Username::new("testchan").unwrap(),
            text: "Hello world".to_string(),
            timestamp: Utc::now(),
            sender_id: Some(UserId::new(42).unwrap()),
            sender_name: Some("Alice".to_string()),
            has_media: false,
            media_type: MediaType::None,
        };

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, msg.id);
        assert_eq!(deserialized.channel_id, msg.channel_id);
        assert_eq!(deserialized.text, msg.text);
    }

    // =========================================================================
    // Channel Tests
    // =========================================================================

    #[test]
    fn channel_serialization() {
        let channel = Channel {
            id: ChannelId::new(200).unwrap(),
            name: ChannelName::new("Tech News").unwrap(),
            username: Username::new("technews").unwrap(),
            description: Some("Latest tech updates".to_string()),
            member_count: 5000,
            is_verified: true,
            is_public: true,
            is_subscribed: true,
            last_message_date: Some(Utc::now()),
        };

        let json = serde_json::to_string(&channel).unwrap();
        let deserialized: Channel = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, channel.id);
        assert_eq!(deserialized.member_count, channel.member_count);
        assert_eq!(deserialized.is_verified, channel.is_verified);
    }

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
    }

    #[test]
    fn search_params_new() {
        let params = SearchParams::new("AI news");
        assert_eq!(params.query, "AI news");
        assert_eq!(params.hours_back, 48);
        assert_eq!(params.limit, 20);
    }

    #[test]
    fn search_params_constants() {
        assert_eq!(SearchParams::DEFAULT_HOURS_BACK, 48);
        assert_eq!(SearchParams::MAX_HOURS_BACK, 72);
        assert_eq!(SearchParams::DEFAULT_LIMIT, 20);
        assert_eq!(SearchParams::MAX_LIMIT, 100);
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
