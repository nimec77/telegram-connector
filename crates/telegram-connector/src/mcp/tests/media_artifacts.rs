use super::super::McpServer;
use crate::mcp::tools::GetMessageMediaRequest;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use std::sync::Arc;

#[tokio::test]
async fn unavailable_media_delivery_rejects_before_account_or_network_work() {
    for delivery in ["resource", "file", "resource_and_file"] {
        let server = McpServer::new(
            Arc::new(MockTelegramClientTrait::new()),
            Arc::new(MockRateLimiterTrait::new()),
        );
        let request: GetMessageMediaRequest = serde_json::from_value(
            serde_json::json!({"channel_id":"news", "message_id":42, "delivery":delivery}),
        )
        .expect("request");
        let error = server
            .get_message_media_data(request)
            .await
            .err()
            .expect("unavailable mode");
        assert!(matches!(error, crate::error::Error::InvalidInput(_)));
    }
}

fn photo() -> crate::telegram::types::MediaDownload {
    crate::telegram::types::MediaDownload {
        lease: None,
        channel_id: crate::telegram::types::ChannelId::new(100).expect("channel"),
        peer_namespace: crate::telegram::types::PeerNamespace::Channel,
        bytes: crate::test_helpers::create_test_jpeg(24, 16),
        media_type: crate::telegram::types::MediaType::Photo,
        is_thumbnail: false,
        caption: None,
        width: Some(24),
        height: Some(16),
        source_size_bytes: 100,
        video_info: None,
        largest_width: None,
        largest_height: None,
    }
}

#[tokio::test]
async fn artifact_delivery_retains_preview_and_exports_exact_processed_jpeg() {
    use crate::mcp::tools::types::contract::WireData as _;
    use base64::Engine as _;
    for delivery in ["inline", "resource", "file", "resource_and_file"] {
        let temp = tempfile::tempdir().expect("root");
        let mut client = MockTelegramClientTrait::new();
        client
            .expect_download_message_media()
            .return_once(|_, _, _| Ok(photo()));
        let server = McpServer::new(
            Arc::new(client),
            Arc::new(crate::test_helpers::permissive_limiter()),
        )
        .with_media_artifacts(&crate::config::MediaArtifactsConfig {
            enabled: true,
            file_export_enabled: true,
            root: Some(temp.path().join("artifacts")),
            ..Default::default()
        })
        .await
        .expect("initialize");
        let request = serde_json::from_value(
            serde_json::json!({"channel_id":"news","message_id":42,"delivery":delivery}),
        )
        .expect("request");
        let (data, blocks) = server
            .get_message_media_data(request)
            .await
            .expect("media")
            .canonical()
            .expect("contract");
        let data = serde_json::to_value(data).expect("JSON");
        assert_eq!(data["summary"]["returned"], 1);
        assert_eq!(data["items"][0]["content_index"], 1);
        let rmcp::model::ContentBlock::Image(image) = &blocks[0] else {
            panic!("preview")
        };
        let jpeg = base64::engine::general_purpose::STANDARD
            .decode(&image.data)
            .expect("JPEG");
        let artifact = &data["items"][0]["artifact"];
        if delivery == "inline" {
            assert!(artifact.is_null());
            continue;
        }
        assert_eq!(artifact["kind"], "processed_jpeg");
        assert_eq!(artifact["size_bytes"], jpeg.len());
        assert_eq!(artifact["width"], 24);
        assert_eq!(artifact["height"], 16);
        if delivery.contains("file") {
            let path = artifact["file_path"].as_str().expect("explicit file path");
            assert_eq!(std::fs::read(path).expect("export"), jpeg);
        } else {
            assert!(artifact.get("file_path").is_none());
        }
        if delivery.contains("resource") {
            assert_eq!(artifact["resource_content_index"], 2);
            assert!(matches!(
                blocks[1],
                rmcp::model::ContentBlock::ResourceLink(_)
            ));
            assert!(
                artifact["resource_uri"]
                    .as_str()
                    .expect("URI")
                    .starts_with("telegram-media://artifact/")
            );
        } else {
            assert_eq!(blocks.len(), 1);
            assert!(artifact.get("resource_uri").is_none());
        }
    }
}

