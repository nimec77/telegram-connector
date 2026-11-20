use crate::mcp::tools::types::contract::ToolError;
use crate::mcp::tools::types::sending::{SendMediaRequest, SendMessageRequest};
use crate::telegram::types::sending::{SendFailure, SendMediaKind};
use serde_json::{Value, json};

fn text(text: &str) -> Value {
    json!({"recipient":{"kind":"saved_messages"},"text":text})
}
fn media() -> Value {
    json!({"recipient":{"kind":"saved_messages"},"source":{"root":"outgoing","relative_path":"demo.mp4"},
        "media":{"kind":"video","duration_seconds":12.5,"width":1280,"height":720}})
}
fn valid_text(value: Value) -> bool {
    serde_json::from_value::<SendMessageRequest>(value)
        .ok()
        .is_some_and(|r| r.try_into_domain().is_ok())
}
fn valid_media(value: Value) -> bool {
    serde_json::from_value::<SendMediaRequest>(value)
        .ok()
        .is_some_and(|r| r.try_into_domain().is_ok())
}

#[test]
fn sending_request_utf16_and_content_preservation() {
    assert!(valid_text(text(&"🦀".repeat(2048))));
    assert!(!valid_text(text(&"🦀".repeat(2049))));
    for invalid in ["", "\t\r\n "] {
        assert!(!valid_text(text(invalid)));
    }
    let request: SendMessageRequest =
        serde_json::from_value(text("  literal *text*\n")).expect("DTO");
    assert_eq!(
        request.try_into_domain().expect("valid").text,
        "  literal *text*\n"
    );
    let mut value = media();
    value["caption"] = json!("🦀".repeat(512));
    assert!(valid_media(value.clone()));
    value["caption"] = json!("🦀".repeat(513));
    assert!(!valid_media(value));
}

#[test]
fn sending_request_requires_qualified_valid_recipient_and_wire_ids() {
    for recipient in [
        json!({"kind":"saved_messages"}),
        json!({"kind":"username","username":"@alice","expected_namespace":"user"}),
        json!({"kind":"username","username":"news","expected_namespace":"channel"}),
        json!({"kind":"peer","namespace":"user","id":"9007199254740993"}),
        json!({"kind":"peer","namespace":"chat","id":91}),
        json!({"kind":"peer","namespace":"channel","id":"91"}),
    ] {
        let mut value = text("ok");
        value["recipient"] = recipient;
        assert!(valid_text(value));
    }
    for recipient in [
        json!(91),
        json!("alice"),
        json!({"kind":"username","username":"alice"}),
        json!({"kind":"username","username":"+1234","expected_namespace":"user"}),
        json!({"kind":"username","username":"https://t.me/alice","expected_namespace":"user"}),
        json!({"kind":"peer","namespace":"user","id":"9223372036854775808"}),
        json!({"kind":"peer","namespace":"channel","id":"-10091"}),
        json!({"kind":"peer","namespace":"channel","id":"+91"}),
        json!({"kind":"peer","namespace":"channel","id":"0"}),
    ] {
        let mut value = text("ok");
        value["recipient"] = recipient;
        assert!(!valid_text(value.clone()), "{value}");
    }
    for id in [json!(0), json!(-1), json!(2_147_483_648_i64)] {
        for field in ["reply_to_message_id", "topic_id"] {
            let mut value = text("ok");
            value[field] = id.clone();
            assert!(!valid_text(value));
        }
    }
}

#[test]
fn sending_request_rejects_unknown_fields_at_every_level() {
    let mut value = text("ok");
    value["silnet"] = json!(true);
    assert!(!valid_text(value));
    let mut value = text("ok");
    value["recipient"]["topic"] = json!(1);
    assert!(!valid_text(value));
    let mut value = media();
    value["source"]["url"] = json!("https://example.com/a");
    assert!(!valid_media(value));
    let mut value = media();
    value["media"]["duraton_seconds"] = json!(1);
    assert!(!valid_media(value));
    let mut value = media();
    value["media"] = json!({"kind":"image", "silent":true});
    assert!(!valid_media(value));
}

#[test]
fn sending_request_metadata_and_paths_are_validated_locally() {
    for invalid in [
        json!({"kind":"audio","duration_seconds":0}),
        json!({"kind":"audio","duration_seconds":2_147_483_648_i64}),
        json!({"kind":"voice","duration_seconds":1,"waveform":"extra"}),
        json!({"kind":"video","duration_seconds":0,"width":1,"height":1}),
        json!({"kind":"video","duration_seconds":1,"width":8193,"height":1}),
        json!({"kind":"video","duration_seconds":1,"width":1,"height":0}),
        json!({"kind":"document"}),
    ] {
        let mut value = media();
        value["media"] = invalid;
        assert!(!valid_media(value));
    }
    for kind in [
        json!({"kind":"image"}),
        json!({"kind":"voice","duration_seconds":1}),
        json!({"kind":"audio","duration_seconds":2_147_483_647_i64,"title":"demo","performer":"artist"}),
    ] {
        let mut value = media();
        value["media"] = kind;
        assert!(valid_media(value));
    }
    for path in ["", "/etc/passwd", "../a", "a/../../b"] {
        let mut value = media();
        value["source"]["relative_path"] = json!(path);
        assert!(!valid_media(value));
    }
    for duration_seconds in [f64::NAN, f64::INFINITY, -1.0] {
        assert!(
            SendMediaKind::Video {
                duration_seconds,
                width: 1,
                height: 1
            }
            .validate()
            .is_err()
        );
    }
    let mut value = media();
    value["media"] = json!({"kind":"audio","duration_seconds":1,"title":"🦀".repeat(129)});
    assert!(!valid_media(value));
}

#[test]
fn sending_request_unknown_outcome_never_has_retry_hint() {
    let error = serde_json::to_value(ToolError::from(SendFailure::unknown())).expect("safe error");
    assert_eq!(error["code"], "delivery_unknown");
    assert_eq!(error["delivery_state"], "unknown");
    assert_eq!(error["retryable"], false);
    assert!(error.get("retry_after_seconds").is_none());
}
