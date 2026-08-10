# Local MCP Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Four user-facing improvements for the local, single-user Telegram MCP: date-range search filters, full channel info (description + member count), a public-channel discovery tool, and `tools/list` cache hints — plus the docs alignment they require.

**Architecture:** Every feature follows the existing trait-based DI seam: request struct in `src/mcp/tools/types/requests.rs` → `#[tool]` wrapper in `src/mcp/server.rs` → `*_impl` in `src/mcp/server/impl_*.rs` → `TelegramClientTrait` method → `TelegramClient` impl in `src/telegram/client/*.rs` → grammers (high-level API or raw TL `invoke`). Tests mock the trait boundary with mockall; converter tests use the offline `Peer` fixture pattern from `src/telegram/converters/channel.rs`.

**Tech Stack:** Rust nightly (edition 2024, let-chains allowed), rmcp 3.1 (`#[tool_router]`/`#[tool]` macros, `ListToolsResult` builders), grammers 0.10 (Codeberg, pinned rev), schemars v1, mockall 0.14, chrono (domain time type — jiff stays behind the grammers boundary).

## Global Constraints

- Pre-commit gate after every task: `cargo fmt --check && cargo clippy -- -D warnings && cargo test` — all three must pass before the task's commit.
- Run `cargo fmt --all` after every code change (before the gate).
- Never `unwrap()` in production code; `expect()` only in tests.
- All tools return `Result<String, String>` (JSON string payload) except `get_message_media`. New tools follow this rule.
- Request structs derive `Deserialize` + `schemars::JsonSchema` (schemars **v1**) and use the flexible deserializer helpers (`flexible_string`, `flexible_opt_string`, `flexible_opt_u32`, `flexible_opt_bool`) from `src/mcp/tools/types/serde_helpers.rs`.
- Domain model stays on chrono `DateTime<Utc>`; jiff types never leave the grammers boundary (`message_timestamp` in `src/telegram/converters/message.rs` is the only conversion site).
- All 12 tools (after Task 3) must remain in the single `#[tool_router] impl` block in `src/mcp/server.rs` (rmcp macro constraint).
- Conventional commits (`feat:`/`fix:`/`docs:`), each ending with:
  `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`
- Commits are made directly on the feature branch for this plan: `feat/local-mcp-improvements` (create from `master` before Task 1).

**Non-goals (deliberately excluded):** MRTR mid-call confirmations and the Tasks-extension transcription rework (blocked on Claude clients negotiating protocol 2026-07-28 in the wild — plan separately when observed); message pagination cursors; reactions/comment-count enrichment; remote Streamable-HTTP deployment (session file makes this a single-user server by design).

---

### Task 1: Date-range filtering for `search_messages` and `get_recent_messages`

Adds `from_date`/`to_date` (RFC 3339 UTC strings) to both tools. Semantics: lower bound = `from_date` if set, else `now - hours_back` (unchanged default); upper bound = `to_date` if set, else none. `from_date` is NOT clamped by `MAX_HOURS_BACK` — reaching further back than 72 h is the point of the feature. Also fixes the pre-existing schema drift: `SearchRequest.hours_back` description says "max: 168" but `SearchParams::MAX_HOURS_BACK` is 72.

**Files:**
- Modify: `src/telegram/types/params.rs` (SearchParams at :11, HistoryParams at :50, add chrono import at :3)
- Modify: `src/mcp/tools/types/requests.rs` (`SearchRequest` :65, `GetRecentMessagesRequest` :93)
- Modify: `src/mcp/server/impl_search.rs` (both `*_impl` methods; parse helper next to `parse_optional_channel_id` — locate it with `ast-index symbol "parse_optional_channel_id"`)
- Modify: `src/telegram/client/ops_search.rs` (:27 cutoff, both loops), `src/telegram/client/ops_history.rs` (:20 cutoff, loop at :89)
- Test: `src/mcp/tests/search.rs`, `src/mcp/tests/history.rs`, inline tests in `src/telegram/types/params.rs`

**Interfaces:**
- Produces: `SearchParams { from_date: Option<DateTime<Utc>>, to_date: Option<DateTime<Utc>>, .. }` and same two fields on `HistoryParams`; method `pub fn window_start(&self) -> DateTime<Utc>` on both; helper `pub(crate) fn parse_optional_utc(field: &str, value: &Option<String>) -> Result<Option<DateTime<Utc>>, String>` in the same module as `parse_optional_channel_id`.
- Consumes: `message_timestamp(&Message) -> Option<DateTime<Utc>>` from `src/telegram/converters/message.rs`.

