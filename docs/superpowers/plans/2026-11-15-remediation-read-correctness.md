# Telegram Read Correctness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Repair global pagination, source identity, fan-out duplication, subscription evidence and URL filtering without changing read contracts.

**Architecture:** Keep raw TL pagination and pure converters below the client seam, with fan-out above it. Make scope-specific exhaustion and membership evidence explicit, and carry namespaced identity consistently from conversion through post counting and merging.

**Tech Stack:** Rust, pinned grammers 0.11 Codeberg revision, Tokio, mockall, existing TL fixture builders and MCP wire harness.

**Spec:** `docs/superpowers/specs/2026-11-15-review-remediation-design.md`

## Global Constraints

- Retain the single `#[tool_router]` implementation, thin entry points, generic client and limiter seams, file-as-module layout, and the Rust manual's scoped exceptions.
- Keep `json_v1`, matching text fallback, explicit legacy reads, the 16/18 tool catalog, and both MCP 2026-07-28 and legacy 2025-11-25 behavior. No output schema version bump.
- Preserve string channel/user IDs, integer message IDs, nullable fields, namespaced source identity, compact-mode ambiguity refusal, and direct Rust compatibility methods.
- Validate locally before acquisition; perform every RPC after acquisition. Do not refund failed fan-out attempts.
- Preserve total operation deadlines, search partial results, cancellation, bounded input, admission, memory, image processing and shutdown. No new application retries.
- Preserve global search's widened server date bounds and exact client-side filtering. Global and fan-out results still have no message-ID continuation cursor.
- Keep all three direct grammers dependencies at the same existing Codeberg revision. Add no dependency.
- Validate locally on macOS; no live Telegram calls or private configuration changes.

## Review Focus

- A short global slice with no next_rate must still advance using its actual last message (Task 1).
- Missing source namespaces in compatibility fixtures must remain unknown, not be silently invented (Task 2).
- A later invalid fan-out alias must still be rejected before any acquisition (Task 3).
- Forbidden SDK channel/community conversion synthesizes apparently joined flags; raw evidence must survive that conversion (Task 4).
- Webpage Empty/Pending variants can carry usable URL metadata even when there is no full Page (Task 5).

---

### Task 1: Separate global exhaustion from page decoding (R02, D01)

**Files:**
- Modify: `src/telegram/client/raw_page.rs`, `src/telegram/client/raw_pager.rs`
- Test: `src/telegram/client/tests/raw_page_tests.rs`, `raw_pager_tests.rs`, `ops_message_tests.rs`, `walk_tests.rs`, `src/mcp/tests/search_shaping.rs`
- Modify documentation: `src/mcp/server.rs`, `src/mcp/tools/types/requests.rs`, `README.md`, `docs/tool-contract.md`, `docs/memory.md`

**Interfaces:**
- Consumes existing `RawGlobalSearchPager::next`, `raw_tl_messages_slice`, `raw_tl_message_grouped`, `MessageWalk::step` and shared search deadline.
- Produces `unpack_page(res: tl::enums::messages::Messages) -> RawPage` retaining envelope kind; `RawPage::single_peer_exhausted(limit: i32) -> bool` and `RawPage::global_exhausted() -> bool` keep scope policies separate. Update all existing pager callers.
- Produces an injectable fetch seam used by the production global `next`, so tests drive actual paging/offset transitions rather than a copied loop. Keep public client APIs unchanged.

- [ ] **Step 1: Add failing real-page and production-pager tests.** Build a 100-message mixed User/Chat/Channel slice whose first ID is 9 and later IDs exceed 1900; return a qualifying message on the next page. Assert second fetch, actual final-message offset_peer/id/rate, two pages and correct raw counts. Add short nonempty slice, missing next_rate, empty and terminal unsliced pages, filtering to later matches, exact-limit exhaustion versus an extra qualifying result, and paused deadline partial-result cases. Preserve User/Chat/Channel namespaces and no global cursor.
- [ ] **Step 2: Run `cargo test --offline --locked raw_page` and exact new pager tests.** Expected RED: low-ID slice incorrectly terminates. Short-slice/missing-rate tests may already pass and serve as compatibility evidence.
- [ ] **Step 3: Implement the scope split and real fetch seam.** Global exhaustion is empty or terminal-unsliced only; preserve last-raw-message offsets and existing single-peer policy. Keep ±1-second server bounds, exact client filtering, per-page accounting and the one total search deadline. Correct descriptions to available account global index, possible private conversations/groups/channels, nonexhaustive scope, and only single-channel cursor support. Add no scope switch or eligibility change.
- [ ] **Step 4: Run `cargo test --offline --locked telegram::client` and `cargo test --offline --locked search`, then the full Rust gate.** Expected all pass, exact-limit has_more reflects refused qualifying messages, and global user results survive.
- [ ] **Step 5: Commit `fix: preserve global search pagination across peer IDs` and obtain independent task review.** Include D01 documentation disposition and concrete R02 RED/GREEN evidence.

