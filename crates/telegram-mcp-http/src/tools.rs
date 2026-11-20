//! Stateless MCP dispatch backed by current origin-client grants.

use crate::catalog::{TOOLS, handle, is_preview, scope, tool};
use crate::delegation::{Caller, Failure, Gateway};
use axum::http::request::Parts;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock,
    Implementation, InitializeRequestParams, InitializeResult, ListToolsResult,
    PaginatedRequestParams, ProtocolVersion, ReadResourceRequestParams, ReadResourceResponse,
    ReadResourceResult, Resource, ResourceContents, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use telegram_api::contracts::Scope;

const PROTOCOLS: &[ProtocolVersion] = &[ProtocolVersion::V_2026_07_28];
const RESOURCE_PREFIX: &str = "telegram-media://preview/";

fn caller(context: &RequestContext<RoleServer>) -> Result<&Caller, ErrorData> {
    context
        .extensions
        .get::<Parts>()
        .and_then(|p| p.extensions.get::<Caller>())
        .ok_or_else(|| ErrorData::internal_error("Missing authenticated request", None))
}

fn render_error(error: Failure, write: bool) -> CallToolResult {
    let retryable = !write && matches!(error.code, "rate_limited" | "timeout");
    let mut details = json!({"code":error.code,"message":error.message,"retryable":retryable});
    if let Some(seconds) = error.retry_after {
        details["retry_after_seconds"] = json!(seconds);
    }
    if write {
        details["delivery_state"] = json!(if error.unknown_delivery {
            "unknown"
        } else {
            "not_sent"
        });
    }
    CallToolResult::structured_error(json!({"schema_version":"1","data":null,"error":details}))
}

fn render(name: &str, mut data: Value) -> Result<CallToolResult, Failure> {
    let mut links = Vec::new();
    if is_preview(name) {
        let items = data["items"].as_array_mut().ok_or(Failure::UNAVAILABLE)?;
        if items.len() > 10 {
            return Err(Failure::LARGE);
        }
        for item in items {
            if let Some(id) = item.get("handle").and_then(Value::as_str) {
                if !handle(id) {
                    return Err(Failure::UNAVAILABLE);
                }
                let uri = format!("{RESOURCE_PREFIX}{id}");
                links.push(ContentBlock::resource_link(
                    Resource::new(uri.clone(), "Telegram message preview")
                        .with_mime_type("image/jpeg"),
                ));
                item["resource_uri"] = json!(uri);
            }
        }
    }
    let mut result =
        CallToolResult::structured(json!({"schema_version":"1","data":data,"error":null}));
    result.content.extend(links);
    if serde_json::to_vec(&result)
        .map_err(|_| Failure::UNAVAILABLE)?
        .len()
        > 1024 * 1024
    {
        return Err(Failure::LARGE);
    }
    Ok(result)
}

impl ServerHandler for Gateway {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
        ))
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(PROTOCOLS)
    }

    async fn initialize(
        &self,
        request: InitializeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        Err(ErrorData::unsupported_protocol_version(
            request.protocol_version,
            PROTOCOLS,
        ))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        tool(name)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let auth = caller(&context)?;
        let mut tools = Vec::new();
        if auth
            .identity
            .scopes()
            .iter()
            .any(|scope| matches!(scope, Scope::Read | Scope::Send))
        {
            let fetch = async {
                let delegation = self.delegate(auth, &[Scope::Read, Scope::Send]).await?;
                self.capabilities(auth, &delegation).await
            };
            let available = tokio::select! {
                _=context.ct.cancelled()=>return Err(ErrorData::internal_error("Request cancelled",None)),
                result=tokio::time::timeout_at(auth.deadline,fetch)=>result.ok().and_then(Result::ok).ok_or_else(||ErrorData::internal_error("Could not obtain authorized tools",None))?,
            };
            tools.extend(
                TOOLS
                    .iter()
                    .copied()
                    .filter(|name| {
                        auth.identity.scopes().contains(&scope(name))
                            && available.iter().any(|item| item == name)
                    })
                    .filter_map(tool),
            );
        }
        Ok(ListToolsResult::with_all_items(tools)
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.as_ref();
        if !TOOLS.contains(&name) {
            return Err(ErrorData::invalid_params("Unknown tool", None));
        }
        let auth = caller(&context)?;
        let required = scope(name);
        let write = required == Scope::Send;
        let submitting = AtomicBool::new(false);
        let execute = async {
            let operation =
                self.operation(name, Value::Object(request.arguments.unwrap_or_default()))?;
            let delegation = self.delegate(auth, &[required]).await?;
            let available = self.capabilities(auth, &delegation).await?;
            if !available.iter().any(|item| item == name) {
                return Err(Failure::DENIED);
            }
            let data = if write {
                submitting.store(true, Ordering::Relaxed);
                self.mutation(operation, auth, &delegation).await?
            } else {
                self.read(name, operation, auth, &delegation).await?
            };
            render(name, data).map_err(|error| if write { Failure::UNKNOWN } else { error })
        };
        let result = tokio::select! {
            _=context.ct.cancelled()=>return Err(ErrorData::internal_error("Request cancelled",None)),
            result=tokio::time::timeout_at(auth.deadline,execute)=>match result{
                Ok(result)=>result,
                Err(_) if submitting.load(Ordering::Relaxed)=>Err(Failure::UNKNOWN),
                Err(_)=>Err(Failure::TIMEOUT),
            }
        };
        Ok(result
            .unwrap_or_else(|error| render_error(error, write))
            .into())
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let auth = caller(&context)?;
        let unavailable = || ErrorData::resource_not_found("Preview unavailable", None);
        let id = request
            .uri
            .strip_prefix(RESOURCE_PREFIX)
            .filter(|id| handle(id))
            .ok_or_else(unavailable)?;
        let fetch = async {
            let delegated = self.delegate(auth, &[Scope::Read]).await?;
            let available = self.capabilities(auth, &delegated).await?;
            if !available.iter().any(|name| is_preview(name)) {
                return Err(Failure::DENIED);
            }
            self.preview(id, auth, &delegated).await
        };
        let bytes = tokio::select! {
            _=context.ct.cancelled()=>return Err(unavailable()),
            result=tokio::time::timeout_at(auth.deadline,fetch)=>result.ok().and_then(Result::ok).ok_or_else(unavailable)?,
        };
        let resource = ResourceContents::blob(STANDARD.encode(bytes), request.uri)
            .with_mime_type("image/jpeg");
        Ok(ReadResourceResult::new(vec![resource])
            .with_cache_scope(CacheScope::Private)
            .with_ttl_ms(0)
            .into())
    }
}
