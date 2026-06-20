//! Shared helper functions for MCP tools.
//!
//! This module extracts common ID parsing logic used across multiple tools.

use crate::telegram::types::{ChannelId, MessageId};

/// Parse a channel ID string to a type-safe ChannelId.
///
/// # Arguments
/// * `id_str` - String representation of channel ID (must be numeric)
///
/// # Returns
/// * `Ok(ChannelId)` - Valid channel ID
/// * `Err(String)` - Error message describing the issue
///
/// # Example
/// ```ignore
/// let id = parse_channel_id("123456")?;
/// assert_eq!(id.get(), 123456);
/// ```
pub fn parse_channel_id(id_str: &str) -> Result<ChannelId, String> {
    let id_num: i64 = id_str
        .parse()
        .map_err(|_| format!("Invalid channel_id: '{}' is not a valid number", id_str))?;

    ChannelId::new(id_num).map_err(|e| format!("Invalid channel_id: {}", e))
}

/// Parse a message ID to a type-safe MessageId.
///
/// # Arguments
/// * `id` - Message ID (must be positive)
///
/// # Returns
/// * `Ok(MessageId)` - Valid message ID
/// * `Err(String)` - Error message describing the issue
pub fn parse_message_id(id: i64) -> Result<MessageId, String> {
    MessageId::new(id).map_err(|e| format!("Invalid message_id: {}", e))
}

/// Parse an optional channel ID string to an optional ChannelId.
///
/// # Arguments
/// * `id_str` - Optional string representation of channel ID
///
/// # Returns
/// * `Ok(Some(ChannelId))` - Valid channel ID when input is Some
/// * `Ok(None)` - When input is None
/// * `Err(String)` - Error message when parsing fails
pub fn parse_optional_channel_id(id_str: &Option<String>) -> Result<Option<ChannelId>, String> {
    match id_str {
        Some(id) => parse_channel_id(id).map(Some),
        None => Ok(None),
    }
}

/// Serialize a value to a JSON string for an MCP tool response.
///
/// Centralizes the `serde_json::to_string(..).map_err(|e| e.to_string())` tail
/// repeated by every `*_impl` method, mapping serialization failures to the
/// `String` error half of the rmcp `Result<String, String>` tool contract.
pub fn json_response<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_channel_id_valid() {
        let result = parse_channel_id("123456");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().get(), 123456);
    }

    #[test]
    fn parse_channel_id_invalid_string() {
        let result = parse_channel_id("not_a_number");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not a valid number"));
    }

    #[test]
    fn parse_channel_id_negative() {
        let result = parse_channel_id("-123");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid channel_id"));
    }

    #[test]
    fn parse_channel_id_zero() {
        let result = parse_channel_id("0");
        assert!(result.is_err());
    }

    #[test]
    fn parse_message_id_valid() {
        let result = parse_message_id(789);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().get(), 789);
    }

    #[test]
    fn parse_message_id_invalid() {
        let result = parse_message_id(-1);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid message_id"));
    }

    #[test]
    fn parse_optional_channel_id_some_valid() {
        let result = parse_optional_channel_id(&Some("123".to_string()));
        assert!(result.is_ok());
        assert_eq!(result.unwrap().unwrap().get(), 123);
    }

    #[test]
    fn parse_optional_channel_id_none() {
        let result = parse_optional_channel_id(&None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn parse_optional_channel_id_some_invalid() {
        let result = parse_optional_channel_id(&Some("invalid".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn json_response_serializes_value() {
        let out = json_response(&vec![1, 2, 3]).expect("serializes");
        assert_eq!(out, "[1,2,3]");
    }
}
