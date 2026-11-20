//! resolve_channels tool tests (work-order A7).

use crate::mcp::server::McpServer;
use crate::mcp::tools::types::requests::ResolveChannelsRequest;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::ChannelResolution;
use crate::test_helpers::create_test_channel;
use std::sync::Arc;

#[tokio::test]
async fn resolve_returns_per_identifier_outcomes() {
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_resolve_channels()
        .withf(|ids| ids == ["swodki", "999"])
        .returning(|_| {
            Ok(vec![
                ChannelResolution {
                    identifier: "swodki".into(),
                    channel: Some(create_test_channel(1144180066, "swodki")),
                    error: None,
                },
                ChannelResolution {
                    identifier: "999".into(),
                    channel: None,
                    error: Some(crate::error::Error::ChannelNotFound("999".into())),
                },
            ])
        });
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().times(1).returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(telegram), Arc::new(limiter));

    let out = server
        .resolve_channels_impl(ResolveChannelsRequest {
            identifiers: vec!["swodki".into(), "999".into()],
        })
        .await
        .expect("ok");
    let json: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert_eq!(json["returned"], 2);
    assert_eq!(json["resolved"], 1);
    assert_eq!(json["resolutions"][0]["channel"]["id"], 1144180066);
    assert_eq!(json["resolutions"][1]["error"], "Channel not found: 999");
}

#[tokio::test]
async fn resolve_rejects_empty_oversized_and_blank_lists() {
    let server = McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    );
    let empty = ResolveChannelsRequest {
        identifiers: vec![],
    };
    assert!(
        server
            .resolve_channels_impl(empty)
            .await
            .unwrap_err()
            .contains("identifiers")
    );

    let oversized = ResolveChannelsRequest {
        identifiers: (0..21).map(|i| i.to_string()).collect(),
    };
    assert!(
        server
            .resolve_channels_impl(oversized)
            .await
            .unwrap_err()
            .contains("20")
    );

    let blank = ResolveChannelsRequest {
        identifiers: vec!["  ".into()],
    };
    assert!(
        server
            .resolve_channels_impl(blank)
            .await
            .unwrap_err()
            .contains("blank")
    );
}

#[tokio::test]
async fn resolved_subscription_boolean_survives_canonical_and_legacy() {
    use crate::config::OutputFormat;
    use rmcp::handler::server::common::RequestId;
    use rmcp::model::NumberOrString;
    use serde_json::{Value, json};
    for mode in [OutputFormat::JsonV1, OutputFormat::Legacy] {
        let mut client = MockTelegramClientTrait::new();
        client.expect_resolve_channels().times(1).returning(|_| {
            Ok([false, true]
                .into_iter()
                .map(|subscribed| {
                    let mut channel = create_test_channel(7, "channelname");
                    channel.is_subscribed = subscribed;
                    ChannelResolution {
                        identifier: "channelname".into(),
                        channel: Some(channel),
                        error: None,
                    }
                })
                .collect())
        });
        let server = McpServer::new(
            Arc::new(client),
            Arc::new(crate::test_helpers::permissive_limiter()),
        )
        .with_output_format(mode);
        let result = server
            .resolve_channels_wire(
                json!({"identifiers":["channelname","joinedname"]})
                    .as_object()
                    .expect("args")
                    .clone(),
                RequestId(NumberOrString::Number(1)),
            )
            .await;
        assert_ne!(result.is_error, Some(true));
        let text: Value =
            serde_json::from_str(&result.content[0].as_text().expect("text").text).expect("JSON");
        let data = if mode == OutputFormat::JsonV1 {
            assert_eq!(result.structured_content.as_ref(), Some(&text));
            &text["data"]
        } else {
            &text
        };
        assert_eq!(data["resolutions"][0]["channel"]["is_subscribed"], false);
        assert_eq!(data["resolutions"][1]["channel"]["is_subscribed"], true);
    }
}
