//! Custom serde deserializers for MCP tool types.

use crate::telegram::types::MediaFilter;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

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
}
