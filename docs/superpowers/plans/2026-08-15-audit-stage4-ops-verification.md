# Audit Stage 4 — Ops-Layer Verification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Lift the three message-accumulation loops in the `TelegramClient` ops layer above the DI seam into a synchronous, unit-tested decision machine, so their branch *ordering* is pinned by tests rather than only their predicates.

**Architecture:** A new `src/telegram/client/walk.rs` holds `MessageWalk`, which owns the `PageAccumulator` and `SearchBudget` for one accumulation loop and exposes a single synchronous `step(fetched, page_size) -> Flow`. The three async loops (`get_recent_messages_impl`, `search_in_channel`, `search_global`) shrink to ~6 lines of wiring around it. Five further pure functions are extracted from logic sitting outside the loops.

**Tech Stack:** Rust nightly (2024 edition), `grammers` (pinned Codeberg rev), `chrono`, `tokio` (test-util for paused clock), `mockall`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-15-audit-stage4-design.md`

## Global Constraints

- **Never `unwrap()` in production code** — use `?` or `.context("...")`. `expect()` only in tests.
- **Line length 100 chars.** Run `cargo fmt --all` after every code change.
- **TDD:** write the failing test first; no production code without a preceding test.
- **No `mod.rs` files.** Test modules use the `#[path]`-included sibling pattern, e.g. `#[cfg(test)] #[path = "tests/walk_tests.rs"] mod tests;`.
- **Pre-commit gate (all must pass):** `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
- **Layering:** zero `grammers` imports in `src/mcp/`, zero `crate::mcp` imports in `src/telegram/`. This plan touches only `src/telegram/` and `src/config.rs`.
- **Behavior-preserving.** Two deliberate deltas are called out in Tasks 4 and 6; no others are permitted. If a test forces a third, stop and report it rather than absorbing it.
- **Branch:** `refactor/audit-stage4-ops-verification`, cut from `master`. Master requires PRs — never push to it.

---

## File Structure

**Created:**
- `src/telegram/client/walk.rs` — `MessageWalk`, `WalkConfig`, `Fetched`, `Flow`, `BelowCutoff`. Sole responsibility: decide what one fetched message does to the page and whether the loop continues.
- `src/telegram/client/tests/walk_tests.rs` — walk unit tests.
- `src/telegram/client/tests/ops_history_tests.rs` — `dialog_fallback_target` tests.
- `src/telegram/client/tests/ops_message_tests.rs` — `partition_batch` tests.

**Modified:**
- `src/telegram/client.rs` — add `mod walk;`, add `assemble_search_result`.
- `src/telegram/client/ops_history.rs` — wire to `MessageWalk`; extract `dialog_fallback_target`.
- `src/telegram/client/ops_search.rs` — wire both paths; `page` → `page_no` tracing rename.
- `src/telegram/client/ops_message.rs` — extract `partition_batch`.
- `src/telegram/client/channels.rs` — extract `ChannelPageBuilder`, `classify_search_hit`.
- `src/telegram/albums.rs` — `#[must_use]` on `PageAccumulator::push`; one added test.
- `src/telegram/client/tests/channels_tests.rs` — tests for the two channel extractions.
- `src/telegram/client/tests/helpers_tests.rs` — tests for `assemble_search_result`.
- `src/config/tests.rs` — config file-loading error-branch tests.

---

### Task 1: `MessageWalk` skeleton — page accounting and terminal cases

**Files:**
- Create: `src/telegram/client/walk.rs`
- Create: `src/telegram/client/tests/walk_tests.rs`
- Modify: `src/telegram/client.rs` (add `mod walk;` to the module list)

**Interfaces:**
- Consumes: `PageAccumulator::{new, push, has_more, len, into_messages}` (`src/telegram/albums.rs`), `SearchBudget::{new, expired, record_page, pages_fetched, messages_scanned, timed_out}` (`src/telegram/client/search_budget.rs`), `convert_raw_message(&tl::enums::Message, &Peer, &EntityLookup) -> Option<Message>`.
- Produces: `MessageWalk<'a>`, `WalkConfig<'a>`, `Fetched<'p>`, `Flow`, `BelowCutoff` — all `pub(super)`. Tasks 2–3 extend `step`; Tasks 4–6 consume the whole type.

- [ ] **Step 1: Write the failing test**

Create `src/telegram/client/tests/walk_tests.rs`:

```rust
//! Unit tests for the synchronous accumulation-loop decision machine.

use super::*;
use crate::telegram::envelope::EntityLookup;
use crate::test_helpers::{raw_tl_channel, raw_tl_message};
use chrono::{DateTime, TimeZone, Utc};
use grammers_client::Client;
use grammers_client::peer::Peer;
use grammers_mtsender::SenderPool;
use grammers_session::storages::MemorySession;
use std::sync::Arc;

/// A `Client` that never touches the network: the `SenderPool` runner is
/// destructured away and never spawned, so `Peer::from_raw` works offline.
fn inert_client() -> Client {
    let session = Arc::new(MemorySession::default());
    let SenderPool { handle, .. } = SenderPool::new(session, 1);
    Client::new(handle)
}

fn channel_peer(client: &Client, id: i64) -> Peer {
    Peer::from_raw(
        client,
        grammers_client::tl::enums::Chat::Channel(raw_tl_channel(id, "Канал", None)),
    )
}

fn no_entities() -> Arc<EntityLookup> {
    Arc::new(EntityLookup::from_envelope(&[], &[]))
}

fn at(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0).single().expect("valid timestamp")
}

/// Config admitting everything at or after `cutoff`, with no other bounds.
fn open_config(cutoff: DateTime<Utc>) -> WalkConfig<'static> {
    WalkConfig {
        cutoff_time: cutoff,
        to_date: None,
        after_bound: None,
        media_filter: None,
        below_cutoff: BelowCutoff::Stop,
    }
}

fn fetched<'p>(id: i32, date: i32, peer: &'p Peer) -> Fetched<'p> {
    Fetched {
        raw: raw_tl_message(id, date, 11),
        entities: no_entities(),
        peer: Some(peer),
    }
}

#[test]
fn an_empty_round_trip_still_counts_a_page() {
    // The `None` fetch means the pager is exhausted, but the round trip that
    // discovered that still cost the caller latency — `pages_fetched` reports
    // round trips, so accounting must happen before the terminal Stop.
    let mut walk = MessageWalk::new(open_config(at(1_000)), false, 10, 0);

    assert_eq!(walk.step(None, Some(0)), Flow::Stop);
    assert_eq!(walk.pages_fetched(), 1);
    assert_eq!(walk.messages_scanned(), 0);
}

#[test]
fn a_full_page_stops_the_walk_and_latches_has_more() {
    let client = inert_client();
    let peer = channel_peer(&client, 11);
    let mut walk = MessageWalk::new(open_config(at(1_000)), false, 1, 0);

    assert_eq!(walk.step(Some(fetched(2, 2_000, &peer)), Some(2)), Flow::Continue);
    assert_eq!(walk.step(Some(fetched(1, 1_500, &peer)), None), Flow::Stop);

    let (page, budget) = walk.into_parts();
    assert!(page.has_more(), "a refused message must latch has_more");
    assert_eq!(page.into_messages().len(), 1);
    assert_eq!(budget.pages_fetched(), 1);
    assert_eq!(budget.messages_scanned(), 2);
}
```

Register the module at the bottom of `src/telegram/client/walk.rs` (written in Step 3) and add `mod walk;` to the `mod` list in `src/telegram/client.rs` (alphabetically, after `mod search_budget;`).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib telegram::client::walk`
Expected: FAIL — compile error, `walk.rs` does not exist / `MessageWalk` not found.

- [ ] **Step 3: Write minimal implementation**

Create `src/telegram/client/walk.rs`:

```rust
//! Synchronous decision machine for the message-accumulation loops.
//!
//! The three ops loops (`get_recent_messages_impl`, `search_in_channel`,
//! `search_global`) differ only in which of this module's knobs they set.
//! Keeping the decisions synchronous and above the DI seam is what makes
//! their *ordering* testable: the loops themselves sit below the seam, where
//! `MockTelegramClientTrait` replaces the whole client.

