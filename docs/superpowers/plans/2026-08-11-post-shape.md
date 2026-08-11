# v0.15 "Post Shape" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship every breaking response-shape change from the v0.13.0 work order in one release — album collapsing with a post-level `limit` (B5+A2), honest metadata (B6–B10), `link` + reactions on every message (D1, D2), `get_message_media` field renames (D3), and a truthful nullable `has_more` on discovery (D10).

**Architecture:** All changes ride existing seams (approved spec: `docs/superpowers/specs/2026-08-11-work-order-roadmap-design.md`). Domain shapes change in `src/telegram/types/entities.rs` / `params.rs`; conversions in `src/telegram/converters/*`; fetch loops in `src/telegram/client/ops_*.rs`; wire mirrors in `src/mcp/tools/types/responses.rs`. One new module: `src/telegram/albums.rs` (post counting + album collapsing, pure functions). No new tools; the count stays 12.

**Tech Stack:** Rust nightly (edition 2024, let-chains), rmcp 3.1, grammers 0.10 (Codeberg, pinned rev `9fef0bae` — checkout at `~/.cargo/git/checkouts/grammers-8937e3b5288aa015/9fef0ba/`; ignore the stale `grammers-2861ac880138ee45` checkout), schemars v1, mockall 0.14, chrono domain time (jiff stays behind the grammers boundary).

## Global Constraints

- Branch: `feat/post-shape` off `master`. Ships later as **v0.15.0** via the `release` skill — the release itself is NOT part of this plan.
- Pre-merge gate after every task, all must pass: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
- Run `cargo fmt --all` after every code change (before the gate).
- Config tests are serial (`cargo test config -- --test-threads=1`) — plain `cargo test` in the gate is fine.
- **Never `unwrap()`** in production code; `expect()` only in tests/impossible cases.
- Never log phone numbers, API hashes, passwords, session tokens.
- TDD: failing test first, always.
- Conventional commits; breaking shape changes use `feat!:`. Every commit ends with:
  `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`
- **Break cleanly** (approved): renamed/removed fields get no compatibility aliases. The single known consumer (`news-digest`) resyncs once after this release.
- Do NOT regress work-order §1.3: parameter clamping, date-window accuracy, window validation messages, media guards, identifier flexibility, the normalized `invalid input: Channel not found: @…` error, and the v0.14 guarantees (no epoch-0 timestamps, no `$ref` without `$defs`, public link forms).
- grammers stays pinned — no dependency changes.
- Verify generated TL names before relying on them (grep the pinned checkout, never guess):
  `G=~/.cargo/git/checkouts/grammers-8937e3b5288aa015/9fef0ba`

---

### Task 1: B9 — `username: Option<Username>` + `chat_type` on channel objects

Deletes the `"unknown"`/`"group"` username sentinels (both are syntactically valid Telegram usernames and collide with real channels — `@premium` exists). `Channel.username` and `Message.channel_username` become `Option<Username>`, serialized as `null` when absent (matching `description`/`member_count` convention). New field `chat_type` on `Channel`.

**Files:**
- Modify: `src/telegram/types/entities.rs` (Channel :89, Message :12, tests :118)
- Modify: `src/telegram/converters/channel.rs` (delete `fallback_username` :12, `peer_identity` :21, `convert_peer_with_subscription` :76, fixture `channel_peer` :145, tests)
- Modify: `src/telegram/converters/message.rs` (`convert_message` :85 — flows through unchanged, compiler confirms)
- Modify: `src/mcp/tools/types/responses.rs` (`MessageResponse.channel_username` :210)
- Modify: `src/test_helpers.rs` (`create_test_message` :29, `create_test_channel` :138, `create_test_channel_detailed` :153)
- Test: converter tests in `src/telegram/converters/channel.rs`; compiler-flagged assertion sites across `src/mcp/tests/*.rs` and `src/telegram/types/entities.rs`

**Interfaces:**
- Produces: `pub enum ChatType { Channel, Group, Supergroup }` (serde `lowercase`); `Channel { username: Option<Username>, chat_type: ChatType, .. }`; `Message { channel_username: Option<Username>, .. }`; `pub(crate) fn peer_chat_type(peer: &Peer) -> Option<ChatType>`; `peer_identity` now returns `Option<(ChannelId, ChannelName, Option<Username>)>`.
- Consumes: `grammers_client::peer::Peer`, `Group::is_megagroup()` (same accessor `supports_full_channel_rpc` uses in `src/telegram/client/channels.rs:228`).

- [ ] **Step 1: Write the failing converter tests**

In `src/telegram/converters/channel.rs` tests, first widen the `channel_peer` fixture with a `broadcast: bool` parameter and route construction through `Peer::from_raw` (grammers sends non-broadcast channels to `Peer::Group`; the direct `Channel::from_raw` would panic on them):

```rust
fn channel_peer(id: i64, title: &str, username: Option<&str>, broadcast: bool) -> Peer {
    // ...existing body, with these two raw fields wired to the parameter:
    //   broadcast,
    //   megagroup: !broadcast,
    // and the final construction replaced by:
    Peer::from_raw(&client, tl::enums::Chat::Channel(raw))
}
```

Update the two existing `channel_peer(...)` call sites (`channel_identity_public_channel_carries_username`, and any other) to pass `true`. Then add:

```rust
#[test]
fn peer_without_username_yields_none_not_sentinel() {
    let peer = community_peer(521440428, "Семейный чатик");
    let (_, _, username) = peer_identity(&peer).expect("community yields an identity");
    assert_eq!(username, None);
}

#[test]
fn peer_chat_type_maps_all_kinds() {
    assert_eq!(
        peer_chat_type(&channel_peer(1, "News", Some("newschan"), true)),
        Some(ChatType::Channel)
    );
    assert_eq!(
        peer_chat_type(&channel_peer(2, "Chatty", None, false)),
        Some(ChatType::Supergroup)
    );
    assert_eq!(
        peer_chat_type(&community_peer(3, "Comm")),
        Some(ChatType::Group)
    );
}

#[test]
fn channel_json_has_null_username_and_chat_type() {
    let channel = convert_peer_to_channel(&community_peer(4, "Private Group"))
        .expect("community converts");
    let json = serde_json::to_value(&channel).expect("serializes");
    assert!(json["username"].is_null(), "sentinel must be gone");
    assert_eq!(json["chat_type"], "group");
}
```

