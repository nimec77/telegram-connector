//! MCP Server tests organized by tool
//!
//! This module contains all tests for the MCP server, organized into submodules
//! by tool/functionality for better maintainability.

#[path = "tests/channels.rs"]
mod channels;
#[path = "tests/history.rs"]
mod history;
#[path = "tests/last_responses.rs"]
mod last_responses;
#[path = "tests/links.rs"]
mod links;
#[path = "tests/message_by_link.rs"]
mod message_by_link;
#[path = "tests/search.rs"]
mod search;
#[path = "tests/server_core.rs"]
mod server_core;
#[path = "tests/status.rs"]
mod status;
