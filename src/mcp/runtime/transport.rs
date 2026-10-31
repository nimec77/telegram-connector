//! Strict bounded framing before SDK dispatch; SDK still owns output encoding.
use super::state::State;
use crate::mcp::observability::{InstrumentedTransport, ResponseBuffer};
use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use rmcp::RoleServer;
use rmcp::model::{
    ClientJsonRpcMessage, ClientNotification, ClientRequest, ErrorCode, ErrorData, GetMeta,
    JsonRpcMessage, RequestId, ServerNotification,
};
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::Transport;
use rmcp::transport::async_rw::AsyncRwTransport;
use serde_json::Value;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader, Empty};

const MAX_PENDING_PROTOCOL_ERRORS: usize = 16;

pub(super) struct StdioTransport<R: AsyncRead, W: AsyncWrite> {
    reader: BufReader<R>,
    writer: InstrumentedTransport<AsyncRwTransport<RoleServer, Empty, W>>,
    state: Arc<State>,
    frame: Vec<u8>,
    discarding: bool,
    eof: bool,
    started: bool,
    modern: bool,
    rejections: FuturesUnordered<BoxFuture<'static, io::Result<()>>>,
}

enum Frame {
    Line(Vec<u8>),
    Oversized,
    Eof,
}

impl<R, W> StdioTransport<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    pub fn new(
        read: R,
        write: W,
        state: Arc<State>,
        buffer: Arc<ResponseBuffer>,
        threshold: Duration,
    ) -> Self {
        let writer = InstrumentedTransport::new(
            AsyncRwTransport::new_server(tokio::io::empty(), write),
            Arc::clone(&state.metrics),
            buffer,
            threshold,
        );
        Self {
            reader: BufReader::new(read),
            writer,
            state,
            frame: Vec::new(),
            discarding: false,
            eof: false,
            started: false,
            modern: false,
            rejections: FuturesUnordered::new(),
        }
    }
    // fill_buf/consume preserve partial frames when select cancels a receive poll.
    async fn frame(
        reader: &mut BufReader<R>,
        frame: &mut Vec<u8>,
        discarding: &mut bool,
        max_bytes: usize,
    ) -> io::Result<Frame> {
        loop {
            let bytes = reader.fill_buf().await?;
            if bytes.is_empty() {
                return Ok(Frame::Eof);
            }
            let newline = bytes.iter().position(|byte| *byte == b'\n');
            let take = newline.map_or(bytes.len(), |index| index + 1);
            if *discarding {
                reader.consume(take);
                if newline.is_some() {
                    *discarding = false;
                }
                continue;
            }
            if frame.len().saturating_add(take) > max_bytes {
                frame.clear();
                reader.consume(take);
                *discarding = newline.is_none();
                return Ok(Frame::Oversized);
            }
            frame.extend_from_slice(&bytes[..take]);
            reader.consume(take);
            if newline.is_some() {
                return Ok(Frame::Line(std::mem::take(frame)));
            }
        }
    }
    async fn reject(&mut self, id: Option<RequestId>, error: ErrorData) -> bool {
        // Keep control input readable under stdout backpressure. The queue owns
        // every write future; it is polled by receive and dropped on shutdown.
        // Raw faults were never admitted, so they bypass response accounting.
        if self.rejections.len() >= MAX_PENDING_PROTOCOL_ERRORS {
            self.state
                .fail(io::Error::other("Too many pending protocol errors"))
                .await;
            return false;
        }
        let state = Arc::clone(&self.state);
        let send = self.writer.inner.send(JsonRpcMessage::error(error, id));
        self.rejections.push(
            async move {
                tokio::select! {
                    biased;
                    _ = state.stopped() => Ok(()),
                    result = async {
                        let _gate = state.write_gate.lock().await;
                        send.await
                    } => result,
                }
            }
            .boxed(),
        );
        true
    }
}

