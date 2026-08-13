//! Domain entities: Message and Channel.

use super::ids::{ChannelId, MessageId, UserId};
use super::media::{AudioInfo, DocumentInfo, MediaType, VideoInfo};
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
    pub channel_username: Option<Username>,
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
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub document_info: Option<DocumentInfo>,
    /// Telegram album (media group) id shared by sibling messages (B5).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub grouped_id: Option<i64>,
    /// Permalink: public t.me form when the channel has a username (D1).
    pub link: String,
    /// Standard-emoji reactions, when any (D2).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reactions: Option<Vec<MessageReaction>>,
    /// Total reactions of every kind, including custom/paid (D2).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reactions_total: Option<u64>,
    /// Post-level album summary, present only on a collapsed post (B5/A2).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub album: Option<AlbumInfo>,
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
/// `channel_name` / `channel_username` / `sender_name` are resolved from the
/// same response envelope the message arrived in (its `chats` + `users`
/// arrays) — never from an extra resolve call. When the envelope does not
/// contain the source peer, the ids-only form is emitted instead; nothing is
/// fabricated (zero-extra-call enrichment invariant).
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
    /// Author signature on signed channel posts (fwd header `post_author`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_author: Option<String>,
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

/// One standard-emoji reaction with its count (work-order D2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MessageReaction {
    pub emoji: String,
    pub count: u64,
}

/// Post-level album summary on a collapsed message (work-order B5/A2).
///
/// Describes only the siblings present in this result set: an album that
/// straddles the fetched window (cut off by `limit`, `media_filter`, a
/// `from_date`/`to_date` bound, or global-search adjacency) is partially
/// represented here, not the full album Telegram holds. A lone surviving
/// sibling is indistinguishable from a genuine non-album post and appears as
/// a plain message with no `album` field at all.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AlbumInfo {
    /// Number of sibling messages in the album present in this result set.
    pub media_count: u32,
    /// Media type of each sibling present in this result set, in ascending id order.
    pub media_types: Vec<MediaType>,
    /// Sibling message ids present in this result set, ascending.
    pub message_ids: Vec<MessageId>,
}

/// Kind of chat a `Channel` object describes (work-order B9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChatType {
    /// Broadcast channel.
    Channel,
    /// Small (basic) group, incl. grammers `Community` peers.
    Group,
    /// Megagroup.
    Supergroup,
}

/// A Telegram channel or group.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Channel {
    pub id: ChannelId,
    pub name: ChannelName,
    pub username: Option<Username>,
    pub chat_type: ChatType,
    pub description: Option<String>,
    /// Number of members/subscribers. `None` means the count was not fetched
    /// (distinct from a real zero); the cheap list/lookup paths leave it unset.
    pub member_count: Option<u64>,
    pub is_verified: bool,
    pub is_public: bool,
    pub is_subscribed: bool,
    pub last_message_date: Option<DateTime<Utc>>,
}

/// One page of the subscribed-channel list plus the genuine full count.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChannelPage {
    pub channels: Vec<Channel>,
    /// Total subscribed channels/groups across the entire dialog list —
    /// a real total, not the page size (work-order B6).
    pub total: usize,
}

/// A channel's canonical numeric ID plus its public username, if any.
///
/// Unlike [`Channel::username`], there are no fallback sentinels here — `None`
/// means the chat has no public username. Used by link generation (work-order
/// B2), where a sentinel would fabricate a `t.me/unknown/…` link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelIdentity {
    pub id: ChannelId,
    pub username: Option<String>,
}

/// Outcome of resolving one identifier in resolve_channels (work-order A7).
/// Exactly one of `channel` / `error` is present.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChannelResolution {
    #[schemars(description = "The identifier as passed in the request")]
    pub identifier: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub channel: Option<Channel>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
}

/// Result of a batch message fetch: found messages plus the ids Telegram
/// reported as deleted/never-existed (work-order A1). Order follows the
/// request's id order in both vectors.
///
/// Invariant: every requested id ends up in exactly one of `messages` /
/// `missing_ids` — never both, never neither. An id whose slot converts to a
/// domain `Message` lands in `messages`; every other case (absent slot,
/// `MessageEmpty` placeholder, or a present-but-unconvertible message) lands
/// in `missing_ids`. The last case is logged as a warning when it happens,
/// since it means Telegram returned a real message that this client could
/// not represent domain-side.
#[derive(Debug, Clone)]
pub struct MessageBatch {
    pub messages: Vec<Message>,
    pub missing_ids: Vec<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_info_serializes_post_author_and_skips_absent_fields() {
        let info = ForwardInfo {
            channel_id: Some(ChannelId::new(1783384254).expect("valid id")),
            channel_name: None,
            channel_username: None,
            sender_name: None,
            post_author: Some("Иван Петров".to_string()),
            original_date: None,
            original_message_id: None,
        };
        let json = serde_json::to_value(&info).expect("serializes");
        assert_eq!(json["post_author"], "Иван Петров");
        // Absent optionals must be skipped, not null (backward-compatible shape).
        assert!(json.get("channel_name").is_none());
        assert!(json.get("sender_name").is_none());

        let bare = ForwardInfo {
            channel_id: None,
            channel_name: None,
            channel_username: None,
            sender_name: None,
            post_author: None,
            original_date: None,
            original_message_id: None,
        };
        let json = serde_json::to_value(&bare).expect("serializes");
        assert!(json.get("post_author").is_none());
    }

    fn create_test_message() -> Message {
        Message {
            id: MessageId::new(1).unwrap(),
            channel_id: ChannelId::new(100).unwrap(),
            channel_name: ChannelName::new("Test").unwrap(),
            channel_username: Some(Username::new("testchan").unwrap()),
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
            document_info: None,
            grouped_id: None,
            link: "https://t.me/testchan/1".to_string(),
            reactions: None,
            reactions_total: None,
            album: None,
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
            post_author: None,
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
            title: None,
            performer: None,
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
            username: Some(Username::new("technews").unwrap()),
            chat_type: ChatType::Channel,
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
            username: Some(Username::new("nocount").unwrap()),
            chat_type: ChatType::Channel,
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
