//! Domain entities: Message and Channel.

use super::ids::{ChannelId, MessageId, UserId};
use super::media::MediaType;
use super::names::{ChannelName, Username};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A Telegram message.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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

/// A Telegram channel or group.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_message() -> Message {
        Message {
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
        }
    }

    #[test]
    fn message_is_recent_within_window() {
        let mut msg = create_test_message();
        msg.timestamp = Utc::now() - chrono::Duration::hours(24);

        assert!(msg.is_recent(48));
        assert!(!msg.is_recent(12));
    }

    #[test]
    fn message_is_text_only() {
        let msg = create_test_message();
        assert!(msg.is_text_only());
    }

    #[test]
    fn message_with_photo_not_text_only() {
        let mut msg = create_test_message();
        msg.has_media = true;
        msg.media_type = MediaType::Photo;

        assert!(!msg.is_text_only());
    }

    #[test]
    fn message_serialization() {
        let mut msg = create_test_message();
        msg.text = "Hello world".to_string();
        msg.sender_id = Some(UserId::new(42).unwrap());
        msg.sender_name = Some("Alice".to_string());

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, msg.id);
        assert_eq!(deserialized.channel_id, msg.channel_id);
        assert_eq!(deserialized.text, msg.text);
    }

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
}