#[tokio::test]
async fn ordinary_eof_and_signal_explicitly_close_store_shared_by_live_clones() {
    for signal in [false, true] {
        let temp = tempfile::tempdir().expect("fixture");
        let root = temp.path().join("artifacts");
        let server = McpServer::new(
            Arc::new(MockTelegramClientTrait::new()),
            Arc::new(MockRateLimiterTrait::new()),
        )
        .with_media_artifacts(&crate::config::MediaArtifactsConfig {
            enabled: true,
            root: Some(root.clone()),
            ..Default::default()
        })
        .await
        .expect("store");
        let survivor = Arc::clone(server.media_artifacts.as_ref().expect("shared store"));
        let store = Arc::clone(server.media_artifacts.as_ref().expect("store"));
        let artifact = crate::mcp::runtime::image_job(move || store.insert(b"jpeg", 1, 1))
            .await
            .expect("worker")
            .expect("insert");
        if signal {
            let (peer, io) = tokio::io::duplex(1024);
            let (read, write) = tokio::io::split(io);
            server
                .run_io((read, write), Default::default(), async { Ok(()) })
                .await
                .expect("signal shutdown");
            drop(peer);
        } else {
            server
                .run_io(
                    (tokio::io::empty(), tokio::io::sink()),
                    Default::default(),
                    std::future::pending(),
                )
                .await
                .expect("EOF shutdown");
        }
        assert!(
            !artifact.path.exists(),
            "owned JPEG remains after ordinary shutdown"
        );
        assert!(
            !root.join(".lock").exists(),
            "owned lock remains after ordinary shutdown"
        );
        let store = survivor;
        assert!(
            crate::mcp::runtime::image_job(move || store.insert(b"late", 1, 1))
                .await
                .expect("worker")
                .is_err()
        );
    }
}

#[tokio::test]
async fn enabled_status_reports_reserved_usage_without_paths_and_preserves_unavailable_state() {
    let temp = tempfile::tempdir().expect("fixture");
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_cached_account_status()
        .returning(Default::default);
    let config: crate::Config = toml::from_str("[telegram]\napi_id=1").expect("config");
    let server = McpServer::new(
        Arc::new(client),
        Arc::new(crate::RateLimiter::new(&config.rate_limiting)),
    )
    .with_media_artifacts(&crate::config::MediaArtifactsConfig {
        enabled: true,
        root: Some(temp.path().join("artifacts")),
        ..Default::default()
    })
    .await
    .expect("store");
    let store = Arc::clone(server.media_artifacts.as_ref().expect("store"));
    crate::mcp::runtime::image_job(move || store.insert(b"jpeg", 1, 1))
        .await
        .expect("worker")
        .expect("insert");
    let status =
        serde_json::to_value(server.check_mcp_status_data().await.expect("status")).expect("JSON");
    assert_eq!(status["media_artifacts"]["usage"]["entries"], 1);
    assert_eq!(status["media_artifacts"]["usage"]["bytes"], 4);
    assert_eq!(
        status["media_artifacts"]["max_resource_read_bytes"],
        6 * 1024 * 1024
    );
    assert!(
        !status
            .to_string()
            .contains(temp.path().to_str().expect("fixture path"))
    );
    std::fs::write(temp.path().join("artifacts/foreign"), b"foreign").expect("foreign file");
    let store = Arc::clone(server.media_artifacts.as_ref().expect("store"));
    assert!(
        crate::mcp::runtime::image_job(move || store.cleanup())
            .await
            .expect("worker")
            .is_err()
    );
    let status =
        serde_json::to_value(server.check_mcp_status_data().await.expect("status")).expect("JSON");
    assert!(status["media_artifacts"]["usage"].is_null());
    assert!(status["media_artifacts"]["unavailable"].is_string());
}

#[tokio::test]
async fn unavailable_file_and_legacy_modes_reject_single_batch_and_direct_adapters() {
    use crate::config::{MediaArtifactsConfig, OutputFormat};
    use rmcp::handler::server::wrapper::Parameters;
    for legacy in [false, true] {
        let temp = tempfile::tempdir().expect("fixture");
        let server = McpServer::new(
            Arc::new(MockTelegramClientTrait::new()),
            Arc::new(MockRateLimiterTrait::new()),
        )
        .with_media_artifacts(&MediaArtifactsConfig {
            enabled: true,
            file_export_enabled: legacy,
            root: Some(temp.path().join("store")),
            ..Default::default()
        })
        .await
        .expect("store")
        .with_output_format(if legacy {
            OutputFormat::Legacy
        } else {
            OutputFormat::JsonV1
        });
        for delivery in if legacy {
            vec!["resource", "file", "resource_and_file"]
        } else {
            vec!["file", "resource_and_file"]
        } {
            let single: GetMessageMediaRequest = serde_json::from_value(
                serde_json::json!({"channel_id":"news","message_id":1,"delivery":delivery}),
            )
            .expect("single");
            let batch: crate::mcp::tools::GetMessagesMediaBatchRequest = serde_json::from_value(
                serde_json::json!({"channel_id":"news","message_ids":[1],"delivery":delivery}),
            )
            .expect("batch");
            assert!(server.get_message_media_data(single.clone()).await.is_err());
            assert!(
                server
                    .get_messages_media_batch_data(batch.clone())
                    .await
                    .is_err()
            );
            assert!(
                server
                    .get_message_media(
                        Parameters(single),
                        rmcp::handler::server::common::RequestId(
                            rmcp::model::NumberOrString::Number(1)
                        )
                    )
                    .await
                    .is_err()
            );
            assert!(
                server
                    .get_messages_media_batch(
                        Parameters(batch),
                        rmcp::handler::server::common::RequestId(
                            rmcp::model::NumberOrString::Number(2)
                        )
                    )
                    .await
                    .is_err()
            );
        }
    }
    let temp = tempfile::tempdir().expect("fixture");
    let server = McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    )
    .with_media_artifacts(&MediaArtifactsConfig {
        enabled: true,
        file_export_enabled: true,
        root: Some(temp.path().join("store")),
        ..Default::default()
    })
    .await
    .expect("store");
    let single = serde_json::from_value(
        serde_json::json!({"channel_id":"news","message_id":1,"delivery":"resource"}),
    )
    .expect("single");
    assert!(
        server
            .get_message_media(
                Parameters(single),
                rmcp::handler::server::common::RequestId(rmcp::model::NumberOrString::Number(1))
            )
            .await
            .is_err(),
        "direct Rust adapter is legacy despite json_v1 server mode"
    );
}

