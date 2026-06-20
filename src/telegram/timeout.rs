//! Hard wall-clock timeout wrapper bounding grammers network operations.
//!
//! Extracted from `client.rs` (LM-2).

use crate::error::Error;
use std::future::Future;
use std::time::Duration as StdDuration;

/// Wrap a fallible async operation in a hard wall-clock timeout.
///
/// If `fut` resolves within `secs`, its result is returned unchanged. Otherwise the
/// in-flight future is dropped and an [`Error::Timeout`] carrying `operation` and
/// `secs` is returned.
///
/// `operation` should be a short stable identifier (`"resolve_username"`,
/// `"iter_messages"`, etc.) so log searches can pivot on it.
pub(crate) async fn with_timeout<F, T>(operation: &str, secs: u64, fut: F) -> Result<T, Error>
where
    F: Future<Output = Result<T, Error>>,
{
    match tokio::time::timeout(StdDuration::from_secs(secs), fut).await {
        Ok(inner) => inner,
        Err(_) => Err(Error::Timeout {
            operation: operation.to_string(),
            secs,
        }),
    }
}