use super::search_budget::SearchBudget;
use crate::telegram::albums::PageAccumulator;
use crate::telegram::converters::convert_raw_message;
use crate::telegram::envelope::EntityLookup;
use chrono::{DateTime, Utc};
use grammers_client::peer::Peer;
use grammers_client::tl;
use std::sync::Arc;

/// Whether the driving loop keeps fetching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Flow {
    Continue,
    Stop,
}

/// What a below-cutoff message means. History and channel search page in
/// reverse chronological order, so the first old message proves the rest are
/// older too — they stop. Global search is ordered by relevance across
/// channels, so an old result says nothing about the next one — it skips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BelowCutoff {
    Stop,
    Skip,
}

/// The five ways the three loops differ. Every field is inert at its default
/// on the paths that do not use it.
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
pub(super) struct Fetched<'p> {
    pub(super) raw: tl::enums::Message,
    pub(super) entities: Arc<EntityLookup>,
    pub(super) peer: Option<&'p Peer>,
}

pub(super) struct MessageWalk<'a> {
    cfg: WalkConfig<'a>,
    page: PageAccumulator,
    budget: SearchBudget,
}

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
    pub(super) fn step(
        &mut self,
        fetched: Option<Fetched<'_>>,
        page_size: Option<usize>,
    ) -> Flow {
        if let Some(size) = page_size {
            self.budget.record_page(size);
        }
        let Some(item) = fetched else {
            return Flow::Stop;
        };
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
```

Add `mod walk;` to `src/telegram/client.rs`, after `mod search_budget;`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib telegram::client::walk`
Expected: PASS — 2 tests.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy -- -D warnings
git add src/telegram/client/walk.rs src/telegram/client/tests/walk_tests.rs src/telegram/client.rs
git commit -m "refactor: add MessageWalk with page accounting and terminal cases (audit S4.1)"
```

---

### Task 2: Window triage — `to_date`, cutoff, and `BelowCutoff`

**Files:**
- Modify: `src/telegram/client/walk.rs` (extend `step`)
- Modify: `src/telegram/client/tests/walk_tests.rs`

**Interfaces:**
- Consumes: `timestamp_from_raw(&tl::enums::Message) -> Option<DateTime<Utc>>` from `crate::telegram::converters`.
- Produces: no signature change — `step` gains two branches ahead of conversion.

- [ ] **Step 1: Write the failing test**

Append to `src/telegram/client/tests/walk_tests.rs`:

```rust
#[test]
fn a_message_newer_than_to_date_is_skipped_not_stopped() {
    // Paging walks backwards toward the window; a too-new message means keep
    // going, never stop.
    let client = inert_client();
    let peer = channel_peer(&client, 11);
    let cfg = WalkConfig {
        to_date: Some(at(2_000)),
        ..open_config(at(1_000))
    };
    let mut walk = MessageWalk::new(cfg, false, 10, 0);

    assert_eq!(walk.step(Some(fetched(9, 3_000, &peer)), None), Flow::Continue);
    assert_eq!(walk.kept(), 0, "a too-new message must not be admitted");
}

#[test]
fn below_cutoff_stops_on_the_reverse_chronological_paths() {
    let client = inert_client();
    let peer = channel_peer(&client, 11);
    let mut walk = MessageWalk::new(open_config(at(1_000)), false, 10, 0);

    assert_eq!(walk.step(Some(fetched(9, 500, &peer)), None), Flow::Stop);
    assert_eq!(walk.kept(), 0);
}

#[test]
fn below_cutoff_skips_on_the_global_path() {
    // Global search is ordered by relevance across channels, so one old
    // result says nothing about the next.
    let client = inert_client();
    let peer = channel_peer(&client, 11);
    let cfg = WalkConfig {
        below_cutoff: BelowCutoff::Skip,
        ..open_config(at(1_000))
    };
    let mut walk = MessageWalk::new(cfg, false, 10, 0);

    assert_eq!(walk.step(Some(fetched(9, 500, &peer)), None), Flow::Continue);
    assert_eq!(walk.kept(), 0);
}

#[test]
fn a_message_with_no_readable_timestamp_takes_the_below_cutoff_path() {
    // `MessageEmpty` has no date. It must not be treated as in-window and
    // handed to conversion, which would reject it anyway — the point is that
    // the *stop* semantics apply, matching the original `is_none_or`.
    let client = inert_client();
    let peer = channel_peer(&client, 11);
    let mut walk = MessageWalk::new(open_config(at(1_000)), false, 10, 0);

    let empty = Fetched {
        raw: grammers_client::tl::enums::Message::Empty(
            grammers_client::tl::types::MessageEmpty {
                id: 7,
                peer_id: None,
            },
        ),
        entities: no_entities(),
        peer: Some(&peer),
    };
    assert_eq!(walk.step(Some(empty), None), Flow::Stop);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib telegram::client::walk`
Expected: FAIL — `below_cutoff_stops_on_the_reverse_chronological_paths` returns `Continue` (no window logic yet); the `to_date` test admits the message so `kept()` is 1.

- [ ] **Step 3: Write minimal implementation**

In `src/telegram/client/walk.rs`, add the import:

```rust
use crate::telegram::converters::{convert_raw_message, timestamp_from_raw};
```

and insert into `step`, immediately after the `let Some(item) = fetched else { ... };` line:

```rust
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib telegram::client::walk`
Expected: PASS — 6 tests.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy -- -D warnings
git add src/telegram/client/walk.rs src/telegram/client/tests/walk_tests.rs
git commit -m "refactor: add window triage to MessageWalk::step (audit S4.1)"
```

---

### Task 3: Cursor bound and media filter

**Files:**
- Modify: `src/telegram/client/walk.rs` (extend `step`)
- Modify: `src/telegram/client/tests/walk_tests.rs`

**Interfaces:**
- Consumes: `matches_media_filter_raw(&tl::enums::Message, &MediaFilter) -> bool` from `crate::telegram::converters`; `MediaFilter` from `crate::telegram::types`.
- Produces: `step` complete. Tasks 4–6 can now wire the loops.

- [ ] **Step 1: Write the failing test**

Append to `src/telegram/client/tests/walk_tests.rs`:

```rust
#[test]
fn the_after_bound_is_exclusive_at_its_own_id() {
    // `after_id` is documented exclusive: the cursor message itself is the
    // one the caller already has.
    let client = inert_client();
    let peer = channel_peer(&client, 11);
    let cfg = WalkConfig {
        after_bound: Some(5),
        ..open_config(at(1_000))
    };
    let mut walk = MessageWalk::new(cfg, false, 10, 0);

    assert_eq!(walk.step(Some(fetched(6, 2_000, &peer)), None), Flow::Continue);
    assert_eq!(walk.kept(), 1);
    assert_eq!(walk.step(Some(fetched(5, 1_900, &peer)), None), Flow::Stop);
    assert_eq!(walk.kept(), 1, "the bound id itself must not be admitted");
}

#[test]
fn a_media_filter_miss_skips_without_stopping() {
    use crate::telegram::types::MediaFilter;

    let client = inert_client();
    let peer = channel_peer(&client, 11);
    // `raw_tl_message` builds a service message: no media, so a Photo filter
    // cannot match it.
    let filter = MediaFilter::Photo;
    let cfg = WalkConfig {
        media_filter: Some(&filter),
        ..open_config(at(1_000))
    };
    let mut walk = MessageWalk::new(cfg, false, 10, 0);

    assert_eq!(walk.step(Some(fetched(9, 2_000, &peer)), None), Flow::Continue);
    assert_eq!(walk.kept(), 0, "a filtered-out message must not be admitted");
}

#[test]
fn a_message_whose_chat_did_not_resolve_is_skipped() {
    // Global search only: the envelope did not name this message's chat, so
    // there is no identity to attribute it to. Skip, never fabricate.
    let mut walk = MessageWalk::new(open_config(at(1_000)), false, 10, 0);

    let orphan = Fetched {
        raw: raw_tl_message(9, 2_000, 11),
        entities: no_entities(),
        peer: None,
    };
    assert_eq!(walk.step(Some(orphan), None), Flow::Continue);
    assert_eq!(walk.kept(), 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib telegram::client::walk`
Expected: FAIL — `the_after_bound_is_exclusive_at_its_own_id` returns `Continue` and admits id 5; the media-filter test admits the message.

- [ ] **Step 3: Write minimal implementation**

In `src/telegram/client/walk.rs`, extend the import:

```rust
use crate::telegram::converters::{
    convert_raw_message, matches_media_filter_raw, timestamp_from_raw,
};
```

and insert into `step`, after the below-cutoff block and before the conversion:

```rust
        // Exclusive lower cursor bound: everything from here on is older
        // (reverse chronological), so stop.
        if let Some(after) = self.cfg.after_bound
            && item.raw.id() <= after
        {
            return Flow::Stop;
        }
        // Client-side media filter (history only — GetHistory has no
        // server-side filtering).
        if self
            .cfg
            .media_filter
            .is_some_and(|filter| !matches_media_filter_raw(&item.raw, filter))
        {
            return Flow::Continue;
        }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib telegram::client::walk`
Expected: PASS — 9 tests.

- [ ] **Step 5: Add the album-boundary regression test**

Albums are the subtlest interaction with `push`: an admitted album's trailing siblings pass even beyond `limit`, so a page can exceed `limit` raw messages without ever reporting `has_more`. Append:

```rust
#[test]
fn an_admitted_album_is_not_split_at_the_limit_boundary() {
    use crate::telegram::types::Message;

    let client = inert_client();
    let peer = channel_peer(&client, 11);
    // limit 1 with collapse on: the first message opens the only allowed
    // post; a sibling of that same post must still be admitted.
    let mut walk = MessageWalk::new(open_config(at(1_000)), true, 1, 0);

    let mut album_member = |id: i32| {
        let mut f = fetched(id, 2_000, &peer);
        if let grammers_client::tl::enums::Message::Service(_) = &f.raw {
            // Service messages carry no grouped_id; swap in a plain message
            // that does, so the album path is actually exercised.
            f.raw = album_raw(id, 2_000, 11, 77);
        }
        f
    };

    assert_eq!(walk.step(Some(album_member(2)), None), Flow::Continue);
    assert_eq!(walk.step(Some(album_member(3)), None), Flow::Continue);

    let (page, _) = walk.into_parts();
    assert!(
        !page.has_more(),
        "a trailing album sibling is not a refusal — has_more must stay false"
    );
    let collapsed: Vec<Message> = page.into_messages();
    assert_eq!(collapsed.len(), 1, "the two siblings collapse to one post");
}
```

Add the `album_raw` fixture near the other helpers at the top of the file — a plain `tl::types::Message` carrying `grouped_id`, since `raw_tl_message` builds a `MessageService` which has no such field:

```rust
/// Plain raw message carrying a `grouped_id`, for album-path tests.
fn album_raw(id: i32, date: i32, channel_id: i64, grouped_id: i64) -> tl::enums::Message {
    tl::enums::Message::Message(tl::types::Message {
        id,
        date,
        peer_id: tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id }),
        grouped_id: Some(grouped_id),
        message: String::new(),
        ..message_defaults()
    })
}
```

**Note for the implementer:** `tl::types::Message` has no `Default` impl in this grammers rev, so `..message_defaults()` will not compile as written. Copy the full field list from the existing fixture at `src/telegram/tests/converters_thumb_forward_tests.rs:80` (`raw_message_with_media`), which already constructs a complete `tl::types::Message`, and set `grouped_id: Some(grouped_id)`. Do not invent field names — read that fixture.

- [ ] **Step 6: Run the album test**

Run: `cargo test --lib telegram::client::walk`
Expected: PASS — 10 tests.

- [ ] **Step 7: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy -- -D warnings
git add src/telegram/client/walk.rs src/telegram/client/tests/walk_tests.rs
git commit -m "refactor: complete MessageWalk::step with cursor bound and media filter (audit S4.1)"
```

---

### Task 4: Wire `get_recent_messages_impl` onto `MessageWalk`

**Files:**
- Modify: `src/telegram/client/ops_history.rs:86-146` (the `with_timeout` closure)

**Interfaces:**
- Consumes: `MessageWalk`, `WalkConfig`, `Fetched`, `Flow`, `BelowCutoff` from Task 3.
- Produces: no new public surface. `get_recent_messages_impl`'s observable behavior is unchanged.

**Deliberate delta (1 of 2):** the history loop gains a `walk.expired()` check it does not have today. This is provably inert — history constructs its budget with `deadline_secs = 0`, and `SearchBudget::new(0)` never expires (pinned by `zero_deadline_is_treated_as_disabled_not_instantly_expired` in `search_budget.rs`). Record it in the PR description.

- [ ] **Step 1: Confirm the existing suite is green before touching the loop**

Run: `cargo test --lib telegram`
Expected: PASS. This is the regression baseline — the ops loops have no direct tests, so the suite around them is the only safety net.

- [ ] **Step 2: Replace the loop body**

In `src/telegram/client/ops_history.rs`, replace the `with_timeout("iter_messages", ...)` closure body. The current body builds `page` and `budget` separately and hand-inlines the triage; the new one delegates:

```rust
        let (page, budget) = with_timeout("iter_messages", self.timeouts.history_secs, async {
            let cfg = WalkConfig {
                cutoff_time,
                to_date: params.to_date,
                after_bound,
                media_filter: params.media_filter.as_ref(),
                below_cutoff: BelowCutoff::Stop,
            };
            // Deadline 0: the spec scopes the search deadline to search, so
            // history's budget carries counters only and never expires.
            let mut walk = MessageWalk::new(cfg, params.collapse_albums, params.limit as usize, 0);
            // Raw GetHistory pager instead of grammers' iter_messages: same
            // request, but it keeps the response envelope so forwards get
            // attributed from data already in hand (zero extra calls).
            let mut pager = RawHistoryPager::new(&self.client, peer_ref);
            if let Some(before) = before_offset {
                pager = pager.offset_id(before);
            }

            loop {
                if walk.expired() {
                    break;
                }
                let next = pager.next().await.map_err(|e| {
                    Error::TelegramApi(format!("Failed to iterate messages: {}", e))
                })?;
                let page_size = pager.take_last_page_size();
                let fetched = next.map(|(raw, entities)| Fetched {
                    raw,
                    entities,
                    peer: Some(&peer),
                });
                if walk.step(fetched, page_size) == Flow::Stop {
                    break;
                }
            }
            Ok(walk.into_parts())
        })
        .await?;
```

Update the imports at the top of the file — replace the `PageAccumulator` and `SearchBudget` imports with:

```rust
use super::walk::{BelowCutoff, Fetched, Flow, MessageWalk, WalkConfig};
```

Delete the now-unused `use super::search_budget::SearchBudget;` and `use crate::telegram::albums::PageAccumulator;` lines if nothing else in the file references them.

- [ ] **Step 3: Run the full suite**

Run: `cargo test`
Expected: PASS, same count as Step 1. Any failure here is a real behavior change — stop and report rather than adjusting the test.

- [ ] **Step 4: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy -- -D warnings
git add src/telegram/client/ops_history.rs
git commit -m "refactor: drive get_recent_messages through MessageWalk (audit S4.2)"
```

---

### Task 5: Wire `search_in_channel` onto `MessageWalk`

**Files:**
- Modify: `src/telegram/client/ops_search.rs:118-216`

**Interfaces:**
- Consumes: `MessageWalk` and friends.
- Produces: `search_in_channel` keeps its signature — `Result<(PageAccumulator, u32, SearchBudget), Error>`.

- [ ] **Step 1: Replace the inner loop**

In `search_in_channel`, replace the `let mut page = ...` / `let mut budget = ...` pair and the inner `loop`:

```rust
                let cfg = WalkConfig {
                    cutoff_time,
                    to_date: params.to_date,
                    after_bound,
                    // messages.Search filters server-side; no client-side pass.
                    media_filter: None,
                    below_cutoff: BelowCutoff::Stop,
                };
                let mut walk = MessageWalk::new(
                    cfg,
                    params.collapse_albums,
                    params.limit as usize,
                    self.search_deadline_secs,
                );
                let mut channels_scanned = 0u32;
```

The dialog-walk `while let` keeps its `budget.expired()` guard, now `walk.expired()`. The inner message loop becomes:

```rust
                        loop {
                            if walk.expired() {
                                break;
                            }
                            let next = pager
                                .next()
                                .await
                                .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?;
                            let page_size = pager.take_last_page_size();
                            let fetched = next.map(|(raw, entities)| Fetched {
                                raw,
                                entities,
                                peer: Some(peer),
                            });
                            if walk.step(fetched, page_size) == Flow::Stop {
                                break;
                            }
                        }
                        break;
```

and the closure's tail:

```rust
                let (page, budget) = walk.into_parts();
                Ok((page, channels_scanned, budget))
```

- [ ] **Step 2: Run the full suite**

Run: `cargo test`
Expected: PASS, unchanged count.

- [ ] **Step 3: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy -- -D warnings
git add src/telegram/client/ops_search.rs
git commit -m "refactor: drive channel search through MessageWalk (audit S4.2)"
```

---

### Task 6: Wire `search_global`, rename the `page` tracing field

**Files:**
- Modify: `src/telegram/client/ops_search.rs:221-312`

**Interfaces:**
- Consumes: `MessageWalk` and friends.
- Produces: `search_global` keeps its signature — `Result<(PageAccumulator, SearchBudget), Error>`.

**Deliberate delta (2 of 2):** the per-page `debug!` moves from before `step` to after it. The logged values are identical — `record_page` has run in either ordering — but the reorder is real. Record it in the PR description.

This task also lands the stage-3 follow-up renaming the tracing field `page` → `page_no`: it previously collided with the `page` accumulator local, and after this change the two would sit in the same scope.

- [ ] **Step 1: Replace the loop**

In `search_global`, replace the accumulator/budget pair and the loop:

```rust
                let cfg = WalkConfig {
                    cutoff_time,
                    to_date: params.to_date,
                    // Cursors are per-channel; the dispatcher rejects them here.
                    after_bound: None,
                    // SearchGlobal filters server-side.
                    media_filter: None,
                    // Relevance-ordered across channels: one old result says
                    // nothing about the next, so skip rather than stop.
                    below_cutoff: BelowCutoff::Skip,
                };
                let mut walk = MessageWalk::new(
                    cfg,
                    params.collapse_albums,
                    params.limit as usize,
                    self.search_deadline_secs,
                );
                // ... pager construction unchanged ...
                let mut mtproto_nanos: u128 = 0;

                loop {
                    if walk.expired() {
                        break;
                    }
                    let fetch_start = Instant::now();
                    let next = pager
                        .next()
                        .await
                        .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?;
                    mtproto_nanos += fetch_start.elapsed().as_nanos();
                    let page_size = pager.take_last_page_size();
                    // Destructured before `Fetched` so `chat_peer` outlives the
                    // borrow taken by `peer`.
                    let flow = match next {
                        Some((raw, entities, chat_peer)) => walk.step(
                            Some(Fetched {
                                raw,
                                entities,
                                peer: chat_peer.as_ref(),
                            }),
                            page_size,
                        ),
                        None => walk.step(None, page_size),
                    };
                    // Moved after `step` so the counters it reads are current;
                    // the logged values are identical either way.
                    if let Some(size) = page_size {
                        tracing::debug!(
                            page_no = walk.pages_fetched(),
                            messages_in_page = size,
                            messages_scanned = walk.messages_scanned(),
                            kept = walk.kept(),
                            "Global search page fetched"
                        );
                    }
                    if flow == Flow::Stop {
                        break;
                    }
                }
```

**Note for the implementer:** do not introduce a `raw.clone()` to keep the tuple alive for the log — a per-message `tl::enums::Message` clone in the hot global-search loop is a real cost. The `match next` form above moves the value into `Fetched` and reads the log values back off `walk`, so nothing needs cloning. If the borrow checker objects to `chat_peer.as_ref()`, bind `let chat_peer = chat_peer;` in the arm before constructing `Fetched` — do not reach for a clone.

Replace the closure tail:

```rust
                let (page, budget) = walk.into_parts();
                tracing::debug!(
                    pages_fetched = budget.pages_fetched(),
                    messages_scanned = budget.messages_scanned(),
                    mtproto_ms = (mtproto_nanos / 1_000_000) as u64,
                    duration_ms = start_time.elapsed().as_millis() as u64,
                    "Global search finished"
                );
                Ok((page, budget))
```

- [ ] **Step 2: Run the full suite**

Run: `cargo test`
Expected: PASS, unchanged count.

- [ ] **Step 3: Verify no `PageAccumulator`/`SearchBudget` construction remains in the ops loops**

Run: `ast-index usages "PageAccumulator"`
Expected: constructions only in `walk.rs` and `albums.rs` tests — not in `ops_search.rs` or `ops_history.rs`.

- [ ] **Step 4: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy -- -D warnings
git add src/telegram/client/ops_search.rs
git commit -m "refactor: drive global search through MessageWalk; rename tracing page to page_no (audit S4.2)"
```

---

### Task 7: Extract `assemble_search_result`

**Files:**
- Modify: `src/telegram/client.rs` (add the free function beside `cursor_wire_bounds`)
- Modify: `src/telegram/client/ops_history.rs`, `src/telegram/client/ops_search.rs` (call it)
- Modify: `src/telegram/client/tests/helpers_tests.rs` (tests)

**Interfaces:**
- Produces:
  ```rust
  fn assemble_search_result(
      messages: Vec<Message>,
      budget: &SearchBudget,
      has_more: bool,
      query: String,
      window_from: DateTime<Utc>,
      window_to: Option<DateTime<Utc>>,
      channels_scanned: Option<u32>,
      search_time_ms: u64,
  ) -> SearchResult
  ```
  Computes `returned` and `channels_in_results` internally.

**Design note:** sorting stays in `search_messages_impl`. `get_recent_messages_impl` does not sort (its pager already yields reverse-chronological order), and folding the sort in here would silently start sorting history.

- [ ] **Step 1: Write the failing test**

Append to `src/telegram/client/tests/helpers_tests.rs`:

```rust
#[test]
fn assembled_result_counts_distinct_channels_not_messages() {
    use crate::test_helpers::create_test_message;

    let budget = SearchBudget::new(0);
    let messages = vec![
        create_test_message(1, 100),
        create_test_message(2, 100),
        create_test_message(3, 200),
    ];
    let result = assemble_search_result(
        messages,
        &budget,
        false,
        "запрос".to_string(),
        at_secs(1_000),
        None,
        Some(2),
        42,
    );

    assert_eq!(result.returned, 3);
    assert_eq!(result.query_metadata.channels_in_results, 2);
    assert_eq!(result.query_metadata.channels_scanned, Some(2));
    assert_eq!(result.search_time_ms, 42);
}

#[test]
fn an_empty_result_reports_zero_channels_in_results() {
    // History's old hand-rolled `if messages.is_empty() { 0 } else { 1 }`
    // must stay equivalent to the unique-count for the single-channel case.
    let budget = SearchBudget::new(0);
    let result = assemble_search_result(
        Vec::new(),
        &budget,
        false,
        String::new(),
        at_secs(1_000),
        None,
        Some(1),
        7,
    );

    assert_eq!(result.returned, 0);
    assert_eq!(result.query_metadata.channels_in_results, 0);
}

#[test]
fn partial_is_paired_with_timed_out_not_with_has_more() {
    // A full page is not a timeout: `has_more` true must leave `partial` false.
    let budget = SearchBudget::new(0);
    let result = assemble_search_result(
        Vec::new(),
        &budget,
        true,
        String::new(),
        at_secs(1_000),
        None,
        None,
        1,
    );

    assert!(result.has_more);
    assert!(!result.query_metadata.partial);
    assert!(!result.query_metadata.timed_out);
}
```

Add near the top of `helpers_tests.rs`, if not already present:

```rust
fn at_secs(secs: i64) -> chrono::DateTime<chrono::Utc> {
    use chrono::TimeZone;
    chrono::Utc
        .timestamp_opt(secs, 0)
        .single()
        .expect("valid timestamp")
}
```

**Note for the implementer:** check `create_test_message`'s signature in `src/test_helpers.rs:25` before use — the calls above assume `(message_id, channel_id)`. Adjust the call sites to the real signature; do not change the helper.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib telegram::client::helpers_tests`
Expected: FAIL — `assemble_search_result` not found.

- [ ] **Step 3: Write the implementation**

In `src/telegram/client.rs`, after `cursor_wire_bounds`:

```rust
/// Build the `SearchResult` envelope shared by search and history.
///
/// `channels_in_results` is the count of distinct channels among `messages`.
/// For history — which fetches from exactly one peer — that is 0 or 1, which
/// is what the hand-rolled `if messages.is_empty()` used to produce.
///
/// `partial` is deliberately paired with `timed_out`, never with `has_more`:
/// expiry stopped the walk without proving anything lies beyond the page,
/// while a full page proves the opposite.
///
/// Sorting is NOT applied here — search sorts by timestamp descending,
/// history relies on its pager's order.
fn assemble_search_result(
    messages: Vec<crate::telegram::Message>,
    budget: &search_budget::SearchBudget,
    has_more: bool,
    query: String,
    window_from: chrono::DateTime<chrono::Utc>,
    window_to: Option<chrono::DateTime<chrono::Utc>>,
    channels_scanned: Option<u32>,
    search_time_ms: u64,
) -> SearchResult {
    let channels_in_results = messages
        .iter()
        .map(|m| m.channel_id.get())
        .collect::<std::collections::HashSet<_>>()
        .len() as u32;
    SearchResult {
        returned: messages.len() as u64,
        has_more,
        search_time_ms,
        query_metadata: QueryMetadata {
            query,
            window_from,
            window_to,
            channels_scanned,
            channels_in_results,
            timed_out: budget.timed_out(),
            partial: budget.timed_out(),
            pages_fetched: budget.pages_fetched(),
            messages_scanned: budget.messages_scanned(),
        },
        messages,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib telegram::client::helpers_tests`
Expected: PASS.

- [ ] **Step 5: Call it from both ops files**

In `ops_history.rs`, replace the trailing `Ok(SearchResult { ... })` block with:

```rust
        Ok(assemble_search_result(
            messages,
            &budget,
            has_more,
            String::new(), // no query for history retrieval
            cutoff_time,
            params.to_date,
            Some(1),
            search_time_ms,
        ))
```

Delete the now-unused `returned` and `channels_in_results` locals. Keep the `tracing::info!` call, adjusting it to use `messages.len()` before the move (bind `let returned = messages.len() as u64;` above the log if needed).

In `ops_search.rs`, replace the trailing `Ok(SearchResult { ... })` with the same call, passing `params.query.clone()` and `channels_scanned`. The `messages.sort_by_key(...)` line stays where it is, before the call.

- [ ] **Step 6: Run the full suite**

Run: `cargo test`
Expected: PASS, unchanged count.

- [ ] **Step 7: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy -- -D warnings
git add src/telegram/client.rs src/telegram/client/ops_history.rs src/telegram/client/ops_search.rs src/telegram/client/tests/helpers_tests.rs
git commit -m "refactor: share assemble_search_result across search and history (audit S4.3)"
```

---

### Task 8: Extract `dialog_fallback_target`

**Files:**
- Modify: `src/telegram/client/ops_history.rs:60-73`
- Create: `src/telegram/client/tests/ops_history_tests.rs`

**Interfaces:**
- Produces: `fn dialog_fallback_target(channel_id: Option<ChannelId>, identifier: Option<&str>) -> Result<i64, Error>` — module-private free function in `ops_history.rs`.

- [ ] **Step 1: Write the failing test**

Create `src/telegram/client/tests/ops_history_tests.rs`:

```rust
//! Unit tests for the history op's pure decision helpers.

use super::*;

#[test]
fn a_numeric_channel_id_is_the_dialog_walk_target() {
    let id = ChannelId::new(12345).expect("valid channel id");
    assert_eq!(dialog_fallback_target(Some(id), None).expect("target"), 12345);
}

#[test]
fn a_username_that_did_not_resolve_hard_errors_instead_of_walking_dialogs() {
    // AD-2: a username reference carries no numeric id, so there is no id to
    // walk dialogs by. Falling back would search for an id we never had.
    let err = dialog_fallback_target(None, Some("@канал"))
        .expect_err("a username with no id must not fall back");
    assert!(
        matches!(err, Error::InvalidInput(ref m) if m.contains("@канал")),
        "the error must name the unresolved reference, got: {err}"
    );
}

#[test]
fn a_missing_id_and_missing_identifier_still_errors_cleanly() {
    let err = dialog_fallback_target(None, None).expect_err("no target is an error");
    assert!(matches!(err, Error::InvalidInput(_)));
}
```

Register it at the bottom of `ops_history.rs`:

```rust
#[cfg(test)]
#[path = "tests/ops_history_tests.rs"]
mod tests;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib telegram::client::ops_history`
Expected: FAIL — `dialog_fallback_target` not found.

- [ ] **Step 3: Write the implementation**

Add to `ops_history.rs`, outside the `impl` block:

```rust
/// The numeric id to walk dialogs by when username resolution did not produce
/// a peer.
///
/// A username reference carries no numeric id (`channel_id == None`), so a
/// username that fails to resolve hard-errors here rather than walking dialogs
/// by an id we never had (AD-2).
fn dialog_fallback_target(
    channel_id: Option<ChannelId>,
    identifier: Option<&str>,
) -> Result<i64, Error> {
    channel_id.map(|id| id.get()).ok_or_else(|| {
        let reference = identifier.unwrap_or("");
        tracing::warn!(reference, "Channel not found: username did not resolve");
        Error::InvalidInput(format!("Channel not found: {}", reference))
    })
}
```

Rewrite the fallback block in `get_recent_messages_impl`:

```rust
        let peer = match resolved_peer {
            Some(peer) => peer,
            None => {
                let id = dialog_fallback_target(
                    params.channel_id,
                    params.channel_identifier.as_deref(),
                )?;
                self.find_dialog_peer(id).await?.ok_or_else(|| {
                    tracing::warn!(channel_id = id, "Channel not found in dialogs");
                    Error::InvalidInput(format!("Channel not found: {}", id))
                })?
            }
        };
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib telegram::client::ops_history`
Expected: PASS — 3 tests.

- [ ] **Step 5: Run the full suite, format, lint, commit**

```bash
cargo test
cargo fmt --all
cargo clippy -- -D warnings
git add src/telegram/client/ops_history.rs src/telegram/client/tests/ops_history_tests.rs
git commit -m "refactor: extract dialog_fallback_target from get_recent_messages (audit S4.3)"
```

---

### Task 9: Extract `partition_batch`

**Files:**
- Modify: `src/telegram/client/ops_message.rs:90-121`
- Create: `src/telegram/client/tests/ops_message_tests.rs`

**Interfaces:**
- Consumes: `is_empty_variant(&tl::enums::Message) -> bool` from `super::guard`.
- Produces:
  ```rust
  fn partition_batch(
      message_ids: &[i32],
      by_id: &mut std::collections::HashMap<i32, tl::enums::Message>,
      peer: &grammers_client::peer::Peer,
      entities: &EntityLookup,
      channel_ref: &str,
  ) -> crate::telegram::MessageBatch
  ```

- [ ] **Step 1: Write the failing test**

Create `src/telegram/client/tests/ops_message_tests.rs`:

```rust
//! Unit tests for the batch-partition invariant.

use super::*;
use crate::telegram::envelope::EntityLookup;
use crate::test_helpers::{raw_tl_channel, raw_tl_message};
use grammers_client::Client;
use grammers_client::peer::Peer;
use grammers_mtsender::SenderPool;
use grammers_session::storages::MemorySession;
use std::collections::HashMap;
use std::sync::Arc;

fn inert_peer() -> (Client, Peer) {
    let session = Arc::new(MemorySession::default());
    let SenderPool { handle, .. } = SenderPool::new(session, 1);
    let client = Client::new(handle);
    let peer = Peer::from_raw(
        &client,
        tl::enums::Chat::Channel(raw_tl_channel(11, "Канал", None)),
    );
    (client, peer)
}

#[test]
fn every_requested_id_lands_in_exactly_one_bucket() {
    // The batch invariant: no id may be silently dropped.
    let (_client, peer) = inert_peer();
    let entities = EntityLookup::from_envelope(&[], &[]);
    let mut by_id = HashMap::new();
    by_id.insert(1, raw_tl_message(1, 1_000, 11));
    // id 2 is absent entirely (never existed)
    by_id.insert(3, tl::enums::Message::Empty(tl::types::MessageEmpty {
        id: 3,
        peer_id: None,
    }));

    let batch = partition_batch(&[1, 2, 3], &mut by_id, &peer, &entities, "@канал");

    assert_eq!(batch.messages.len(), 1, "only id 1 exists");
    assert_eq!(batch.missing_ids, vec![2, 3]);
    assert_eq!(
        batch.messages.len() + batch.missing_ids.len(),
        3,
        "every requested id must land in exactly one bucket"
    );
}

#[test]
fn a_message_empty_placeholder_is_missing_not_a_fabricated_message() {
    // grammers wraps a deleted id in a MessageEmpty-backed object rather than
    // omitting it; converting it blind would fabricate an epoch-0 message.
    let (_client, peer) = inert_peer();
    let entities = EntityLookup::from_envelope(&[], &[]);
    let mut by_id = HashMap::new();
    by_id.insert(9, tl::enums::Message::Empty(tl::types::MessageEmpty {
        id: 9,
        peer_id: None,
    }));

    let batch = partition_batch(&[9], &mut by_id, &peer, &entities, "@канал");

    assert!(batch.messages.is_empty());
    assert_eq!(batch.missing_ids, vec![9]);
}

#[test]
fn requested_order_is_preserved_in_missing_ids() {
    let (_client, peer) = inert_peer();
    let entities = EntityLookup::from_envelope(&[], &[]);
    let mut by_id = HashMap::new();

    let batch = partition_batch(&[5, 3, 9], &mut by_id, &peer, &entities, "@канал");

    assert_eq!(batch.missing_ids, vec![5, 3, 9]);
}
```

Register it at the bottom of `ops_message.rs`:

```rust
#[cfg(test)]
#[path = "tests/ops_message_tests.rs"]
mod tests;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib telegram::client::ops_message`
Expected: FAIL — `partition_batch` not found.

- [ ] **Step 3: Write the implementation**

Add to `ops_message.rs`, outside the `impl` block:

```rust
/// Split a fetched id-keyed map into found messages and missing ids.
///
/// Single pass so every requested id lands in exactly one bucket — never
/// silently in neither. An absent entry and a `MessageEmpty` both mean the id
/// does not exist in this channel; a present, non-empty message that still
/// fails domain conversion is logged and reported as missing rather than
/// dropped.
fn partition_batch(
    message_ids: &[i32],
    by_id: &mut std::collections::HashMap<i32, tl::enums::Message>,
    peer: &grammers_client::peer::Peer,
    entities: &EntityLookup,
    channel_ref: &str,
) -> crate::telegram::MessageBatch {
    let mut messages = Vec::with_capacity(message_ids.len());
    let mut missing_ids = Vec::with_capacity(message_ids.len());
    for &message_id in message_ids {
        match by_id.remove(&message_id) {
            Some(raw) if !is_empty_variant(&raw) => {
                match convert_raw_message(&raw, peer, entities) {
                    Some(converted) => messages.push(converted),
                    None => {
                        tracing::warn!(
                            channel_ref = %channel_ref,
                            message_id,
                            "Failed to convert message in batch; reporting as missing"
                        );
                        missing_ids.push(i64::from(message_id));
                    }
                }
            }
            _ => missing_ids.push(i64::from(message_id)),
        }
    }
    crate::telegram::MessageBatch {
        messages,
        missing_ids,
    }
}
```

Add `use crate::telegram::envelope::EntityLookup;` to the file's imports if not already reachable via `use super::*;`.

Replace the loop in `get_messages_batch_impl` with:

```rust
        Ok(partition_batch(
            message_ids,
            &mut by_id,
            &peer,
            &entities,
            channel_ref,
        ))
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib telegram::client::ops_message`
Expected: PASS — 3 tests.

- [ ] **Step 5: Run the full suite, format, lint, commit**

```bash
cargo test
cargo fmt --all
cargo clippy -- -D warnings
git add src/telegram/client/ops_message.rs src/telegram/client/tests/ops_message_tests.rs
git commit -m "refactor: extract partition_batch from get_messages_batch (audit S4.3)"
```

---

### Task 10: Extract `ChannelPageBuilder`

**Files:**
- Modify: `src/telegram/client/channels.rs:8-47`
- Modify: `src/telegram/client/tests/channels_tests.rs`

**Interfaces:**
- Produces: `ChannelPageBuilder` with `new(offset: u32, limit: u32)`, `admit(&mut self, channel: Channel)`, `finish(self) -> ChannelPage`.

- [ ] **Step 1: Write the failing test**

Append to `src/telegram/client/tests/channels_tests.rs`:

```rust
#[test]
fn total_counts_every_channel_while_the_page_is_cut_out_in_passing() {
    // B6: the walk continues past the page so `total` is the genuine
    // subscription count, not the page length.
    use crate::test_helpers::create_test_channel_named;

    let mut builder = ChannelPageBuilder::new(1, 2);
    for id in 1..=5 {
        builder.admit(create_test_channel_named(id, &format!("Канал {id}")));
    }
    let page = builder.finish();

    assert_eq!(page.total, 5, "total must count every channel walked");
    assert_eq!(page.channels.len(), 2, "the page honours limit");
    assert_eq!(
        page.channels[0].id.get(),
        2,
        "offset 1 skips the first channel"
    );
}

#[test]
fn an_offset_past_the_end_yields_an_empty_page_with_a_real_total() {
    use crate::test_helpers::create_test_channel_named;

    let mut builder = ChannelPageBuilder::new(10, 5);
    for id in 1..=3 {
        builder.admit(create_test_channel_named(id, "Канал"));
    }
    let page = builder.finish();

    assert!(page.channels.is_empty());
    assert_eq!(page.total, 3);
}
```

**Note for the implementer:** verify `create_test_channel_named`'s signature at `src/test_helpers.rs:210` and adjust the calls; do not change the helper.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib telegram::client::channels`
Expected: FAIL — `ChannelPageBuilder` not found.

- [ ] **Step 3: Write the implementation**

Add to `channels.rs`, outside the `impl` block:

```rust
/// Accumulates a `ChannelPage` while the dialog walk runs to completion.
///
/// The walk covers the WHOLE dialog list: the page is cut out in passing and
/// iteration continues, so `total` is the genuine subscription count (B6).
/// Iteration always started from the beginning anyway — offset pages already
/// paid the full walk.
struct ChannelPageBuilder {
    offset: usize,
    limit: usize,
    page: Vec<crate::telegram::Channel>,
    total: usize,
}

impl ChannelPageBuilder {
    fn new(offset: u32, limit: u32) -> Self {
        Self {
            offset: offset as usize,
            limit: limit as usize,
            page: Vec::new(),
            total: 0,
        }
    }

    fn admit(&mut self, channel: crate::telegram::Channel) {
        if self.total >= self.offset && self.page.len() < self.limit {
            self.page.push(channel);
        }
        self.total += 1;
    }

    fn finish(self) -> crate::telegram::ChannelPage {
        crate::telegram::ChannelPage {
            channels: self.page,
            total: self.total,
        }
    }
}
```

Rewrite `get_subscribed_channels_impl`'s body to use it:

```rust
        let mut builder = ChannelPageBuilder::new(offset, limit);
        let mut dialogs = self.client.iter_dialogs();

        while let Some(dialog) = dialogs.next().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to iterate dialogs in get_subscribed_channels");
            Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
        })? {
            if let Some(mut channel) = convert_peer_to_channel(dialog.peer()) {
                // Free enrichment: the dialog already carries its top message (B8).
                channel.last_message_date =
                    dialog.last_message.as_ref().and_then(message_timestamp);
                builder.admit(channel);
            }
        }

        let page = builder.finish();
        tracing::debug!(
            returned = page.channels.len(),
            total = page.total,
            offset,
            limit,
            "get_subscribed_channels completed"
        );
        Ok(page)
```

**Behavior note:** the original set `last_message_date` only for channels that landed in the page. The rewrite sets it for every converted channel before `admit` decides. The enrichment is a pure field read off data already in hand — no RPC — so this is a wasted assignment on skipped channels, not a behavior change to the response. If you prefer to preserve the exact original work profile, have `admit` take a closure instead; the simpler form above is what this plan intends.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib telegram::client::channels`
Expected: PASS.

- [ ] **Step 5: Run the full suite, format, lint, commit**

```bash
cargo test
cargo fmt --all
cargo clippy -- -D warnings
git add src/telegram/client/channels.rs src/telegram/client/tests/channels_tests.rs
git commit -m "refactor: extract ChannelPageBuilder from get_subscribed_channels (audit S4.3)"
```

---

### Task 11: Extract `classify_search_hit`

**Files:**
- Modify: `src/telegram/client/channels.rs:207-230`
- Modify: `src/telegram/client/tests/channels_tests.rs`

**Interfaces:**
- Consumes: `SubscriptionKey`, `chat_subscription_key`, `subscribed_peer_keys` (already in `channels.rs`).
- Produces: `enum SearchHit { Skip, Subscribed, Discovered }` and `fn classify_search_hit(chat: &tl::enums::Chat, subscribed: &HashSet<SubscriptionKey>) -> SearchHit`.

- [ ] **Step 1: Write the failing test**

Append to `src/telegram/client/tests/channels_tests.rs`:

```rust
#[test]
fn an_empty_chat_result_is_skipped_rather_than_shown_as_unknown() {
    let subscribed = std::collections::HashSet::new();
    let chat = tl::enums::Chat::Empty(tl::types::ChatEmpty { id: 7 });

    assert_eq!(classify_search_hit(&chat, &subscribed), SearchHit::Skip);
}

#[test]
fn a_chat_in_my_results_classifies_as_subscribed() {
    let my_results = vec![tl::enums::Peer::Channel(tl::types::PeerChannel {
        channel_id: 11,
    })];
    let subscribed = subscribed_peer_keys(&my_results);
    let chat = tl::enums::Chat::Channel(crate::test_helpers::raw_tl_channel(11, "Канал", None));

    assert_eq!(
        classify_search_hit(&chat, &subscribed),
        SearchHit::Subscribed
    );
}

#[test]
fn a_numeric_collision_across_namespaces_does_not_mark_a_channel_subscribed() {
    // PeerChat.chat_id and PeerChannel.channel_id are independent namespaces;
    // a bare i64 key would wrongly match here.
    let my_results = vec![tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: 11 })];
    let subscribed = subscribed_peer_keys(&my_results);
    let chat = tl::enums::Chat::Channel(crate::test_helpers::raw_tl_channel(11, "Канал", None));

    assert_eq!(
        classify_search_hit(&chat, &subscribed),
        SearchHit::Discovered,
        "a chat-namespace id must not match a channel-namespace id"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib telegram::client::channels`
Expected: FAIL — `classify_search_hit` / `SearchHit` not found.

- [ ] **Step 3: Write the implementation**

Add to `channels.rs`:

```rust
/// What a `contacts.search` hit turns into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchHit {
    /// `Chat::Empty` carries no usable identity — skip it rather than
    /// surfacing a placeholder "Unknown" channel.
    Skip,
    /// Already in the caller's dialogs: full channel conversion.
    Subscribed,
    /// A public-directory result the caller has not joined.
    Discovered,
}

/// Classify one `contacts.search` result against the caller's own dialogs.
fn classify_search_hit(
    chat: &tl::enums::Chat,
    subscribed: &std::collections::HashSet<SubscriptionKey>,
) -> SearchHit {
    if matches!(chat, tl::enums::Chat::Empty(_)) {
        return SearchHit::Skip;
    }
    if subscribed.contains(&chat_subscription_key(chat)) {
        SearchHit::Subscribed
    } else {
        SearchHit::Discovered
    }
}
```

Rewrite the `filter_map` in `search_public_channels_impl`:

```rust
        let channels = found
            .chats
            .into_iter()
            .filter_map(|chat| {
                let hit = classify_search_hit(&chat, &subscribed_keys);
                if hit == SearchHit::Skip {
                    return None;
                }
                // `Peer::from_raw` already routes each `Chat` variant to the
                // right peer kind (including the broadcast-vs-megagroup
                // distinction inside `Chat::Channel`/`ChannelForbidden`, which
                // the individual constructors panic on if mismatched).
                let peer = Peer::from_raw(&self.client, chat);
                match hit {
                    SearchHit::Subscribed => convert_peer_to_channel(&peer),
                    SearchHit::Discovered => convert_discovered_peer(&peer),
                    SearchHit::Skip => None,
                }
            })
            .take(clamped_limit as usize)
            .collect();
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib telegram::client::channels`
Expected: PASS.

- [ ] **Step 5: Run the full suite, format, lint, commit**

```bash
cargo test
cargo fmt --all
cargo clippy -- -D warnings
git add src/telegram/client/channels.rs src/telegram/client/tests/channels_tests.rs
git commit -m "refactor: extract classify_search_hit from search_public_channels (audit S4.3)"
```

---

### Task 12: Config file-loading error branches

**Files:**
- Modify: `src/config/tests.rs`

**Interfaces:**
- Consumes: `Config::load_from(Option<&Path>) -> anyhow::Result<Config>` (`src/config.rs:351`).

**Scope note:** this task covers the file-read, TOML-parse, and the four sub-config validation branches. `${VAR}` expansion is covered by Step 5, which first observes the behavior rather than assuming it.

- [ ] **Step 1: Write the failing tests**

Append to `src/config/tests.rs`:

```rust
#[test]
fn loading_a_missing_file_names_the_path_in_the_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("nope.toml");

    let err = Config::load_from(Some(&missing)).expect_err("a missing file must error");

    assert!(
        format!("{err:#}").contains(&missing.display().to_string()),
        "the error must name the path it tried, got: {err:#}"
    );
}

