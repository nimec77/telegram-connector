//! Instrumented stdio transport decorator: logs every inbound request and
//! every stdout write, feeding `SessionMetrics` and `ResponseBuffer`.
//!
//! Unit of `observability` (LM-5).

use super::buffer::{BufferedResponse, GET_LAST_RESPONSES_TOOL, ResponseBuffer};
use super::metrics::SessionMetrics;
use rmcp::RoleServer;
use rmcp::model::{ClientRequest, JsonRpcMessage};
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::Transport;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// `true` when a stdout write took longer than the configured threshold.
pub(crate) fn is_slow_write(elapsed: Duration, threshold: Duration) -> bool {
    elapsed > threshold
}

/// Decorator around the stdio transport that logs every inbound request and every
/// actual stdout write, feeding [`SessionMetrics`] and [`ResponseBuffer`].
///
/// Instrumentation never fails the message path: serialization or buffer problems
/// are logged and swallowed; the inner transport's result is returned untouched.
pub struct InstrumentedTransport<T> {
    pub(crate) inner: T,
    metrics: Arc<SessionMetrics>,
    buffer: Arc<ResponseBuffer>,
    slow_write_threshold: Duration,
}

impl<T> InstrumentedTransport<T> {
    pub fn new(
        inner: T,
        metrics: Arc<SessionMetrics>,
        buffer: Arc<ResponseBuffer>,
        slow_write_threshold: Duration,
    ) -> Self {
        Self {
            inner,
            metrics,
            buffer,
            slow_write_threshold,
        }
    }
}

impl<T: Transport<RoleServer>> Transport<RoleServer> for InstrumentedTransport<T> {
    type Error = T::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        // Pre-compute identity and payload here: the returned future must be
        // 'static, so it cannot borrow from self or the message.
        let response_id = match &item {
            JsonRpcMessage::Response(response) => Some(response.id.to_string()),
            JsonRpcMessage::Error(error) => error.id.as_ref().map(ToString::to_string),
            _ => None,
        };
        // One extra serialization buys exact payload size + the recovery copy.
        // Deliberately not gated on the buffer being enabled: the size also
        // feeds the `bytes` field of every "Response written" log line below.
        let payload = response_id
            .as_ref()
            .and_then(|_| serde_json::to_string(&item).ok());
        let metrics = Arc::clone(&self.metrics);
        let buffer = Arc::clone(&self.buffer);
        let threshold = self.slow_write_threshold;
        let inner_send = self.inner.send(item);

        async move {
            let write_started = Instant::now();
            let result = inner_send.await;
            let Some(request_id) = response_id else {
                return result; // notifications/requests pass through unaccounted
            };
            match &result {
                Ok(()) => {
                    let write_elapsed = write_started.elapsed();
                    let write_ms = write_elapsed.as_millis() as u64;
                    let in_flight = metrics.record_response_written(&request_id);
                    let tool_name = in_flight
                        .as_ref()
                        .map(|request| request.tool_name.clone())
                        .unwrap_or_default();
                    let total_ms =
                        in_flight.map(|request| request.received_at.elapsed().as_millis() as u64);
                    // Counts the serialized envelope bytes; excludes the framing '\n'
                    // that rmcp's AsyncRwTransport appends on the wire.
                    let size_bytes = payload.as_ref().map_or(0, String::len);
                    tracing::info!(
                        request_id = %request_id,
                        tool = %tool_name,
                        bytes = size_bytes,
                        write_ms,
                        total_ms = ?total_ms,
                        "Response written to stdout"
                    );
                    if is_slow_write(write_elapsed, threshold) {
                        tracing::warn!(
                            request_id = %request_id,
                            write_ms,
                            threshold_ms = threshold.as_millis() as u64,
                            "Slow stdout write - peer may have stopped reading"
                        );
                    }
                    if let Some(payload) = payload
                        && tool_name != GET_LAST_RESPONSES_TOOL
                    {
                        buffer.push(BufferedResponse {
                            request_id,
                            tool_name,
                            written_at: SystemTime::now(),
                            size_bytes,
                            payload,
                        });
                    }
                }
                Err(error) => {
                    tracing::error!(
                        request_id = %request_id,
                        error = %error,
                        "Response write failed"
                    );
                }
            }
            result
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        let message = self.inner.receive().await;
        match &message {
            Some(JsonRpcMessage::Request(request)) => {
                let request_id = request.id.to_string();
                let method = request.request.method();
                let tool = match &request.request {
                    ClientRequest::CallToolRequest(call) => call.params.name.as_ref(),
                    _ => method,
                };
                self.metrics.record_request(&request_id, tool);
                tracing::debug!(request_id = %request_id, method, tool, "Request received");
            }
            Some(_) => {}
            None => self.metrics.log_summary("input stream terminated"),
        }
        message
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.inner.close().await
    }
}

#[cfg(test)]
#[path = "tests/transport_tests.rs"]
mod tests;
