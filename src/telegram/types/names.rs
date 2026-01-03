//! Validated string types for Telegram entities.

use crate::error::Error;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Telegram username (alphanumeric + underscore, 5-32 chars)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Username(String);

impl Username {
    pub fn new(username: impl Into<String>) -> Result<Self, Error> {
        let username = username.into();

        if username.len() < 5 || username.len() > 32 {
            return Err(Error::InvalidInput(format!(
                "Username must be 5-32 characters, got {}",
                username.len()
            )));
        }

        if !username.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(Error::InvalidInput(
                "Username must contain only alphanumeric characters and underscores".into(),
            ));
        }

        Ok(Self(username))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Non-empty channel/chat name
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct ChannelName(String);

impl ChannelName {
    pub fn new(name: impl Into<String>) -> Result<Self, Error> {
        let name = name.into();
        let trimmed = name.trim();

        if trimmed.is_empty() {
            return Err(Error::InvalidInput("Channel name cannot be empty".into()));
        }

        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ChannelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Username Tests
    // =========================================================================

    #[test]
    fn username_rejects_too_short() {
        let result = Username::new("abc");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("5-32"));
    }

    #[test]
    fn username_rejects_too_long() {
        let result = Username::new("a".repeat(33));
        assert!(result.is_err());
    }

    #[test]
    fn username_rejects_special_chars() {
        let result = Username::new("user@name");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("alphanumeric"));
    }

    #[test]
    fn username_accepts_underscore() {
        let result = Username::new("valid_user");
        assert!(result.is_ok());
    }

    #[test]
    fn username_accepts_numbers() {
        let result = Username::new("user123");
        assert!(result.is_ok());
    }

    #[test]
    fn username_accepts_valid() {
        let result = Username::new("valid_user123");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_str(), "valid_user123");
    }

    #[test]
    fn username_display() {
        let username = Username::new("telegram_user").unwrap();
        assert_eq!(format!("{}", username), "telegram_user");
    }

    #[test]
    fn username_serde_transparent() {
        let username = Username::new("testuser").unwrap();
        let json = serde_json::to_string(&username).unwrap();
        assert_eq!(json, "\"testuser\"");
    }

    // =========================================================================
    // ChannelName Tests
    // =========================================================================

    #[test]
    fn channel_name_rejects_empty() {
        let result = ChannelName::new("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn channel_name_rejects_whitespace_only() {
        let result = ChannelName::new("   ");
        assert!(result.is_err());
    }

    #[test]
    fn channel_name_trims_whitespace() {
        let result = ChannelName::new("  Tech News  ");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_str(), "Tech News");
    }

    #[test]
    fn channel_name_accepts_valid() {
        let result = ChannelName::new("AI Updates");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_str(), "AI Updates");
    }

    #[test]
    fn channel_name_display() {
        let name = ChannelName::new("News Channel").unwrap();
        assert_eq!(format!("{}", name), "News Channel");
    }
}
