//! Custom serde deserializers for MCP tool types.

use super::requests::ResponseFormat;
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

/// Deserialize Option<ResponseFormat> treating empty strings as None.
/// This handles MCP clients that send `"format": ""` instead of omitting the field.
pub fn deserialize_optional_response_format<'de, D>(
    deserializer: D,
) -> Result<Option<ResponseFormat>, D::Error>
where
    D: Deserializer<'de>,
{
    // First try to deserialize as an Option<String> to check for empty string
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrResponseFormat {
        String(String),
        ResponseFormat(ResponseFormat),
        Null,
    }

    match Option::<StringOrResponseFormat>::deserialize(deserializer)? {
        None => Ok(None),
        Some(StringOrResponseFormat::Null) => Ok(None),
        Some(StringOrResponseFormat::String(s)) if s.is_empty() => Ok(None),
        Some(StringOrResponseFormat::String(s)) => {
            // Try to parse non-empty string as ResponseFormat
            serde_json::from_value(serde_json::Value::String(s))
                .map(Some)
                .map_err(serde::de::Error::custom)
        }
        Some(StringOrResponseFormat::ResponseFormat(f)) => Ok(Some(f)),
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

/// Deserialize `Option<i64>` accepting either a JSON number or a numeric string.
///
/// The string form is trimmed before parsing. An empty/whitespace string or a
/// JSON `null` becomes `None`. Floats and non-numeric values produce an error.
pub fn flexible_opt_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(i64),
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
                .parse::<i64>()
                .map(Some)
                .map_err(|_| Error::custom(format!("expected an integer, got '{}'", s)))
        }
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
#[path = "tests/serde_helpers_tests.rs"]
mod tests;
