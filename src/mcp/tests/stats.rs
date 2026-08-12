//! get_channel_stats tool tests (work-order A5).

use crate::mcp::server::McpServer;
use crate::mcp::tools::types::requests::GetChannelStatsRequest;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::ChannelId;
use crate::telegram::types::stats::{ChannelStats, compute_stats};
use chrono::Utc;
use std::sync::Arc;

fn fixture_stats() -> ChannelStats {
    let now = Utc::now();
    compute_stats(ChannelId::new(1).expect("id"), &[], 0, now, now, true)
}

#[tokio::test]
async fn stats_clamps_days_back_and_returns_stats() {
    let mut telegram = MockTelegramClientTrait::new();
    telegram
        .expect_get_channel_stats()
        .withf(|c, days| c == "swodki" && *days == ChannelStats::MAX_DAYS_BACK)
        .returning(|_, _| Ok(fixture_stats())); // local helper: ChannelStats literal
    let mut limiter = MockRateLimiterTrait::new();
    limiter.expect_acquire().times(1).returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(telegram), Arc::new(limiter));

    let out = server
        .get_channel_stats_impl(GetChannelStatsRequest {
            channel_id: "swodki".into(),
            days_back: Some(365), // clamped to 30, silently (§1.3 clamping style)
        })
        .await
        .expect("ok");
    let json: serde_json::Value = serde_json::from_str(&out).expect("json");
    assert!(json["posts_per_day"].is_number());
    assert!(json["sample"]["complete"].is_boolean());
}

#[tokio::test]
async fn stats_rejects_blank_channel_and_zero_days() {
    let server = McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    );
    let blank = GetChannelStatsRequest {
        channel_id: "  ".into(),
        days_back: None,
    };
    assert!(
        server
            .get_channel_stats_impl(blank)
            .await
            .unwrap_err()
            .contains("channel_id")
    );
    let zero = GetChannelStatsRequest {
        channel_id: "swodki".into(),
        days_back: Some(0),
    };
    assert!(
        server
            .get_channel_stats_impl(zero)
            .await
            .unwrap_err()
            .contains("days_back")
    );
}
