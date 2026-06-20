//! Domain entities: Message and Channel.

use super::ids::{ChannelId, MessageId, UserId};
use super::media::{AudioInfo, MediaType, VideoInfo};
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
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub forwarded_from: Option<ForwardInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub link_preview: Option<LinkPreview>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub views: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub forwards: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reply_to_message_id: Option<MessageId>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub video_info: Option<VideoInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub audio_info: Option<AudioInfo>,
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

/// Attribution for a forwarded message.
///
/// `channel_name` / `channel_username` are intentionally never populated: the
/// grammers forward header carries only the source's numeric `from_id`, and the
/// resolved title/username live in the response's peer map (not exposed per
/// message). Filling them would require an extra resolve call, which the
/// zero-extra-call enrichment path must avoid.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ForwardInfo {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub channel_id: Option<ChannelId>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub channel_name: Option<ChannelName>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub channel_username: Option<Username>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sender_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub original_date: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub original_message_id: Option<MessageId>,
}

/// Telegram's server-side webpage preview attached to a message.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LinkPreview {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub site_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
}

/// A Telegram channel or group.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Channel {
    pub id: ChannelId,
    pub name: ChannelName,
    pub username: Username,
    pub description: Option<String>,
    /// Number of members/subscribers. `None` means the count was not fetched
    /// (distinct from a real zero); the cheap list/lookup paths leave it unset.
    pub member_count: Option<u64>,
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
            forwarded_from: None,
            link_preview: None,
            views: None,
            forwards: None,
            reply_to_message_id: None,
            video_info: None,
            audio_info: None,
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
    fn message_omits_new_fields_when_absent() {
        let msg = create_test_message();
        let json = serde_json::to_value(&msg).unwrap();
        assert!(json.get("forwarded_from").is_none());
        assert!(json.get("link_preview").is_none());
        assert!(json.get("views").is_none());
        assert!(json.get("forwards").is_none());
        assert!(json.get("reply_to_message_id").is_none());
        assert!(json.get("video_info").is_none());
        assert!(json.get("audio_info").is_none());
    }

    #[test]
    fn message_includes_new_fields_when_present() {
        let mut msg = create_test_message();
        msg.views = Some(1234);
        msg.forwards = Some(56);
        msg.reply_to_message_id = Some(MessageId::new(99).unwrap());
        msg.forwarded_from = Some(ForwardInfo {
            channel_id: Some(ChannelId::new(100).unwrap()),
            channel_name: None,
            channel_username: None,
            sender_name: None,
            original_date: None,
            original_message_id: Some(MessageId::new(7).unwrap()),
        });
        msg.link_preview = Some(LinkPreview {
            url: "https://example.com".to_string(),
            site_name: Some("Example".to_string()),
            title: Some("Title".to_string()),
            description: Some("Desc".to_string()),
        });

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["views"], 1234);
        assert_eq!(json["forwards"], 56);
        assert_eq!(json["reply_to_message_id"], 99);
        assert_eq!(json["forwarded_from"]["channel_id"], 100);
        assert_eq!(json["forwarded_from"]["original_message_id"], 7);
        assert!(json["forwarded_from"].get("channel_name").is_none());
        assert_eq!(json["link_preview"]["url"], "https://example.com");
    }

    #[test]
    fn message_includes_video_and_audio_info_when_present() {
        use super::super::media::{AudioInfo, AudioKind, VideoInfo, VideoKind};

        let mut msg = create_test_message();
        msg.has_media = true;
        msg.media_type = MediaType::Video;
        msg.video_info = Some(VideoInfo {
            duration_seconds: 42,
            width: 1280,
            height: 720,
            file_size_bytes: 9_000_000,
            kind: VideoKind::Video,
            has_thumbnail: true,
            mime_type: Some("video/mp4".to_string()),
        });
        msg.audio_info = Some(AudioInfo {
            duration_seconds: 8,
            file_size_bytes: 4096,
            kind: AudioKind::Voice,
            mime_type: None,
        });

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["video_info"]["kind"], "video");
        assert_eq!(json["video_info"]["duration_seconds"], 42);
        assert_eq!(json["video_info"]["has_thumbnail"], true);
        assert_eq!(json["audio_info"]["kind"], "voice");
        assert!(json["audio_info"].get("mime_type").is_none());
    }

    #[test]
    fn channel_serialization() {
        let channel = Channel {
            id: ChannelId::new(200).unwrap(),
            name: ChannelName::new("Tech News").unwrap(),
            username: Username::new("technews").unwrap(),
            description: Some("Latest tech updates".to_string()),
            member_count: Some(5000),
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

    #[test]
    fn channel_member_count_none_serializes_as_null_when_unfetched() {
        // member_count is Option<u64>: None means "not fetched" and must stay
        // distinguishable from a real zero (CQ-4). It serializes as JSON null.
        let channel = Channel {
            id: ChannelId::new(201).unwrap(),
            name: ChannelName::new("No Count").unwrap(),
            username: Username::new("nocount").unwrap(),
            description: None,
            member_count: None,
            is_verified: false,
            is_public: false,
            is_subscribed: true,
            last_message_date: None,
        };

        let json = serde_json::to_value(&channel).unwrap();
        assert!(
            json["member_count"].is_null(),
            "unfetched member_count must serialize as null, got {}",
            json["member_count"]
        );

        let back: Channel = serde_json::from_value(json).unwrap();
        assert_eq!(back.member_count, None);
    }
}