#[tokio::test]
async fn batch_entry_quota_never_reports_evicted_artifacts() {
    batch_artifact_quota(1, 1024 * 1024, 1).await;
}

#[tokio::test]
async fn batch_byte_quota_never_reports_evicted_artifacts() {
    batch_artifact_quota(10, 1024, 1).await;
}

#[tokio::test]
async fn resource_batch_indices_count_links_and_quota_failures_refund_images_not_delivered() {
    batch_artifact_quota(10, 1, 0).await;
    batch_artifact_quota(10, 1024 * 1024, 2).await;
}

async fn batch_artifact_quota(entries: usize, quota: u64, expected_returned: usize) {
    use super::media_batch_fixtures::{media_batch, no_media, ok_outcome};
    use crate::mcp::tools::types::contract;
    use base64::Engine as _;
    for delivery in ["resource", "file", "resource_and_file"] {
        let temp = tempfile::tempdir().expect("fixture");
        let mut client = MockTelegramClientTrait::new();
        client
            .expect_download_messages_media_pipeline()
            .return_once(|_, _, _| {
                Ok(media_batch(vec![
                    ok_outcome(1, 24, 16),
                    no_media(2),
                    ok_outcome(3, 24, 16),
                ]))
            });
        let config: crate::Config =
            toml::from_str("[telegram]\napi_id=1\n[rate_limiting]\nrefill_rate=0.0")
                .expect("config");
        let limiter = Arc::new(crate::RateLimiter::new(&config.rate_limiting));
        let server = McpServer::new(Arc::new(client), Arc::clone(&limiter))
            .with_media_artifacts(&crate::config::MediaArtifactsConfig {
                enabled: true,
                file_export_enabled: true,
                root: Some(temp.path().join("store")),
                max_entries: entries,
                max_total_bytes: quota,
                ..Default::default()
            })
            .await
            .expect("store");
        let request = serde_json::from_value(
            serde_json::json!({"channel_id":"news","message_ids":[1,2,3],"delivery":delivery}),
        )
        .expect("batch");
        let result = contract::render(
            crate::config::OutputFormat::JsonV1,
            server.get_messages_media_batch_data(request).await,
        );
        let data = &result.structured_content.as_ref().expect("JSON")["data"];
        let mut next_index = 1;
        for item in data["items"].as_array().expect("items") {
            let Some(artifact) = item.get("artifact") else {
                continue;
            };
            assert_eq!(item["content_index"], next_index);
            let rmcp::model::ContentBlock::Image(image) = &result.content[next_index] else {
                panic!("image index");
            };
            let jpeg = base64::engine::general_purpose::STANDARD
                .decode(&image.data)
                .expect("JPEG");
            next_index += 1;
            if let Some(uri) = artifact["resource_uri"].as_str() {
                assert_eq!(artifact["resource_content_index"], next_index);
                assert!(matches!(
                    result.content[next_index],
                    rmcp::model::ContentBlock::ResourceLink(_)
                ));
                assert!(
                    server
                        .read_artifact_resource(uri.to_owned(), None)
                        .await
                        .is_ok(),
                    "every successful artifact must be immediately retrievable"
                );
                next_index += 1;
            }
            if let Some(path) = artifact["file_path"].as_str() {
                assert_eq!(
                    std::fs::read(path).expect("successful file must exist"),
                    jpeg
                );
            }
        }
        assert_eq!(result.content.len(), next_index);
        assert_eq!(data["summary"]["returned"], expected_returned);
        assert_eq!(data["summary"]["failed"], 3 - expected_returned);
        assert_eq!(
            limiter.try_available_tokens().expect("tokens"),
            60.0 - 3.0 * expected_returned as f64
        );
        if quota == 1 {
            assert_eq!(data["items"][0]["error"]["code"], "media_too_large");
        } else if expected_returned == 1 {
            assert_eq!(data["items"][2]["error"]["code"], "unavailable");
        }
    }
}

