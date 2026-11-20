//! Stateless hosted MCP gateway backed by the scoped Telegram HTTPS API.

mod catalog;
mod delegation;
mod tools;
mod transport;

pub use transport::router;
