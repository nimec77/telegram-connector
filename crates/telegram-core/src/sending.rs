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