Rewrite the now-obsolete sentinel test `peer_identity_maps_community_with_group_fallback_username` (:235) as `peer_identity_maps_community_without_username` asserting `username == None`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test peer_without_username`
Expected: FAIL — `peer_identity` still returns a bare `Username`; `ChatType`/`peer_chat_type` don't exist.

- [ ] **Step 3: Change the domain types**

`src/telegram/types/entities.rs`:

```rust
/// Kind of chat a `Channel` object describes (work-order B9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChatType {
    /// Broadcast channel.
    Channel,
    /// Small (basic) group, incl. grammers `Community` peers.
    Group,
    /// Megagroup.
    Supergroup,
}
```

On `Channel`: `pub username: Option<Username>,` and add `pub chat_type: ChatType,` right after it. On `Message`: `pub channel_username: Option<Username>,`. Export `ChatType` wherever `Channel` is re-exported (`src/telegram/types.rs`, and check `src/telegram.rs` / `src/lib.rs` re-export chains with `grep -rn "pub use" src/telegram/types.rs src/telegram.rs src/lib.rs | grep -i channel`).

- [ ] **Step 4: Rework the converters**

`src/telegram/converters/channel.rs`: delete `fallback_username` (:12) entirely. `peer_identity` third tuple element becomes `Option<Username>`:

```rust
        Peer::Channel(ch) => (
            ChannelId::new(ch.id().bare_id()?).ok()?,
            ChannelName::new(ch.title()).ok()?,
            ch.username().and_then(|u| Username::new(u).ok()),
        ),
        Peer::Group(g) => (
            ChannelId::new(g.id().bare_id()?).ok()?,
            ChannelName::new(g.title().unwrap_or("Unknown")).ok()?,
            g.username().and_then(|u| Username::new(u).ok()),
        ),
        Peer::Community(c) => (
            ChannelId::new(c.id().bare_id()?).ok()?,
            ChannelName::new(c.title()).ok()?,
            None, // Community exposes no username accessor in grammers 0.10
        ),
        Peer::User(u) => (
            ChannelId::new(u.id().bare_id()?).ok()?,
            ChannelName::new(u.first_name().unwrap_or("User")).ok()?,
            u.username().and_then(|un| Username::new(un).ok()),
        ),
```

Add next to it:

```rust
/// The `chat_type` a peer maps to; `None` for user peers, which are not
/// channel objects. grammers routes megagroups to `Peer::Group`, so
/// `Peer::Channel` is always a broadcast (same routing fact
/// `supports_full_channel_rpc` relies on).
pub(crate) fn peer_chat_type(peer: &grammers_client::peer::Peer) -> Option<ChatType> {
    use grammers_client::peer::Peer;

    match peer {
        Peer::Channel(_) => Some(ChatType::Channel),
        Peer::Group(g) => Some(if g.is_megagroup() {
            ChatType::Supergroup
        } else {
            ChatType::Group
        }),
        Peer::Community(_) => Some(ChatType::Group),
        Peer::User(_) => None,
    }
}
```

In `convert_peer_with_subscription`, add `chat_type: peer_chat_type(peer)?,` to the `Channel` literal (the `?` is unreachable for the kinds that get this far — the `Peer::User` arm already returned `None` above). Import `ChatType` in the `use crate::telegram::types::{...}` list.

- [ ] **Step 5: Chase the compiler through the fixture and assertion sites**

Run `cargo build && cargo test 2>&1 | head -50` and fix flagged sites; the known set:

- `src/test_helpers.rs`: `create_test_message` → `channel_username: Some(Username::new("testchannel").expect("Valid username")),`; `create_test_channel` / `create_test_channel_detailed` → `username: Some(Username::new(username).expect("Valid username")), chat_type: ChatType::Channel,` (import `ChatType`).
- `src/telegram/types/entities.rs` test fixture `create_test_message` (:118) → same `Some(...)` change.
- `src/mcp/tools/types/responses.rs`: `MessageResponse.channel_username: Option<Username>` (keep the always-serialize convention — no `skip_serializing_if` — so the wire shows an explicit `null`).
- Assertion sites: `grep -rn "username.as_str()\|channel_username" src/mcp/tests/ src/telegram/ src/test_helpers.rs` — wrap expectations in `Some(...)` / `.as_ref().map(...)` as the compiler dictates.
- `grep -rn "fallback_username" src/` must come back empty.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test converters && cargo test`
Expected: PASS, including `channel_json_has_null_username_and_chat_type`.

- [ ] **Step 7: Gate, docs, commit**

Update README's channel-object examples (`grep -n "username" README.md`): show `"username": null` + `"chat_type": "group"` for a private group, `"chat_type": "channel"` for `@swodki`. CHANGELOG `[Unreleased] → Changed (breaking)`: username sentinels removed, `chat_type` added.

```bash
cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "feat!: nullable channel username and explicit chat_type (B9)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: B6/B7 — honest `QueryMetadata` and `returned`

`total_found` becomes `returned` (it was always the page size). `QueryMetadata` reports the executed window instead of echoing an overridden `hours_back`, and splits "scanned" from "in results".

**Files:**
- Modify: `src/telegram/types/params.rs` (`SearchResult` :137, `QueryMetadata` :146, serialization test :274)
- Modify: `src/telegram/client/ops_history.rs` (:120-141), `src/telegram/client/ops_search.rs` (:30, :121-151)
- Modify: `src/mcp/tools/types/responses.rs` (`SearchResponse` :259-275)
- Modify: `src/mcp/server/impl_search.rs` (logging :77-90, :171-182)
- Modify: `src/test_helpers.rs` (`create_test_search_result` :174)
- Test: `src/mcp/tests/search.rs`, `src/mcp/tests/history.rs` (grep-driven `total_found` updates), new window test in `search.rs`

**Interfaces:**
- Produces: `SearchResult { messages, returned: u64, search_time_ms, query_metadata }`; `QueryMetadata { query: String, window_from: DateTime<Utc>, window_to: Option<DateTime<Utc>>, channels_scanned: Option<u32>, channels_in_results: u32 }` (`window_to` skipped when `None`; `channels_scanned` always serialized, `null` = unknown).
- Consumes: `params.window_start()`, `params.to_date` (both params structs, from v0.14.0).

- [ ] **Step 1: Write the failing MCP-layer test**

In `src/mcp/tests/search.rs`, mirroring the neighboring mock style:

```rust
#[tokio::test]
async fn search_response_reports_window_and_returned() {
    // Mock search_messages returning create_test_search_result(vec![msg], "q", 1)
    // with one create_test_message; permissive mock limiter as neighbors do.
    // Call the search_messages tool with query "q", then parse the JSON string:
    let json: serde_json::Value = serde_json::from_str(&result_string).expect("valid JSON");
    assert_eq!(json["returned"], 1);
    assert!(json.get("total_found").is_none(), "total_found must be renamed");
    let meta = &json["query_metadata"];
    assert!(meta.get("hours_back").is_none(), "hours_back echo removed (B7)");
    assert!(meta["window_from"].is_string(), "executed window start present");
    assert_eq!(meta["channels_in_results"], 1);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test search_response_reports_window`
Expected: FAIL — fields don't exist yet.

- [ ] **Step 3: Change the domain types**

`src/telegram/types/params.rs`:

```rust
/// Search result aggregate.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchResult {
    pub messages: Vec<Message>,
    /// Number of messages in this response (page size, not a match count — B6).
    pub returned: u64,
    pub search_time_ms: u64,
    pub query_metadata: QueryMetadata,
}

/// The window and scope a query actually executed with (work-order B6/B7).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryMetadata {
    pub query: String,
    /// Effective window start actually applied (from_date, or now - hours_back).
    pub window_from: DateTime<Utc>,
    /// Effective upper bound; omitted when the window is open-ended.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub window_to: Option<DateTime<Utc>>,
    /// Channels the search actually scanned; `null` when unknowable
    /// (server-side global search).
    pub channels_scanned: Option<u32>,
    /// Distinct channels present in `messages`.
    pub channels_in_results: u32,
}
```

Update the `search_result_serialization` test (:274) to the new fields (`returned: 42`, a fixed `window_from`, `window_to: None`, `channels_scanned: Some(5)`, `channels_in_results: 5`).

- [ ] **Step 4: Rework both ops files**

`src/telegram/client/ops_history.rs` (:120-141): rename local `total_found` → `returned`; metadata becomes:

```rust
        Ok(SearchResult {
            returned,
            search_time_ms,
            query_metadata: QueryMetadata {
                query: String::new(), // No query for history retrieval
                window_from: cutoff_time,
                window_to: params.to_date,
                channels_scanned: Some(1),
                channels_in_results: if messages.is_empty() { 0 } else { 1 },
            },
            messages,
        })
