//! Channel posting statistics (work-order A5): pure math over an
//! album-collapsed history sample, plus the sample descriptor itself.

use super::entities::Message;
use super::ids::ChannelId;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Posting-rate and engagement statistics for one channel (work-order A5).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChannelStats {
    pub channel_id: ChannelId,
    #[schemars(description = "Album-collapsed posts in the sampled window")]
    pub post_count: u64,
    #[schemars(description = "post_count over the sampled span (span floored at 1 hour)")]
    pub posts_per_day: f64,
    #[schemars(description = "Median views over sampled posts; null when views are unavailable")]
    pub median_views: Option<u64>,
    #[schemars(description = "Share of posts carrying media, 0.0-1.0")]
    pub media_share: f64,
    #[schemars(description = "Share of posts that are albums, 0.0-1.0")]
    pub album_share: f64,
    pub sample: StatsSample,
}

/// What the sweep actually covered, so the caller can tell a full window
/// from a cap-truncated one (work-order A5).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StatsSample {
    #[schemars(description = "Raw message records scanned (pre-collapse)")]
    pub messages_scanned: u32,
    pub window_from: DateTime<Utc>,
    pub window_to: DateTime<Utc>,
    #[schemars(description = "False when the scan cap cut the sweep short of days_back")]
    pub complete: bool,
}

impl ChannelStats {
    pub const DEFAULT_DAYS_BACK: u32 = 7;
    pub const MAX_DAYS_BACK: u32 = 30;
    pub const MAX_MESSAGES_SCANNED: u32 = 500;
}

/// Compute stats over already-collapsed posts. Pure so the math is
/// offline-testable; the client sweep supplies the sample bounds.
pub fn compute_stats(
    channel_id: ChannelId,
    posts: &[Message],
    messages_scanned: u32,
    window_from: DateTime<Utc>,
    window_to: DateTime<Utc>,
    complete: bool,
) -> ChannelStats {
    let post_count = posts.len() as u64;
    let span_hours = ((window_to - window_from).num_seconds() as f64 / 3600.0).max(1.0);
    let posts_per_day = if post_count == 0 {
        0.0
    } else {
        post_count as f64 / (span_hours / 24.0)
    };

    let mut views: Vec<u64> = posts.iter().filter_map(|p| p.views).collect();
    views.sort_unstable();
    let median_views = if views.is_empty() {
        None
    } else {
        Some(views[(views.len() - 1) / 2])
    };

    let share = |n: usize| {
        if posts.is_empty() {
            0.0
        } else {
            n as f64 / posts.len() as f64
        }
    };
    ChannelStats {
        channel_id,
        post_count,
        posts_per_day,
        median_views,
        media_share: share(posts.iter().filter(|p| p.has_media).count()),
        album_share: share(posts.iter().filter(|p| p.album.is_some()).count()),
        sample: StatsSample {
            messages_scanned,
            window_from,
            window_to,
            complete,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telegram::types::{AlbumInfo, MediaType, MessageId};
    use crate::test_helpers::create_test_message;
    use chrono::{Duration, Utc};

    fn post(id: i64, views: Option<u64>, media: bool, album: bool) -> crate::telegram::Message {
        let mut m = create_test_message(id, "t", 100);
        m.views = views;
        m.has_media = media;
        if album {
            m.album = Some(AlbumInfo {
                media_count: 2,
                media_types: vec![MediaType::Photo, MediaType::Photo],
                message_ids: vec![
                    MessageId::new(id).expect("id"),
                    MessageId::new(id + 1).expect("id"),
                ],
            });
        }
        m
    }

    #[test]
    fn stats_over_mixed_posts() {
        let to = Utc::now();
        let from = to - Duration::days(2);
        let posts = vec![
            post(1, Some(100), true, true),
            post(2, Some(300), true, false),
            post(3, Some(200), false, false),
            post(4, None, false, false),
        ];
        let s = compute_stats(
            crate::telegram::ChannelId::new(100).expect("id"),
            &posts,
            5,
            from,
            to,
            true,
        );
        assert_eq!(s.post_count, 4);
        assert!((s.posts_per_day - 2.0).abs() < 1e-9);
        assert_eq!(
            s.median_views,
            Some(200),
            "lower-middle median of [100,200,300]"
        );
        assert!((s.media_share - 0.5).abs() < 1e-9);
        assert!((s.album_share - 0.25).abs() < 1e-9);
        assert_eq!(s.sample.messages_scanned, 5);
        assert!(s.sample.complete);
    }

    #[test]
    fn stats_on_empty_window_are_zero_not_nan() {
        let to = Utc::now();
        let s = compute_stats(
            crate::telegram::ChannelId::new(100).expect("id"),
            &[],
            0,
            to - Duration::days(7),
            to,
            true,
        );
        assert_eq!(s.post_count, 0);
        assert_eq!(s.posts_per_day, 0.0);
        assert_eq!(s.median_views, None);
        assert_eq!(s.media_share, 0.0);
        assert_eq!(s.album_share, 0.0);
    }

    #[test]
    fn posts_per_day_floors_span_at_one_hour() {
        let to = Utc::now();
        let from = to - Duration::minutes(5); // tiny sampled span
        let s = compute_stats(
            crate::telegram::ChannelId::new(100).expect("id"),
            &[post(1, None, false, false)],
            1,
            from,
            to,
            false,
        );
        // 1 post over a floored 1-hour span = 24 posts/day, not 288.
        assert!(
            (s.posts_per_day - 24.0).abs() < 1e-6,
            "got {}",
            s.posts_per_day
        );
        assert!(!s.sample.complete);
    }
}
