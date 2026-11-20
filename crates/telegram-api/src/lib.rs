//! Hosted Telegram API.

pub mod contracts;
#[cfg(feature = "identity")]
pub mod identity;
#[cfg(feature = "service")]
pub mod service;

#[cfg(all(test, feature = "service"))]
#[path = "service/identity_http.rs"]
mod identity_http;