#[test]
fn loading_malformed_toml_reports_a_parse_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "this is not = = valid toml").expect("write");

    let err = Config::load_from(Some(&path)).expect_err("malformed TOML must error");

    assert!(
        format!("{err:#}").contains("Failed to parse config.toml"),
        "expected a parse-stage error, got: {err:#}"
    );
}

#[test]
fn an_invalid_timeouts_table_fails_validation_with_its_own_context() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[telegram]\napi_id = 1\n\n[telegram.timeouts]\nresolve_secs = 0\n",
    )
    .expect("write");

    let err = Config::load_from(Some(&path)).expect_err("a zero timeout must be rejected");

    assert!(
        format!("{err:#}").contains("invalid telegram.timeouts configuration"),
        "expected the timeouts validation context, got: {err:#}"
    );
}
```

**Note for the implementer:** these tests do not mutate the environment, so they must NOT take `EnvGuard` — `ENV_LOCK` is non-reentrant and a needless guard risks self-deadlock against other config tests. Confirm `tempfile` is already a dev-dependency; if not, add it to `[dev-dependencies]` in the same commit.

Also confirm that `resolve_secs = 0` is genuinely rejected by `TimeoutConfig::validate()` before relying on it — read `src/config.rs`'s `validate` implementation and pick whichever field has an actual invariant. Do not guess.

- [ ] **Step 2: Run tests to verify they fail (or reveal wrong assumptions)**

Run: `cargo test --lib config::tests`
Expected: the first two PASS immediately if the branches behave as documented; the third FAILs if `resolve_secs = 0` is not actually invalid. Fix the fixture to match the real invariant — this step exists to catch exactly that.

- [ ] **Step 3: Add the remaining three validation-context tests**

Repeat the `an_invalid_timeouts_table_...` shape for `limits`, `search`, and `rate_limiting`, each asserting its own context string (`"invalid limits configuration"`, `"invalid search configuration"`, `"invalid rate_limiting configuration"`). Read each sub-config's `validate()` to pick a genuinely invalid value.

- [ ] **Step 4: Add the unreadable-file test (Unix only)**

```rust
#[cfg(unix)]
#[test]
fn an_unreadable_file_reports_a_read_failure() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[telegram]\napi_id = 1\n").expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");

    let result = Config::load_from(Some(&path));

    // Restore before asserting so the tempdir cleans up even on failure.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    let err = result.expect_err("an unreadable file must error");
    assert!(format!("{err:#}").contains("Failed to read config"));
}
```

**Note:** this test is a no-op when the suite runs as root (root bypasses the mode). That is acceptable; do not add a root check.

- [ ] **Step 5: Pin `${VAR}` expansion behavior**

The behavior of an *unset* variable is not established — `expand_env_vars` may error, or may expand to empty and fail later at the parse stage. Observe first, then pin.

Read `expand_env_vars` in `src/config.rs` (called at `config.rs:364`) and determine which it does. Then write the matching test:

```rust
#[test]
fn an_unset_env_var_in_a_config_value_is_reported_not_silently_accepted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[telegram]\napi_id = \"${STAGE4_DEFINITELY_UNSET}\"\n")
        .expect("write");

    let err = Config::load_from(Some(&path)).expect_err("an unset var must not load cleanly");

    // Assert the stage the failure comes from, per what expand_env_vars
    // actually does — record which in this comment when you write it.
    assert!(format!("{err:#}").contains(/* the real marker */));
}
```

Replace the `/* the real marker */` placeholder with the actual substring before committing — either the expansion error's text or `"Failed to parse config.toml"`. This test does not touch the environment, so it takes no `EnvGuard`; a variable named `STAGE4_DEFINITELY_UNSET` is assumed absent, which is safe because nothing in this repo sets it.

If it turns out an unset variable expands to empty and the config still loads successfully, that is a finding, not a test to force — stop and report it rather than asserting an error that does not happen.

- [ ] **Step 6: Run the full suite, format, lint, commit**

```bash
cargo test
cargo fmt --all
cargo clippy -- -D warnings
git add src/config/tests.rs Cargo.toml
git commit -m "test: cover config file-loading error branches (audit S4.4)"
```

---

### Task 13: Stage-3 follow-ups

**Files:**
- Modify: `src/telegram/albums.rs:83` (add `#[must_use]`)
- Modify: `src/telegram/albums.rs` tests (add the collapse=false case)

