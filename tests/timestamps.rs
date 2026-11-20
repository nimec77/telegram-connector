use serde_json::json;
use telegram_connector::telegram::types::{Message, QueryMetadata};
use telegram_core::timestamp::UtcTimestamp;

#[test]
fn timestamps_preserve_utc_wire_formats_and_fractional_precision() {
    let timestamp: UtcTimestamp = "1969-12-31T18:59:59.1000000009-05:00"
        .parse()
        .expect("RFC 3339 offset");
    assert_eq!(timestamp.unix_seconds(), -1);
    assert_eq!(
        serde_json::to_value(timestamp).expect("JSON timestamp"),
        "1969-12-31T23:59:59.100Z"
    );
    assert_eq!(timestamp.to_rfc3339(), "1969-12-31T23:59:59.100+00:00");
    assert_eq!(timestamp.to_rfc3339_seconds(), "1969-12-31T23:59:59Z");
    for (input, expected) in [
        ("2026-11-20t00:00:00z", "2026-11-20T00:00:00Z"),
        ("2026-11-20 00:00:00.000001Z", "2026-11-20T00:00:00.000001Z"),
        (
            "2026-11-20T00:00:00.000000001Z",
            "2026-11-20T00:00:00.000000001Z",
        ),
        ("2017-01-01T00:59:60.2+01:00", "2016-12-31T23:59:60.200Z"),
    ] {
        let stamp: UtcTimestamp = input.parse().expect("compatible timestamp");
        assert_eq!(stamp.to_string(), expected);
        assert_eq!(
            serde_json::from_value::<UtcTimestamp>(json!(expected)).expect("round trip"),
            stamp
        );
    }
    for input in [
        "2026-11-20T00:00:00",
        "2026-11-20T00:00:00+0100",
        "2026-11-20T00:00:00Z[UTC]",
    ] {
        assert!(input.parse::<UtcTimestamp>().is_err(), "{input}");
    }
}

#[test]
fn public_timestamp_fields_keep_the_inline_date_time_schema() {
    let message_schema =
        serde_json::to_value(schemars::schema_for!(Message)).expect("message schema");
    let field = &message_schema["properties"]["timestamp"];
    assert_eq!(field["type"], "string");
    assert_eq!(field["format"], "date-time");
    assert!(field.get("$ref").is_none());
    let metadata_schema =
        serde_json::to_value(schemars::schema_for!(QueryMetadata)).expect("metadata schema");
    assert_eq!(
        metadata_schema["properties"]["window_from"]["format"],
        "date-time"
    );
    assert_eq!(
        metadata_schema["properties"]["window_to"]["format"],
        "date-time"
    );
    fn message_timestamp(message: &Message) -> UtcTimestamp {
        message.timestamp
    }
    let _ = message_timestamp;
}

#[test]
fn timestamps_preserve_calendar_edges_and_leap_second_arithmetic() {
    let upper: UtcTimestamp = "9999-12-31T23:59:59Z"
        .parse()
        .expect("four-digit upper bound");
    assert_eq!(upper.to_string(), "9999-12-31T23:59:59Z");
    let lower: UtcTimestamp = "0000-01-01T00:00:00+01:00"
        .parse()
        .expect("four-digit lower bound");
    assert_eq!(lower.to_string(), "-0001-12-31T23:00:00Z");
    assert_eq!(
        serde_json::from_value::<UtcTimestamp>(json!(lower.to_string()))
            .expect("UTC spill round trip"),
        lower
    );
    assert!(UtcTimestamp::parse_rfc3339(&lower.to_string()).is_err());
    for (input, utc) in [
        ("2026-11-20T00:00:00+0100", "2026-11-19T23:00:00Z"),
        ("2026-11-20 00:00:00 UTC", "2026-11-20T00:00:00Z"),
    ] {
        assert!(UtcTimestamp::parse_rfc3339(input).is_err());
        let decoded: UtcTimestamp =
            serde_json::from_value(json!(input)).expect("legacy DTO timestamp");
        assert_eq!(decoded.to_string(), utc);
    }
    let leap: UtcTimestamp = "2016-12-31T12:00:60.500Z".parse().expect("leap");
    let next: UtcTimestamp = "2016-12-31T12:01:00Z".parse().expect("next minute");
    assert_eq!((next - leap).as_millis(), 500);
    assert_eq!(
        leap.checked_add(std::time::Duration::from_millis(100))
            .expect("within leap")
            .to_string(),
        "2016-12-31T12:00:60.600Z"
    );
}