- [ ] **Step 1: Write failing unit tests for `window_start`**

In `src/telegram/types/params.rs`, add to the existing `#[cfg(test)] mod tests` (create one at the bottom if absent):

```rust
#[test]
fn window_start_defaults_to_hours_back() {
    let params = SearchParams::new("q"); // hours_back = 48 default
    let expected = Utc::now() - Duration::hours(48);
    let diff = (params.window_start() - expected).num_seconds().abs();
    assert!(diff <= 1, "window_start should be ~now - hours_back");
}

#[test]
fn window_start_prefers_from_date() {
    let mut params = SearchParams::new("q");
    let from = Utc::now() - Duration::days(30);
    params.from_date = Some(from);
    assert_eq!(params.window_start(), from);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test window_start`
Expected: FAIL — `from_date` field and `window_start` method do not exist.

- [ ] **Step 3: Add fields and `window_start` to both params structs**

In `src/telegram/types/params.rs`: add `use chrono::{DateTime, Duration, Utc};` to imports. Add to **both** `SearchParams` and `HistoryParams`:

```rust
    /// Inclusive lower bound. When set, overrides `hours_back` as the window
    /// start (and is deliberately NOT clamped by `MAX_HOURS_BACK`).
    pub from_date: Option<DateTime<Utc>>,
    /// Inclusive upper bound. Messages newer than this are skipped.
    pub to_date: Option<DateTime<Utc>>,
```

Add to `impl SearchParams` and a new `impl HistoryParams`:

```rust
    /// Effective window start: `from_date` if set, else `now - hours_back`.
    pub fn window_start(&self) -> DateTime<Utc> {
        self.from_date
            .unwrap_or_else(|| Utc::now() - Duration::hours(self.hours_back as i64))
    }
```

Update `SearchParams::new` (and any other constructors/literal sites — find them with `ast-index usages "SearchParams"` and `ast-index usages "HistoryParams"`; test fixtures in `src/test_helpers.rs` and `src/mcp/tests/*.rs` construct these as struct literals) to initialize both new fields to `None`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test window_start` → PASS. Then `cargo build` — fix any struct-literal sites the compiler flags (missing fields).

- [ ] **Step 5: Use the window in the client loops**

`src/telegram/client/ops_history.rs:20` and `src/telegram/client/ops_search.rs:27` — replace

```rust
let cutoff_time = Utc::now() - Duration::hours(params.hours_back as i64);
```

with

```rust
let cutoff_time = params.window_start();
```

(remove the now-unused `Duration`/`Utc` imports if the compiler warns). In **all three** message loops (history loop at ops_history.rs:89, channel-search loop at ops_search.rs:63, global-search loop below it), insert the upper-bound skip immediately **before** the existing `cutoff_time` check:

```rust
if let Some(to) = params.to_date
    && message_timestamp(&msg).is_some_and(|t| t > to)
{
    continue; // newer than the requested window; keep iterating toward it
}
```

Note `continue` in every loop (including the two that `break` on the lower bound): iteration is reverse-chronological, so too-new messages precede the window.

- [ ] **Step 6: Write failing MCP-layer tests**

In `src/mcp/tests/search.rs` (mirror the existing test style in that file — mock `search_messages` with `withf`):

```rust
#[tokio::test]
async fn search_passes_date_range_to_client() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_search_messages()
        .withf(|p| {
            p.from_date == Some("2026-08-01T00:00:00Z".parse().unwrap())
                && p.to_date == Some("2026-08-05T00:00:00Z".parse().unwrap())
        })
        .return_once(|_| Ok(create_test_search_result(vec![], "q", 0)));
    // ^ fixture from src/test_helpers.rs:174 — check its exact parameter list
    //   there; neighboring tests in this file already call it.
    // Build server with a permissive mock limiter as in neighboring tests,
    // call search_messages with from_date/to_date set, assert Ok.
}

#[tokio::test]
async fn search_rejects_invalid_from_date() {
    // no client call expected; from_date = "not-a-date" must return Err
    // containing "Invalid from_date".
}

