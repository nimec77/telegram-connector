use super::*;
use crate::telegram::AccountStatus;
use crate::telegram::types::{MediaDownload, MediaType, TranscriptionOutcome};

fn transcription_server() -> McpServer<MockTelegramClientTrait, MockRateLimiterTrait> {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_cached_account_status()
        .returning(|| AccountStatus {
            authorized: true,
            premium: Some(true),
            ..Default::default()
        });
    client.expect_transcribe_audio().returning(|_, _, _| {
        Ok(TranscriptionOutcome {
            channel_id: crate::telegram::types::ChannelId::new(100).expect("fixture channel"),
            peer_namespace: crate::telegram::types::PeerNamespace::Channel,
            message_id: crate::telegram::types::MessageId::new(1).expect("fixture message"),
            text: "PRIVATE TRANSCRIPTION".into(),
            partial: false,
            media_type: MediaType::Voice,
            duration_seconds: Some(2),
        })
    });
    McpServer::new(
        Arc::new(client),
        Arc::new(crate::test_helpers::permissive_limiter()),
    )
}
fn call(id: i64, name: &str, token: Option<Value>) -> Value {
    let mut request = modern(json!(id), "tools/call");
    request["params"]["name"] = json!(name);
    request["params"]["arguments"] = json!({"channel_id":"news", "message_id":7});
    if let Some(token) = token {
        request["params"]["_meta"]["progressToken"] = token;
    }
    request
}
async fn progress_until_result(wire: &mut Wire, id: i64, token: Value) -> (usize, Value) {
    let mut last = 0.0;
    let mut count = 0;
    loop {
        let frame = wire.recv().await;
        if frame.get("id").is_some() {
            assert_eq!(frame["id"], id);
            return (count, frame);
        }
        assert_eq!(frame["method"], "notifications/progress");
        assert_eq!(frame["params"]["progressToken"], token);
        let progress = frame["params"]["progress"]
            .as_f64()
            .expect("numeric progress");
        assert!(progress > last);
        last = progress;
        count += 1;
        let text = frame.to_string();
        assert!(!text.contains("PRIVATE"));
        assert!(!text.contains("news"));
    }
}

