//! Type conversion helpers for grammers types to our domain types.
//!
//! Split into sub-domains (LM-4):
//! - `media`   media classification, sizing, video/audio info
//! - `message` message assembly (forward header, link preview, convert_message)
//! - `channel` peer -> channel conversion

mod channel;
mod media;
mod message;

pub use channel::{convert_discovered_peer, convert_peer_to_channel};
pub use media::{
    convert_media_filter, convert_media_to_type, extract_audio_duration, extract_audio_info,
    extract_video_info, matches_media_filter, select_size_candidate, size_candidates,
};
pub use message::convert_message;
pub(crate) use message::message_timestamp;

#[cfg(test)]
#[path = "tests/converters_tests.rs"]
mod tests;