```

(Note the field order: `messages` moves last so the `returned`/`channels_in_results` reads borrow before the move — or compute them into locals first, as the current code does with `total_found`.)

`src/telegram/client/ops_search.rs`: the single-channel branch's `channels_searched` counter becomes the `Some(...)` scanned value; the global branch's unique-channel count moves out so both branches share it:

```rust
        let (mut messages, channels_scanned) = if let Some(channel_id) = &params.channel_id {
            // ...existing branch body unchanged, still returning (messages, channels_searched)
            // — wrap at the end: (messages, Some(channels_searched))
        } else {
            // ...existing global branch, returning just `collected`
            (collected, None) // server-side global search: scan scope unknowable
        };

        // Sort by timestamp (newest first)
        messages.sort_by_key(|b| std::cmp::Reverse(b.timestamp));

        let channels_in_results = {
            let unique: std::collections::HashSet<_> =
                messages.iter().map(|m| m.channel_id.get()).collect();
            unique.len() as u32
        };
        let search_time_ms = start_time.elapsed().as_millis() as u64;
        let returned = messages.len() as u64;
        // tracing::info! — rename total_found/channels fields accordingly
        Ok(SearchResult {
            returned,
            search_time_ms,
            query_metadata: QueryMetadata {
                query: params.query.clone(),
                window_from: cutoff_time,
                window_to: params.to_date,
                channels_scanned,
                channels_in_results,
            },
            messages,
        })
```

(Delete the old `unique_channels` block at :121-124 — it's replaced by the shared computation.)

- [ ] **Step 5: Chase the rename through wire + helpers + logging**

- `src/mcp/tools/types/responses.rs`: `SearchResponse.total_found` → `returned`; `From` impl follows.
- `src/test_helpers.rs`:

```rust
pub fn create_test_search_result(
    messages: Vec<Message>,
    query: &str,
    channels_in_results: u32,
) -> SearchResult {
    SearchResult {
        returned: messages.len() as u64,
        search_time_ms: 100,
        query_metadata: QueryMetadata {
            query: query.to_string(),
            window_from: Utc::now() - chrono::Duration::hours(48),
            window_to: None,
            channels_scanned: Some(channels_in_results),
            channels_in_results,
        },
        messages,
    }
}
```

- `src/mcp/server/impl_search.rs` logging: `total_found = result.total_found` → `returned = result.returned`; `channels_searched = result.query_metadata.channels_searched` → `channels_in_results = result.query_metadata.channels_in_results` (both tools).
- Sweep the rest: `grep -rn "total_found\|channels_searched" src/` and fix every remaining site (tests assert the old names in `src/mcp/tests/search.rs`, `history.rs`, and `src/mcp/tools/types/tests/responses_tests.rs`).

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test search && cargo test history && cargo test`
Expected: PASS; `grep -rn "total_found" src/` returns nothing.

- [ ] **Step 7: Gate, docs, commit**

README: update the `search_messages` / `get_recent_messages` response examples (renamed fields, `window_from`/`window_to`). CHANGELOG Changed (breaking): `total_found`→`returned`, `QueryMetadata` reworked.

```bash
cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "feat!: report executed window and honest scope in query metadata (B6, B7)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: B6a/D10 — `ChannelPage` with a genuine `total`, nullable `has_more`

`get_subscribed_channels` walks the whole dialog list (it already iterates from the start for every offset) and reports the real subscription total; `ChannelsResponse` gains `returned`, `total` becomes `Option` (discovery can't know it), `has_more` becomes `Option<bool>` (`null` = unknown, work-order D10).

**Files:**
- Modify: `src/telegram/types/entities.rs` (new `ChannelPage` after `Channel`)
- Modify: `src/telegram/trait_def.rs` (`get_subscribed_channels` signature)
- Modify: `src/telegram/client/channels.rs` (`get_subscribed_channels_impl` :8-43)
- Modify: `src/telegram/client.rs` (trait forwarding)
- Modify: `src/mcp/tools/types/responses.rs` (`ChannelsResponse` :67-77)
- Modify: `src/mcp/server/impl_channels.rs` (:9-37), `src/mcp/server/impl_discovery.rs`
- Test: `src/mcp/tests/channels.rs`, `src/mcp/tests/discovery.rs`

**Interfaces:**
- Produces: `pub struct ChannelPage { pub channels: Vec<Channel>, pub total: usize }` (in `entities.rs`, re-exported like `Channel`); trait method `async fn get_subscribed_channels(&self, limit: u32, offset: u32) -> Result<ChannelPage, Error>`; `ChannelsResponse { channels, returned: usize, total: Option<usize>, has_more: Option<bool> }`.
- Consumes: `convert_peer_to_channel` (unchanged).

- [ ] **Step 1: Write the failing MCP-layer tests**

`src/mcp/tests/channels.rs`:

```rust
#[tokio::test]
async fn subscribed_channels_report_true_total_and_has_more() {
    // Mock get_subscribed_channels with .withf(|l, o| *l == 3 && *o == 0)
    // returning ChannelPage { channels: vec![3 × create_test_channel(..)], total: 186 }.
    // Call the tool with limit 3; parse JSON:
    assert_eq!(json["returned"], 3);
    assert_eq!(json["total"], 186);
    assert_eq!(json["has_more"], true);
}
```

`src/mcp/tests/discovery.rs`:

```rust
#[tokio::test]
async fn discovery_has_more_is_unknown_at_limit() {
    // Mock search_public_channels("rust", 1) returning vec![one channel];
    // call the tool with limit 1; parse JSON:
    assert_eq!(json["returned"], 1);
    assert!(json["total"].is_null(), "contacts.Search has no global match count");
    assert!(json["has_more"].is_null(), "full page ⇒ unknown, not false (D10)");
}

#[tokio::test]
async fn discovery_has_more_false_under_limit() {
    // limit 10, mock returns 1 channel → has_more == false, not null.
    assert_eq!(json["has_more"], false);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test subscribed_channels_report_true_total`
Expected: FAIL — `ChannelPage` doesn't exist; mock signature mismatch.

- [ ] **Step 3: Implement the domain type, trait, and client walk**

`entities.rs`:

```rust
/// One page of the subscribed-channel list plus the genuine full count.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChannelPage {
    pub channels: Vec<Channel>,
    /// Total subscribed channels/groups across the entire dialog list —
    /// a real total, not the page size (work-order B6).
    pub total: usize,
}
```

`trait_def.rs`: change the return type to `Result<ChannelPage, Error>` (update the doc comment: "returns one page plus the full subscription count"). `client.rs` forwarding follows.

`src/telegram/client/channels.rs`:

```rust
    pub(super) async fn get_subscribed_channels_impl(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<crate::telegram::ChannelPage, Error> {
        let mut page = Vec::new();
        let mut total = 0usize;
        let mut dialogs = self.client.iter_dialogs();

        // Walk the WHOLE dialog list: the page is cut out in passing, and the
        // walk continues so `total` is the genuine subscription count (B6).
        // Iteration always started from the beginning anyway — offset pages
        // already paid the full walk.
        while let Some(dialog) = dialogs.next().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to iterate dialogs in get_subscribed_channels");
            Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
        })? {
            if let Some(channel) = convert_peer_to_channel(dialog.peer()) {
                if total >= offset as usize && page.len() < limit as usize {
                    page.push(channel);
                }
                total += 1;
            }
        }

        tracing::debug!(returned = page.len(), total, offset, limit, "get_subscribed_channels completed");
        Ok(crate::telegram::ChannelPage { channels: page, total })
    }
