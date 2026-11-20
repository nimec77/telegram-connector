//! Private, bounded upload preparation.

mod privacy;
mod probe;
mod reader;
pub mod staging;
pub use reader::StagedReader;
pub use staging::{StagedUpload, StagingConfig, StagingSource, StagingStore};

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[path = "sending/tests/media_fixtures.rs"]
pub mod media_fixtures;
#[cfg(all(test, unix))]
#[path = "sending/tests/privacy_tests.rs"]
mod privacy_tests;
#[cfg(test)]
#[path = "sending/tests/probe_tests.rs"]
mod probe_tests;
#[cfg(all(test, unix))]
#[path = "sending/tests/staging_tests.rs"]
mod staging_tests;
#[cfg(all(test, unix))]
#[path = "sending/tests/fixtures.rs"]
pub(crate) mod test_support;

/// Inspect a bounded, private hosted upload using the same container checks as
/// local staging. This exposes no path or file name to a remote caller.
///
/// # Errors
/// Returns a safe failure when bytes do not match the advertised media kind.
pub fn check_upload_type(
    file: &mut std::fs::File,
    media: &crate::telegram::types::sending::SendMediaKind,
    size: u64,
    deadline: crate::telegram::sending::SendDeadline,
) -> Result<&'static str, crate::telegram::types::sending::SendFailure> {
    probe::probe(file, media, size, deadline).map(|result| result.mime_type)
}
