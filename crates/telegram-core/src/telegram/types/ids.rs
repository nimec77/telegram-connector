//! Type-safe ID wrappers with validation.
//!
//! These wrapper types prevent accidental ID mixups at compile time. Constructors
//! and deserialization both reject nonpositive IDs. Message IDs are unique only within a channel.

#![warn(missing_docs)]

use crate::error::Error;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Neutral Telegram peer namespace. A bare numeric ID is unique only within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub enum PeerNamespace {
    /// Individual account, excluded from channel/group resolution.
    User,
    /// Basic group.
    Chat,
    /// Broadcast, supergroup or community.
    Channel,
}

/// Telegram channel ID (must be positive)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct ChannelId(#[schemars(range(min = 1))] i64);

impl<'de> Deserialize<'de> for ChannelId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = i64::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl ChannelId {
    /// Construct a positive ID without contacting Telegram.
    ///
    /// # Errors
    /// Returns [`Error::InvalidInput`] for zero or a negative value. Positivity
    /// alone does not prove that the entity exists or is accessible.
    pub fn new(id: i64) -> Result<Self, Error> {
        if id <= 0 {
            return Err(Error::InvalidInput(format!(
                "Channel ID must be positive, got {}",
                id
            )));
        }
        Ok(Self(id))
    }

    /// Return the stored numeric ID without allocation.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct MessageId(#[schemars(range(min = 1))] i64);

impl<'de> Deserialize<'de> for MessageId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = i64::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl MessageId {
    /// Construct a positive ID without contacting Telegram.
    ///
    /// # Errors
    /// Returns [`Error::InvalidInput`] for zero or a negative value. Positivity
    /// alone does not prove that the entity exists or is accessible.
    pub fn new(id: i64) -> Result<Self, Error> {
        if id <= 0 {
            return Err(Error::InvalidInput(format!(
                "Message ID must be positive, got {}",
                id
            )));
        }
        Ok(Self(id))
    }

    /// Return the stored numeric ID without allocation.
    pub fn get(&self) -> i64 {
        self.0
    }

    /// The id as Telegram's wire type. `None` when it exceeds `i32` —
    /// MTProto message ids are `i32`, so such a cursor cannot be sent.
    pub fn as_i32(&self) -> Option<i32> {
        i32::try_from(self.0).ok()
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Telegram user ID (must be positive)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct UserId(#[schemars(range(min = 1))] i64);

impl<'de> Deserialize<'de> for UserId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = i64::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl UserId {
    /// Construct a positive ID without contacting Telegram.
    ///
    /// # Errors
    /// Returns [`Error::InvalidInput`] for zero or a negative value. Positivity
    /// alone does not prove that the entity exists or is accessible.
    pub fn new(id: i64) -> Result<Self, Error> {
        if id <= 0 {
            return Err(Error::InvalidInput(format!(
                "User ID must be positive, got {}",
                id
            )));
        }
        Ok(Self(id))
    }

    /// Return the stored numeric ID without allocation.
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

    #[test]
    fn serde_constructor_invariants_ids() {
        for raw in ["0", "-1", "-9223372036854775808", "\"1\"", "null", "1.5"] {
            assert!(
                serde_json::from_str::<ChannelId>(raw).is_err(),
                "channel: {raw}"
            );
            assert!(
                serde_json::from_str::<MessageId>(raw).is_err(),
                "message: {raw}"
            );
            assert!(serde_json::from_str::<UserId>(raw).is_err(), "user: {raw}");
        }
        for raw in ["1", "9007199254740993", "9223372036854775807"] {
            let id: ChannelId = serde_json::from_str(raw).expect("valid channel");
            assert_eq!(serde_json::to_string(&id).expect("serialize"), raw);
            assert!(serde_json::from_str::<MessageId>(raw).is_ok());
            assert!(serde_json::from_str::<UserId>(raw).is_ok());
        }
    }

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

    #[test]
    fn message_id_as_i32_converts_in_range() {
        let id = MessageId::new(610_119).expect("valid id");
        assert_eq!(id.as_i32(), Some(610_119_i32));
    }

    #[test]
    fn message_id_as_i32_rejects_out_of_range() {
        let id = MessageId::new(i64::from(i32::MAX) + 1).expect("valid id");
        assert_eq!(id.as_i32(), None);
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
