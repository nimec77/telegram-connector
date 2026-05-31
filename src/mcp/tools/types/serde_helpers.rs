//! Custom serde deserializers for MCP tool types.

use crate::telegram::types::MediaFilter;
use serde::de::Error;
use serde::{Deserialize, Deserializer};

/// Deserialize Option<MediaFilter> treating empty strings as None.
/// This handles MCP clients that send `"media_filter": ""` instead of omitting the field.
pub fn deserialize_optional_media_filter<'de, D>(
    deserializer: D,
) -> Result<Option<MediaFilter>, D::Error>
where
    D: Deserializer<'de>,
{
    // First try to deserialize as an Option<String> to check for empty string
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrMediaFilter {
        String(String),
        MediaFilter(MediaFilter),
        Null,
    }

    match Option::<StringOrMediaFilter>::deserialize(deserializer)? {
        None => Ok(None),
        Some(StringOrMediaFilter::Null) => Ok(None),
        Some(StringOrMediaFilter::String(s)) if s.is_empty() => Ok(None),
        Some(StringOrMediaFilter::String(s)) => {
            // Try to parse non-empty string as MediaFilter
            serde_json::from_value(serde_json::Value::String(s))
                .map(Some)
                .map_err(serde::de::Error::custom)
        }
        Some(StringOrMediaFilter::MediaFilter(f)) => Ok(Some(f)),
    }
}

/// Deserialize `Option<u32>` accepting either a JSON number or a numeric string.
///
/// The string form is trimmed before parsing. An empty/whitespace string or a
/// JSON `null` becomes `None`. Floats, negatives, out-of-range, and non-numeric
/// values produce an error.
pub fn flexible_opt_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(u32),
        Str(String),
    }

    match Option::<NumOrStr>::deserialize(deserializer)? {
        None => Ok(None),
        Some(NumOrStr::Num(n)) => Ok(Some(n)),
        Some(NumOrStr::Str(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed
                .parse::<u32>()
                .map(Some)
                .map_err(|_| Error::custom(format!("expected an integer, got '{}'", s)))
        }
    }
}

/// Deserialize `i64` accepting either a JSON number or a numeric string.
///
/// The string form is trimmed before parsing. Empty, non-numeric, or float
/// values produce an error.
pub fn flexible_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(i64),
        Str(String),
    }

    match NumOrStr::deserialize(deserializer)? {
        NumOrStr::Num(n) => Ok(n),
        NumOrStr::Str(s) => s
            .trim()
            .parse::<i64>()
            .map_err(|_| Error::custom(format!("expected an integer, got '{}'", s))),
    }
}

/// Deserialize `String` accepting either a JSON string or an integer JSON number.
///
/// Integer numbers are stringified (`123` -> `"123"`). Float numbers error.
pub fn flexible_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StrOrInt {
        Str(String),
        Int(i64),
    }

    match StrOrInt::deserialize(deserializer)? {
        StrOrInt::Str(s) => Ok(s),
        StrOrInt::Int(n) => Ok(n.to_string()),
    }
}

/// Deserialize `Option<String>` accepting a JSON string or an integer number.
///
/// Integer numbers are stringified. An empty/whitespace string or JSON `null`
/// becomes `None`. Float numbers error.
pub fn flexible_opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StrOrInt {
        Str(String),
        Int(i64),
    }

    match Option::<StrOrInt>::deserialize(deserializer)? {
        None => Ok(None),
        Some(StrOrInt::Str(s)) if s.trim().is_empty() => Ok(None),
        Some(StrOrInt::Str(s)) => Ok(Some(s)),
        Some(StrOrInt::Int(n)) => Ok(Some(n.to_string())),
    }
}

/// Deserialize `Option<bool>` accepting a JSON bool, the numbers `0`/`1`, or the
/// strings `"true"`/`"false"`/`"1"`/`"0"` (case-insensitive, trimmed).
///
/// An empty/whitespace string or JSON `null` becomes `None`. Anything else errors.
pub fn flexible_opt_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BoolOrIntOrStr {
        Bool(bool),
        Int(i64),
        Str(String),
    }

    match Option::<BoolOrIntOrStr>::deserialize(deserializer)? {
        None => Ok(None),
        Some(BoolOrIntOrStr::Bool(b)) => Ok(Some(b)),
        Some(BoolOrIntOrStr::Int(n)) => match n {
            0 => Ok(Some(false)),
            1 => Ok(Some(true)),
            other => Err(Error::custom(format!(
                "expected a boolean, got '{}'",
                other
            ))),
        },
        Some(BoolOrIntOrStr::Str(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            match trimmed.to_ascii_lowercase().as_str() {
                "true" | "1" => Ok(Some(true)),
                "false" | "0" => Ok(Some(false)),
                _ => Err(Error::custom(format!("expected a boolean, got '{}'", s))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct OptU32T {
        #[serde(default, deserialize_with = "flexible_opt_u32")]
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

    #[derive(Deserialize)]
    struct TestStruct {
        #[serde(default, deserialize_with = "deserialize_optional_media_filter")]
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
}
