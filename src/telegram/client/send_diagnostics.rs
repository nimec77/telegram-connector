//! Suppress dependency payload diagnostics only while polling write RPC futures.
use crate::telegram::types::sending::SendFailure;
use std::future::Future;

pub(super) async fn private_rpc<T>(future: impl Future<Output = T>) -> T {
    use tracing::instrument::WithSubscriber as _;
    // The pinned retry helper formats raw provider errors before logging them.
    // Future-local dispatch survives yields without changing existing read logs
    // or a host's global subscriber. Upload workers are futures, not spawned tasks.
    future
        .with_subscriber(tracing::subscriber::NoSubscriber::default())
        .await
}
pub(super) fn stage_result<T>(
    stage: &'static str,
    result: Result<T, SendFailure>,
) -> Result<T, SendFailure> {
    match &result {
        Ok(_) => tracing::debug!(stage, "Send stage completed"),
        Err(error) => tracing::warn!(stage, category = error.kind().as_str(), "Send stage failed"),
    }
    result
}