#[tokio::test]
async fn transcription_progress_has_real_phases_and_only_requested_tokens() {
    let mut wire = Wire::with_handler(transcription_server());
    wire.send(call(1, "transcribe_voice_message", None)).await;
    assert_eq!(wire.recv().await["id"], 1);
    for (id, token) in [(2, json!("voice")), (3, json!(9))] {
        wire.send(call(id, "transcribe_voice_message", Some(token.clone())))
            .await;
        let (count, result) = progress_until_result(&mut wire, id, token).await;
        assert!(
            count >= 3,
            "must report preparation/processing/result phases, got {count}"
        );
        assert_eq!(
            result["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "telegram-mcp"
        );
        assert!(result.to_string().contains("PRIVATE TRANSCRIPTION"));
    }
    wire.send(modern(json!(99), "tools/list")).await;
    assert_eq!(wire.recv().await["id"], 99);
    wire.close().await;
}

#[tokio::test]
async fn media_progress_keeps_binary_content_and_input_limit_independent() {
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_message_media()
        .return_once(|_, _, _| {
            let bytes = crate::test_helpers::create_test_jpeg(200, 100);
            Ok(MediaDownload {
                channel_id: crate::telegram::types::ChannelId::new(100).expect("fixture channel"),
                peer_namespace: crate::telegram::types::PeerNamespace::Channel,
                source_size_bytes: bytes.len() as u64,
                bytes,
                media_type: MediaType::Photo,
                is_thumbnail: false,
                caption: Some("PRIVATE CAPTION".into()),
                width: Some(200),
                height: Some(100),
                video_info: None,
                largest_width: None,
                largest_height: None,
            })
        });
    let server = McpServer::new(
        Arc::new(client),
        Arc::new(crate::test_helpers::permissive_limiter()),
    );
    let mut wire = Wire::configured(
        server,
        crate::config::ServerConfig {
            max_request_bytes: 512,
            ..Default::default()
        },
        65536,
    );
    wire.send(call(1, "get_message_media", Some(json!("image"))))
        .await;
    let (count, result) = progress_until_result(&mut wire, 1, json!("image")).await;
    assert!(count >= 3, "download/encode/result progress");
    assert_eq!(
        result["result"]["structuredContent"]["data"]["items"][0]["content_index"],
        1
    );
    assert_eq!(result["result"]["content"][1]["type"], "image");
    assert!(result.to_string().len() > 512);
    wire.close().await;
}

#[tokio::test]
async fn malformed_long_tool_metadata_has_no_progress() {
    let mut wire = Wire::with_handler(transcription_server());
    wire.send(modern(json!(90), "tools/list")).await;
    wire.recv().await;
    let mut request = call(1, "transcribe_voice_message", Some(json!("invalid")));
    request["params"]["_meta"]
        .as_object_mut()
        .expect("meta")
        .remove("io.modelcontextprotocol/clientCapabilities");
    wire.send(request).await;
    assert_eq!(wire.recv().await["error"]["code"], -32602);
    wire.close().await;
}

#[derive(Clone)]
struct ProgressWait {
    started: Arc<tokio::sync::Notify>,
    dropped: Arc<tokio::sync::Notify>,
}
impl rmcp::ServerHandler for ProgressWait {
    async fn list_tools(
        &self,
        _: Option<rmcp::model::PaginatedRequestParams>,
        _: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::ErrorData> {
        Ok(rmcp::model::ListToolsResult::with_all_items(vec![
            rmcp::model::Tool::new(
                "offline",
                "x".repeat(1024),
                Arc::new(serde_json::Map::new()),
            ),
        ]))
    }

    async fn call_tool(
        &self,
        _: rmcp::model::CallToolRequestParams,
        _: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        let _guard = DropNotice(Arc::clone(&self.dropped));
        crate::mcp::runtime::report_progress("Phase started").await;
        self.started.notify_one();
        std::future::pending::<()>().await;
        unreachable!("pending fixture")
    }
}
#[tokio::test(start_paused = true)]
async fn cancellation_suppresses_progress_already_queued_behind_a_blocked_write() {
    let handler = ProgressWait {
        started: Arc::new(tokio::sync::Notify::new()),
        dropped: Arc::new(tokio::sync::Notify::new()),
    };
    let mut wire = Wire::configured(handler.clone(), crate::config::ServerConfig::default(), 256);
    // The fixture list deliberately fills stdout before progress can write.
    wire.send(modern(json!(1), "tools/list")).await;
    tokio::task::yield_now().await;
    let mut call = pending_call(json!(2));
    call["params"]["_meta"]["progressToken"] = json!("queued");
    wire.send(call).await;
    tokio::time::timeout(Duration::from_millis(10), handler.started.notified())
        .await
        .expect("progress send is bounded");
    wire.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2}}))
        .await;
    handler.dropped.notified().await;
    assert_eq!(wire.recv().await["id"], 1);
    wire.send(modern(json!(3), "tools/list")).await;
    assert_eq!(wire.recv().await["id"], 3, "no late queued progress frame");
    assert!(wire.metrics.abandoned_requests().is_empty());
    wire.close().await;
}

struct WaitingLimiter {
    started: Arc<tokio::sync::Notify>,
    dropped: Arc<tokio::sync::Notify>,
}
#[async_trait::async_trait]
impl crate::rate_limiter::RateLimiterTrait for WaitingLimiter {
    async fn acquire(&self, _: u32) -> Result<(), crate::error::Error> {
        let _guard = DropNotice(Arc::clone(&self.dropped));
        self.started.notify_one();
        std::future::pending().await
    }
    fn refund(&self, _: u32) {}
    fn available_tokens(&self) -> f64 {
        0.0
    }
    fn capacity(&self) -> f64 {
        60.0
    }
    fn refill_rate(&self) -> f64 {
        2.0
    }
}
#[tokio::test(start_paused = true)]
async fn cancelling_real_media_call_drops_admission_wait_and_never_starts_telegram_rpc() {
    let started = Arc::new(tokio::sync::Notify::new());
    let dropped = Arc::new(tokio::sync::Notify::new());
    let mut client = MockTelegramClientTrait::new();
    client.expect_download_message_media().never();
    let server = McpServer::new(
        Arc::new(client),
        Arc::new(WaitingLimiter {
            started: Arc::clone(&started),
            dropped: Arc::clone(&dropped),
        }),
    );
    let mut wire = Wire::with_handler(server);
    wire.send(call(1, "get_message_media", Some(json!("cancel-media"))))
        .await;
    assert_eq!(wire.recv().await["method"], "notifications/progress");
    started.notified().await;
    wire.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}))
        .await;
    dropped.notified().await;
    wire.send(modern(json!(2), "tools/list")).await;
    assert_eq!(wire.recv().await["id"], 2);
    wire.close().await;
}
