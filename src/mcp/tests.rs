//! MCP Server tests organized by tool
//!
//! This module contains all tests for the MCP server, organized into submodules
//! by tool/functionality for better maintainability.

#[path = "tests/batch.rs"]
mod batch;
#[path = "tests/channels.rs"]
mod channels;
#[path = "tests/discovery.rs"]
mod discovery;
#[path = "tests/history.rs"]
mod history;
#[path = "tests/last_responses.rs"]
mod last_responses;
#[path = "tests/links.rs"]
mod links;
#[path = "tests/media.rs"]
mod media;
#[path = "tests/media_batch.rs"]
mod media_batch;
#[path = "tests/message_by_link.rs"]
mod message_by_link;
#[path = "tests/multi_channel.rs"]
mod multi_channel;
#[path = "tests/parity.rs"]
mod parity;
#[path = "tests/resolve.rs"]
mod resolve;
#[path = "tests/schema_integrity.rs"]
mod schema_integrity;
#[path = "tests/search.rs"]
mod search;
#[path = "tests/server_core.rs"]
mod server_core;
#[path = "tests/stats.rs"]
mod stats;
#[path = "tests/status.rs"]
mod status;
#[path = "tests/transcription.rs"]
mod transcription;