#[tokio::test]
async fn search_rejects_inverted_range() {
    // from_date = 2026-08-05, to_date = 2026-08-01 → Err containing
    // "from_date must be earlier than to_date".
}
```

(Adapt `SearchResult` construction to the real struct shape used by neighboring tests in the same file.) Add the analogous pass-through test to `src/mcp/tests/history.rs`.

- [ ] **Step 7: Run tests to verify they fail**

Run: `cargo test search_passes_date_range` — FAIL (fields don't exist on `SearchRequest`).

- [ ] **Step 8: Extend request structs and server impls**

In `src/mcp/tools/types/requests.rs`, add to **both** `SearchRequest` and `GetRecentMessagesRequest`:

```rust
    #[schemars(
        description = "Optional: inclusive start of the time window as RFC 3339 UTC, e.g. \"2026-08-01T00:00:00Z\". Overrides hours_back and may reach arbitrarily far back."
    )]
    #[serde(default, deserialize_with = "flexible_opt_string")]
    pub from_date: Option<String>,

    #[schemars(
        description = "Optional: inclusive end of the time window as RFC 3339 UTC. Messages newer than this are excluded."
    )]
    #[serde(default, deserialize_with = "flexible_opt_string")]
    pub to_date: Option<String>,
```

While in the file, fix the `hours_back` description on `SearchRequest` (:76): "max: 168" → "max: 72" (matching `SearchParams::MAX_HOURS_BACK`).

Next to `parse_optional_channel_id` (same module), add:

```rust
/// Parse an optional RFC 3339 datetime request field into UTC.
pub(crate) fn parse_optional_utc(
    field: &str,
    value: &Option<String>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
    value
        .as_deref()
        .map(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| {
                    format!("Invalid {field}: {e} (expected RFC 3339, e.g. 2026-08-01T00:00:00Z)")
                })
        })
        .transpose()
}
```

In `src/mcp/server/impl_search.rs`, in both `*_impl` methods, after the existing limit validation:

```rust
let from_date = parse_optional_utc("from_date", &request.from_date)?;
let to_date = parse_optional_utc("to_date", &request.to_date)?;
if let (Some(f), Some(t)) = (from_date, to_date)
    && f >= t
{
    return Err("from_date must be earlier than to_date".to_string());
}
```

and add `from_date, to_date,` to the `SearchParams { .. }` / `HistoryParams { .. }` literals.

- [ ] **Step 9: Run the full gate**

Run: `cargo fmt --all && cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all green (445+ tests).

- [ ] **Step 10: Update docs and commit**

README.md documents both tools (search for `hours_back` in README to find the sections) — add the two new optional fields with one example. Add a CHANGELOG `[Unreleased] → Added` bullet: date-range filters on `search_messages`/`get_recent_messages`, plus the hours_back description fix under Fixed.

```bash
git add -A
git commit -m "feat: add from_date/to_date range filters to search and history tools

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Full channel info (`include_full` on `get_channel_info`)

`get_channel_info` today always returns `description: null, member_count: null` (honest but empty — CQ-4). An opt-in `include_full: true` performs one extra RPC (`channels.GetFullChannel`) and fills both fields for channel-kind peers (broadcasts and megagroups — in the TL schema both are `Channel`). Groups/communities fall back to basic info silently (their full-info RPC is a different method; YAGNI until someone asks).

**Files:**
- Modify: `src/telegram/trait_def.rs` (after `get_channel_info` at :25)
- Modify: `src/telegram/client/channels.rs` (after `get_channel_info_impl` at :45)
- Modify: `src/telegram/client.rs` (trait impl block, next to the existing `get_channel_info` forwarding at :67)
- Modify: `src/mcp/tools/types/requests.rs` (`GetChannelInfoRequest` :25)
- Modify: `src/mcp/server/impl_channels.rs` (`get_channel_info_impl` :39)
- Modify: `src/mcp/server.rs` (only the `#[tool]` doc text for get_channel_info if it enumerates fields)
- Test: `src/mcp/tests/channels.rs`

**Interfaces:**
- Produces: `TelegramClientTrait::get_full_channel_info(&self, identifier: &str) -> Result<Channel, Error>` (async, same shape as `get_channel_info`).
- Consumes: `resolve_peer` (client-internal), `peer_to_ref` (src/telegram/client.rs), `convert_peer_to_channel`, `with_timeout`, `self.timeouts.resolve_secs`.

