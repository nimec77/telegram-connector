//! Observe terminal-frame acceptance without replacing the SDK's NDJSON encoder.
//! AsyncWrite may buffer accepted bytes; flush and delivery are separate events.
use super::state::State;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;

pub(super) struct CompletionWriter<W> {
    inner: W,
    state: Arc<State>,
}
impl<W> CompletionWriter<W> {
    pub fn new(inner: W, state: Arc<State>) -> Self {
        Self { inner, state }
    }
}
impl<W: AsyncWrite + Unpin> AsyncWrite for CompletionWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        // Lock order: admission/finish take operations then this barrier. This
        // nonblocking write poll never takes the operations mutex or awaits.
        let frame = match this.state.terminal_frame.lock() {
            Ok(frame) => frame,
            Err(_) => {
                return Poll::Ready(Err(io::Error::other("Output completion state unavailable")));
            }
        };
        let result = Pin::new(&mut this.inner).poll_write(cx, bytes);
        if let Poll::Ready(Ok(written)) = &result
            && bytes.get(..*written).and_then(|bytes| bytes.last()) == Some(&b'\n')
            && let Some(operation) = frame.as_ref()
        {
            // JSON string newlines are escaped by the SDK. Its framing newline
            // is accepted only after every byte of this terminal response/error.
            // Keep the slot owned until SDK flush/metrics settlement or shutdown.
            operation.terminal_accepted.store(true, Ordering::Release);
            operation.cancelled.send_replace(true);
            this.state.changed.notify_waiters();
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