### Task 2: Preserve album and sender namespaces (R03, R04)

**Files:**
- Modify: `src/telegram/albums.rs`, `src/telegram/converters/message.rs`
- Test: album inline tests, `src/telegram/tests/message_tests.rs`, `src/telegram/client/tests/walk_tests.rs`, MCP parity/search shaping tests

**Interfaces:**
- Consumes `Message.peer_namespace: Option<PeerNamespace>` and the existing envelope lookup.
- Produces internal `type AlbumKey = (Option<PeerNamespace>, i64, i64)` used by `album_key(msg: &Message) -> Option<AlbumKey>`, `PostCounter::admit(group_key: Option<AlbumKey>, limit: usize) -> bool`, collapse buckets and ordered slots.
- Retains `convert_raw_message(raw: &tl::enums::Message, peer: &Peer, entities: &EntityLookup) -> Option<Message>` and all public schemas.

- [ ] **Step 1: Add regressions for equal bare channel/grouped IDs in User/Chat/Channel namespaces.** Assert three distinct admitted posts, sibling collapse within one namespace and raw admission beyond the post limit. Real TL sender fixtures assert User ID survives missing names; Channel/Chat sender IDs are null while names remain; absent sender stays absent. Verify canonical and legacy output. Unknown namespace fixtures retain `None`.
- [ ] **Step 2: Run the new album/sender test filters.** Expected RED: cross-namespace albums merge and channel/chat senders are coerced to user IDs.
- [ ] **Step 3: Implement the common AlbumKey and explicit `Peer::User` sender-ID match.** Keep sender-name lookup independent. Preserve existing representative/caption/single-sibling behavior. Correct the converter comment claiming all paths exclude private users, without changing conversion eligibility.
- [ ] **Step 4: Run `cargo test --offline --locked albums`, message converter/parity tests and full Rust gate.** Expected all pass with unchanged schemas.
- [ ] **Step 5: Commit `fix: preserve peer namespaces in albums and senders`; obtain independent task review.**

### Task 3: Normalize fan-out references and deduplicate observations (R05)

**Files:**
- Modify: `src/mcp/tools/fanout.rs`, `src/mcp/tools/validation.rs`, `src/mcp/tools/helpers.rs` only for shared reference parsing, `src/mcp/server/impl_search.rs` (both search and history handlers)
- Modify: `src/mcp/tools/types/responses.rs`, `src/telegram/client/results.rs` only for matching namespaced returned-source counts
- Test: fanout inline tests, `src/mcp/tests/multi_channel.rs`, `src/mcp/tests/search_shaping.rs`, `src/mcp/tools/types/tests/responses_tests.rs`, `src/telegram/client/tests/helpers_tests.rs`
- Adjust fixture: `src/mcp/tests/protocol_contract.rs` compact-header conflict must use distinct surviving message identities

**Interfaces:**
- Consumes `ChannelId`, `Username::is_valid_shape`, existing atomic acquire and ordered buffered fan-out.
- Produces `FanoutChannel { reference: String, original: String }`, internal `ChannelReferenceKey::{Id(ChannelId), Username(String)}` and `channel_reference_key(reference: &str) -> Result<ChannelReferenceKey, String>`.
- Updates `validate_channel_scope(...) -> Result<Option<Vec<FanoutChannel>>, String>` and `run<F,Fut>(list: Vec<FanoutChannel>, fetch: F) -> Vec<ChannelFetchOutcome>`; fetch still receives the normalized usable String and errors carry the first supplied spelling.
- Retains `merge_results`' existing signature and typed per-channel failures.
- Returned-source counting in both result assembly and MCP shaping uses `(Option<PeerNamespace>, channel_id)`, retaining unknown namespaces and existing missing-ID behavior. RPC/page/raw/attempt counters are unchanged.

- [ ] **Step 1: Add failing normalization, charging and merge tests.** Optional @, case and whitespace variants spend one attempt; accepted `+123`/`00123` normalize to numeric 123 without conflating `@123`. Validate every supplied entry before dedupe/acquire. Numeric-plus-username observations spend two real attempts but return one message/album. Assert first complete payload wins in input order, full `(namespace, channel_id, message_id)` identity, three returned sources for same-bare-ID User/Chat/Channel observations, true RPC/page/error counts, no duplicate-only has_more, and surviving compact-header conflicts still fail. Existing conflict fixtures must use distinct message identities; same-identity payload conflicts keep the first observation.
- [ ] **Step 2: Run `cargo test --offline --locked fanout` and new `multi_channel` filters.** Expected RED: aliases charge multiple attempts and duplicates survive final merge.
- [ ] **Step 3: Implement typed local keys and first-observation dedupe before sorting/truncation.** Preserve all attempted charges, partial flags and upstream has_more. Normalize only equivalent local spellings; never resolve ID/username equivalence before charging. Preserve invalid-input rejection and unqualified read resolver ambiguity checks.
- [ ] **Step 4: Run both search/history multi-channel suites, compact ambiguity tests and full Rust gate.** Expected all pass; atomic acquire uses locally deduplicated count, no refunds.
- [ ] **Step 5: Commit `fix: deduplicate equivalent fan-out channel observations`; obtain independent task review.**