```

- [ ] **Step 4: Rework the wire struct and both server impls**

`responses.rs`:

```rust
/// Response for channel-returning tools (subscriptions and discovery).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChannelsResponse {
    #[schemars(description = "Channels in this page")]
    pub channels: Vec<Channel>,

    #[schemars(description = "Number of channels in this response")]
    pub returned: usize,

    #[schemars(description = "Genuine total matches; null when the source cannot know it")]
    pub total: Option<usize>,

    #[schemars(description = "Whether more results exist; null when unknown (D10)")]
    pub has_more: Option<bool>,
}
```

`impl_channels.rs` (`get_subscribed_channels_impl`) — the CQ-5 over-fetch is obsolete, `has_more` derives from the true total:

```rust
        let limit = request.limit.unwrap_or(20);
        let offset = request.offset.unwrap_or(0);

        let page = self
            .telegram_client
            .get_subscribed_channels(limit, offset)
            .await
            .map_err(|e| e.to_string())?;

        let returned = page.channels.len();
        let response = ChannelsResponse {
            channels: page.channels,
            returned,
            total: Some(page.total),
            has_more: Some(offset as usize + returned < page.total),
        };

        json_response(&response)
```

`impl_discovery.rs` (after the existing `search_public_channels` call):

```rust
        let returned = channels.len();
        let response = ChannelsResponse {
            channels,
            returned,
            total: None, // contacts.Search reports no global match count
            // A full page says nothing about what lies beyond it (D10).
            has_more: if returned as u32 == limit { None } else { Some(false) },
        };
        json_response(&response)
```

- [ ] **Step 5: Chase remaining sites and verify**

`grep -rn "has_more\|ChannelsResponse" src/mcp/tests/ src/mcp/tools/` — update old assertions (`total == 3`-style page-size checks, `has_more == false` booleans). Run: `cargo test channels && cargo test discovery && cargo test` → PASS.

- [ ] **Step 6: Gate, docs, commit**

README: `get_subscribed_channels` + `search_public_channels` response examples (returned/total/has_more semantics). CHANGELOG Changed (breaking).

```bash
cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "feat!: genuine channel totals and truthful nullable has_more (B6a, D10)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: B8 — populate `last_message_date`

From dialog data (free) on the subscribed list; via a one-message history peek under `include_full` on `get_channel_info` (opt-in RPC budget, degrade-not-fail like the existing GetFullChannel enrichment). Everywhere else it stays honestly `null`.

**Files:**
- Modify: `src/telegram/client/channels.rs` (`get_subscribed_channels_impl` walk from Task 3; `get_full_channel_info_impl` :65-101; new private helper)
- Test: `src/mcp/tests/channels.rs`

