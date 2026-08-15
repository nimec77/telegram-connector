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
#[path = "tests/history_core.rs"]
mod history_core;
#[path = "tests/history_dates.rs"]
mod history_dates;
#[path = "tests/history_paging.rs"]
mod history_paging;
#[path = "tests/last_responses.rs"]
mod last_responses;
#[path = "tests/links.rs"]
mod links;
#[path = "tests/media.rs"]
mod media;
#[path = "tests/media_batch_budget.rs"]
mod media_batch_budget;
#[path = "tests/media_batch_core.rs"]
mod media_batch_core;
#[path = "tests/media_batch_fixtures.rs"]
mod media_batch_fixtures;
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
#[path = "tests/search_core.rs"]
mod search_core;
#[path = "tests/search_dates.rs"]
mod search_dates;
#[path = "tests/search_shaping.rs"]
mod search_shaping;
#[path = "tests/server_core.rs"]
mod server_core;
#[path = "tests/stats.rs"]
mod stats;
#[path = "tests/status.rs"]
mod status;
#[path = "tests/transcription.rs"]
mod transcription;