#[tokio::test]
async fn delayed_batch_rejects_expired_artifact_and_refunds_undelivered_image() {
    batch_invalidated_before_publication(false).await;
}

#[tokio::test]
async fn failed_store_rejects_earlier_batch_artifact_and_refunds_undelivered_images() {
    batch_invalidated_before_publication(true).await;
}

async fn batch_invalidated_before_publication(fail_store: bool) {
    use super::media_batch_fixtures::{media_batch, ok_outcome};
    use crate::mcp::tools::types::contract;
    use futures::StreamExt as _;
    let temp = tempfile::tempdir().expect("fixture");
    let root = temp.path().join("store");
    let mutation_root = root.clone();
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media_pipeline()
        .return_once(move |_, _, _| {
            let mut batch = media_batch(vec![ok_outcome(1, 24, 16)]);
            batch.stream = Some(
                futures::stream::once(async move {
                    // The ordered consumer only requests this outcome after inserting the first.
                    if fail_store {
                        std::fs::write(mutation_root.join("foreign"), b"preserve")
                            .expect("foreign file");
                    } else {
                        tokio::time::advance(std::time::Duration::from_secs(2)).await;
                    }
                    ok_outcome(2, 24, 16)
                })
                .boxed(),
            );
            Ok(batch)
        });
    let config: crate::Config =
        toml::from_str("[telegram]\napi_id=1\n[rate_limiting]\nrefill_rate=0.0").expect("config");
    let limiter = Arc::new(crate::RateLimiter::new(&config.rate_limiting));
    let server = McpServer::new(Arc::new(client), Arc::clone(&limiter))
        .with_media_artifacts(&crate::config::MediaArtifactsConfig {
            enabled: true,
            file_export_enabled: true,
            root: Some(root),
            ttl_seconds: 1,
            ..Default::default()
        })
        .await
        .expect("store");
    tokio::time::pause();
    let clock_guard = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    let request = serde_json::from_value(serde_json::json!({
        "channel_id":"news", "message_ids":[1,2], "delivery":"resource_and_file"
    }))
    .expect("request");
    let result = contract::render(
        crate::config::OutputFormat::JsonV1,
        server.get_messages_media_batch_data(request).await,
    );
    clock_guard.abort();
    let data = &result.structured_content.as_ref().expect("JSON")["data"];
    assert_eq!(data["items"][0]["error"]["code"], "unavailable");
    if fail_store {
        assert_eq!(data["summary"]["returned"], 0);
        assert_eq!(result.content.len(), 1);
        assert_eq!(limiter.try_available_tokens().expect("tokens"), 60.0);
    } else {
        assert_eq!(data["summary"]["returned"], 1);
        assert_eq!(data["items"][1]["content_index"], 1);
        assert_eq!(data["items"][1]["artifact"]["resource_content_index"], 2);
        assert_eq!(result.content.len(), 3);
        assert_eq!(limiter.try_available_tokens().expect("tokens"), 57.0);
        let path = data["items"][1]["artifact"]["file_path"]
            .as_str()
            .expect("path");
        assert!(std::fs::read(path).is_ok());
    }
}

#[tokio::test]
async fn artifact_pins_survive_shadow_and_canonical_assembly_then_release_capacity() {
    use crate::mcp::tools::types::contract::{self, WireData as _};
    let temp = tempfile::tempdir().expect("fixture");
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_message_media()
        .returning(|_, _, _| Ok(photo()));
    let server = McpServer::new(
        Arc::new(client),
        Arc::new(crate::test_helpers::permissive_limiter()),
    )
    .with_media_artifacts(&crate::config::MediaArtifactsConfig {
        enabled: true,
        root: Some(temp.path().join("store")),
        max_entries: 1,
        ..Default::default()
    })
    .await
    .expect("store");
    let request = || {
        serde_json::from_value(serde_json::json!({
            "channel_id":"news", "message_id":1, "delivery":"resource"
        }))
        .expect("request")
    };
    let output = server
        .get_message_media_data(request())
        .await
        .expect("first output");
    // The data method already measured a descriptor-bearing overhead shadow.
    assert!(matches!(
        server.get_message_media_data(request()).await,
        Err(crate::error::Error::ArtifactUnavailable)
    ));
    let (data, blocks) = output.canonical().expect("canonical assembly");
    assert!(
        matches!(
            server.get_message_media_data(request()).await,
            Err(crate::error::Error::ArtifactUnavailable)
        ),
        "canonical data still owns protection"
    );
    let mut result = contract::tool_result(contract::ToolResponse::success(data)).expect("result");
    result.content.extend(blocks);
    let old = result.structured_content.as_ref().expect("JSON")["data"]["items"][0]["artifact"]["resource_uri"]
        .as_str().expect("URI").to_owned();
    assert!(
        server
            .read_artifact_resource(old.clone(), None)
            .await
            .is_ok()
    );
    let next = contract::render(
        crate::config::OutputFormat::JsonV1,
        server.get_message_media_data(request()).await,
    );
    assert_eq!(
        next.structured_content.as_ref().expect("JSON")["data"]["summary"]["returned"],
        1
    );
    assert!(
        server.read_artifact_resource(old, None).await.is_err(),
        "later requests may evict completed responses"
    );
}