**Interfaces:** none changed. The `page_no` rename already landed in Task 6.

- [ ] **Step 1: Write the failing test**

`albums.rs`'s unit tests are the only pin on `PageAccumulator` outside `walk.rs`, and they all run with `collapse = true`. Append to the `tests` module in `src/telegram/albums.rs`:

```rust
#[test]
fn album_siblings_stay_separate_when_collapse_is_off() {
    // With collapse off, `into_messages` must not merge siblings — each
    // raw message is its own post, and `limit` counts raw messages.
    let mut page = PageAccumulator::new(false, 10);
    assert!(page.push(album_member(1, 77)));
    assert!(page.push(album_member(2, 77)));

    let messages = page.into_messages();
    assert_eq!(messages.len(), 2, "collapse=false must not merge siblings");
    assert!(
        messages.iter().all(|m| m.album.is_none()),
        "collapse=false leaves album metadata unset"
    );
}
```

**Note for the implementer:** `album_member` already exists in that test module at `src/telegram/albums.rs:176` — check its signature and match it. If its `album` field assertion does not hold for this codebase's `Message` shape, assert on the ids instead; do not weaken the length assertion.

- [ ] **Step 2: Run test to verify it fails or passes**

Run: `cargo test --lib telegram::albums`
Expected: PASS if the behavior is already correct — this is a characterization test closing a coverage gap, not a bug fix. If it FAILS, stop: that is a real bug and needs its own report before proceeding.