**Interfaces:**
- Produces: populated `Channel.last_message_date` on the subscribed list and under `include_full`.
- Consumes: `Dialog.last_message: Option<grammers Message>` (public field, verified at pinned rev `grammers-client/src/peer/dialog.rs:30`); `message_timestamp` from `src/telegram/converters/message.rs` (in scope via the client's `super::*` glob — confirm with `cargo build`, add the import if not).

- [ ] **Step 1: Write the failing MCP-layer test**

`src/mcp/tests/channels.rs` (trait-boundary wiring test; the client-side walk itself is network-bound and is covered by the live smoke in the final task):

```rust
#[tokio::test]
async fn include_full_passes_last_message_date_through() {
    // Mock get_full_channel_info("@news") returning create_test_channel(..) with
    // last_message_date: Some("2026-08-10T05:55:12Z".parse().unwrap());
    // call get_channel_info with include_full = Some(true); parse JSON:
    assert_eq!(json["last_message_date"], "2026-08-10T05:55:12Z");
}
```

Run: `cargo test include_full_passes_last_message_date` — this passes trivially if `create_test_channel` already sets a date; make it meaningful by asserting the exact mocked value (fixture default is `Utc::now()`, so the fixed value proves pass-through). Expected first run: FAIL only if wiring drops the field — if it passes immediately, keep it as the regression lock and proceed.

- [ ] **Step 2: Fill the field on the subscribed walk**

In the Task-3 walk in `channels.rs`, replace the `if let Some(channel)` body:

```rust
            if let Some(mut channel) = convert_peer_to_channel(dialog.peer()) {
                if total >= offset as usize && page.len() < limit as usize {
                    // Free enrichment: the dialog already carries its top message (B8).
                    channel.last_message_date =
                        dialog.last_message.as_ref().and_then(message_timestamp);
                    page.push(channel);
                }
                total += 1;
            }
```

- [ ] **Step 3: Add the include_full peek**

In `get_full_channel_info_impl`, after the `fetch_channel_full` match block:

```rust
        // include_full already means "extra RPC accepted": peek the newest
        // message for last_message_date. Degrade, never fail (same policy as
        // the GetFullChannel enrichment above).
        match self.fetch_last_message_date(&peer).await {
            Ok(date) => channel.last_message_date = date,
            Err(e) => {
                tracing::warn!(error = %e, channel_id = channel.id.get(),
                    "last-message peek failed; leaving last_message_date null");
            }
        }
```

New private helper in the same `impl TelegramClient` block:

```rust
    /// Newest message's timestamp, via a single-message history peek.
    async fn fetch_last_message_date(
        &self,
        peer: &grammers_client::peer::Peer,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, Error> {
        let peer_ref = peer_to_ref(peer).await?;
        with_timeout("last_message_peek", self.timeouts.history_secs, async {
            let mut iter = self.client.iter_messages(peer_ref);
            match iter.next().await {
                Ok(Some(msg)) => Ok(message_timestamp(&msg)),
                Ok(None) => Ok(None),
                Err(e) => Err(Error::TelegramApi(format!("last-message peek failed: {e}"))),
            }
        })
        .await
    }
```

- [ ] **Step 4: Run tests, gate, docs, commit**

Run: `cargo test channels && cargo test` → PASS. README: note `last_message_date` population rules on both tools. CHANGELOG Added.

```bash
cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "feat: populate last_message_date from dialogs and include_full peek (B8)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: D1/D2 + `grouped_id` — permalink, reactions, and album id on every message

All three are zero-RPC enrichments at the single conversion point (`convert_message`).

**Files:**
- Modify: `src/telegram/types/entities.rs` (`Message` + new `MessageReaction`)
- Modify: `src/telegram/converters/message.rs` (two new helpers + `convert_message`)
- Modify: `src/mcp/tools/types/responses.rs` (`MessageResponse` + `From`)
- Modify: `src/test_helpers.rs` (`create_test_message`)
- Test: converter tests (new, in `src/telegram/converters/message.rs` — create the `#[cfg(test)]` module; reuse the peer fixtures via `super::channel::tests` is not possible across modules, so import the fixture pattern as shown below)

**Interfaces:**
- Produces: `Message { grouped_id: Option<i64>, link: String, reactions: Option<Vec<MessageReaction>>, reactions_total: Option<u64>, .. }`; `pub struct MessageReaction { pub emoji: String, pub count: u64 }`; `pub(crate) fn build_message_link(peer: &Peer, message_id: MessageId) -> Option<String>`; `pub(crate) fn extract_reactions(reactions: Option<&tl::enums::MessageReactions>) -> (Option<Vec<MessageReaction>>, Option<u64>)`.
- Consumes: `channel_identity` (`src/telegram/converters/channel.rs:119`), `MessageLink::new` (`src/link.rs:91`), `msg.grouped_id()` (verified at pinned rev `grammers-client/src/message/message.rs:557`), `msg.raw` (public field).

- [ ] **Step 1: Verify the TL reaction shape (generated code — do not guess)**

```bash
G=~/.cargo/git/checkouts/grammers-8937e3b5288aa015/9fef0ba
grep -n "pub enum MessageReactions" -A6 "$G/grammers-tl-types/src/generated.rs" | head -10
grep -n "pub struct MessageReactions " -A12 "$G/grammers-tl-types/src/generated.rs" | head -16
grep -n "pub enum ReactionCount" -A6 "$G/grammers-tl-types/src/generated.rs" | head -10
grep -n "pub struct ReactionCount " -A8 "$G/grammers-tl-types/src/generated.rs" | head -12
grep -n "pub enum Reaction " -A8 "$G/grammers-tl-types/src/generated.rs" | head -12
```

Record the exact variant names and the full field list of `tl::types::MessageReactions` / `tl::types::ReactionCount` — the code and test below assume `MessageReactions::Reactions`, `ReactionCount::Count { reaction, count, .. }`, `Reaction::Emoji(ReactionEmoji { emoticon })`; adapt to what the grep shows.

- [ ] **Step 2: Write the failing converter tests**

In `src/telegram/converters/message.rs`, add at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use grammers_client::peer::{Community, Peer};
    use grammers_client::Client;
    use grammers_mtsender::SenderPool;
    use grammers_session::storages::MemorySession;
    use std::sync::Arc;

    /// Inert client for offline Peer construction (same trick as the
    /// channel-converter tests).
    fn inert_client() -> Client {
        let session = Arc::new(MemorySession::default());
        let SenderPool { handle, .. } = SenderPool::new(session, 1);
        Client::new(handle)
    }

    fn public_channel_peer(id: i64, username: &str) -> Peer {
        // Copy the raw tl::types::Channel literal from the channel-converter
        // tests' `channel_peer` fixture (broadcast: true, username: Some(username)),
        // built via Peer::from_raw(&inert_client(), tl::enums::Chat::Channel(raw)).
    }

    fn private_community_peer(id: i64) -> Peer {
        // Copy the `community_peer` fixture body from the channel-converter tests.
    }

    #[test]
    fn build_message_link_uses_public_form_when_username_exists() {
        let peer = public_channel_peer(1144180066, "swodki");
        let link = build_message_link(&peer, MessageId::new(610121).expect("valid id"));
        assert_eq!(link.as_deref(), Some("https://t.me/swodki/610121"));
    }

    #[test]
    fn build_message_link_falls_back_to_internal_form() {
        let peer = private_community_peer(521440428);
        let link = build_message_link(&peer, MessageId::new(5).expect("valid id"));
        assert_eq!(link.as_deref(), Some("https://t.me/c/521440428/5"));
    }

    #[test]
    fn extract_reactions_itemizes_emoji_and_totals_everything() {
        // Adapt constructor names/fields to the Step-1 grep output.
        let raw = tl::enums::MessageReactions::Reactions(tl::types::MessageReactions {
            min: false,
            can_see_list: false,
            reactions_as_tags: false,
            results: vec![
                tl::enums::ReactionCount::Count(tl::types::ReactionCount {
                    chosen_order: None,
                    reaction: tl::enums::Reaction::Emoji(tl::types::ReactionEmoji {
                        emoticon: "🔥".to_string(),
                    }),
                    count: 41,
                }),
                tl::enums::ReactionCount::Count(tl::types::ReactionCount {
                    chosen_order: None,
                    reaction: tl::enums::Reaction::CustomEmoji(
                        tl::types::ReactionCustomEmoji { document_id: 7 },
                    ),
                    count: 2,
                }),
            ],
            recent_reactions: None,
            top_reactors: None,
        });

        let (itemized, total) = extract_reactions(Some(&raw));
        let itemized = itemized.expect("emoji reactions present");
        assert_eq!(itemized.len(), 1, "custom emoji is not itemized");
        assert_eq!(itemized[0].emoji, "🔥");
        assert_eq!(itemized[0].count, 41);
        assert_eq!(total, Some(43), "total counts every reaction kind");
    }

    #[test]
    fn extract_reactions_none_when_absent() {
        assert_eq!(extract_reactions(None), (None, None));
    }
}
```

Run: `cargo test build_message_link` → FAIL (helpers missing).

- [ ] **Step 3: Implement the helpers and extend `convert_message`**

`src/telegram/converters/message.rs` (import `channel_identity` from `super::channel`, `MessageReaction` from types, `MessageLink` from `crate::link`):

```rust
/// Permalink for a message, from data already in hand (work-order D1):
/// public `t.me/<username>` form when the channel has one, members-only
/// `t.me/c/…` otherwise. Same builder as generate_message_link (B2).
pub(crate) fn build_message_link(
    peer: &grammers_client::peer::Peer,
    message_id: MessageId,
) -> Option<String> {
    let identity = channel_identity(peer)?;
    Some(MessageLink::new(identity.id, message_id, identity.username.as_deref()).https_link)
}

/// Itemized standard-emoji reactions plus an all-kinds total (work-order D2).
/// Custom-emoji and paid reactions count toward the total but are not
/// itemized (no renderable emoji string).
pub(crate) fn extract_reactions(
    reactions: Option<&tl::enums::MessageReactions>,
) -> (Option<Vec<MessageReaction>>, Option<u64>) {
    let Some(tl::enums::MessageReactions::Reactions(r)) = reactions else {
        return (None, None);
    };
    let mut itemized = Vec::new();
    let mut total = 0u64;
    for result in &r.results {
        let tl::enums::ReactionCount::Count(rc) = result;
        let count = u64::try_from(rc.count).unwrap_or(0);
        total += count;
        if let tl::enums::Reaction::Emoji(e) = &rc.reaction {
            itemized.push(MessageReaction {
                emoji: e.emoticon.clone(),
                count,
            });
        }
    }
    (Some(itemized).filter(|v| !v.is_empty()), Some(total))
}
```

`entities.rs` — new struct plus four `Message` fields:

```rust
/// One standard-emoji reaction with its count (work-order D2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MessageReaction {
    pub emoji: String,
    pub count: u64,
}
```

```rust
    /// Telegram album (media group) id shared by sibling messages (B5).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub grouped_id: Option<i64>,
    /// Permalink: public t.me form when the channel has a username (D1).
    pub link: String,
    /// Standard-emoji reactions, when any (D2).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reactions: Option<Vec<MessageReaction>>,
    /// Total reactions of every kind, including custom/paid (D2).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reactions_total: Option<u64>,
```

`convert_message` — before the final literal:

```rust
    let raw_reactions = match &msg.raw {
        tl::enums::Message::Message(m) => m.reactions.as_ref(),
        _ => None,
    };
    let (reactions, reactions_total) = extract_reactions(raw_reactions);
    let link = build_message_link(peer, message_id)?;
