//! SDK dispatch fixtures; none of these optional features are enabled in McpServer.
use super::*;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ClientCapabilities, ContentBlock,
    ErrorData, Implementation, InitializeResult, InputRequiredResult, ServerCapabilities,
    SubscriptionFilter,
};
use rmcp::service::{RequestContext, RoleServer, SubscriptionContext};

#[derive(Clone)]
struct PatternHandler {
    close: Arc<tokio::sync::Notify>,
}
impl rmcp::ServerHandler for PatternHandler {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .build(),
        )
        .with_server_info(Implementation::new("pattern-fixture", "1"))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.name == "context" {
            return Ok(CallToolResult::success(vec![ContentBlock::text(json!({
                "version":context.protocol_version(), "client":context.client_info(), "caps":context.client_capabilities()
            }).to_string())]).into());
        }
        if request.name == "requires-input"
            && context.client_capabilities().is_none_or(|caps| {
                serde_json::to_value(caps)
                    .expect("caps")
                    .get("elicitation")
                    .is_none()
            })
        {
            return Err(ErrorData::missing_required_client_capability(
                serde_json::from_value::<ClientCapabilities>(json!({"elicitation":{}}))
                    .expect("capabilities"),
            ));
        }
        if request.request_state.as_deref() == Some("phase-1") {
            Ok(CallToolResult::success(vec![ContentBlock::text("complete")]).into())
        } else {
            Ok(InputRequiredResult::new(None, Some("phase-1".into())).into())
        }
    }
    fn accepted_subscription_filter(
        &self,
        request: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        Some(request.clone())
    }
    async fn listen(&self, context: SubscriptionContext) -> Result<(), ErrorData> {
        context
            .sink()
            .notify_tool_list_changed()
            .await
            .map_err(|_| ErrorData::internal_error("fixture notification", None))?;
        self.close.notified().await;
        Ok(())
    }
}
fn fixture() -> PatternHandler {
    PatternHandler {
        close: Arc::new(tokio::sync::Notify::new()),
    }
}

#[tokio::test]
async fn request_identity_and_capabilities_do_not_leak_across_interleaved_calls() {
    let mut wire = Wire::with_handler(fixture());
    let mut first = pending_call(json!(1));
    first["params"]["name"] = json!("context");
    first["params"]["_meta"]["io.modelcontextprotocol/clientInfo"] =
        json!({"name":"A","version":"1"});
    first["params"]["_meta"]["io.modelcontextprotocol/clientCapabilities"] =
        json!({"elicitation":{}});
    let mut second = pending_call(json!(2));
    second["params"]["name"] = json!("context");
    wire.send(first).await;
    wire.send(second).await;
    for _ in 0..2 {
        let reply = wire.recv().await;
        let data: Value = serde_json::from_str(
            reply["result"]["content"][0]["text"]
                .as_str()
                .expect("text"),
        )
        .expect("context JSON");
        if reply["id"] == 1 {
            assert_eq!(data["client"]["name"], "A");
            assert!(data["caps"].get("elicitation").is_some());
        } else {
            assert!(data["client"].is_null());
            assert_eq!(data["caps"], json!({}));
        }
    }
    wire.close().await;
}

#[tokio::test]
async fn mrtr_round_trip_is_explicit_uncached_and_legacy_is_rejected() {
    let mut wire = Wire::with_handler(fixture());
    wire.send(pending_call(json!(1))).await;
    let first = wire.recv().await;
    assert_eq!(first["result"]["resultType"], "input_required");
    assert_eq!(first["result"]["requestState"], "phase-1");
    assert!(first["result"].get("ttlMs").is_none());
    let mut retry = pending_call(json!(2));
    retry["params"]["requestState"] = first["result"]["requestState"].clone();
    retry["params"]["inputResponses"] = json!({});
    wire.send(retry).await;
    assert_eq!(wire.recv().await["result"]["resultType"], "complete");
    let mut unsupported = pending_call(json!(3));
    unsupported["params"]["name"] = json!("requires-input");
    wire.send(unsupported).await;
    let error = wire.recv().await;
    assert_eq!(error["error"]["code"], -32021);
    assert!(
        error["error"]["data"]["requiredCapabilities"]
            .get("elicitation")
            .is_some()
    );
    let mut legacy = pending_call(json!(4));
    legacy["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("2025-11-25");
    wire.send(legacy).await;
    assert_eq!(wire.recv().await["error"]["code"], -32600);
    wire.close().await;
}

#[tokio::test]
async fn subscriptions_acknowledge_filtered_events_then_close_or_cancel() {
    let fixture = fixture();
    let mut wire = Wire::with_handler(fixture.clone());
    for id in [1, 2] {
        let mut request = modern(json!(id), "subscriptions/listen");
        request["params"]["notifications"] =
            json!({"toolsListChanged":true,"promptsListChanged":true});
        wire.send(request).await;
        let ack = wire.recv().await;
        assert_eq!(ack["method"], "notifications/subscriptions/acknowledged");
        assert_eq!(
            ack["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
            id
        );
        assert_eq!(
            ack["params"]["notifications"],
            json!({"toolsListChanged":true})
        );
        let event = wire.recv().await;
        assert_eq!(event["method"], "notifications/tools/list_changed");
        assert_eq!(
            event["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
            id
        );
        if id == 1 {
            fixture.close.notify_one();
            let result = wire.recv().await;
            assert_eq!(result["id"], 1);
            assert_eq!(result["result"]["resultType"], "complete");
        } else {
            wire.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}})).await;
        }
    }
    wire.send(modern(json!(3), "tools/list")).await;
    assert_eq!(wire.recv().await["id"], 3);
    assert!(wire.metrics.abandoned_requests().is_empty());
    wire.close().await;
}