- [ ] **Step 3: Add `#[must_use]`**

In `src/telegram/albums.rs`, annotate `push`:

```rust
    /// Admit `message` into the page. Returns `false` when the page is full —
    /// the caller stops fetching.
    #[must_use = "a false return means the page is full and the caller must stop fetching"]
    pub(crate) fn push(&mut self, message: Message) -> bool {
```

- [ ] **Step 4: Run the full suite**

Run: `cargo test`
Expected: PASS. If `#[must_use]` produces warnings anywhere, those call sites are ignoring a stop signal — fix them rather than suppressing.

Run: `cargo clippy -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add src/telegram/albums.rs
git commit -m "test: pin collapse=false album behavior; mark PageAccumulator::push must_use (audit S4.5)"
```

---

### Task 14: Coverage check and documentation

**Files:**
- Modify: `docs/memory.md`
- Delete: `docs/superpowers/specs/2026-08-15-audit-stage4-design.md`, `docs/superpowers/plans/2026-08-15-audit-stage4-ops-verification.md` (delete-on-merge)

- [ ] **Step 1: Measure**

Run: `cargo llvm-cov --lib --summary-only`
Record the new overall line coverage and the per-file numbers for `walk.rs`, `ops_search.rs`, `ops_history.rs`, `ops_message.rs`, `channels.rs`, `config.rs`.