```

and in the literal: `grouped_id: msg.grouped_id(), link, reactions, reactions_total,`.

- [ ] **Step 4: Mirror on the wire and in fixtures**

- `MessageResponse` gains the same four fields (same serde attributes); `From<Message>` copies them.
- `create_test_message` in `src/test_helpers.rs`: `grouped_id: None, link: format!("https://t.me/testchannel/{}", id), reactions: None, reactions_total: None,`. The `entities.rs` test fixture (fixed id 1, username `testchan`): `link: "https://t.me/testchan/1".to_string(),` plus the same three `None`s.
- Compiler sweep: `cargo build 2>&1 | head -30` — any other `Message { .. }` literal sites.

- [ ] **Step 5: Run tests, gate, docs, commit**

Run: `cargo test build_message_link && cargo test extract_reactions && cargo test` → PASS.
README: message-object field table gains `link`, `grouped_id`, `reactions`, `reactions_total`. CHANGELOG Added.

```bash
cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "feat: permalink, reactions, and grouped_id on every message (D1, D2)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: B5+A2 — album collapsing and post-level `limit`

New pure module `src/telegram/albums.rs`: `PostCounter` makes fetch loops count posts (albums count once, siblings never cut at the boundary), `collapse_albums` merges siblings into one post-level `Message` carrying `AlbumInfo`. Default ON.

**Files:**
- Create: `src/telegram/albums.rs`; declare `pub(crate) mod albums;` in `src/telegram.rs` next to the other module declarations
- Modify: `src/telegram/types/entities.rs` (`AlbumInfo` + `Message.album`)
- Modify: `src/telegram/types/params.rs` (`collapse_albums: bool` on both params, constructors)
- Modify: `src/mcp/tools/types/requests.rs` (`SearchRequest`, `GetRecentMessagesRequest`)
- Modify: `src/mcp/server/impl_search.rs` (both `*_impl`: params literal)
- Modify: `src/telegram/client/ops_history.rs` (loop :99-114), `src/telegram/client/ops_search.rs` (both loops)
- Modify: `src/mcp/tools/types/responses.rs` (`MessageResponse.album`), `src/test_helpers.rs` + `entities.rs` fixture (`album: None`)
- Test: inline tests in `src/telegram/albums.rs`; pass-through tests in `src/mcp/tests/history.rs`

**Interfaces:**
- Produces: `pub struct AlbumInfo { pub media_count: u32, pub media_types: Vec<MediaType>, pub message_ids: Vec<MessageId> }`; `Message.album: Option<AlbumInfo>`; `pub(crate) struct PostCounter` with `pub(crate) fn admit(&mut self, grouped_id: Option<i64>, limit: usize) -> bool`; `pub(crate) fn collapse_albums(messages: Vec<Message>) -> Vec<Message>`; `SearchParams.collapse_albums: bool` / `HistoryParams.collapse_albums: bool` (constructors default `true`).
- Consumes: `Message.grouped_id` (Task 5), `create_test_message` fixture.

- [ ] **Step 1: Write the failing unit tests**

Create `src/telegram/albums.rs` with only the test module first:

```rust
//! Album grouping: post-level limit counting and album collapsing (B5+A2).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telegram::types::MediaType;
    use crate::test_helpers::create_test_message;

    fn album_member(id: i64, gid: i64, text: &str) -> crate::telegram::types::Message {
        let mut m = create_test_message(id, text, 100);
        m.grouped_id = Some(gid);
        m.has_media = true;
        m.media_type = MediaType::Photo;
        m
    }

    #[test]
    fn audit_fixture_collapses_to_three_posts() {
        // Work-order fixture: 8 siblings (610047–610054) + 2 singles → 3 posts.
        let mut messages: Vec<_> = (610047..=610054)
            .map(|id| album_member(id, 13950, if id == 610047 { "album caption" } else { "" }))
            .collect();
        messages.push(create_test_message(610119, "single one", 100));
        messages.push(create_test_message(610121, "single two", 100));

        let collapsed = collapse_albums(messages);

        assert_eq!(collapsed.len(), 3);
        let post = &collapsed[0];
        assert_eq!(post.id.get(), 610047, "representative is the lowest sibling id");
        assert_eq!(post.text, "album caption", "text from the carrying sibling");
        let album = post.album.as_ref().expect("album info present");
        assert_eq!(album.media_count, 8);
        assert_eq!(album.message_ids.len(), 8);
        assert_eq!(album.media_types.len(), 8);
        assert!(collapsed[1].album.is_none());
    }

    #[test]
    fn caption_on_a_later_sibling_still_wins() {
        let messages = vec![
            album_member(11, 5, ""),
            album_member(12, 5, "late caption"),
        ];
        let collapsed = collapse_albums(messages);
        assert_eq!(collapsed[0].id.get(), 11);
        assert_eq!(collapsed[0].text, "late caption");
    }

    #[test]
    fn lone_album_member_stays_plain() {
        let collapsed = collapse_albums(vec![album_member(5, 99, "caption")]);
        assert_eq!(collapsed.len(), 1);
        assert!(collapsed[0].album.is_none(), "an album of one is noise");
    }

    #[test]
    fn post_counter_admits_siblings_beyond_limit() {
        let mut c = PostCounter::default();
        assert!(c.admit(Some(7), 1), "first sibling starts post 1");
        assert!(c.admit(Some(7), 1), "sibling of an admitted album is free");
        assert!(!c.admit(None, 1), "a single would start post 2 — stop");
        assert!(!c.admit(Some(8), 1), "a new album would start post 2 — stop");
    }

    #[test]
    fn post_counter_counts_singles() {
        let mut c = PostCounter::default();
        assert!(c.admit(None, 2));
        assert!(c.admit(None, 2));
        assert!(!c.admit(None, 2));
    }
}
```

Run: `cargo test albums` → FAIL (module has no implementation; also add the `mod albums;` declaration now so the failure is about the missing symbols, not the missing module).

- [ ] **Step 2: Implement `PostCounter` and `collapse_albums`**

Above the test module in `src/telegram/albums.rs`:

