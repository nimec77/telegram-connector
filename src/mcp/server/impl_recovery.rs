//! Reserve caller keys before dispatch, retaining completed results and cancellation state.
use super::McpServer;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use rmcp::service::{RequestContext, RoleServer};

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn call_tool_with_recovery(
        &self,
        mut request: rmcp::model::CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        use crate::mcp::tools::types::contract;
        let failure = |error| {
            Ok(
                contract::render::<crate::mcp::tools::LastResponsesResponse>(
                    self.output_format,
                    Err(error),
                )
                .into(),
            )
        };
        if !self.tool_router.has_route(&request.name) {
            let call = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
            return self.tool_router.call(call).await;
        }
        let key = request
            .arguments
            .as_mut()
            .and_then(|arguments| arguments.remove("recovery_key"));
        if request.name == "get_last_responses" && key.is_some() {
            return failure(crate::error::Error::InvalidInput(
                "Recovery responses cannot be reserved".into(),
            ));
        }
        let reservation = match key {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(key)) => match self.response_buffer.reserve_key(&key) {
                Ok(reservation) => Some(reservation),
                Err(error) => return failure(error),
            },
            Some(_) => {
                return failure(crate::error::Error::InvalidInput(
                    "recovery_key must be a string".into(),
                ));
            }
        };
        let request_id = context.id.to_string();
        let tool_name = request.name.to_string();
        let call = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        let result = self.tool_router.call(call).await?;
        if let Some(reservation) = reservation
            && let rmcp::model::CallToolResponse::Complete(response) = &result
            && let Ok(payload) = crate::mcp::runtime::response_payload(response)
        {
            let _ = reservation.complete(crate::mcp::observability::BufferedResponse {
                request_id,
                tool_name,
                written_at: std::time::SystemTime::now(),
                size_bytes: payload.len(),
                payload,
            });
        }
        Ok(result)
    }
}
