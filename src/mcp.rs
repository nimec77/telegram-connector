//! MCP server facade, request/response compatibility types, and transport support.
//!
//! Prefer [`McpServer`] for embedding. [`runtime`] and [`observability`] expose
//! explicit lifecycle and diagnostic handles; [`tools`] retains established DTO
//! and helper paths. Domain failures are typed below the server; tool entry points
//! preserve their JSON/string error contracts and media content blocks.

pub mod observability;
pub mod runtime;
pub mod server;
pub mod tools;

pub use server::McpServer;