- [ ] **Step 1: Write the failing MCP-layer test**

In `src/mcp/tests/channels.rs`, mirroring the existing `get_channel_info` tests:

```rust
#[tokio::test]
async fn include_full_routes_to_full_channel_info() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_get_full_channel_info()
        .with(eq("@news"))
        .return_once(|_| {
            let mut ch = create_test_channel(); // src/test_helpers.rs fixture
            ch.description = Some("full description".to_string());
            ch.member_count = Some(12345);
            Ok(ch)
        });
    // build server as neighboring tests do; call get_channel_info with
    // include_full = Some(true); assert the JSON contains "full description"
    // and 12345.
}

#[tokio::test]
async fn include_full_absent_keeps_basic_path() {
    // expect_get_channel_info (NOT full) with eq("@news"); include_full = None.
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test include_full` — FAIL: `expect_get_full_channel_info` does not exist (trait method missing).

- [ ] **Step 3: Add the trait method**

`src/telegram/trait_def.rs`, after `get_channel_info`:

```rust
    /// Like [`Self::get_channel_info`], but additionally fetches
    /// `channels.GetFullChannel` to fill `description` and `member_count`.
    /// Falls back to basic info for non-channel peers (small groups,
    /// communities), whose full-info RPC differs.
    async fn get_full_channel_info(&self, identifier: &str) -> Result<Channel, Error>;
```

mockall regenerates the mock automatically. `cargo build` now fails: `TelegramClient` doesn't implement it — good.

- [ ] **Step 4: Implement in the client**

First confirm the TL enum variant names (generated code — do not guess):

```bash
f=$(find target/debug/build -path "*grammers-tl-types*" -name "generated_types.rs" | head -1)
grep -n -A6 "pub enum ChatFull" "$f" | head -10
grep -n -A4 "pub enum messages::ChatFull\|pub mod messages" "$f" | grep -n -A4 "enum ChatFull" | head -6
```

Then in `src/telegram/client/channels.rs` add (adapting the two `match`/destructure lines to the exact variant names the grep shows — the code below assumes `tl::enums::messages::ChatFull::Full` and `tl::enums::ChatFull::ChannelFull`):

```rust
    pub(super) async fn get_full_channel_info_impl(
        &self,
        identifier: &str,
    ) -> Result<crate::telegram::Channel, Error> {
        let peer = self.resolve_peer(identifier).await?;
        let mut channel = convert_peer_to_channel(&peer).ok_or_else(|| {
            Error::InvalidInput("Not a channel or group".to_string())
        })?;

        // channels.GetFullChannel only exists for channel-kind peers
        // (broadcasts + megagroups). Others keep basic info.
        if matches!(peer, grammers_client::peer::Peer::Channel(_)) {
            let peer_ref = peer_to_ref(&peer).await?;
            let request = tl::functions::channels::GetFullChannel {
                channel: (&peer_ref).into(),
            };
            let full = with_timeout("get_full_channel", self.timeouts.resolve_secs, async {
                self.client
                    .invoke(&request)
                    .await
                    .map_err(|e| Error::TelegramApi(format!("GetFullChannel failed: {e}")))
            })
            .await?;

            let tl::enums::messages::ChatFull::Full(chat_full) = full;
            if let tl::enums::ChatFull::ChannelFull(cf) = chat_full.full_chat {
                if !cf.about.is_empty() {
                    channel.description = Some(cf.about);
                }
                channel.member_count =
                    cf.participants_count.and_then(|c| u64::try_from(c).ok());
            }
        }

        Ok(channel)
    }
```

(`From<&PeerRef> for tl::enums::InputChannel` exists — grammers-session `peer.rs:844`.) Wire the trait impl in `src/telegram/client.rs` next to the existing forwarding methods:

```rust
    async fn get_full_channel_info(&self, identifier: &str) -> Result<Channel, Error> {
        self.get_full_channel_info_impl(identifier).await
    }
```

- [ ] **Step 5: Extend request + server routing**

`src/mcp/tools/types/requests.rs`, `GetChannelInfoRequest`:

```rust
    #[schemars(
        description = "Optional: fetch full channel info (description, member_count) with one extra Telegram RPC. Default false."
    )]
    #[serde(default, deserialize_with = "flexible_opt_bool")]
    pub include_full: Option<bool>,
```

`src/mcp/server/impl_channels.rs`, `get_channel_info_impl` body becomes:

