use super::*;

#[test]
fn get_channels_request_deserializes() {
    let json = r#"{"limit": 10, "offset": 5}"#;
    let request: GetChannelsRequest = serde_json::from_str(json).unwrap();

    assert_eq!(request.limit, Some(10));
    assert_eq!(request.offset, Some(5));
}

#[test]
fn get_channels_request_defaults() {
    let json = r#"{}"#;
    let request: GetChannelsRequest = serde_json::from_str(json).unwrap();

    assert_eq!(request.limit, None);
    assert_eq!(request.offset, None);
}

#[test]
fn search_request_validates_required_query() {
    let json = r#"{"query": "test"}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();

    assert_eq!(request.query, "test");
    assert!(request.channel_id.is_none());
    assert!(request.media_filter.is_none());
}

#[test]
fn search_request_with_media_filter_deserializes() {
    let json = r#"{"query": "AI news", "media_filter": "photo"}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();

    assert_eq!(request.query, "AI news");
    assert_eq!(request.media_filter, Some(MediaFilter::Photo));
}

#[test]
fn search_request_media_filter_snake_case() {
    let json = r#"{"query": "", "media_filter": "photo_video"}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();

    assert_eq!(request.query, "");
    assert_eq!(request.media_filter, Some(MediaFilter::PhotoVideo));
}

#[test]
fn search_request_all_media_filters_deserialize() {
    let filters = vec![
        ("photo", MediaFilter::Photo),
        ("video", MediaFilter::Video),
        ("photo_video", MediaFilter::PhotoVideo),
        ("document", MediaFilter::Document),
        ("audio", MediaFilter::Audio),
        ("voice", MediaFilter::Voice),
        ("video_note", MediaFilter::VideoNote),
        ("gif", MediaFilter::Gif),
        ("url", MediaFilter::Url),
        ("pinned", MediaFilter::Pinned),
    ];

    for (json_value, expected) in filters {
        let json = format!(r#"{{"query": "test", "media_filter": "{}"}}"#, json_value);
        let request: SearchRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(
            request.media_filter,
            Some(expected),
            "Failed for filter: {}",
            json_value
        );
    }
}

#[test]
fn search_request_empty_string_media_filter_treated_as_none() {
    let json = r#"{"query": "test", "media_filter": ""}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();

    assert_eq!(request.query, "test");
    assert_eq!(request.media_filter, None);
}

#[test]
fn search_request_null_media_filter_treated_as_none() {
    let json = r#"{"query": "test", "media_filter": null}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();

    assert_eq!(request.query, "test");
    assert_eq!(request.media_filter, None);
}

#[test]
fn search_request_accepts_channel_ids() {
    let json = r#"{"query": "тест", "channel_ids": ["111", "222"]}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();

    assert!(request.channel_id.is_none());
    assert_eq!(
        request.channel_ids,
        Some(vec!["111".to_string(), "222".to_string()])
    );
}

#[test]
fn get_recent_messages_request_deserializes() {
    let json = r#"{"channel_id": "123456"}"#;
    let request: GetRecentMessagesRequest = serde_json::from_str(json).unwrap();

    assert_eq!(request.channel_id, Some("123456".to_string()));
    assert!(request.channel_ids.is_none());
    assert!(request.hours_back.is_none());
    assert!(request.limit.is_none());
    assert!(request.media_filter.is_none());
}

#[test]
fn get_recent_messages_request_accepts_channel_ids() {
    let json = r#"{"channel_ids": ["111", "222"]}"#;
    let request: GetRecentMessagesRequest = serde_json::from_str(json).unwrap();

    assert!(request.channel_id.is_none());
    assert_eq!(
        request.channel_ids,
        Some(vec!["111".to_string(), "222".to_string()])
    );
}

#[test]
fn get_recent_messages_request_with_all_params() {
    let json = r#"{
            "channel_id": "tech_news",
            "hours_back": 72,
            "limit": 50,
            "media_filter": "photo"
        }"#;
    let request: GetRecentMessagesRequest = serde_json::from_str(json).unwrap();

    assert_eq!(request.channel_id, Some("tech_news".to_string()));
    assert_eq!(request.hours_back, Some(72));
    assert_eq!(request.limit, Some(50));
    assert_eq!(request.media_filter, Some(MediaFilter::Photo));
}

#[test]
fn get_message_by_link_request_deserializes() {
    let json = r#"{"link": "https://t.me/swodki/575403"}"#;
    let request: GetMessageByLinkRequest = serde_json::from_str(json).unwrap();

    assert_eq!(request.link, "https://t.me/swodki/575403");
}

#[test]
fn get_recent_messages_request_empty_media_filter() {
    let json = r#"{"channel_id": "123", "media_filter": ""}"#;
    let request: GetRecentMessagesRequest = serde_json::from_str(json).unwrap();

    assert_eq!(request.channel_id, Some("123".to_string()));
    assert_eq!(request.media_filter, None);
}