#[tokio::test]
async fn cancelled_batch_releases_completed_artifact_pins() {
    use super::media_batch_fixtures::{media_batch, ok_outcome};
    use futures::StreamExt as _;
    let temp = tempfile::tempdir().expect("fixture");
    let (started, observed) = tokio::sync::oneshot::channel();
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media_pipeline()
        .return_once(move |_, _, _| {
            let mut batch = media_batch(vec![ok_outcome(1, 24, 16)]);
            batch.stream = Some(
                futures::stream::once(async move {
                    started.send(()).expect("first artifact inserted");
                    futures::future::pending().await
                })
                .boxed(),
            );
            Ok(batch)
        });
    client
        .expect_download_message_media()
        .return_once(|_, _, _| Ok(photo()));
    let server = McpServer::new(
        Arc::new(client),
        Arc::new(crate::test_helpers::permissive_limiter()),
    )
    .with_media_artifacts(&crate::config::MediaArtifactsConfig {
        enabled: true,
        root: Some(temp.path().join("store")),
        max_entries: 1,
        ..Default::default()
    })
    .await
    .expect("store");
    let request = serde_json::from_value(serde_json::json!({
        "channel_id":"news", "message_ids":[1,2], "delivery":"resource"
    }))
    .expect("request");
    let mut operation = Box::pin(server.get_messages_media_batch_data(request));
    tokio::select! {
        _ = &mut operation => panic!("second outcome must remain pending"),
        started = observed => started.expect("first artifact cached"),
    }
    drop(operation);
    let request = serde_json::from_value(serde_json::json!({
        "channel_id":"news", "message_id":3, "delivery":"resource"
    }))
    .expect("request");
    assert!(
        server.get_message_media_data(request).await.is_ok(),
        "cancellation must release completed pins without waiting for TTL"
    );
}

#[tokio::test]
async fn publication_rechecks_expiry_after_data_preparation_without_shadow_refunds() {
    use super::media_batch_fixtures::{media_batch, ok_outcome};
    use crate::mcp::tools::types::contract;
    let temp = tempfile::tempdir().expect("fixture");
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media_pipeline()
        .return_once(|_, _, _| Ok(media_batch(vec![ok_outcome(1, 24, 16)])));
    let config: crate::Config =
        toml::from_str("[telegram]\napi_id=1\n[rate_limiting]\nrefill_rate=0.0").expect("config");
    let limiter = Arc::new(crate::RateLimiter::new(&config.rate_limiting));
    let server = McpServer::new(Arc::new(client), Arc::clone(&limiter))
        .with_media_artifacts(&crate::config::MediaArtifactsConfig {
            enabled: true,
            root: Some(temp.path().join("store")),
            ttl_seconds: 1,
            ..Default::default()
        })
        .await
        .expect("store");
    let request = serde_json::from_value(serde_json::json!({
        "channel_id":"news", "message_ids":[1], "delivery":"resource"
    }))
    .expect("request");
    let output = server
        .get_messages_media_batch_data(request)
        .await
        .expect("output");
    tokio::time::pause();
    tokio::time::advance(std::time::Duration::from_secs(2)).await;
    output
        .check_overhead(crate::config::OutputFormat::JsonV1)
        .expect("shadow");
    assert_eq!(limiter.try_available_tokens().expect("charged"), 57.0);
    let result = contract::render(crate::config::OutputFormat::JsonV1, Ok(output));
    let data = &result.structured_content.as_ref().expect("JSON")["data"];
    assert_eq!(data["summary"]["returned"], 0);
    assert_eq!(data["summary"]["total_base64_bytes"], 0);
    assert_eq!(data["items"][0]["error"]["code"], "unavailable");
    assert_eq!(limiter.try_available_tokens().expect("refund"), 60.0);
    assert_eq!(result.content.len(), 1);
}

