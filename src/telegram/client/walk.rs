//! Synchronous decision machine for the message-accumulation loops.
//!
//! The three ops loops (`get_recent_messages_impl`, `search_in_channel`,
//! `search_global`) differ only in which of this module's knobs they set.
//! Keeping the decisions synchronous and above the DI seam is what makes
//! their *ordering* testable: the loops themselves sit below the seam, where
//! `MockTelegramClientTrait` replaces the whole client.

use super::search_budget::SearchBudget;
use crate::telegram::albums::PageAccumulator;
use crate::telegram::converters::{convert_raw_message, timestamp_from_raw};
use crate::telegram::envelope::EntityLookup;
use chrono::{DateTime, Utc};
use grammers_client::peer::Peer;
use grammers_client::tl;
use std::sync::Arc;

/// Whether the driving loop keeps fetching.
//
// `dead_code` is allowed only until Task 4 wires `MessageWalk` into the ops
// loops; remove this attribute when that lands.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Flow {
    Continue,
    Stop,
}

/// What a below-cutoff message means. History and channel search page in
/// reverse chronological order, so the first old message proves the rest are
/// older too — they stop. Global search is ordered by relevance across
/// channels, so an old result says nothing about the next one — it skips.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BelowCutoff {
    Stop,
    Skip,
}

/// The five ways the three loops differ. Every field is inert at its default
/// on the paths that do not use it.
#[allow(dead_code)]
pub(super) struct WalkConfig<'a> {
    pub(super) cutoff_time: DateTime<Utc>,
    pub(super) to_date: Option<DateTime<Utc>>,
    /// Exclusive lower cursor bound; `None` on global search, which rejects
    /// cursors upstream.
    pub(super) after_bound: Option<i32>,
    /// Client-side media filter; `Some` only on history, where `GetHistory`
    /// has no server-side filtering.
    pub(super) media_filter: Option<&'a crate::telegram::types::MediaFilter>,
    pub(super) below_cutoff: BelowCutoff,
}

/// One message off a pager, with the envelope entities it arrived with and
/// the peer to attribute it to. `peer` is `None` only on global search, when
/// the envelope did not name the message's chat.
#[allow(dead_code)]
pub(super) struct Fetched<'p> {
    pub(super) raw: tl::enums::Message,
    pub(super) entities: Arc<EntityLookup>,
    pub(super) peer: Option<&'p Peer>,
}

#[allow(dead_code)]
pub(super) struct MessageWalk<'a> {
    cfg: WalkConfig<'a>,
    page: PageAccumulator,
    budget: SearchBudget,
}

#[allow(dead_code)]
impl<'a> MessageWalk<'a> {
    pub(super) fn new(
        cfg: WalkConfig<'a>,
        collapse_albums: bool,
        limit: usize,
        deadline_secs: u64,
    ) -> Self {
        Self {
            cfg,
            page: PageAccumulator::new(collapse_albums, limit),
            budget: SearchBudget::new(deadline_secs),
        }
    }

    /// True once the wall-clock budget is spent. Latches `timed_out`.
    pub(super) fn expired(&mut self) -> bool {
        self.budget.expired()
    }

    /// Fold one pager result into the page.
    ///
    /// `page_size` is `Some` exactly when the fetch that produced `fetched`
    /// crossed a page boundary. It is recorded *before* any early return, so
    /// a round trip that came back empty is still counted — that is what
    /// `pages_fetched` reports.
    pub(super) fn step(&mut self, fetched: Option<Fetched<'_>>, page_size: Option<usize>) -> Flow {
        if let Some(size) = page_size {
            self.budget.record_page(size);
        }
        let Some(item) = fetched else {
            return Flow::Stop;
        };
        let timestamp = timestamp_from_raw(&item.raw);
        // Newer than the requested window: keep iterating toward it.
        if let Some(to) = self.cfg.to_date
            && timestamp.is_some_and(|t| t > to)
        {
            return Flow::Continue;
        }
        // Below the window, or undated. `is_none_or` matches the original
        // loops: an unreadable date is treated as out-of-window, not admitted.
        if timestamp.is_none_or(|t| t < self.cfg.cutoff_time) {
            return match self.cfg.below_cutoff {
                BelowCutoff::Stop => Flow::Stop,
                BelowCutoff::Skip => Flow::Continue,
            };
        }
        let Some(converted) = item
            .peer
            .and_then(|peer| convert_raw_message(&item.raw, peer, &item.entities))
        else {
            return Flow::Continue;
        };
        if self.page.push(converted) {
            Flow::Continue
        } else {
            Flow::Stop
        }
    }

    pub(super) fn pages_fetched(&self) -> u32 {
        self.budget.pages_fetched()
    }

    pub(super) fn messages_scanned(&self) -> u64 {
        self.budget.messages_scanned()
    }

    /// Messages admitted so far (pre-collapse) — for progress logging.
    pub(super) fn kept(&self) -> usize {
        self.page.len()
    }

    pub(super) fn into_parts(self) -> (PageAccumulator, SearchBudget) {
        (self.page, self.budget)
    }
}

#[cfg(test)]
#[path = "tests/walk_tests.rs"]
mod tests;
