//! Stdio runtime: bounded admission, request-local execution, and graceful drain.
//!
//! The SDK retains protocol dispatch and serialization. The transport adapter only
//! supplies strict framing and lifecycle behavior the SDK does not expose.
pub mod process;
mod processing;
mod state;
mod transport;

use crate::config::ServerConfig;
use crate::mcp::observability::{ResponseBuffer, SessionMetrics};
use futures::FutureExt;
use rmcp::model::{
    ClientNotification, ClientRequest, ErrorData, Implementation, InitializeResult, MetaObject,
    ProgressNotificationParam, ProtocolVersion, ServerResult,
};
use rmcp::service::{NotificationContext, RequestContext, RoleServer, Service};
use rmcp::{Peer, ServiceExt};
use state::{Operation, State};
use std::borrow::Cow;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const INPUT_BUFFER_BYTES: usize = 8192;

/// Revisions covered by the offline conformance matrix; SDK updates cannot widen it.
pub const SUPPORTED_VERSIONS: &[ProtocolVersion] =
    &[ProtocolVersion::V_2025_11_25, ProtocolVersion::V_2026_07_28];

/// Failures of runtime setup, transport I/O, or SDK lifecycle ownership.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// Rejected input/admission/drain limits, before any input is read.
    #[error("Invalid server runtime configuration: {0}")]
    Configuration(anyhow::Error),
    /// Read/write or supplied shutdown-trigger failure, reported after cleanup.
    #[error("Stdio transport failed: {0}")]
    Io(#[from] io::Error),
    /// SDK bootstrap/negotiation failure outside an ordinary shutdown.
    #[error("MCP startup failed: {0}")]
    Initialize(#[from] Box<rmcp::service::ServerInitializeError>),
    /// Owned SDK task failed to join normally.
    #[error("MCP service task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

/// One transport session's configuration and shared observability handles.
pub struct Runtime {
    state: Arc<State>,
    buffer: Arc<ResponseBuffer>,
    slow_write_threshold: Duration,
}
impl Runtime {
    /// Create a bounded session without reading input or acquiring account tokens.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Configuration`] when [`ServerConfig::validate`]
    /// rejects the input/admission/drain limits.
    pub fn try_new(
        config: ServerConfig,
        metrics: Arc<SessionMetrics>,
        buffer: Arc<ResponseBuffer>,
        slow_write_threshold: Duration,
    ) -> Result<Self, RuntimeError> {
        config.validate().map_err(RuntimeError::Configuration)?;
        Ok(Self {
            state: State::new(config, metrics),
            buffer,
            slow_write_threshold,
        })
    }

    /// Serve one NDJSON connection, draining accepted work on EOF or shutdown.
    ///
    /// Bootstrap discover/initialize handlers must remain local and nonblocking;
    /// arbitrary custom asynchronous bootstrap extensions are outside this contract.
    /// The supplied shutdown future installs/awaits platform signals or a test
    /// trigger. Its errors propagate after cleanup. Output writes share the same
    /// absolute shutdown deadline as operations. Started blocking CPU work remains
    /// non-abortable; callers own their process/runtime termination policy.
    ///
    /// # Errors
    /// Returns transport/shutdown-trigger I/O, SDK initialization or owned-task
    /// failures after cleanup. EOF and ordinary shutdown are successful outcomes.
    pub async fn serve<S, R, W, F>(
        self,
        service: S,
        io: (R, W),
        shutdown: F,
    ) -> Result<(), RuntimeError>
    where
        S: Service<RoleServer>,
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
        F: Future<Output = io::Result<()>> + Send,
    {
        let state = Arc::clone(&self.state);
        // The SDK awaits bootstrap writes inline. Keep physical EOF observable
        // during those writes without changing its negotiation implementation.
        let (reader, writer) = tokio::io::duplex(INPUT_BUFFER_BYTES);
        let read_task = tokio::spawn(pump_input(io.0, writer, Arc::clone(&state)));
        let transport = transport::StdioTransport::new(
            reader,
            io.1,
            Arc::clone(&state),
            self.buffer,
            self.slow_write_threshold,
        );
        let handler = RuntimeService {
            service,
            state: Arc::clone(&state),
        };
        let runner = async {
            match handler.serve(transport).await {
                Ok(running) => {
                    running.waiting().await?;
                    Ok(())
                }
                Err(_) if state.stopping() => Ok(()),
                Err(error) => Err(RuntimeError::Initialize(Box::new(error))),
            }
        };
        tokio::pin!(runner);
        let result = tokio::select! {
            result = &mut runner => result,
            signal = shutdown => {
                state.drain();
                let result = runner.await;
                match signal {
                    Ok(()) => result,
                    Err(error) => Err(RuntimeError::Io(error)),
                }
            }
        };
        state.stop().await;
        read_task.await?;
        state.metrics.log_summary("stdio session terminated");
        if let Some(error) = state.failure.lock().await.take() {
            return Err(error.into());
        }
        result
    }
}

async fn pump_input<R: AsyncRead + Unpin>(
    mut input: R,
    mut writer: tokio::io::DuplexStream,
    state: Arc<State>,
) {
    let mut bytes = [0; INPUT_BUFFER_BYTES];
    loop {
        let read = tokio::select! {
            biased;
            _ = state.stopped() => return,
            read = input.read(&mut bytes) => read,
        };
        let count = match read {
            Ok(0) => {
                state.input_eof();
                return;
            }
            Ok(count) => count,
            Err(error) => {
                state.fail(error).await;
                return;
            }
        };
        tokio::select! {
            biased;
            _ = state.stopped() => return,
            write = writer.write_all(&bytes[..count]) => {
                if let Err(error) = write {
                    if !state.stopping() { state.fail(error).await; }
                    return;
                }
            }
        }
    }
}

struct RuntimeService<S> {
    service: S,
    state: Arc<State>,
}
impl<S: Service<RoleServer>> Service<RoleServer> for RuntimeService<S> {
    async fn handle_request(
        &self,
        request: ClientRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<ServerResult, ErrorData> {
        let operation = self.state.operations.lock().await.get(&context.id).cloned();
        let Some(operation) = operation else {
            return Err(ErrorData::invalid_request("Request cancelled", None));
        };
        let modern = context
            .protocol_version()
            .is_some_and(|version| version == ProtocolVersion::V_2026_07_28)
            && !matches!(request, ClientRequest::InitializeRequest(_));
        let ct = context.ct.clone();
        let progress = OperationContext {
            response_id: context.id.clone(),
            modern,
            identity: self.service.get_info().server_info,
            operation: Arc::clone(&operation),
            peer: context.peer.clone(),
            next: AtomicU64::new(0),
        };
        let work = OPERATION.scope(progress, self.service.handle_request(request, context));
        let mut result = tokio::select! {
            biased;
            _ = operation.cancelled() => return Err(ErrorData::invalid_request("Request cancelled", None)),
            _ = ct.cancelled() => return Err(ErrorData::invalid_request("Request cancelled", None)),
            _ = self.state.stopped() => return Err(ErrorData::invalid_request("Server stopped", None)),
            result = work => result?,
        };
        if modern {
            add_identity(&mut result, self.service.get_info().server_info)?;
        }
        Ok(result)
    }
    async fn handle_notification(
        &self,
        notification: ClientNotification,
        context: NotificationContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.service
            .handle_notification(notification, context)
            .await
    }
    fn get_info(&self) -> InitializeResult {
        self.service.get_info()
    }
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(SUPPORTED_VERSIONS)
    }
}

fn add_identity(result: &mut ServerResult, identity: Implementation) -> Result<(), ErrorData> {
    let meta = match result {
        ServerResult::CallToolResult(result) => &mut result.meta,
        ServerResult::InputRequiredResult(result) => &mut result.meta,
        ServerResult::ListToolsResult(result) => &mut result.meta,
        ServerResult::ListPromptsResult(result) => &mut result.meta,
        ServerResult::ListResourcesResult(result) => &mut result.meta,
        ServerResult::ListResourceTemplatesResult(result) => &mut result.meta,
        ServerResult::ReadResourceResult(result) => &mut result.meta,
        ServerResult::GetPromptResult(result) => &mut result.meta,
        ServerResult::CompleteResult(result) => &mut result.meta,
        ServerResult::DiscoverResult(result) => {
            result.set_server_info(identity);
            return Ok(());
        }
        ServerResult::SubscriptionsListenResult(result) => {
            result.meta.set_server_info(identity);
            return Ok(());
        }
        _ => return Ok(()),
    };
    let identity = serde_json::to_value(identity)
        .map_err(|_| ErrorData::internal_error("Server identity serialization failed", None))?;
    meta.get_or_insert_with(MetaObject::new)
        .insert("io.modelcontextprotocol/serverInfo".into(), identity);
    Ok(())
}

struct OperationContext {
    response_id: rmcp::model::RequestId,
    modern: bool,
    identity: Implementation,
    operation: Arc<Operation>,
    peer: Peer<RoleServer>,
    next: AtomicU64,
}
tokio::task_local! { static OPERATION: OperationContext; }

struct ResponseContext {
    id: rmcp::model::RequestId,
    modern: bool,
}
tokio::task_local! { static RESPONSE: ResponseContext; }

/// Preserve real framing data for SDK embeddings that do not use our stdio runtime.
pub(crate) async fn scope_response<F: Future>(
    id: rmcp::model::RequestId,
    version: Option<ProtocolVersion>,
    future: F,
) -> F::Output {
    RESPONSE
        .scope(
            ResponseContext {
                id,
                modern: version == Some(ProtocolVersion::V_2026_07_28),
            },
            future,
        )
        .await
}

/// Report a completed phase for the current operation, if the caller supplied a
/// token. Direct tool-method callers have no wire context and emit nothing.
/// Messages must be static descriptions without user data. No detached tasks.
pub(crate) async fn report_progress(message: &'static str) {
    let context = OPERATION.try_with(|context| {
        let token = context.operation.token.clone()?;
        Some((
            Arc::clone(&context.operation),
            context.peer.clone(),
            ProgressNotificationParam::new(
                token,
                (context.next.fetch_add(1, Ordering::Relaxed) + 1) as f64,
            )
            .with_message(message),
        ))
    });
    if let Ok(Some((operation, peer, notification))) = context
        && !*operation.cancelled.borrow()
    {
        // rmcp's Peer enqueues into a bounded channel before awaiting delivery.
        // Poll once: enqueue immediately if possible, otherwise drop optional
        // progress. Dropping the delivery waiter does not cancel an enqueued
        // notification. Never spend application deadline on stdout backpressure.
        let _ = peer.notify_progress(notification).now_or_never();
        // Give the transport a scheduling opportunity without waiting for its
        // write or introducing a wall-clock timer into the operation deadline.
        tokio::task::yield_now().await;
    }
}

pub(crate) use processing::{IMAGE_PROCESSING_SLOTS, image_job};

/// Exact JSON-RPC size for the active request, excluding its framing newline.
/// SDK embeddings use their actual ID/version without runtime identity; direct
/// Rust convenience methods have no JSON-RPC frame and use a numeric fixture ID.
pub(crate) fn response_size(
    result: &rmcp::model::CallToolResult,
) -> Result<usize, crate::error::Error> {
    response_payload(result).map(|payload| payload.len())
}

pub(crate) fn response_payload(
    result: &rmcp::model::CallToolResult,
) -> Result<String, crate::error::Error> {
    let mut result = ServerResult::CallToolResult(result.clone());
    let id = OPERATION
        .try_with(|context| {
            if context.modern {
                add_identity(&mut result, context.identity.clone()).map_err(|_| {
                    crate::error::Error::Internal("response identity serialization failed")
                })?;
            } else {
                result.strip_result_type_for_legacy_peer();
            }
            Ok::<_, crate::error::Error>(context.response_id.clone())
        })
        .or_else(|_| {
            RESPONSE.try_with(|context| {
                if !context.modern {
                    result.strip_result_type_for_legacy_peer();
                }
                Ok::<_, crate::error::Error>(context.id.clone())
            })
        })
        .unwrap_or(Ok(rmcp::model::RequestId::Number(0)))?;
    let response = rmcp::model::JsonRpcResponse {
        jsonrpc: rmcp::model::JsonRpcVersion2_0,
        id,
        result,
    };
    serde_json::to_string(&response)
        .map_err(|_| crate::error::Error::Internal("response serialization failed"))
}