#[tokio::test]
async fn resource_read_enforces_six_mib_boundary_expiry_and_safe_storage_failures() {
    let temp = tempfile::tempdir().expect("fixture");
    let server = McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    )
    .with_media_artifacts(&crate::config::MediaArtifactsConfig {
        enabled: true,
        root: Some(temp.path().join("store")),
        ttl_seconds: 600,
        ..Default::default()
    })
    .await
    .expect("store");
    let store = Arc::clone(server.media_artifacts.as_ref().expect("store"));
    let (exact, large) = crate::mcp::runtime::image_job(move || {
        (
            store.insert(&vec![0; 6 * 1024 * 1024], 1, 1),
            store.insert(&vec![0; 6 * 1024 * 1024 + 1], 1, 1),
        )
    })
    .await
    .expect("worker");
    let exact = exact.expect("exact");
    let large = large.expect("large");
    let result = server
        .read_artifact_resource(
            exact.uri.clone(),
            Some(rmcp::model::ProtocolVersion::V_2026_07_28),
        )
        .await
        .expect("exact 6MiB allowed");
    let value = serde_json::to_value(result).expect("JSON");
    assert_eq!(
        value["contents"][0]["blob"].as_str().expect("base64").len(),
        8 * 1024 * 1024
    );
    let error = server
        .read_artifact_resource(large.uri, None)
        .await
        .expect_err("oversized read");
    assert_eq!(
        error.data.expect("safe discriminator")["code"],
        "artifact_too_large"
    );
    tokio::time::pause();
    let runnable = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    tokio::time::advance(std::time::Duration::from_secs(600)).await;
    let error = server
        .read_artifact_resource(exact.uri, None)
        .await
        .expect_err("expired immediately");
    assert_eq!(
        error.data.expect("safe discriminator")["code"],
        "artifact_missing"
    );
    runnable.abort();
    tokio::time::resume();
    std::fs::write(temp.path().join("store/foreign"), b"foreign").expect("foreign file");
    let missing = format!("telegram-media://artifact/{}", "0".repeat(48));
    let error = server
        .read_artifact_resource(missing, None)
        .await
        .expect_err("storage unavailable");
    assert_eq!(
        error.data.expect("safe discriminator")["code"],
        "artifact_unavailable"
    );
    assert!(!error.message.contains(temp.path().to_str().expect("path")));
}

struct ReleaseWorkers(Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>);
impl Drop for ReleaseWorkers {
    fn drop(&mut self) {
        let (lock, wake) = &*self.0;
        *lock.lock().expect("fixture lock") = true;
        wake.notify_all();
    }
}
async fn occupy_workers() -> (ReleaseWorkers, Vec<tokio::task::JoinHandle<()>>) {
    let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let (started, mut observed) = tokio::sync::mpsc::unbounded_channel();
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let release = Arc::clone(&release);
        let started = started.clone();
        tasks.push(tokio::spawn(async move {
            crate::mcp::runtime::image_job(move || {
                started.send(()).expect("worker started");
                let (lock, wake) = &*release;
                let mut ready = lock.lock().expect("fixture lock");
                while !*ready {
                    ready = wake.wait(ready).expect("fixture wait");
                }
            })
            .await
            .expect("worker");
        }));
    }
    observed.recv().await.expect("first slot");
    observed.recv().await.expect("second slot");
    (ReleaseWorkers(release), tasks)
}

async fn resource_read_deadline_includes_worker_admission_and_cancellation_has_no_rpc_case() {
    let temp = tempfile::tempdir().expect("fixture");
    let server = Arc::new(
        McpServer::new(
            Arc::new(MockTelegramClientTrait::new()),
            Arc::new(MockRateLimiterTrait::new()),
        )
        .with_media_artifacts(&crate::config::MediaArtifactsConfig {
            enabled: true,
            root: Some(temp.path().join("store")),
            ..Default::default()
        })
        .await
        .expect("store"),
    );
    let (release, tasks) = occupy_workers().await;
    tokio::time::pause();
    let runnable = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    let worker_server = Arc::clone(&server);
    let uri = format!("telegram-media://artifact/{}", "0".repeat(48));
    let (started, ready) = tokio::sync::oneshot::channel();
    let read = tokio::spawn(async move {
        started.send(()).expect("start read");
        worker_server.read_artifact_resource(uri, None).await
    });
    ready.await.expect("read entered");
    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_millis(5001)).await;
    let error = read
        .await
        .expect("read task")
        .expect_err("five-second deadline");
    assert_eq!(
        error.data.expect("discriminator")["code"],
        "artifact_timeout"
    );
    let worker_server = Arc::clone(&server);
    let read = tokio::spawn(async move {
        worker_server
            .read_artifact_resource("not a URI".into(), None)
            .await
    });
    tokio::task::yield_now().await;
    read.abort();
    assert!(read.await.expect_err("cancelled waiter").is_cancelled());
    drop(release);
    for task in tasks {
        task.await.expect("released worker");
    }
    runnable.abort();
}