```rust
use crate::telegram::types::{AlbumInfo, Message};
use std::collections::{HashMap, HashSet};

/// Counts posts (albums count once) while a fetch loop admits messages.
#[derive(Debug, Default)]
pub(crate) struct PostCounter {
    seen_groups: HashSet<i64>,
    posts: usize,
}

impl PostCounter {
    /// Admit a message into a window capped at `limit` posts. Returns `false`
    /// when the message would START a post beyond the limit — the caller
    /// stops fetching. Siblings of an already-admitted album are always
    /// admitted, so an album is never cut at the limit boundary (A2).
    pub(crate) fn admit(&mut self, grouped_id: Option<i64>, limit: usize) -> bool {
        if let Some(gid) = grouped_id
            && self.seen_groups.contains(&gid)
        {
            return true;
        }
        if self.posts >= limit {
            return false;
        }
        self.posts += 1;
        if let Some(gid) = grouped_id {
            self.seen_groups.insert(gid);
        }
        true
    }
}

/// Collapse album siblings (same `grouped_id`) into one post-level `Message`
/// (B5). Order-preserving on each group's first occurrence. The representative
/// is the lowest-id sibling (stable referencing); `text` comes from whichever
/// sibling carries it. A group with a single member in the window stays plain.
pub(crate) fn collapse_albums(messages: Vec<Message>) -> Vec<Message> {
    enum Slot {
        Single(Box<Message>),
        Group(i64),
    }

    let mut slots = Vec::new();
    let mut buckets: HashMap<i64, Vec<Message>> = HashMap::new();
    for msg in messages {
        match msg.grouped_id {
            Some(gid) => {
                let bucket = buckets.entry(gid).or_default();
                if bucket.is_empty() {
                    slots.push(Slot::Group(gid));
                }
                bucket.push(msg);
            }
            None => slots.push(Slot::Single(Box::new(msg))),
        }
    }

    slots
        .into_iter()
        .filter_map(|slot| match slot {
            Slot::Single(msg) => Some(*msg),
            Slot::Group(gid) => {
                let mut siblings = buckets.remove(&gid)?;
                siblings.sort_by_key(|m| m.id.get());
                if siblings.len() == 1 {
                    return siblings.pop();
                }
                let text = siblings
                    .iter()
                    .find(|m| !m.text.is_empty())
                    .map(|m| m.text.clone())
                    .unwrap_or_default();
                let album = AlbumInfo {
                    media_count: siblings.len() as u32,
                    media_types: siblings.iter().map(|m| m.media_type.clone()).collect(),
                    message_ids: siblings.iter().map(|m| m.id).collect(),
                };
                let mut post = siblings.swap_remove(0); // lowest id after the sort
                post.text = text;
                post.album = Some(album);
                Some(post)
            }
        })
        .collect()
}
```

(If `MediaType` is `Copy`, drop the `.clone()`; if `MessageId` is not `Copy`, add one — the compiler decides.)

`entities.rs`:

```rust
/// Post-level album summary on a collapsed message (work-order B5/A2).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AlbumInfo {
    /// Number of sibling messages in the album.
    pub media_count: u32,
    /// Media type of each sibling, in ascending id order.
    pub media_types: Vec<MediaType>,
    /// All sibling message ids, ascending — every part stays reachable.
    pub message_ids: Vec<MessageId>,
}
```

`Message` gains `#[serde(skip_serializing_if = "Option::is_none", default)] pub album: Option<AlbumInfo>,`; `MessageResponse` mirrors it; both `create_test_message` fixtures gain `album: None,`.

Run: `cargo test albums` → PASS.

- [ ] **Step 3: Write the failing pass-through test**

`src/mcp/tests/history.rs`:

```rust
#[tokio::test]
async fn collapse_albums_flag_reaches_params() {
    // Mock get_recent_messages with .withf(|p| !p.collapse_albums)
    // returning create_test_search_result(vec![], "", 0);
    // call get_recent_messages with collapse_albums = Some(false); assert Ok.
}

#[tokio::test]
async fn collapse_albums_defaults_to_true() {
    // Same shape, .withf(|p| p.collapse_albums), request field left None.
}
```

Run: `cargo test collapse_albums_flag` → FAIL (field missing on params/request).

- [ ] **Step 4: Thread the parameter**

- `params.rs`: `pub collapse_albums: bool,` on `SearchParams` and `HistoryParams` with doc `/// Collapse album siblings into one post-level result; limit counts posts (B5+A2).`; both `new()` constructors set `true`; the `search_params_with_media_filter` literal test gains the field.
- `requests.rs`, both request structs:

```rust
    #[schemars(
        description = "Optional: collapse album (grouped media) siblings into one post-level result; when true, limit counts posts, and each collapsed post carries album.message_ids. Default: true."
    )]
    #[serde(default, deserialize_with = "flexible_opt_bool")]
    pub collapse_albums: Option<bool>,
```

- `impl_search.rs`, both params literals: `collapse_albums: request.collapse_albums.unwrap_or(true),`.

- [ ] **Step 5: Rework the three fetch loops**

`ops_history.rs` — before the loop add `let mut counter = PostCounter::default();`, then replace the push block (:108-113):

```rust
                if let Some(converted) = convert_message(&msg, &peer) {
                    if params.collapse_albums {
                        // Post-level limit: stop only when a NEW post would
                        // overflow; trailing siblings of admitted albums pass.
                        if !counter.admit(converted.grouped_id, params.limit as usize) {
                            break;
                        }
                        messages.push(converted);
                    } else {
                        messages.push(converted);
                        if messages.len() >= params.limit as usize {
                            break;
                        }
                    }
                }
```

and after the `with_timeout` block resolves to `messages`:

```rust
        let messages = if params.collapse_albums {
            collapse_albums(messages)
        } else {
            messages
        };
```

`ops_search.rs` — the same `PostCounter` admit logic replaces the raw push-and-check in BOTH branch loops (channel branch push at :71-76, global branch push at :108-115; each branch gets its own `let mut counter = PostCounter::default();` inside its async block). The collapse itself happens ONCE, at the branch join — insert immediately after the `let (mut messages, channels_scanned) = …` binding from Task 2 and before the sort:

```rust
        let mut messages = if params.collapse_albums {
            collapse_albums(messages)
        } else {
            messages
        };
```