```rust
        let channel = if request.include_full.unwrap_or(false) {
            self.telegram_client
                .get_full_channel_info(&request.channel_identifier)
                .await
        } else {
            self.telegram_client
                .get_channel_info(&request.channel_identifier)
                .await
        }
        .map_err(|e| e.to_string())?;

        json_response(&channel)
```

- [ ] **Step 6: Run tests, then the full gate**

Run: `cargo test include_full` → PASS. Then the full gate → green.

- [ ] **Step 7: Update docs and commit**

README `get_channel_info` section: document `include_full` and show `description`/`member_count` populated in the example (the README example was made honest in CQ-4 — keep the basic example null, add a second `include_full` example). CHANGELOG Added bullet.

```bash
git add -A
git commit -m "feat: opt-in full channel info (description, member_count) via include_full

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: `search_public_channels` discovery tool (tool #12)

New tool: keyword search over Telegram's public directory (`contacts.Search`), returning the same `Channel` shape with `is_subscribed: false`. Closes the "find sources" gap — today the connector only works with channels the user already knows.

**Files:**
- Modify: `src/telegram/converters/channel.rs` (refactor `convert_peer_to_channel` :63; fixture tests at bottom)
- Modify: `src/telegram/trait_def.rs`, `src/telegram/client/channels.rs`, `src/telegram/client.rs` (same seam as Task 2)
- Modify: `src/mcp/tools/types/requests.rs` (new request struct), `src/mcp/tools.rs`-adjacent re-exports (mirror how `GetChannelsRequest` is exported — check `grep -n "GetChannelsRequest" src/mcp/tools.rs src/mcp/tools/*.rs`)
- Modify: `src/mcp/server.rs` (new `#[tool]` wrapper), new file `src/mcp/server/impl_discovery.rs` (+ `mod impl_discovery;` next to `mod impl_channels;` at server.rs:124)
- Create: `src/mcp/tests/discovery.rs` (+ register in the tests module the same way `channels.rs` is — see `src/mcp/tests.rs` or the `mod` declarations near it)
- Modify: `CLAUDE.md` (два "11 tools" → "12 tools": architecture diagram :41 and Key Patterns :59), `README.md` (architecture diagram :33 + new tool section)

**Interfaces:**
- Produces: `TelegramClientTrait::search_public_channels(&self, query: &str, limit: u32) -> Result<Vec<Channel>, Error>`; converter `pub fn convert_discovered_peer(peer: &Peer) -> Option<Channel>`; MCP tool `search_public_channels` with request `SearchPublicChannelsRequest { query: String, limit: Option<u32> }`, response = existing `ChannelsResponse` (`has_more: false`, `total = channels.len()`).
- Consumes: `convert_peer_to_channel` internals (shared via new private fn), `with_timeout`, `self.timeouts.search_secs`, `ChannelsResponse`, `json_response`.

- [ ] **Step 1: Write failing converter tests (offline fixture)**

In `src/telegram/converters/channel.rs` tests, alongside `community_peer` (reuse its `MemorySession`/`SenderPool`/`Client` scaffolding via a shared helper `fn inert_client() -> Client` extracted from `community_peer`):

```rust
#[test]
fn discovered_peer_is_not_subscribed() {
    let peer = community_peer(555, "Discovered");
    let channel = convert_discovered_peer(&peer).expect("must convert");
    assert!(!channel.is_subscribed);
    // and the existing path still reports subscribed:
    assert!(convert_peer_to_channel(&peer).expect("must convert").is_subscribed);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test discovered_peer` — FAIL: `convert_discovered_peer` not found.

- [ ] **Step 3: Refactor the converter**

In `src/telegram/converters/channel.rs`, rename the body of `convert_peer_to_channel` into a private fn and re-expose both entry points:

```rust
/// Convert grammers Peer to our Channel type (dialog-list path: subscribed).
pub fn convert_peer_to_channel(peer: &grammers_client::peer::Peer) -> Option<Channel> {
    convert_peer_with_subscription(peer, true)
}

/// Same conversion for peers found via public search (not subscribed).
pub fn convert_discovered_peer(peer: &grammers_client::peer::Peer) -> Option<Channel> {
    convert_peer_with_subscription(peer, false)
}

fn convert_peer_with_subscription(
    peer: &grammers_client::peer::Peer,
    is_subscribed: bool,
) -> Option<Channel> {
    // ...existing body, with `is_subscribed: true` replaced by `is_subscribed`
}
```

Export `convert_discovered_peer` wherever `convert_peer_to_channel` is re-exported (`src/telegram/converters.rs:12`). Run: `cargo test discovered_peer` → PASS. 

- [ ] **Step 4: Write failing client + MCP tests**

Create `src/mcp/tests/discovery.rs` (copy the header/imports of `src/mcp/tests/channels.rs`; register the module where `channels` is declared):

```rust
#[tokio::test]
async fn search_public_channels_returns_channels_response() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_search_public_channels()
        .withf(|q, limit| q == "rust" && *limit == 10)
        .return_once(|_, _| Ok(vec![create_test_channel()]));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().with(eq(1)).returning(|_| Ok(()));
    // build server; call search_public_channels with query "rust", limit None
    // (defaults to 10); parse JSON into ChannelsResponse; assert total == 1
    // and has_more == false.
}

#[tokio::test]
async fn search_public_channels_rejects_empty_query() {
    // query "" → Err containing "query cannot be empty"; no client expectation.
}
```

Run: `cargo test discovery` — FAIL (trait method + tool missing).

- [ ] **Step 5: Add trait method + client implementation**

`src/telegram/trait_def.rs`:

```rust
    /// Search Telegram's public directory for channels/groups by keyword.
    /// Results carry `is_subscribed: false`.
    async fn search_public_channels(&self, query: &str, limit: u32)
    -> Result<Vec<Channel>, Error>;
```

`src/telegram/client/channels.rs` (variants of `tl::enums::Chat` are exactly: `Empty`, `Chat`, `Forbidden`, `Channel`, `ChannelForbidden`, `Community`, `CommunityForbidden` — grammers 0.10 `Community::from_raw` matches the same set; `from_raw` panics on a wrong-kind variant, so route each to its own peer kind and skip `Empty`):

```rust
    pub(super) async fn search_public_channels_impl(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<crate::telegram::Channel>, Error> {
        use grammers_client::peer::{Channel as PeerChannel, Community, Group, Peer};

        if query.trim().is_empty() {
            return Err(Error::InvalidInput(
                "Search query cannot be empty".to_string(),
            ));
        }

        let request = tl::functions::contacts::Search {
            q: query.to_string(),
            limit: limit.clamp(1, 50) as i32,
        };
        let found = with_timeout("contacts_search", self.timeouts.search_secs, async {
            self.client
                .invoke(&request)
                .await
                .map_err(|e| Error::TelegramApi(format!("Public search failed: {e}")))
        })
        .await?;

        let tl::enums::contacts::Found::Found(found) = found;
        let channels = found
            .chats
            .into_iter()
            .filter_map(|chat| {
                let peer = match &chat {
                    tl::enums::Chat::Channel(_) | tl::enums::Chat::ChannelForbidden(_) => {
                        Peer::Channel(PeerChannel::from_raw(&self.client, chat))
                    }
                    tl::enums::Chat::Chat(_) | tl::enums::Chat::Forbidden(_) => {
                        Peer::Group(Group::from_raw(&self.client, chat))
                    }
                    tl::enums::Chat::Community(_)
                    | tl::enums::Chat::CommunityForbidden(_) => {
                        Peer::Community(Community::from_raw(&self.client, chat))
                    }
                    tl::enums::Chat::Empty(_) => return None,
                };
                convert_discovered_peer(&peer)
            })
            .collect();
        Ok(channels)
    }
```

(Verify the `contacts::Found` enum variant name with the same generated-file grep pattern as Task 2 before relying on `Found::Found`.) Import `convert_discovered_peer` in `src/telegram/client.rs`'s converter import list and forward the trait method:

```rust
    async fn search_public_channels(&self, query: &str, limit: u32)
    -> Result<Vec<Channel>, Error> {
        self.search_public_channels_impl(query, limit).await
    }
```

- [ ] **Step 6: Add request struct, impl module, and tool wrapper**

`src/mcp/tools/types/requests.rs`:

```rust
/// Request for search_public_channels tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchPublicChannelsRequest {
    #[schemars(description = "Keyword or name to search Telegram's public directory for")]
    #[serde(deserialize_with = "flexible_string")]
    pub query: String,

    #[schemars(description = "Maximum results to return (default: 10, max: 50)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub limit: Option<u32>,
}
```

Re-export it alongside the other request types. Create `src/mcp/server/impl_discovery.rs`:

```rust
//! `McpServer` inherent `*_impl` method: public channel discovery.

use super::*;

impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn search_public_channels_impl(
        &self,
        request: SearchPublicChannelsRequest,
    ) -> Result<String, String> {
        let limit = request.limit.unwrap_or(10).clamp(1, 50);

        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        let channels = self
            .telegram_client
            .search_public_channels(&request.query, limit)
            .await
            .map_err(|e| e.to_string())?;

        let total = channels.len();
        let response = ChannelsResponse {
            channels,
            total,
            has_more: false,
        };
        json_response(&response)
    }
}
```

Declare `mod impl_discovery;` next to the other `impl_*` modules at `src/mcp/server.rs:124`. In the `#[tool_router]` block, copy the `get_subscribed_channels` `#[tool]` wrapper shape exactly (including the `ToolInvocation` logging pattern):

```rust
    #[tool(
        description = "Search Telegram's public directory for channels and groups by keyword. Results are not from your subscriptions (is_subscribed: false); use get_channel_info or search_messages with the returned id/username to go deeper."
    )]
    async fn search_public_channels(
        &self,
        Parameters(request): Parameters<SearchPublicChannelsRequest>,
        RequestId(id): RequestId,
    ) -> Result<String, String> {
        let inv = ToolInvocation::start("search_public_channels", id);
        tracing::info!(request_id = %inv.request_id, query = %request.query, "Tool invocation started");
        inv.finish(self.search_public_channels_impl(request).await)
    }
```

(Match the exact wrapper idiom of the neighboring tools — copy one and adapt; the snippet above is the shape, the neighboring code is the authority.)

- [ ] **Step 7: Run tests, then the full gate**

Run: `cargo test discovery` → PASS. Full gate → green.

- [ ] **Step 8: Update docs and commit**

- CLAUDE.md: "11 tools" → "12 tools" at :41 and :59 (and the `.claude/rules/ast-index.md` mention is gitignored — leave it).
- README.md: diagram :33 "(11 tools)" → "(12 tools)"; add a `search_public_channels` tool section following the README's per-tool format.
- `docs/tasklist.md` is the source of truth for tool/test counts per the June journal — update its current totals.
- CHANGELOG Added bullet.

```bash
git add -A
git commit -m "feat: add search_public_channels discovery tool (tool 12)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: `tools/list` cache hints (`ttlMs` + `cacheScope`)

MCP 2026-07-28 (SEP-2549) lets list results carry freshness hints; rmcp 3.1's `ListToolsResult` has `.with_ttl_ms(u64)` / `.with_cache_scope(CacheScope)` builders (`rmcp-3.1.2/src/model.rs`, `paginated_result!` macro), but the `#[tool_handler]` macro's generated `list_tools` doesn't set them. The tool list is static per build → 1 hour TTL, `CacheScope::Private` (single-user server).

**Files:**
- Modify: `src/mcp/server.rs` (the `#[tool_handler]` attribute site and the `impl ServerHandler` block at :350)
- Test: `src/mcp/tests/server_core.rs`

**Interfaces:**
- Produces: `pub(crate) fn tools_list_result(&self) -> ListToolsResult` on `McpServer` (pure, testable without a `RequestContext`); a manual `ServerHandler::list_tools` using it.
- Consumes: `self.tool_router: ToolRouter<Self>` field (server.rs:39), `ToolRouter::list_all() -> Vec<Tool>`, `ToolCallContext::new(service, params, ctx)` + `ToolRouter::call` (rmcp 3.1.2 `handler/server/router/tool.rs:560`, `handler/server/tool.rs:52`).

- [ ] **Step 1: Confirm what `#[tool_handler]` generates**

```bash
cd /tmp && curl -sL https://static.crates.io/crates/rmcp-macros/rmcp-macros-3.1.2.crate -o rm.crate && tar xzf rm.crate && grep -rn -A20 "list_tools" rmcp-macros-3.1.2/src/tool_handler.rs | head -40
```

Two outcomes:
- (a) The macro supports passing a customized `ListToolsResult` (an attr arg) → use that, skip Step 4's manual impl.
- (b) It hardcodes the default (expected) → proceed with the manual implementation below.

- [ ] **Step 2: Write the failing test**

In `src/mcp/tests/server_core.rs`, following the existing `get_info` test pattern (direct trait-method call, no transport):

```rust
#[test]
fn tools_list_carries_cache_hints_and_stable_order() {
    // build server with fresh mocks as neighboring tests do
    let result = server.tools_list_result();
    assert_eq!(result.tools.len(), 12);
    assert_eq!(result.ttl_ms, Some(3_600_000));
    assert!(result.cache_scope.is_some());
    // deterministic ordering: two calls agree
    let names: Vec<_> = result.tools.iter().map(|t| t.name.clone()).collect();
    let again: Vec<_> = server.tools_list_result().tools.iter().map(|t| t.name.clone()).collect();
    assert_eq!(names, again);
}
```

Run: `cargo test cache_hints` — FAIL: `tools_list_result` not found.

- [ ] **Step 3: Add the pure helper**

In `src/mcp/server.rs`, in the plain (non-router) `impl McpServer<T, R>` block (near `metrics()`/`response_buffer()` at :93):

```rust
    /// `tools/list` payload with SEP-2549 cache hints. The tool list is static
    /// per build, so clients may cache it for an hour; Private scope because
    /// this is a single-user (per-session-file) server.
    pub(crate) fn tools_list_result(&self) -> rmcp::model::ListToolsResult {
        rmcp::model::ListToolsResult::with_all_items(self.tool_router.list_all())
            .with_ttl_ms(3_600_000)
            .with_cache_scope(rmcp::model::CacheScope::Private)
    }
```

(If `CacheScope`'s variant is named differently, `grep -n "pub enum CacheScope" -A5` in the rmcp source at `/private/tmp/claude-501/.../scratchpad/rmcp-3.1.2/src/model.rs`.) Remove the `#[allow(dead_code)]` on the `tool_router` field if it's now warned as unnecessary.

- [ ] **Step 4: Wire `list_tools` (outcome (b) only)**

Replace the `#[tool_handler]` attribute on the `impl ServerHandler` block with manual methods inside it:

```rust
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        let ctx = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.tool_router.call(ctx).await
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::ErrorData> {
        Ok(self.tools_list_result())
    }
```

(Exact paths for `RequestContext`/`RoleServer` may be `rmcp::service::{RequestContext, RoleServer}` — the compiler will confirm; the removed macro's expansion from Step 1 is the authority for the delegation shape. If the macro supports customization — outcome (a) — keep the macro and pass the helper through its documented hook instead.)

- [ ] **Step 5: Run tests, then the full gate**

Run: `cargo test cache_hints` → PASS. Full gate → green. Manually smoke the stdio server if configured: `cargo run --bin telegram-mcp` + an `initialize`/`tools/list` handshake from a scratch client is optional; the unit test plus rmcp's own conformance coverage suffices.

- [ ] **Step 6: Commit**

CHANGELOG Added bullet (tools/list now carries ttlMs/cacheScope per SEP-2549).

```bash
git add -A
git commit -m "feat: advertise tools/list cache hints (SEP-2549 ttlMs + cacheScope)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Docs alignment sweep

**Files:**
- Modify: `docs/phase-20-plan.md` (:3 `**Status:** Proposed, awaiting implementation`)
- Modify: `docs/memory.md` (append entry), `docs/tasklist.md` (test totals if drifted after Tasks 1–4)

- [ ] **Step 1: Fix phase-20 status**

`TimeoutConfig` + `with_timeout` shipped long ago (every ops file uses it; `[timeouts]` table in CLAUDE.md). Change the status line to `**Status:** Implemented (see CLAUDE.md "Timeout budgets")`.

- [ ] **Step 2: Journal the plan's outcome**

Append a dated entry to `docs/memory.md` summarizing what Tasks 1–4 shipped and any non-obvious lessons found during execution (fixture reuse, TL variant naming, macro findings from Task 4 Step 1).

- [ ] **Step 3: Gate and commit**

Full gate (docs-only, but keep the habit) → green.

```bash
git add -A
git commit -m "docs: align phase-20 status; journal local-MCP improvements

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## After all tasks

Open a PR from `feat/local-mcp-improvements` to `master` (conventional body, gate results, the usual attribution footer), request `/code-review`, and merge after findings are addressed. A `minor` release (`/release minor`) is warranted: three new user-facing capabilities.
