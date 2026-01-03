//! Domain types for Telegram entities.
//!
//! This module provides type-safe wrappers for Telegram IDs, validated string types,
//! media enums, and request/response structures.
//!
//! ## Module Organization
//! - `ids` - Type-safe ID wrappers (ChannelId, MessageId, UserId)
//! - `names` - Validated string types (Username, ChannelName)
//! - `media` - Media type enums (MediaType, MediaFilter)
//! - `entities` - Domain entities (Message, Channel)
//! - `params` - Request/response types (SearchParams, HistoryParams, SearchResult)

pub mod entities;
pub mod ids;
pub mod media;
pub mod names;
pub mod params;

// Re-export all public types for convenience
pub use entities::{Channel, Message};
pub use ids::{ChannelId, MessageId, UserId};
pub use media::{MediaFilter, MediaType};
pub use names::{ChannelName, Username};
pub use params::{HistoryParams, QueryMetadata, SearchParams, SearchResult};