Import `PostCounter`/`collapse_albums` in both ops files as `use crate::telegram::albums::{collapse_albums, PostCounter};` (or via the client's `super::*` glob if a re-export in `src/telegram.rs` fits the existing import style better — match how `convert_message` reaches these files).

Note `returned`/`channels_in_results` (Task 2) are computed AFTER collapsing — they count posts, which is the point.

- [ ] **Step 6: Run tests, gate, docs, commit**

Run: `cargo test collapse && cargo test albums && cargo test` → PASS.
README: `collapse_albums` on both tools + album-object example (the audit's 8-photo album). CHANGELOG Changed (breaking: default true changes result shapes on album-heavy channels) + Added (`grouped_id` was Task 5 but document the pair here coherently).

```bash
cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "feat!: collapse albums into post-level results with post-counting limit (B5, A2)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: D3 — `source_variant_*` renames + `largest_available_*`

`original_*` in `get_message_media` describes the selected variant, not the original. Rename, and report the largest variant so callers know whether a better one exists.

**Files:**
- Modify: `src/telegram/types/media.rs` (`MediaDownload` — locate with `ast-index class "MediaDownload"`)
- Modify: `src/telegram/client/ops_media.rs` (:74-77 candidates, :139-148 literal)
- Modify: `src/mcp/tools/types/responses.rs` (`GetMessageMediaResponse` :156-198)
- Modify: `src/mcp/server/impl_media.rs` (:37-51 mapping)
- Test: `src/mcp/tests/media.rs`

**Interfaces:**
- Produces: `MediaDownload { largest_width: Option<u32>, largest_height: Option<u32>, .. }`; `GetMessageMediaResponse { source_variant_width, source_variant_height, source_variant_size_bytes, largest_available_width, largest_available_height, .. }`.
- Consumes: `size_candidates` / `select_size_candidate` (existing, `ops_media.rs:74-75`).

- [ ] **Step 1: Write the failing MCP-layer test**

In `src/mcp/tests/media.rs`, copy an existing success-path test and change the assertions:

```rust
#[tokio::test]
async fn media_metadata_uses_variant_naming_and_reports_largest() {
    // Mock download_message_media returning the fixture MediaDownload the
    // neighboring test uses, with width: Some(320), height: Some(180),
    // largest_width: Some(1280), largest_height: Some(720).
    // Call get_message_media; find the text content block; parse its JSON:
    assert_eq!(json["source_variant_width"], 320);
    assert_eq!(json["largest_available_width"], 1280);
    assert_eq!(json["largest_available_height"], 720);
    assert!(json.get("original_width").is_none(), "original_* renamed (D3)");
}
```

Run: `cargo test media_metadata_uses_variant` → FAIL.

- [ ] **Step 2: Extend `MediaDownload` and fill it**

Add to the struct (with `/// Largest variant Telegram offers — lets callers detect a better re-fetch (D3).` on the pair):

```rust
    pub largest_width: Option<u32>,
    pub largest_height: Option<u32>,
```

`ops_media.rs`, next to the `selected` computation (:74-77):

```rust
        let largest = candidates
            .iter()
            .max_by_key(|c| u64::from(c.width) * u64::from(c.height));
```

and in the final literal: `largest_width: largest.map(|c| c.width), largest_height: largest.map(|c| c.height),`. Fix any other `MediaDownload { .. }` literal sites the compiler flags (test fixtures in `src/mcp/tests/media.rs`).

- [ ] **Step 3: Rename on the wire**

`GetMessageMediaResponse`: `original_width` → `source_variant_width` (description: "Pixel width of the downloaded source variant (the variant actually fetched, not necessarily the original)"), same for height/size; add:

```rust
    #[schemars(description = "Pixel width of the largest variant Telegram offers")]
    pub largest_available_width: Option<u32>,

    #[schemars(description = "Pixel height of the largest variant Telegram offers")]
    pub largest_available_height: Option<u32>,
```

`impl_media.rs` mapping follows (`source_variant_width: download.width, … largest_available_width: download.largest_width, …`). Sweep: `grep -rn "original_width\|original_height\|original_size_bytes" src/` → update every site (media tests assert the old names).

- [ ] **Step 4: Run tests, gate, docs, commit**

Run: `cargo test media && cargo test` → PASS. README `get_message_media` example updated. CHANGELOG Changed (breaking).

```bash
cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "feat!: rename media variant fields and report largest available (D3)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 8: B10 — forward-attribution names: verify, then wire or document

The audit claimed forward source names are "in the entity set attached to the same response — no extra RPC needed". The current code disagrees (`extract_forward_info` doc, `src/telegram/converters/message.rs:25-30`). Settle it against the pinned rev; the zero-extra-RPC invariant is non-negotiable either way (approved design).

**Files:**
- Modify (outcome-dependent): `src/telegram/converters/message.rs` and/or `src/telegram/types/entities.rs` (`ForwardInfo` doc :52-58)
- Test (outcome (a) only): converter test in `message.rs`

- [ ] **Step 1: Investigate the pinned grammers API**

```bash
G=~/.cargo/git/checkouts/grammers-8937e3b5288aa015/9fef0ba
grep -n "pub fn" "$G/grammers-client/src/message/message.rs"
ls "$G/grammers-client/src/message/"
grep -rn "PeerMap" "$G/grammers-client/src/message/"*.rs | grep -v "^.*://"
```

Decide:
- **(a)** A public accessor resolves arbitrary peers from the message's peer map (something beyond `sender()`/`peer()` — e.g. a `peer_map()` getter or a `fwd_from`-resolving method) → proceed to Step 2a.
- **(b)** No such accessor (expected — the pre-plan survey of `pub fn`s found only `sender()`/`peer()`) → proceed to Step 2b.

- [ ] **Step 2a (only if (a)): Fill the names**

TDD: converter test first (offline construction of a message with a forward header will hit the same PeerMap-constructibility wall as v0.14 — if a fixture is impossible, test the extracted pure mapping function instead, passing the resolved peer in). Then extend `extract_forward_info` to accept the resolved source peer and fill `channel_name`/`channel_username` (no sentinels — `Option` stays). Update the `ForwardInfo` doc comment.

- [ ] **Step 2b (expected): Update the documentation trail**

Replace the `ForwardInfo` doc comment's last sentence (entities.rs:52-58) with:

```
/// Filling them would require an extra resolve call per message, which the
/// zero-extra-call enrichment invariant forbids; batch attribution is the
/// `resolve_channels` tool planned for v0.18 (roadmap A7).
```

Mirror the same pointer in `extract_forward_info`'s comment. Add a CHANGELOG note under a `Known limitations` heading: forward attribution stays id-only pending `resolve_channels`.

- [ ] **Step 3: Gate and commit**

```bash
cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test
git add -A
git commit -m "docs: settle forward-attribution names against pinned grammers (B10)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

(Use `feat:` instead of `docs:` if outcome (a) shipped code.)

---

### Task 9: Docs sweep, journal, PR

**Files:**
- Modify: `README.md` (final consistency pass), `CHANGELOG.md` (organize `[Unreleased]`), `docs/tasklist.md` (Phase 29 row + progress counts), `docs/memory.md` (journal entry), `CLAUDE.md` (only if any response-shape example it contains drifted — check with `grep -n "total_found\|username\|has_more" CLAUDE.md`)

- [ ] **Step 1: Verify the deleted-id guard covers media and transcription (work-order §5)**

```bash
grep -n "require_found" src/telegram/client/ops_media.rs src/telegram/client/ops_transcribe.rs src/telegram/client/ops_message.rs
```

All three fetch paths must route their fetched message through `require_found` (`src/telegram/client/guard.rs:19`) — `ops_media.rs:44` already does. If `ops_transcribe.rs` fetches without it, wire it in exactly as `ops_media.rs` does (TDD: extend the guard tests in `guard.rs` only if a new seam appears; the existing `require_found_maps_absent_slot_to_not_found_error` test already locks the mechanism). This closes the audit's §5 "untested against deleted ids" row for both tools.

- [ ] **Step 2: Consistency pass over the docs**

- README: every per-tool section reflects Tasks 1–8 (one read-through; the per-task edits already landed — this pass catches cross-references and the message-object field table).
- CHANGELOG `[Unreleased]`: group into `Changed (breaking)` / `Added` / `Known limitations`; one line per work-order id (B5–B10, A2, D1–D3, D10).
- `docs/tasklist.md`: add Phase 29 — "Post shape (work-order B5-B10, D1-D3, D10, A2)" with the final test count; bump the overall progress line.
- `docs/memory.md`: dated entry — decisions worth remembering (post-counting admit rule, lone-album-member stays plain, `channels_scanned: null` for global search, B10 outcome, ChannelPage full-walk rationale).

- [ ] **Step 3: Full gate + live smoke**

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

Optional but recommended live smoke (needs the real session): `cargo run` + one `get_recent_messages` against the album-heavy channel `1292964247` with `limit=10` — expect ~10 distinct posts, each album carrying `album.message_ids`; and one `get_subscribed_channels` with `limit=3` — expect `total` ≈ the real subscription count and populated `last_message_date`.

- [ ] **Step 4: Commit and open the PR**

```bash
git add -A
git commit -m "docs: align README/CHANGELOG/tasklist for the post-shape release

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
git push -u origin feat/post-shape
```

Open a PR `feat/post-shape` → `master`: summary of the five breaking changes + three additive ones, gate output, and a reminder line: **"After release: resync the news-digest skill (albums default-collapsed, renamed metadata, nullable username/chat_type, link/reactions fields)."** Body ends with:

```
🤖 Generated with [Claude Code](https://claude.com/claude-code)
```

Request `/code-review`, address findings, merge. Release (v0.15.0) happens afterwards via the `release` skill — not in this plan.
