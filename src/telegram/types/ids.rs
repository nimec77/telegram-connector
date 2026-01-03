//! Type-safe ID wrappers with validation.
//!
//! These wrapper types prevent accidental ID mixups at compile time.

use crate::error::Error;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Telegram channel ID (must be positive)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct ChannelId(i64);

impl ChannelId {
    pub fn new(id: i64) -> Result<Self, Error> {
        if id <= 0 {
            return Err(Error::InvalidInput(format!(
                "Channel ID must be positive, got {}",
                id
            )));
        }
        Ok(Self(id))
    }

    pub fn get(&self) -> i64 {
        self.0
    }
}

impl fmt::Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Telegram message ID (must be positive)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct MessageId(i64);

impl MessageId {
    pub fn new(id: i64) -> Result<Self, Error> {
        if id <= 0 {
            return Err(Error::InvalidInput(format!(
                "Message ID must be positive, got {}",
                id
            )));
        }
        Ok(Self(id))
    }

    pub fn get(&self) -> i64 {
        self.0
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Telegram user ID (must be positive)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct UserId(i64);

impl UserId {
    pub fn new(id: i64) -> Result<Self, Error> {
        if id <= 0 {
            return Err(Error::InvalidInput(format!(
                "User ID must be positive, got {}",
                id
            )));
        }
        Ok(Self(id))
    }

    pub fn get(&self) -> i64 {
        self.0
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // ChannelId Tests
    // =========================================================================

    #[test]
    fn channel_id_rejects_negative() {
        let result = ChannelId::new(-1);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("positive"));
    }

    #[test]
    fn channel_id_rejects_zero() {
        let result = ChannelId::new(0);
        assert!(result.is_err());
    }

    #[test]
    fn channel_id_accepts_positive() {
        let result = ChannelId::new(123);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().get(), 123);
    }

    #[test]
    fn channel_id_display() {
        let id = ChannelId::new(123456).unwrap();
        assert_eq!(format!("{}", id), "123456");
    }

    #[test]
    fn channel_id_serde_transparent() {
        let id = ChannelId::new(123456).unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "123456"); // No wrapping object
    }

    // =========================================================================
    // MessageId Tests
    // =========================================================================

    #[test]
    fn message_id_rejects_negative() {
        assert!(MessageId::new(-1).is_err());
    }

    #[test]
    fn message_id_rejects_zero() {
        assert!(MessageId::new(0).is_err());
    }

    #[test]
    fn message_id_accepts_positive() {
        let result = MessageId::new(456);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().get(), 456);
    }

    #[test]
    fn message_id_display() {
        let id = MessageId::new(789).unwrap();
        assert_eq!(format!("{}", id), "789");
    }

    // =========================================================================
    // UserId Tests
    // =========================================================================

    #[test]
    fn user_id_rejects_negative() {
        assert!(UserId::new(-1).is_err());
    }

    #[test]
    fn user_id_rejects_zero() {
        assert!(UserId::new(0).is_err());
    }

    #[test]
    fn user_id_accepts_positive() {
        let result = UserId::new(999);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().get(), 999);
    }

    #[test]
    fn user_id_display() {
        let id = UserId::new(111).unwrap();
        assert_eq!(format!("{}", id), "111");
    }
}
