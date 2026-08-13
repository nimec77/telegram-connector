use super::*;

#[test]
fn status_response_serializes() {
    let response = StatusResponse {
        telegram_connected: true,
        rate_limiter: RateLimiterStatus {
            tokens: 45.5,
            capacity: 50.0,
            refill_per_sec: 2.0,
            costs: RateLimiterCosts {
                search: 1,
                media_download: 5,
                transcription: 5,
            },
        },
        server_version: "0.1.0".to_string(),
        requests_received: 1,
        responses_written: 1,
        last_response_write_age_secs: Some(0),
        session_started_at: "2026-06-12T00:00:00+00:00".to_string(),
        session_uptime_secs: 60,
        premium: Some(true),
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("telegram_connected"));
    assert!(json.contains("true"));
}

#[test]
fn message_link_response_serializes() {
    let response = MessageLinkResponse {
        channel_id: "123".to_string(),
        message_id: 456,
        https_link: "https://t.me/c/123/456".to_string(),
        tg_protocol_link: Some("tg://privatepost?channel=123&post=456".to_string()),
        internal_link: "https://t.me/c/123/456".to_string(),
        is_public: false,
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("https_link"));
    assert!(json.contains("tg_protocol_link"));
    assert!(json.contains("internal_link"));
    assert!(json.contains("is_public"));
}

#[test]
fn open_message_response_serializes() {
    let response = OpenMessageResponse {
        success: true,
        message: "Message opened".to_string(),
        link_used: "tg://link".to_string(),
        app_opened: true,
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("success"));
    assert!(json.contains("app_opened"));
}

#[test]
fn get_message_media_response_serializes() {
    use crate::telegram::types::MediaType;

    let response = GetMessageMediaResponse {
        channel_id: "news".to_string(),
        message_id: 42,
        media_type: MediaType::Photo,
        is_thumbnail: false,
        caption: Some("benchmark table".to_string()),
        source_variant_width: Some(2560),
        source_variant_height: Some(1440),
        source_variant_size_bytes: 400_000,
        largest_available_width: Some(4096),
        largest_available_height: Some(2160),
        returned_width: 1280,
        returned_height: 720,
        returned_size_bytes: 150_000,
        mime_type: "image/jpeg".to_string(),
        video_info: None,
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"media_type\":\"photo\""));
    assert!(json.contains("\"is_thumbnail\":false"));
    assert!(json.contains("benchmark table"));
}

#[test]
fn message_response_maps_and_omits_absent_fields() {
    use crate::telegram::types::{ChannelId, ChannelName, MediaType, Message, MessageId, Username};

    let msg = Message {
        id: MessageId::new(1).unwrap(),
        channel_id: ChannelId::new(100).unwrap(),
        channel_name: ChannelName::new("Test").unwrap(),
        channel_username: Some(Username::new("testchan").unwrap()),
        text: "hi".to_string(),
        timestamp: chrono::Utc::now(),
        sender_id: None,
        sender_name: None,
        has_media: false,
        media_type: MediaType::None,
        forwarded_from: None,
        link_preview: None,
        views: Some(10),
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
    };

    let dto = MessageResponse::from(msg);
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(json["views"], 10);
    assert!(json.get("forwards").is_none());
    assert!(json.get("forwarded_from").is_none());
    // sender_id mirrors the domain type: present as null, not skipped.
    assert!(json.get("sender_id").is_some());
    assert!(json["sender_id"].is_null());
}

#[test]
fn message_response_maps_video_info() {
    use crate::telegram::types::{
        ChannelId, ChannelName, MediaType, Message, MessageId, Username, VideoInfo, VideoKind,
    };

    let msg = Message {
        id: MessageId::new(1).unwrap(),
        channel_id: ChannelId::new(100).unwrap(),
        channel_name: ChannelName::new("Test").unwrap(),
        channel_username: Some(Username::new("testchan").unwrap()),
        text: String::new(),
        timestamp: chrono::Utc::now(),
        sender_id: None,
        sender_name: None,
        has_media: true,
        media_type: MediaType::Video,
        forwarded_from: None,
        link_preview: None,
        views: None,
        forwards: None,
        reply_to_message_id: None,
        video_info: Some(VideoInfo {
            duration_seconds: 30,
            width: 1920,
            height: 1080,
            file_size_bytes: 5_000_000,
            kind: VideoKind::Video,
            has_thumbnail: true,
            mime_type: Some("video/mp4".to_string()),
        }),
        audio_info: None,
        document_info: None,
        grouped_id: None,
        link: "https://t.me/testchan/1".to_string(),
        reactions: None,
        reactions_total: None,
        album: None,
    };

    let dto = MessageResponse::from(msg);
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(json["video_info"]["kind"], "video");
    assert_eq!(json["video_info"]["width"], 1920);
    assert!(json.get("audio_info").is_none());
}

#[test]
fn search_response_maps_from_search_result() {
    use crate::telegram::types::{
        ChannelId, ChannelName, MediaType, Message, MessageId, QueryMetadata, SearchResult,
        Username,
    };

    let result = SearchResult {
        messages: vec![Message {
            id: MessageId::new(1).unwrap(),
            channel_id: ChannelId::new(100).unwrap(),
            channel_name: ChannelName::new("Test").unwrap(),
            channel_username: Some(Username::new("testchan").unwrap()),
            text: "hi".to_string(),
            timestamp: chrono::Utc::now(),
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
        }],
        returned: 1,
        has_more: false,
        search_time_ms: 5,
        query_metadata: QueryMetadata {
            query: "x".to_string(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(48),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
        },
    };

    let dto = SearchResponse::from(result);
    assert_eq!(dto.messages.len(), 1);
    assert_eq!(dto.returned, 1);
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(json["query_metadata"]["query"], "x");
}