### Task 4: Preserve explicit subscription evidence (R06)

**Files:**
- Modify: `src/telegram/converters/channel.rs`, `src/telegram/client/channels.rs`, `src/telegram/client/ops_resolve.rs`, `src/telegram/client/resolve.rs` if contextual resolution belongs there
- Create: focused `src/telegram/client/resolve_subscription.rs` and its private client module declaration/converter reexports
- Test: `src/telegram/client/tests/subscription_tests.rs` (raw converter, routing, cache and enrichment coverage), existing converter/channel suites, `src/mcp/tests/channels.rs`, `src/mcp/tests/resolve.rs`
- Modify: `docs/tool-contract.md`, `docs/memory.md` for conservative boolean semantics

**Interfaces:**
- Produces internal `SubscriptionEvidence::{Dialog, Joined, Unconfirmed}`, `ResolvedChannelPeer { peer: Peer, subscription: SubscriptionEvidence }`, `subscription_evidence(chat: &tl::enums::Chat) -> SubscriptionEvidence`, and `convert_peer_with_evidence(peer: &Peer, evidence: SubscriptionEvidence) -> Option<Channel>`.
- Produces `resolve_username_peer_with_evidence(&self, reference: &str) -> Result<Option<ResolvedChannelPeer>, Error>` using the single existing contacts.ResolveUsername RPC and preserving SDK peer-cache effects through public session APIs.
- Retains established public dialog/discovery converter APIs as context-specific compatibility wrappers; basic/full and unmatched username batch paths consume contextual evidence.

- [ ] **Step 1: Enumerate pinned TL variants in a fixture table and add failing converter/decoder/path tests.** Chat Empty/Forbidden, ChannelForbidden and CommunityForbidden => unconfirmed; Chat requires !left && !deactivated; full Channel/Community requires !left && !min. Minimal username observations remain unconfirmed even if creator is set: Telegram's authoritative minimal-field whitelist does not establish membership from creator/left. Dialog evidence and public-search my_results stay true. Joined/left username cases must exercise basic/full/batch contextual production decoders, no extra membership RPC, and unchanged best-effort enrichment.
- [ ] **Step 2: Run subscription/channel/resolve test filters.** Expected RED: username resolution currently reports true without membership evidence. Include forbidden conversion: SDK fabricates left=false/min=false for forbidden channel/community, so high-level raw flags alone must fail the regression.
- [ ] **Step 3: Capture raw TL evidence before Peer::from_raw.** Preserve missing username/error behavior and relevant session-cache insertion performed by the SDK resolver; consult its pinned `build_peer_map` behavior. Keep ordinary message/read resolution unchanged where evidence is unnecessary. No full dialog traversal or extra membership call. Boolean true means positively established; false includes unknown.
- [ ] **Step 4: Run converter/channel/resolve suites and full Rust gate.** Expected all pass, existing public-search my_results and full-info degradation remain intact.
- [ ] **Step 5: Commit `fix: derive channel subscription from explicit evidence`; obtain independent task review.**

### Task 5: Match Telegram URL entities and webpage metadata (R07)

**Files:**
- Modify: `src/telegram/converters/media.rs`
- Test: `src/telegram/tests/converters_thumb_forward_tests.rs`, `src/telegram/client/tests/walk_tests.rs`, existing MCP history filter tests

**Interfaces:**
- Retains `matches_media_filter_raw(raw: &tl::enums::Message, filter: &MediaFilter) -> bool`.
- Produces one private raw URL predicate used by that production function; no new network calls or public type.

- [ ] **Step 1: Add failing real TL cases.** TextUrl with display-only text and Url on `example.com` match. Nonempty URL in Page/Empty/Pending webpage variants matches; empty/NotModified does not establish a match. Literal HTTP(S) remains a fallback; unrelated entities, plain prose and unrelated media remain false. Exercise history's MessageWalk with entity-only links.
- [ ] **Step 2: Run new URL tests.** Expected RED on entity-only and metadata-only links.
- [ ] **Step 3: Implement raw URL evidence before generic media conversion.** Do not add domain heuristics or alter server-side search filters/other media filters.
- [ ] **Step 4: Run converter/walk/history tests and full Rust gate.** Expected all pass.
- [ ] **Step 5: Commit `fix: honor Telegram URL entities in history filters`; obtain independent task review.**

## Shared Verification

Each task records failing-before/passing-after evidence, runs `cargo fmt --all --check`, strict all-target Clippy, full locked offline tests and warning-free docs before commit, then gets independent task review. Update Unreleased notes once during final integration. Continue with the other subsystem plans; final whole-branch acceptance owns A01–A19 and the complete spec command matrix.
