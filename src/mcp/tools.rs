//! MCP tool implementations
//!
//! This module contains all 16 MCP tools.
//! Tools are organized in subdirectory for better maintainability.
//!
//! Compatibility facade: request/response types remain available both here and
//! through [`types`]. [`helpers`] and [`image`] keep their established public
//! paths, but their string errors, wire coercions and image policy belong to the
//! MCP boundary. Prefer strict Telegram domain constructors for new domain code.

pub(crate) mod fanout;
pub mod helpers;
pub mod image;
pub(crate) mod media_budget;
pub(crate) mod shaping;
pub mod types;
pub(crate) mod wire_budget;

// Re-export types for convenience
pub use types::*;
// Re-export helpers for convenience
pub use helpers::{dedupe_and_validate_ids, json_response, parse_channel_id, parse_message_id};

pub(crate) mod validation;