#[test]
fn get_channels_request_accepts_string_numbers() {
    let json = r#"{"limit": "10", "offset": "5"}"#;
    let request: GetChannelsRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.limit, Some(10));
    assert_eq!(request.offset, Some(5));
}

#[test]
fn get_channels_request_empty_string_limit_is_none() {
    let json = r#"{"limit": ""}"#;
    let request: GetChannelsRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.limit, None);
}

#[test]
fn search_request_accepts_string_numbers() {
    let json = r#"{"query": "ai", "hours_back": "72", "limit": "50"}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.hours_back, Some(72));
    assert_eq!(request.limit, Some(50));
    assert_eq!(request.query, "ai");
}

#[test]
fn search_request_channel_id_accepts_number() {
    let json = r#"{"query": "ai", "channel_id": 123456}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.channel_id, Some("123456".to_string()));
}

#[test]
fn search_request_query_accepts_number() {
    let json = r#"{"query": 42}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.query, "42");
}

#[test]
fn generate_link_request_message_id_accepts_string() {
    let json = r#"{"channel_id": "123", "message_id": "575403"}"#;
    let request: GenerateLinkRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.channel_id, "123");
    assert_eq!(request.message_id, 575403);
}

#[test]
fn generate_link_request_channel_id_accepts_number() {
    let json = r#"{"channel_id": 456, "message_id": 1}"#;
    let request: GenerateLinkRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.channel_id, "456");
}

#[test]
fn open_message_request_bool_accepts_string() {
    let json = r#"{"channel_id": "1", "message_id": "2", "use_tg_protocol": "false"}"#;
    let request: OpenMessageRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.use_tg_protocol, Some(false));
}

#[test]
fn generate_link_request_bool_accepts_string_true() {
    let json = r#"{"channel_id": "1", "message_id": "2", "include_tg_protocol": "true"}"#;
    let request: GenerateLinkRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.include_tg_protocol, Some(true));
}

#[test]
fn get_recent_messages_request_channel_id_accepts_number() {
    let json = r#"{"channel_id": 123456}"#;
    let request: GetRecentMessagesRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.channel_id, Some("123456".to_string()));
}

#[test]
fn get_message_by_link_request_link_accepts_number() {
    let json = r#"{"link": 575403}"#;
    let request: GetMessageByLinkRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.link, "575403");
}

#[test]
fn get_channel_info_request_accepts_numeric_id() {
    let json = r#"{"channel_identifier": 1234567}"#;
    let request: GetChannelInfoRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.channel_identifier, "1234567");
}

#[test]
fn get_message_media_request_deserializes_with_flexible_scalars() {
    // message_id arrives as a numeric string; channel_id as a number.
    let json = r#"{"channel_id": 123456, "message_id": "42", "max_dimension": "640"}"#;
    let request: GetMessageMediaRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.channel_id, "123456");
    assert_eq!(request.message_id, 42);
    assert_eq!(request.max_dimension, Some(640));
}

#[test]
fn get_message_media_request_max_dimension_defaults_to_none() {
    let json = r#"{"channel_id": "news", "message_id": 42}"#;
    let request: GetMessageMediaRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.max_dimension, None);
}

#[test]
fn transcribe_request_deserializes_with_flexible_scalars() {
    let json = r#"{"channel_id": 123456, "message_id": "42", "timeout_seconds": "60"}"#;
    let request: TranscribeVoiceMessageRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.channel_id, "123456");
    assert_eq!(request.message_id, 42);
    assert_eq!(request.timeout_seconds, Some(60));
}

#[test]
fn transcribe_request_timeout_defaults_to_none() {
    let json = r#"{"channel_id": "news", "message_id": 42}"#;
    let request: TranscribeVoiceMessageRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.timeout_seconds, None);
}

#[test]
fn search_request_preserves_blank_from_date() {
    // A present-but-blank date must survive deserialization so the tool layer can
    // report it as invalid, rather than being folded to None ("no filter") and
    // silently widening the window.
    let json = r#"{"query": "ai", "from_date": ""}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.from_date, Some("".to_string()));
}

#[test]
fn search_request_preserves_whitespace_to_date() {
    let json = r#"{"query": "ai", "to_date": "   "}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.to_date, Some("   ".to_string()));
}

#[test]
fn search_request_null_dates_are_none() {
    let json = r#"{"query": "ai", "from_date": null, "to_date": null}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.from_date, None);
    assert_eq!(request.to_date, None);
}

#[test]
fn get_recent_messages_request_preserves_blank_dates() {
    let json = r#"{"channel_id": "123", "from_date": "", "to_date": " "}"#;
    let request: GetRecentMessagesRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.from_date, Some("".to_string()));
    assert_eq!(request.to_date, Some(" ".to_string()));
}