impl<R, W> Transport<RoleServer> for StdioTransport<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    type Error = io::Error;
    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = io::Result<()>> + Send + 'static {
        let state = Arc::clone(&self.state);
        let response_id = match &item {
            JsonRpcMessage::Response(response) => Some(response.id.clone()),
            JsonRpcMessage::Error(error) => error.id.clone(),
            _ => None,
        };
        let progress = match &item {
            JsonRpcMessage::Notification(notification) => match &notification.notification {
                ServerNotification::ProgressNotification(progress) => Some((
                    progress.params.progress_token.clone(),
                    progress.params.progress,
                )),
                _ => None,
            },
            _ => None,
        };
        let subscription = match &item {
            JsonRpcMessage::Notification(notification) => {
                notification.notification.get_meta().subscription_id()
            }
            _ => None,
        };
        let outbound_request = matches!(&item, JsonRpcMessage::Request(_));
        let send = self.writer.send(item);
        async move {
            if outbound_request {
                return Err(io::Error::other("Server initiated requests are disabled"));
            }
            let operation = {
                let entries = state.operations.lock().await;
                if let Some(id) = response_id.as_ref().or(subscription.as_ref()) {
                    entries.get(id).cloned()
                } else if let Some((token, _)) = &progress {
                    entries
                        .values()
                        .find(|op| op.token.as_ref() == Some(token))
                        .cloned()
                } else {
                    None
                }
            };
            let Some(operation) = operation else {
                // A cancelled request may already have a result/notification in
                // the SDK queue. Never write it or call the instrumentation future.
                return Ok(());
            };
            let gate = tokio::select! {
                biased;
                _ = operation.cancelled() => return Ok(()),
                _ = state.stopped() => return Ok(()),
                gate = state.write_gate.lock() => gate,
            };
            if *operation.cancelled.borrow() {
                return Ok(());
            }
            if let Some((_, value)) = progress {
                let mut last = operation.last_progress.lock().await;
                if value <= *last {
                    return Ok(());
                }
                *last = value;
            }
            // Once a frame starts writing it is committed: finish its bytes even
            // if cancellation races. Only connection shutdown interrupts a frame.
            let result = tokio::select! {
                biased;
                _ = state.stopped() => Err(io::Error::new(io::ErrorKind::Interrupted, "Shutdown deadline")),
                result = send => result,
            };
            if let Some(id) = &response_id {
                state.finish(id).await;
            }
            drop(gate);
            if let Err(error) = &result
                && !matches!(*state.phase.borrow(), super::state::Phase::Stopped)
            {
                state
                    .fail(io::Error::new(error.kind(), "Stdio write failed"))
                    .await;
            }
            result
        }
    }
    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        loop {
            let state = Arc::clone(&self.state);
            let frame = tokio::select! {
                biased;
                _ = state.stopped() => { self.rejections.clear(); return None; },
                _ = state.drained(), if self.rejections.is_empty() => return None,
                result = self.rejections.next(), if !self.rejections.is_empty() => {
                    if let Some(Err(error)) = result { state.fail(error).await; return None; }
                    continue;
                }
                frame = Self::frame(&mut self.reader, &mut self.frame, &mut self.discarding, state.config.max_request_bytes), if !self.eof => frame,
            };
            let bytes = match frame {
                Ok(Frame::Eof) => {
                    self.eof = true;
                    state.drain();
                    continue;
                }
                Err(error) => {
                    state.fail(error).await;
                    return None;
                }
                Ok(Frame::Oversized) => {
                    if !self
                        .reject(
                            None,
                            ErrorData::invalid_request("Request exceeds max_request_bytes", None),
                        )
                        .await
                    {
                        return None;
                    }
                    continue;
                }
                Ok(Frame::Line(bytes)) => bytes,
            };
            if bytes.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let value: Value = match serde_json::from_slice(&bytes) {
                Ok(value) => value,
                Err(_) => {
                    if !self
                        .reject(
                            None,
                            ErrorData::new(ErrorCode::PARSE_ERROR, "Parse error", None),
                        )
                        .await
                    {
                        return None;
                    }
                    continue;
                }
            };
            let id = value
                .get("id")
                .and_then(|id| serde_json::from_value::<RequestId>(id.clone()).ok());
            let notification =
                value.get("method").is_some_and(Value::is_string) && value.get("id").is_none();
            let valid = value.is_object()
                && value["jsonrpc"] == "2.0"
                && value["method"].is_string()
                && value.get("result").is_none()
                && value.get("error").is_none()
                && value.get("params").is_none_or(Value::is_object)
                && (notification || id.is_some());
            if !valid {
                if !notification
                    && !self
                        .reject(id, ErrorData::invalid_request("Invalid request", None))
                        .await
                {
                    return None;
                }
                continue;
            }
            let message = match serde_json::from_value::<ClientJsonRpcMessage>(value) {
                Ok(message) => message,
                Err(_) if notification => continue,
                Err(_) => {
                    if !self
                        .reject(id, ErrorData::invalid_request("Invalid request", None))
                        .await
                    {
                        return None;
                    }
                    continue;
                }
            };
            match &message {
                JsonRpcMessage::Request(request) => {
                    let meta = request.request.get_meta();
                    let inline_modern = meta
                        .protocol_version()
                        .is_some_and(|version| version.as_str() >= "2026-07-28");
                    if !matches!(request.request, ClientRequest::InitializeRequest(_)) {
                        if let Some(version) = meta.protocol_version()
                            && !super::SUPPORTED_VERSIONS.contains(&version)
                        {
                            let error = ErrorData::unsupported_protocol_version(
                                version,
                                super::SUPPORTED_VERSIONS,
                            );
                            if !self.reject(Some(request.id.clone()), error).await {
                                return None;
                            }
                            continue;
                        }
                        let requires_meta = self.modern
                            || inline_modern
                            || matches!(request.request, ClientRequest::DiscoverRequest(_))
                            || (!self.started
                                && !matches!(request.request, ClientRequest::PingRequest(_)));
                        if requires_meta
                            && !meta
                                .missing_required_keys(&rmcp::model::ProtocolVersion::V_2026_07_28)
                                .is_empty()
                        {
                            if !self
                                .reject(
                                    Some(request.id.clone()),
                                    ErrorData::invalid_params(
                                        "Missing or malformed required request metadata",
                                        None,
                                    ),
                                )
                                .await
                            {
                                return None;
                            }
                            continue;
                        }
                    }
                    if (self.modern || inline_modern)
                        && matches!(
                            request.request,
                            ClientRequest::PingRequest(_) | ClientRequest::InitializeRequest(_)
                        )
                    {
                        if !self
                            .reject(
                                Some(request.id.clone()),
                                ErrorData::new(
                                    ErrorCode::METHOD_NOT_FOUND,
                                    "Method is unavailable in this protocol revision",
                                    None,
                                ),
                            )
                            .await
                        {
                            return None;
                        }
                        continue;
                    }
                    if let Err(error) = state
                        .admit(request.id.clone(), meta.get_progress_token())
                        .await
                    {
                        if !self.reject(Some(request.id.clone()), error).await {
                            return None;
                        }
                        continue;
                    }
                    if matches!(request.request, ClientRequest::InitializeRequest(_)) {
                        self.started = true;
                    } else if !self.started
                        && !matches!(
                            request.request,
                            ClientRequest::DiscoverRequest(_) | ClientRequest::PingRequest(_)
                        )
                        && meta
                            .protocol_version()
                            .is_some_and(|version| super::SUPPORTED_VERSIONS.contains(&version))
                        && meta.client_capabilities().is_some()
                    {
                        self.started = true;
                        self.modern = true;
                    }
                    let tool = match &request.request {
                        ClientRequest::CallToolRequest(call) => call.params.name.as_ref(),
                        other => other.method(),
                    };
                    state
                        .metrics
                        .record_request(&request.id.clone().into_json_value().to_string(), tool);
                }
                JsonRpcMessage::Notification(notification) => {
                    if let ClientNotification::CancelledNotification(cancelled) =
                        &notification.notification
                        && let Some(id) = &cancelled.params.request_id
                    {
                        state.cancel(id).await;
                    }
                    if !self.started {
                        continue;
                    }
                }
                _ => continue,
            }
            return Some(message);
        }
    }
    async fn close(&mut self) -> io::Result<()> {
        self.state.stop().await;
        self.rejections.clear();
        self.writer.close().await
    }
}

#[cfg(test)]
#[path = "tests/transport.rs"]
mod tests;