#[tokio::test]
async fn zero_grace_shutdown_returns_without_starting_store_closure() {
    let temp = tempfile::tempdir().expect("fixture");
    let root = temp.path().join("store");
    let server = McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    )
    .with_media_artifacts(&crate::config::MediaArtifactsConfig {
        enabled: true,
        root: Some(root.clone()),
        ..Default::default()
    })
    .await
    .expect("store");
    let survivor = Arc::clone(server.media_artifacts.as_ref().expect("store"));
    tokio::time::pause();
    let before = tokio::time::Instant::now();
    server
        .run_io(
            (tokio::io::empty(), tokio::io::sink()),
            crate::config::ServerConfig {
                shutdown_timeout_seconds: 0,
                ..Default::default()
            },
            std::future::pending(),
        )
        .await
        .expect("immediate EOF");
    assert_eq!(tokio::time::Instant::now(), before);
    assert!(
        root.join(".lock").exists(),
        "zero grace does not start new blocking cleanup work"
    );
    tokio::time::resume();
    crate::mcp::runtime::image_job(move || survivor.close())
        .await
        .expect("worker")
        .expect("fixture close");
}

#[tokio::test]
async fn inert_constructor_and_disabled_builder_do_not_touch_files_and_enabled_clones_share_store()
{
    let temp = tempfile::tempdir().expect("fixture");
    let root = temp.path().join("untouched");
    let server = McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    );
    assert!(server.media_artifacts.is_none());
    let server = server
        .with_media_artifacts(&crate::config::MediaArtifactsConfig {
            root: Some(root.clone()),
            ..Default::default()
        })
        .await
        .expect("disabled");
    assert!(!root.exists());
    let server = server
        .with_media_artifacts(&crate::config::MediaArtifactsConfig {
            enabled: true,
            root: Some(root),
            ..Default::default()
        })
        .await
        .expect("enabled");
    let clone = server.clone();
    assert!(Arc::ptr_eq(
        server.media_artifacts.as_ref().expect("store"),
        clone.media_artifacts.as_ref().expect("shared store")
    ));
}

#[tokio::test]
async fn artifact_metadata_overflow_refunds_and_removes_failed_output_artifacts() {
    for delivery in ["inline", "resource_and_file"] {
        let temp = tempfile::tempdir().expect("fixture");
        let root = temp.path().join("artifacts");
        let mut client = MockTelegramClientTrait::new();
        client
            .expect_download_message_media()
            .return_once(|_, _, _| {
                let mut media = photo();
                media.caption = Some("x".repeat(31_750));
                Ok(media)
            });
        let config: crate::Config =
            toml::from_str("[telegram]\napi_id=1\n[rate_limiting]\nrefill_rate=0.0")
                .expect("config");
        let limiter = Arc::new(crate::RateLimiter::new(&config.rate_limiting));
        let server = McpServer::new(Arc::new(client), Arc::clone(&limiter))
            .with_media_artifacts(&crate::config::MediaArtifactsConfig {
                enabled: true,
                file_export_enabled: true,
                root: Some(root.clone()),
                ..Default::default()
            })
            .await
            .expect("store");
        let request = serde_json::from_value(
            serde_json::json!({"channel_id":"news","message_id":1,"delivery":delivery}),
        )
        .expect("request");
        let result = server.get_message_media_data(request).await;
        if delivery == "inline" {
            assert!(result.is_ok(), "caption alone fits the non-binary cap");
            assert_eq!(limiter.try_available_tokens().expect("tokens"), 57.0);
        } else {
            assert!(
                matches!(result, Err(crate::error::Error::ResponseTooLarge)),
                "artifact metadata is counted in both JSON copies"
            );
            assert_eq!(
                limiter.try_available_tokens().expect("refunded tokens"),
                60.0
            );
            assert_eq!(
                std::fs::read_dir(root).expect("root").count(),
                1,
                "failed output artifact removed; only process lock remains"
            );
        }
    }
}

async fn media_artifact_work_uses_original_media_deadline_including_worker_admission_case() {
    use super::media_batch_fixtures::{media_batch, ok_outcome};
    let temp = tempfile::tempdir().expect("fixture");
    let (started, ready) = tokio::sync::oneshot::channel();
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_messages_media_pipeline()
        .return_once(move |_, _, _| {
            let mut batch = media_batch(vec![ok_outcome(1, 24, 16)]);
            batch.deadline = Some(
                crate::telegram::media_pipeline::MediaDeadline::try_new(1).expect("media budget"),
            );
            started.send(()).expect("download completed");
            Ok(batch)
        });
    let config: crate::Config =
        toml::from_str("[telegram]\napi_id=1\n[rate_limiting]\nrefill_rate=0.0").expect("config");
    let limiter = Arc::new(crate::RateLimiter::new(&config.rate_limiting));
    let server = Arc::new(
        McpServer::new(Arc::new(client), Arc::clone(&limiter))
            .with_media_artifacts(&crate::config::MediaArtifactsConfig {
                enabled: true,
                root: Some(temp.path().join("store")),
                ..Default::default()
            })
            .await
            .expect("store"),
    );
    let (release, tasks) = occupy_workers().await;
    tokio::time::pause();
    let runnable = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    let worker_server = Arc::clone(&server);
    let request = serde_json::from_value(
        serde_json::json!({"channel_id":"news","message_ids":[1],"delivery":"resource"}),
    )
    .expect("request");
    let media =
        tokio::spawn(async move { worker_server.get_messages_media_batch_data(request).await });
    ready.await.expect("download complete");
    tokio::time::advance(std::time::Duration::from_millis(1001)).await;
    let output = media.await.expect("media task").expect("per-item timeout");
    assert!(matches!(
        &output.items[0].1,
        Err(crate::error::Error::Timeout { .. })
    ));
    assert_eq!(
        limiter.try_available_tokens().expect("refunded tokens"),
        60.0
    );
    drop(release);
    for task in tasks {
        task.await.expect("released worker");
    }
    runnable.abort();
    tokio::time::resume();
    assert_eq!(
        std::fs::read_dir(temp.path().join("store"))
            .expect("root")
            .count(),
        1,
        "queued cancelled cache work never started"
    );
}

