//! Application runtime ownership, shared with the credential-free process fixture.
use std::io;
use std::time::Duration;

/// Run the application, then release Tokio without waiting on an uncancellable
/// platform stdin read. The protocol runtime must finish its configured drain
/// before this function returns. Started blocking work is not forcibly aborted:
/// it may run until process exit, with image concurrency bounded separately.
pub fn run<F: Future<Output = anyhow::Result<()>>>(application: F) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(application);
    runtime.shutdown_timeout(Duration::ZERO);
    result
}

/// Install and await shutdown signals. Registration errors propagate to the
/// protocol supervisor, which drains accepted work before returning the error.
pub async fn shutdown_signal() -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            signal = terminate.recv() => signal.ok_or_else(|| io::Error::other("SIGTERM stream closed")),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}
