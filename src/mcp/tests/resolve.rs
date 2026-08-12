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
                    error: Some("Channel not found: 999".into()),
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