async fn exhausted_runtime_drain_does_not_restart_artifact_shutdown_grace_case() {
    let temp = tempfile::tempdir().expect("fixture");
    let root = temp.path().join("store");
    let (downloaded, ready) = tokio::sync::oneshot::channel();
    let mut client = MockTelegramClientTrait::new();
    client
        .expect_download_message_media()
        .return_once(move |_, _, _| {
            downloaded.send(()).expect("download marker");
            Ok(photo())
        });
    let server = McpServer::new(
        Arc::new(client),
        Arc::new(crate::test_helpers::permissive_limiter()),
    )
    .with_media_artifacts(&crate::config::MediaArtifactsConfig {
        enabled: true,
        root: Some(root.clone()),
        ..Default::default()
    })
    .await
    .expect("store");
    let survivor = Arc::clone(server.media_artifacts.as_ref().expect("store"));
    let (release, tasks) = occupy_workers().await;
    tokio::time::pause();
    let runnable = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    let input = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_message_media","arguments":{"channel_id":"news","message_id":1,"delivery":"resource"},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}});
    let start = tokio::time::Instant::now();
    let task = tokio::spawn(async move {
        server
            .run_io(
                (
                    std::io::Cursor::new(format!("{input}\n").into_bytes()),
                    tokio::io::sink(),
                ),
                crate::config::ServerConfig {
                    shutdown_timeout_seconds: 1,
                    ..Default::default()
                },
                std::future::pending(),
            )
            .await
    });
    ready.await.expect("admitted download");
    tokio::time::advance(std::time::Duration::from_millis(1001)).await;
    task.await.expect("runtime task").expect("drain completes");
    assert!(
        tokio::time::Instant::now() < start + std::time::Duration::from_secs(2),
        "store closure must not obtain a fresh grace"
    );
    assert!(
        root.join(".lock").exists(),
        "expired deadline leaves documented bounded fallback"
    );
    drop(release);
    for task in tasks {
        task.await.expect("released worker");
    }
    runnable.abort();
    tokio::time::resume();
    crate::mcp::runtime::image_job(move || survivor.close())
        .await
        .expect("worker")
        .expect("fixture close");
}

// Held process-wide image permits must not interleave across parallel test runtimes.
async fn held_worker_case(case: &str) {
    let mut command =
        tokio::process::Command::new(std::env::current_exe().expect("test executable"));
    command
        .args([
            "--exact",
            "mcp::server::tests::media_artifacts::held_worker_child",
            "--ignored",
            "--nocapture",
        ])
        .env("MEDIA_ARTIFACT_WORKER_CASE", case)
        .kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(15), command.output())
        .await
        .expect("isolated worker test bound")
        .expect("child process");
    assert!(
        output.status.success(),
        "isolated worker test failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}
#[tokio::test]
#[ignore = "isolated child invoked by the named parent cases"]
async fn held_worker_child() {
    match std::env::var("MEDIA_ARTIFACT_WORKER_CASE")
        .expect("case")
        .as_str()
    {
        "0" => {
            resource_read_deadline_includes_worker_admission_and_cancellation_has_no_rpc_case()
                .await
        }
        "1" => {
            media_artifact_work_uses_original_media_deadline_including_worker_admission_case().await
        }
        "2" => exhausted_runtime_drain_does_not_restart_artifact_shutdown_grace_case().await,
        _ => panic!("unknown worker case"),
    }
}
#[tokio::test]
async fn resource_read_deadline_includes_worker_admission_and_cancellation_has_no_rpc() {
    held_worker_case("0").await;
}
#[tokio::test]
async fn media_artifact_work_uses_original_media_deadline_including_worker_admission() {
    held_worker_case("1").await;
}
#[tokio::test]
async fn exhausted_runtime_drain_does_not_restart_artifact_shutdown_grace() {
    held_worker_case("2").await;
}