The number is not a gate — the goal was verified behavior. Report it in the PR description alongside the baseline (75.1% overall; ops layer 0%).

- [ ] **Step 2: Update `docs/memory.md`**

Three edits, all in "Current state" / "Open items":

- Correct the stale v0.22.2 open item — `Cargo.toml` is at 0.22.3 and the release commit is on `chore/audit-stage3-cleanup`.
- Mark audit stage 4 done, with the new coverage figure.
- Add a "Key decisions" entry: the ops loops' decision logic lives in `MessageWalk::step` (synchronous, above the DI seam) because the loops themselves sit below it; the three loops differ only in `WalkConfig`'s five fields.

Do not add anything the code or git history already records.

- [ ] **Step 3: Delete the plan and spec (delete-on-merge)**

```bash
git rm docs/superpowers/specs/2026-08-15-audit-stage4-design.md
git rm docs/superpowers/plans/2026-08-15-audit-stage4-ops-verification.md
```

Also update `docs/superpowers/specs/2026-08-15-project-audit.md`: mark Stage 4 done, matching how stages 1–3 are recorded.

- [ ] **Step 4: Final gate and commit**

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add docs/
git commit -m "docs: record audit stage 4 completion; archive plan and spec"
```

- [ ] **Step 5: Open the PR**

```bash
git push -u origin refactor/audit-stage4-ops-verification
gh pr create --base master --title "refactor: verify the ops layer via MessageWalk (audit stage 4)" --body "..."
```

The PR body must list the two deliberate behavior deltas (Task 4's inert `expired()` check on history; Task 6's per-page log reorder in global search), the coverage before/after, and the note that the walk's five-field config is the complete difference between the three loops.

---

## Self-Review

**Spec coverage:**

| Spec section | Task |
|---|---|
| §1 `MessageWalk` | 1, 2, 3 |
| §1 deliberate deltas | 4 (expired), 6 (log reorder) |
| §2 `assemble_search_result` | 7 |
| §2 `dialog_fallback_target` | 8 |
| §2 `partition_batch` | 9 |
| §2 `ChannelPageBuilder` | 10 |
| §2 `classify_search_hit` | 11 |
| §3 walk tests | 1, 2, 3 |
| §3 config error branches | 12 |
| §4 stage-3 follow-ups | 6 (`page_no`), 13 (`#[must_use]`, collapse=false) |
| §5 hygiene | out of scope — separate PR, per the spec |

No gaps.

**Known soft spots**, flagged rather than hidden:

- Tasks 3, 7, 10, 12, 13 contain explicit "verify this signature / invariant before relying on it" steps. These are real fixture signatures I did not read in full (`create_test_message`, `create_test_channel_named`, `album_member`, `tl::types::Message`'s field list, each sub-config's `validate()`). Rather than invent field names that would compile-fail, the plan points at the exact file and line to copy from.
- Task 12 Step 5 is an observe-then-pin step for `${VAR}` expansion, because I did not establish whether an unset variable errors or expands to empty.
- Task 10 changes when `last_message_date` is assigned (every converted channel, not only paged ones). It is a pure field read with no RPC, so the response is unchanged — but it is a work-profile difference and is called out in the task.
