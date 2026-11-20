use super::*;
use crate::sending::SendingRuntimeConfig;
#[path = "../../../examples/support/offline_client.rs"]
pub(super) mod offline_client;
use offline_client::OfflineClient;

pub(super) fn policy() -> SendingRuntimeConfig {
    let mut config: crate::Config = toml::from_str("[telegram]\napi_id=1").expect("config");
    config.sending.enabled = true;
    SendingRuntimeConfig::try_from_config(&config, std::path::Path::new("/tmp/inert-config.toml"))
        .expect("policy")
}
pub(super) async fn handler(
    client: Arc<OfflineClient>,
    policy: SendingRuntimeConfig,
    capacity: usize,
) -> McpServer<OfflineClient, crate::RateLimiter> {
    let config: crate::Config = toml::from_str("[telegram]\napi_id=1").expect("config");
    let mut observe = config.observability.clone();
    observe.response_buffer_size = capacity;
    McpServer::new(
        client,
        Arc::new(crate::RateLimiter::new(&config.rate_limiting)),
    )
    .with_observability(&observe)
    .with_sending(policy)
    .await
    .expect("inert sending server")
}
pub(super) fn native_wire<
    T: crate::TelegramClientTrait + 'static,
    R: crate::RateLimiterTrait + 'static,
>(
    server: McpServer<T, R>,
    seconds: u64,
) -> Wire {
    native_wire_with_capacity(server, seconds, 65536)
}

pub(super) fn native_wire_with_capacity<
    T: crate::TelegramClientTrait + 'static,
    R: crate::RateLimiterTrait + 'static,
>(
    server: McpServer<T, R>,
    seconds: u64,
    capacity: usize,
) -> Wire {
    let metrics = server.metrics();
    let (client, io) = tokio::io::duplex(capacity);
    let (read, write) = tokio::io::split(io);
    let (shutdown, signal) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        server
            .run_io(
                (read, write),
                crate::config::ServerConfig {
                    shutdown_timeout_seconds: seconds,
                    ..Default::default()
                },
                async {
                    let _ = signal.await;
                    Ok(())
                },
            )
            .await
            .expect("native runtime stops");
    });
    let (read, write) = tokio::io::split(client);
    Wire {
        read: BufReader::new(read),
        write,
        task,
        metrics,
        shutdown: Some(shutdown),
    }
}
pub(super) async fn call(wire: &mut Wire, id: i64, name: &str, args: Value) -> Value {
    let mut r = modern(json!(id), "tools/call");
    r["params"]["name"] = json!(name);
    r["params"]["arguments"] = args;
    wire.send(r).await;
    wire.recv().await
}
pub(super) fn text(key: &str) -> Value {
    json!({"recipient":{"kind":"saved_messages"},"text":"private text","recovery_key":key})
}

#[tokio::test]
async fn sending_protocol_real_output_schema_matches_receipts_in_both_protocol_versions() {
    for version in ["2026-07-28", "2025-11-25"] {
        let client = Arc::new(OfflineClient::new(true));
        let mut wire = native_wire(handler(client, policy(), 8).await, 1);
        if version == "2025-11-25" {
            wire.send(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"offline","version":"1"}}})).await;
            assert_eq!(wire.recv().await["result"]["protocolVersion"], version);
            wire.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
                .await;
        }
        let mut list = modern(json!(1), "tools/list");
        list["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!(version);
        wire.send(list).await;
        let catalog = wire.recv().await;
        let tools = catalog["result"]["tools"].as_array().expect("tools");
        assert_eq!(tools.len(), 18);
        let schema = tools
            .iter()
            .find(|t| t["name"] == "send_message")
            .expect("send schema")["outputSchema"]
            .clone();
        let mut req = modern(json!(2), "tools/call");
        req["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!(version);
        req["params"]["name"] = json!("send_message");
        req["params"]["arguments"] = text("schema-call");
        wire.send(req).await;
        let result = wire.recv().await;
        let data = &result["result"]["structuredContent"];
        assert!(
            jsonschema::validator_for(&schema)
                .expect("actual advertised schema")
                .is_valid(data)
        );
        assert_eq!(data["data"]["delivery_state"], "sent");
        assert_eq!(data["data"]["message_id"], 42);
        assert_eq!(
            serde_json::from_str::<Value>(
                result["result"]["content"][0]["text"]
                    .as_str()
                    .expect("text")
            )
            .expect("JSON"),
            *data
        );
        assert_eq!(
            result["result"].get("resultType").is_some(),
            version == "2026-07-28"
        );
        wire.close().await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn sending_protocol_all_media_receipts_validate_against_actual_schemas_in_both_versions() {
    use crate::sending::test_support::Fixture;
    for version in ["2026-07-28", "2025-11-25"] {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.source.join("audio.mp3"),
            b"\xff\xfb\x90\x00abcdefgh",
        )
        .expect("audio fixture");
        std::fs::write(
            fixture.source.join("video.mp4"),
            b"\0\0\0\x10ftypmp42\0\0\0\0",
        )
        .expect("video fixture");
        let mut voice = vec![0; 27];
        voice[..4].copy_from_slice(b"OggS");
        voice[5] = 2;
        voice[26] = 1;
        voice.push(19);
        voice.extend_from_slice(b"OpusHead\x01\x01\0\0\x80\xbb\0\0\0\0\0");
        std::fs::write(fixture.source.join("voice.ogg"), voice).expect("voice fixture");
        let mut wire = native_wire(
            handler(
                Arc::new(OfflineClient::new(true)),
                fixture.config.clone(),
                8,
            )
            .await,
            1,
        );
        if version == "2025-11-25" {
            wire.send(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"offline","version":"1"}}})).await;
            assert_eq!(wire.recv().await["result"]["protocolVersion"], version);
            wire.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
                .await;
        }
        let mut list = modern(json!(1), "tools/list");
        list["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!(version);
        wire.send(list).await;
        let list = wire.recv().await;
        let schema = &list["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .find(|t| t["name"] == "send_media")
            .expect("media schema")["outputSchema"];
        let validator = jsonschema::validator_for(schema).expect("actual output schema");
        for (index, (name, media, kind, mime)) in [
            ("image.jpg", json!({"kind":"image"}), "image", "image/jpeg"),
            (
                "audio.mp3",
                json!({"kind":"audio","duration_seconds":1}),
                "audio",
                "audio/mpeg",
            ),
            (
                "voice.ogg",
                json!({"kind":"voice","duration_seconds":1}),
                "voice",
                "audio/ogg",
            ),
            (
                "video.mp4",
                json!({"kind":"video","duration_seconds":1.0,"width":16,"height":16}),
                "video",
                "video/mp4",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let mut req = modern(json!(index + 2), "tools/call");
            req["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!(version);
            req["params"]["name"] = json!("send_media");
            req["params"]["arguments"] = json!({"recipient":{"kind":"saved_messages"},"source":{"root":"outgoing","relative_path":name},"media":media,"caption":"do not echo this caption"});
            wire.send(req).await;
            let response = wire.recv().await;
            let body = &response["result"]["structuredContent"];
            assert!(validator.is_valid(body), "{body}");
            assert_eq!(response["result"]["isError"], false);
            assert_eq!(body["data"]["content_kind"], kind);
            assert_eq!(body["data"]["media"]["mime_type"], mime);
            assert!(!response.to_string().contains("do not echo"));
            assert_eq!(
                serde_json::from_str::<Value>(
                    response["result"]["content"][0]["text"]
                        .as_str()
                        .expect("text")
                )
                .expect("JSON"),
                *body
            );
        }
        wire.close().await;
    }
}
