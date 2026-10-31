//! Compatibility conversion helpers from grammers types to domain data.
//!
//! Signatures intentionally depend on the pinned grammers/TL types. These pure
//! helpers perform no RPCs, charging or peer resolution. Optional extractors return
//! `None` when metadata/media is absent or unsupported; channel conversion returns
//! `None` for peers that are not representable channels/groups. New consumers should
//! prefer the client/domain facade instead of depending on upstream TL layout.
//!
//! Split into sub-domains (LM-4):
//! - `media`   media classification, sizing, video/audio info
//! - `message` message assembly (forward header, link preview, convert_raw_message)
//! - `channel` peer -> channel conversion

mod channel;
mod media;
mod message;

pub(crate) use channel::{channel_identity, peer_namespace};
pub use channel::{convert_discovered_peer, convert_peer_to_channel};
pub(crate) use media::matches_media_filter_raw;
pub use media::{
    convert_media_filter, convert_media_to_type, extract_audio_duration, extract_audio_info,
    extract_document_info, extract_poll_info, extract_video_info, select_size_candidate,
    size_candidates,
};
pub(crate) use message::{convert_raw_message, message_timestamp, timestamp_from_raw};

#[cfg(test)]
#[path = "tests/converters_av_tests.rs"]
mod av_tests;
#[cfg(test)]
#[path = "tests/converters_doc_poll_tests.rs"]
mod doc_poll_tests;
#[cfg(test)]
#[path = "tests/converters_thumb_forward_tests.rs"]
mod thumb_forward_tests;
