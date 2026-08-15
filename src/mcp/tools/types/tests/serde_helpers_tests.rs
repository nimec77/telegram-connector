use super::*;
use crate::mcp::tools::types::requests::ResponseFormat;
use crate::telegram::types::MediaFilter;
use serde::Deserialize;

#[derive(Deserialize)]
struct OptU32T {
    #[serde(default, deserialize_with = "flexible_opt_int")]
    v: Option<u32>,
}

#[test]
fn opt_u32_accepts_number() {
    let t: OptU32T = serde_json::from_str(r#"{"v": 10}"#).unwrap();
    assert_eq!(t.v, Some(10));
}

#[test]
fn opt_u32_accepts_numeric_string() {
    let t: OptU32T = serde_json::from_str(r#"{"v": "10"}"#).unwrap();
    assert_eq!(t.v, Some(10));
}

#[test]
fn opt_u32_trims_whitespace() {
    let t: OptU32T = serde_json::from_str(r#"{"v": " 10 "}"#).unwrap();
    assert_eq!(t.v, Some(10));
}

#[test]
fn opt_u32_empty_string_is_none() {
    let t: OptU32T = serde_json::from_str(r#"{"v": ""}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_u32_whitespace_only_string_is_none() {
    let t: OptU32T = serde_json::from_str(r#"{"v": "   "}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_u32_missing_is_none() {
    let t: OptU32T = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_u32_null_is_none() {
    let t: OptU32T = serde_json::from_str(r#"{"v": null}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_u32_float_string_errors() {
    assert!(serde_json::from_str::<OptU32T>(r#"{"v": "1.5"}"#).is_err());
}

#[test]
fn opt_u32_garbage_string_errors() {
    assert!(serde_json::from_str::<OptU32T>(r#"{"v": "abc"}"#).is_err());
}

#[test]
fn opt_u32_negative_string_errors() {
    assert!(serde_json::from_str::<OptU32T>(r#"{"v": "-5"}"#).is_err());
}

#[test]
fn opt_u32_negative_number_errors() {
    assert!(serde_json::from_str::<OptU32T>(r#"{"v": -5}"#).is_err());
}

#[test]
fn opt_u32_float_number_errors() {
    assert!(serde_json::from_str::<OptU32T>(r#"{"v": 10.0}"#).is_err());
}

#[derive(Deserialize)]
struct OptIntBothT {
    #[serde(default, deserialize_with = "flexible_opt_int")]
    small: Option<u32>,
    #[serde(default, deserialize_with = "flexible_opt_int")]
    wide: Option<i64>,
}

#[test]
fn flexible_opt_int_serves_both_widths() {
    let t: OptIntBothT = serde_json::from_str(r#"{"small": "10", "wide": -5}"#).unwrap();
    assert_eq!(t.small, Some(10));
    assert_eq!(t.wide, Some(-5));
    assert!(serde_json::from_str::<OptIntBothT>(r#"{"small": -1}"#).is_err());
}

#[derive(Deserialize)]
struct OptEnumBothT {
    #[serde(default, deserialize_with = "flexible_opt_enum")]
    format: Option<ResponseFormat>,
    #[serde(default, deserialize_with = "flexible_opt_enum")]
    filter: Option<MediaFilter>,
}

#[test]
fn flexible_opt_enum_serves_both_enums() {
    let t: OptEnumBothT =
        serde_json::from_str(r#"{"format": "compact", "filter": "photo"}"#).unwrap();
    assert_eq!(t.format, Some(ResponseFormat::Compact));
    assert_eq!(t.filter, Some(MediaFilter::Photo));
    let empty: OptEnumBothT = serde_json::from_str(r#"{"format": "", "filter": ""}"#).unwrap();
    assert!(empty.format.is_none() && empty.filter.is_none());
}

#[derive(Deserialize)]
struct I64T {
    #[serde(deserialize_with = "flexible_i64")]
    v: i64,
}

#[test]
fn i64_accepts_number() {
    let t: I64T = serde_json::from_str(r#"{"v": 575403}"#).unwrap();
    assert_eq!(t.v, 575403);
}

#[test]
fn i64_accepts_numeric_string() {
    let t: I64T = serde_json::from_str(r#"{"v": "575403"}"#).unwrap();
    assert_eq!(t.v, 575403);
}

#[test]
fn i64_accepts_negative_string() {
    let t: I64T = serde_json::from_str(r#"{"v": " -42 "}"#).unwrap();
    assert_eq!(t.v, -42);
}

#[test]
fn i64_empty_string_errors() {
    assert!(serde_json::from_str::<I64T>(r#"{"v": ""}"#).is_err());
}

#[test]
fn i64_garbage_errors() {
    assert!(serde_json::from_str::<I64T>(r#"{"v": "abc"}"#).is_err());
}

#[test]
fn i64_missing_errors() {
    assert!(serde_json::from_str::<I64T>(r#"{}"#).is_err());
}

#[test]
fn i64_null_errors() {
    assert!(serde_json::from_str::<I64T>(r#"{"v": null}"#).is_err());
}

#[test]
fn i64_float_number_errors() {
    assert!(serde_json::from_str::<I64T>(r#"{"v": 1.5}"#).is_err());
}

#[test]
fn flexible_opt_int_accepts_number_string_and_null() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(
            default,
            deserialize_with = "crate::mcp::tools::types::serde_helpers::flexible_opt_int"
        )]
        v: Option<i64>,
    }
    let n: Probe = serde_json::from_str(r#"{"v": 610119}"#).unwrap();
    assert_eq!(n.v, Some(610_119));
    let s: Probe = serde_json::from_str(r#"{"v": "610119"}"#).unwrap();
    assert_eq!(s.v, Some(610_119));
    let null: Probe = serde_json::from_str(r#"{"v": null}"#).unwrap();
    assert_eq!(null.v, None);
    let absent: Probe = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(absent.v, None);
    let blank: Probe = serde_json::from_str(r#"{"v": "  "}"#).unwrap();
    assert_eq!(blank.v, None);
}

#[derive(Deserialize)]
struct TestStruct {
    #[serde(default, deserialize_with = "flexible_opt_enum")]
    media_filter: Option<MediaFilter>,
}

#[test]
fn deserialize_none_when_missing() {
    let json = r#"{}"#;
    let result: TestStruct = serde_json::from_str(json).unwrap();
    assert!(result.media_filter.is_none());
}

#[test]
fn deserialize_none_when_null() {
    let json = r#"{"media_filter": null}"#;
    let result: TestStruct = serde_json::from_str(json).unwrap();
    assert!(result.media_filter.is_none());
}

#[test]
fn deserialize_none_when_empty_string() {
    let json = r#"{"media_filter": ""}"#;
    let result: TestStruct = serde_json::from_str(json).unwrap();
    assert!(result.media_filter.is_none());
}

#[test]
fn deserialize_valid_filter() {
    let json = r#"{"media_filter": "photo"}"#;
    let result: TestStruct = serde_json::from_str(json).unwrap();
    assert_eq!(result.media_filter, Some(MediaFilter::Photo));
}

#[test]
fn deserialize_snake_case_filter() {
    let json = r#"{"media_filter": "photo_video"}"#;
    let result: TestStruct = serde_json::from_str(json).unwrap();
    assert_eq!(result.media_filter, Some(MediaFilter::PhotoVideo));
}

#[derive(Deserialize)]
struct StrT {
    #[serde(deserialize_with = "flexible_string")]
    v: String,
}

#[test]
fn string_accepts_string() {
    let t: StrT = serde_json::from_str(r#"{"v": "swodki"}"#).unwrap();
    assert_eq!(t.v, "swodki");
}

#[test]
fn string_accepts_integer_number() {
    let t: StrT = serde_json::from_str(r#"{"v": 123}"#).unwrap();
    assert_eq!(t.v, "123");
}

#[test]
fn string_accepts_negative_integer() {
    let t: StrT = serde_json::from_str(r#"{"v": -1001234}"#).unwrap();
    assert_eq!(t.v, "-1001234");
}

#[test]
fn string_passes_empty_through() {
    let t: StrT = serde_json::from_str(r#"{"v": ""}"#).unwrap();
    assert_eq!(t.v, "");
}

#[test]
fn string_float_number_errors() {
    assert!(serde_json::from_str::<StrT>(r#"{"v": 1.5}"#).is_err());
}

#[test]
fn string_null_errors() {
    assert!(serde_json::from_str::<StrT>(r#"{"v": null}"#).is_err());
}

#[derive(Deserialize)]
struct OptStrT {
    #[serde(default, deserialize_with = "flexible_opt_string")]
    v: Option<String>,
}

#[test]
fn opt_string_accepts_string() {
    let t: OptStrT = serde_json::from_str(r#"{"v": "tech_news"}"#).unwrap();
    assert_eq!(t.v, Some("tech_news".to_string()));
}

#[test]
fn opt_string_accepts_integer_number() {
    let t: OptStrT = serde_json::from_str(r#"{"v": 123}"#).unwrap();
    assert_eq!(t.v, Some("123".to_string()));
}

#[test]
fn opt_string_empty_is_none() {
    let t: OptStrT = serde_json::from_str(r#"{"v": ""}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_string_missing_is_none() {
    let t: OptStrT = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_string_null_is_none() {
    let t: OptStrT = serde_json::from_str(r#"{"v": null}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_string_whitespace_only_is_none() {
    let t: OptStrT = serde_json::from_str(r#"{"v": "   "}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_string_float_number_errors() {
    assert!(serde_json::from_str::<OptStrT>(r#"{"v": 1.5}"#).is_err());
}

#[derive(Deserialize)]
struct OptBoolT {
    #[serde(default, deserialize_with = "flexible_opt_bool")]
    v: Option<bool>,
}

#[test]
fn opt_bool_accepts_bool() {
    let t: OptBoolT = serde_json::from_str(r#"{"v": true}"#).unwrap();
    assert_eq!(t.v, Some(true));
}

#[test]
fn opt_bool_accepts_true_string() {
    let t: OptBoolT = serde_json::from_str(r#"{"v": "true"}"#).unwrap();
    assert_eq!(t.v, Some(true));
}

#[test]
fn opt_bool_accepts_false_string_case_insensitive() {
    let t: OptBoolT = serde_json::from_str(r#"{"v": "FALSE"}"#).unwrap();
    assert_eq!(t.v, Some(false));
}

#[test]
fn opt_bool_accepts_numeric_one_and_zero() {
    let one: OptBoolT = serde_json::from_str(r#"{"v": 1}"#).unwrap();
    let zero: OptBoolT = serde_json::from_str(r#"{"v": 0}"#).unwrap();
    assert_eq!(one.v, Some(true));
    assert_eq!(zero.v, Some(false));
}

#[test]
fn opt_bool_accepts_string_one_and_zero() {
    let one: OptBoolT = serde_json::from_str(r#"{"v": "1"}"#).unwrap();
    let zero: OptBoolT = serde_json::from_str(r#"{"v": "0"}"#).unwrap();
    assert_eq!(one.v, Some(true));
    assert_eq!(zero.v, Some(false));
}

#[test]
fn opt_bool_trims_whitespace() {
    let t: OptBoolT = serde_json::from_str(r#"{"v": " True "}"#).unwrap();
    assert_eq!(t.v, Some(true));
}

#[test]
fn opt_bool_empty_is_none() {
    let t: OptBoolT = serde_json::from_str(r#"{"v": ""}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_bool_missing_is_none() {
    let t: OptBoolT = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_bool_null_is_none() {
    let t: OptBoolT = serde_json::from_str(r#"{"v": null}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_bool_invalid_number_errors() {
    assert!(serde_json::from_str::<OptBoolT>(r#"{"v": 2}"#).is_err());
}

#[test]
fn opt_bool_invalid_string_errors() {
    assert!(serde_json::from_str::<OptBoolT>(r#"{"v": "yes"}"#).is_err());
}
