# Correctness Core (B1, B2+D9, B3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop fabricating deleted messages (B1), emit public `t.me/<username>` links for public channels with username input accepted (B2+D9), and make `media_filter` callable by inlining its enum schema with a recurrence guard (B3).

**Architecture:** Targeted fixes at existing seams (approved spec: `docs/superpowers/specs/2026-08-10-correctness-core-design.md`). A new guard module detects Telegram's `MessageEmpty` placeholder at the grammers boundary; a new sentinel-free `ChannelIdentity` resolution feeds a reworked shared link builder in `src/link.rs`; a `#[schemars(inline)]` attribute plus a schema-walk test fix and lock the published schemas.

**Tech Stack:** Rust nightly (2024 edition), rmcp v3.1, grammers (Codeberg git, pinned rev `9fef0bae`), schemars v1, mockall, tokio.

## Global Constraints

- Branch: `fix/correctness-core` off `master`. Ships as v0.13.1 (release itself is done later via the `release` skill — NOT in this plan).
- Pre-merge gate, all must pass: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
- Config tests are serial: `cargo test config -- --test-threads=1` (the plain `cargo test` in the gate is fine; only config-focused runs need the flag).
- Run `cargo fmt --all` after every code change.
- **Never `unwrap()`** in production code; `expect()` only in tests/impossible cases.
- Never log phone numbers, API hashes, passwords, session tokens.
- TDD: failing test first, always.
- Conventional commits (`fix:`, `feat:`, `docs:`).
- Do NOT regress work-order §1.3 verified behavior: parameter clamping, date-window accuracy, window validation messages, media guards, identifier flexibility on the four tools that have it, the normalized unknown-channel error `invalid input: Channel not found: @…`.
- The grammers deps stay pinned to the Codeberg rev — no dependency changes in this plan.

---

### Task 1: Empty-message guard module (B1 detection)

Telegram reports deleted/never-existed ids in `GetMessages` responses as `MessageEmpty` instead of omitting them. grammers wraps that variant in a normal-looking `Message` (`date_timestamp()` → 0, `text()` → `""`), which our code maps blindly — fabricating the epoch-timestamp message the audit observed. grammers `Message` has a public `raw: tl::enums::Message` field, so detection is a `matches!` on the variant. A grammers `Message` cannot be constructed offline (its `PeerMap` has no public constructor), so the guard exposes its decision on the raw TL enum, which IS constructible — that's the testable seam.

**Files:**
- Create: `src/telegram/client/guard.rs`
- Modify: `src/telegram/client.rs` (add `mod guard;` next to the existing `mod ops_message;`-style declarations)

**Interfaces:**
- Produces: `pub(super) fn is_empty_variant(raw: &tl::enums::Message) -> bool` and `pub(super) fn require_found(fetched: Option<grammers_client::message::Message>, channel_ref: &str, message_id: i32) -> Result<grammers_client::message::Message, Error>` — Task 2 calls `require_found` from the three fetch ops via `use super::guard::require_found;`.

- [ ] **Step 1: Create the branch**

```bash
git checkout master && git pull && git checkout -b fix/correctness-core
```

- [ ] **Step 2: Write the failing tests**

Create `src/telegram/client/guard.rs` with ONLY the test module first (the `use super::*;` line at top so `tl` and `Error` resolve from the parent's glob):

```rust
//! Deleted/missing message detection at the grammers boundary.
//!
//! Telegram reports deleted or never-existed ids in `GetMessages` responses as
//! `MessageEmpty` rather than omitting them; grammers wraps that variant in a
//! normal-looking `Message` (epoch date, empty text). Mapping it blindly
//! fabricates a message (work-order B1) — these helpers make the case explicit.

use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_raw(id: i32) -> tl::enums::Message {
        tl::enums::Message::Empty(tl::types::MessageEmpty {
            id,
            peer_id: None,
        })
    }

    /// The smallest constructible non-empty TL message (Service has 16 fields
    /// vs ~50 on Message; the guard only discriminates Empty vs not-Empty).
    fn service_raw(id: i32) -> tl::enums::Message {
        tl::enums::Message::Service(tl::types::MessageService {
            out: false,
            mentioned: false,
            media_unread: false,
            reactions_are_possible: false,
            silent: false,
            post: true,
            legacy: false,
            id,
            from_id: None,
            peer_id: tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 1 }),
            saved_peer_id: None,
            reply_to: None,
            date: 1_700_000_000,
            action: tl::enums::MessageAction::Empty,
            reactions: None,
            ttl_period: None,
        })
    }

    #[test]
    fn empty_variant_is_detected() {
        assert!(is_empty_variant(&empty_raw(609784)));
    }

    #[test]
    fn non_empty_variant_is_not_detected() {
        assert!(!is_empty_variant(&service_raw(610119)));
    }

    #[test]
    fn require_found_maps_absent_slot_to_not_found_error() {
        let result = require_found(None, "swodki", 999_999_999);

        let err = result.expect_err("absent slot must be an error");
        assert!(matches!(err, Error::InvalidInput(_)));
        assert_eq!(
            err.to_string(),
            "invalid input: Message 999999999 not found or deleted in channel swodki"
        );
    }
}
```

Note on the error string: `Error::InvalidInput` is declared at `src/error.rs:23-24` as `#[error("invalid input: {0}")]` (verified) — the assertion above is exact.

- [ ] **Step 3: Declare the module and run the tests to verify they fail to compile**

In `src/telegram/client.rs`, next to the existing ops module declarations (`mod ops_message;` etc.), add:

```rust
mod guard;
```

Run: `cargo test guard`
Expected: COMPILE ERROR — `is_empty_variant` and `require_found` not found.

- [ ] **Step 4: Write the implementation**

Add above the test module in `src/telegram/client/guard.rs`:

```rust
/// True when the raw TL message is the `MessageEmpty` placeholder.
pub(super) fn is_empty_variant(raw: &tl::enums::Message) -> bool {
    matches!(raw, tl::enums::Message::Empty(_))
}

/// The single fetched message, or a not-found error.
///
/// Both an absent slot and the `MessageEmpty` placeholder mean the id does not
/// exist in this channel (deleted, or never existed).
pub(super) fn require_found(
    fetched: Option<grammers_client::message::Message>,
    channel_ref: &str,
    message_id: i32,
) -> Result<grammers_client::message::Message, Error> {
    match fetched {
        Some(msg) if !is_empty_variant(&msg.raw) => Ok(msg),
        _ => {
            tracing::warn!(
                channel_ref = %channel_ref,
                message_id,
                "Message not found or deleted"
            );
            Err(Error::InvalidInput(format!(
                "Message {message_id} not found or deleted in channel {channel_ref}"
            )))
        }
    }
}
```

If `tl` is not in scope via `use super::*;` (compile error), add `use grammers_client::tl;` explicitly at the top of `guard.rs`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo fmt --all && cargo test guard`
Expected: 3 passed.

- [ ] **Step 6: Commit**

```bash
git add src/telegram/client.rs src/telegram/client/guard.rs
git commit -m "fix: add MessageEmpty guard helpers for message fetches"
```

---

### Task 2: Wire the guard into all fetch paths (B1 fix)

Three ops share the identical vulnerable pattern `messages.into_iter().next().flatten().ok_or_else(…)` and then trust the message: `ops_message.rs` (feeds `get_message_by_link`), `ops_media.rs` (feeds `get_message_media`), `ops_transcribe.rs` (feeds `transcribe_voice_message`). Without the guard, a deleted id in the media/transcribe paths produces the misleading `message has no visual media (media type: none)` / `not transcribable` instead of not-found. `convert_message` additionally gets a defense-in-depth early return protecting the iteration paths (`ops_search.rs`, `ops_history.rs`).

**Files:**
- Modify: `src/telegram/client/ops_message.rs:44-55`
- Modify: `src/telegram/client/ops_media.rs:43-48`
- Modify: `src/telegram/client/ops_transcribe.rs:37-42`
- Modify: `src/telegram/converters/message.rs:75-79` (top of `convert_message`)

**Interfaces:**
- Consumes: `require_found` from Task 1 (`use super::guard::require_found;` in each ops file).
- Produces: all three fetch paths return `Error::InvalidInput("Message {id} not found or deleted in channel {ref}")` for deleted/missing ids; `convert_message` returns `None` for the `MessageEmpty` variant.

- [ ] **Step 1: Replace the extraction in `ops_message.rs`**

Add `use super::guard::require_found;` after the existing `use super::*;`. Replace lines 44-55 (the `// get_messages_by_id returns Vec<Option<Message>> — extract the single result` block ending at `})?;`) with:

```rust
        // get_messages_by_id returns Vec<Option<Message>>; deleted ids come
        // back as a wrapped MessageEmpty, not None (work-order B1).
        let grammers_msg =
            require_found(messages.into_iter().next().flatten(), channel_ref, message_id)?;
```

- [ ] **Step 2: Same replacement in `ops_media.rs`**

Add `use super::guard::require_found;` after `use super::*;`. Replace lines 43-48 (`let msg = messages.into_iter().next().flatten().ok_or_else(…)?;`) with:

```rust
        let msg = require_found(messages.into_iter().next().flatten(), channel_ref, message_id)?;
```

- [ ] **Step 3: Same replacement in `ops_transcribe.rs`**

Add `use super::guard::require_found;` after `use super::*;`. Replace lines 37-42 with:

```rust
        let msg = require_found(messages.into_iter().next().flatten(), channel_ref, message_id)?;
```

- [ ] **Step 4: Defense-in-depth in `convert_message`**

In `src/telegram/converters/message.rs`, at the very top of `convert_message` (before the `peer_identity` line), add:

```rust
    // A MessageEmpty placeholder (deleted / never-existed id) must never map
    // to a domain Message — it has an epoch-0 date and empty text (B1).
    if matches!(msg.raw, tl::enums::Message::Empty(_)) {
        return None;
    }
```

`tl` is already imported in this file (`use grammers_client::tl;`).

- [ ] **Step 5: Run the full suite**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: all pass. The MCP-layer tests mock at the trait boundary, so none assert the old `"Message {} not found in channel {}"` text from these ops — if one does fail on the new wording, update its expectation to `not found or deleted`.

- [ ] **Step 6: Commit**

```bash
git add -A src/
git commit -m "fix: treat deleted messages as not-found in fetch and convert paths"
```

---

### Task 3: Sentinel-free channel identity resolution (B2 groundwork)

`Channel.username` uses sentinel strings (`"unknown"`, `"group"` — see `fallback_username` in `src/telegram/converters/channel.rs`), which would fabricate `t.me/unknown/…` links. Link generation needs a source of truth where `None` means "no username": a new `ChannelIdentity` type, a converter that extracts it from a resolved peer, and a new trait method so the MCP layer can reach it through mocks.

**Files:**
- Modify: `src/telegram/types/entities.rs` (add `ChannelIdentity` after the `Channel` struct, which ends at line 101)
- Modify: `src/telegram/types.rs` (add `ChannelIdentity` to the re-export list)
- Modify: `src/telegram.rs:12` (add `ChannelIdentity` to the `pub use types::{…}` list)
- Modify: `src/telegram/converters/channel.rs` (add `channel_identity` fn + tests)
- Modify: `src/telegram/converters.rs` (re-export `channel_identity` alongside the existing `pub use` items)
- Modify: `src/telegram/trait_def.rs` (new trait method; add `ChannelIdentity` to the `use crate::telegram::types::{…}` import)
- Modify: `src/telegram/client/resolve.rs` (new `resolve_channel_identity_impl`)
- Modify: `src/telegram/client.rs` (wire trait method in the `impl TelegramClientTrait for TelegramClient` block)

**Interfaces:**
- Consumes: existing `resolve_peer` (`src/telegram/client/resolve.rs:13`), `peer.id().bare_id()`, per-variant `.username()` accessors.
- Produces: `pub struct ChannelIdentity { pub id: ChannelId, pub username: Option<String> }`; trait method `async fn resolve_channel_identity(&self, channel_ref: &str) -> Result<ChannelIdentity, Error>` (auto-mocked as `expect_resolve_channel_identity` on `MockTelegramClientTrait`); converter `pub(crate) fn channel_identity(peer: &grammers_client::peer::Peer) -> Option<ChannelIdentity>`. Tasks 4–5 depend on these exact names.

- [ ] **Step 1: Write the failing converter tests**

In the existing `#[cfg(test)] mod tests` of `src/telegram/converters/channel.rs` (it already has the offline `community_peer` helper), add a channel-peer helper and two tests:

```rust
    /// Offline `Peer::Channel` fixture, mirroring `community_peer` — grammers'
    /// `Channel::from_raw` is public and needs only an inert client.
    fn channel_peer(id: i64, title: &str, username: Option<&str>) -> Peer {
        let session = Arc::new(MemorySession::default());
        let SenderPool { handle, .. } = SenderPool::new(session, 1);
        let client = Client::new(handle);
        let raw = tl::types::Channel {
            creator: false,
            left: false,
            broadcast: true,
            verified: false,
            megagroup: false,
            restricted: false,
            signatures: false,
            min: false,
            scam: false,
            has_link: false,
            has_geo: false,
            slowmode_enabled: false,
            call_active: false,
            call_not_empty: false,
            fake: false,
            gigagroup: false,
            noforwards: false,
            join_to_send: false,
            join_request: false,
            forum: false,
            stories_hidden: false,
            stories_hidden_min: false,
            stories_unavailable: true,
            signature_profiles: false,
            autotranslation: false,
            broadcast_messages_allowed: false,
            monoforum: false,
            forum_tabs: false,
            id,
            access_hash: Some(0),
            title: title.to_string(),
            username: username.map(str::to_string),
            photo: tl::enums::ChatPhoto::Empty,
            date: 0,
            restriction_reason: None,
            admin_rights: None,
            banned_rights: None,
            default_banned_rights: None,
            participants_count: None,
            usernames: None,
            stories_max_id: None,
            color: None,
            profile_color: None,
            emoji_status: None,
            level: None,
            subscription_until_date: None,
            bot_verification_icon: None,
            send_paid_messages_stars: None,
            linked_monoforum_id: None,
        };
        Peer::Channel(grammers_client::peer::Channel::from_raw(
            &client,
            tl::enums::Chat::Channel(raw),
        ))
    }

    #[test]
    fn channel_identity_public_channel_carries_username() {
        let peer = channel_peer(1144180066, "Сводки", Some("swodki"));

        let identity = channel_identity(&peer).expect("channel must yield an identity");

        assert_eq!(identity.id.get(), 1144180066);
        assert_eq!(identity.username.as_deref(), Some("swodki"));
    }

    #[test]
    fn channel_identity_private_peer_has_no_username_sentinel() {
        let peer = community_peer(521440428, "Семейный чатик");

        let identity = channel_identity(&peer).expect("community must yield an identity");

        assert_eq!(identity.id.get(), 521440428);
        assert_eq!(identity.username, None);
    }
```

If the `tl::types::Channel` field list drifts with the pinned grammers rev, fix the fixture to match the compiler — the two assertions are the contract, the fixture is scaffolding.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test channel_identity`
Expected: COMPILE ERROR — `channel_identity` not found.

- [ ] **Step 3: Implement the type and converter**

In `src/telegram/types/entities.rs`, after the `Channel` struct (line 101):

```rust
/// A channel's canonical numeric ID plus its public username, if any.
///
/// Unlike [`Channel::username`], there are no fallback sentinels here — `None`
/// means the chat has no public username. Used by link generation (work-order
/// B2), where a sentinel would fabricate a `t.me/unknown/…` link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelIdentity {
    pub id: ChannelId,
    pub username: Option<String>,
}
```

Add `ChannelIdentity` to the re-export lists in `src/telegram/types.rs` and `src/telegram.rs:12`.

In `src/telegram/converters/channel.rs` (add `ChannelIdentity` to its `use crate::telegram::types::{…}` import):

```rust
/// Extract the numeric-ID + optional-username pair used for link generation.
///
/// Unlike [`peer_identity`], no username fallback sentinel is applied: `None`
/// means the peer has no public username (work-order B2).
pub(crate) fn channel_identity(peer: &grammers_client::peer::Peer) -> Option<ChannelIdentity> {
    use grammers_client::peer::Peer;

    let id = ChannelId::new(peer.id().bare_id()?).ok()?;
    let username = match peer {
        Peer::Channel(ch) => ch.username().map(str::to_string),
        Peer::Group(g) => g.username().map(str::to_string),
        Peer::Community(_) => None,
        Peer::User(u) => u.username().map(str::to_string),
    };
    Some(ChannelIdentity { id, username })
}
```

Re-export it from `src/telegram/converters.rs` next to the existing `pub use` lines (e.g. `pub use channel::channel_identity;` — match the visibility of the neighboring exports; `pub(crate)` is fine if the others use it).

- [ ] **Step 4: Run converter tests**

Run: `cargo fmt --all && cargo test channel_identity`
Expected: 2 passed.

- [ ] **Step 5: Add the trait method and client wiring**

In `src/telegram/trait_def.rs` (inside the trait, after `get_message_by_id`; add `ChannelIdentity` to the types import at the top):

```rust
    /// Resolve a channel reference (username or numeric-ID string) to its
    /// canonical numeric ID and public username, if any (`None` = no public
    /// username). One peer resolution, no full-info RPC — used by link
    /// generation so public channels get `t.me/<username>` links.
    async fn resolve_channel_identity(&self, channel_ref: &str)
    -> Result<ChannelIdentity, Error>;
```

In `src/telegram/client/resolve.rs`, add to the `impl TelegramClient` block:

```rust
    /// Trait backing for `resolve_channel_identity` (work-order B2): one
    /// resolve, then sentinel-free identity extraction.
    pub(super) async fn resolve_channel_identity_impl(
        &self,
        channel_ref: &str,
    ) -> Result<ChannelIdentity, Error> {
        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }
        let peer = self.resolve_peer(channel_ref).await?;
        channel_identity(&peer).ok_or_else(|| {
            Error::TelegramApi(format!(
                "Failed to read channel identity for {}",
                channel_ref
            ))
        })
    }
```

If `channel_identity` / `ChannelIdentity` are not in scope via `use super::*;`, add `use crate::telegram::converters::channel_identity;` and `use crate::telegram::types::ChannelIdentity;` to `resolve.rs` (or to the parent `client.rs` import block, matching how `convert_message` reaches the ops files).

In `src/telegram/client.rs`, add to the `impl TelegramClientTrait for TelegramClient` block (after `get_message_by_id`):

```rust
    async fn resolve_channel_identity(
        &self,
        channel_ref: &str,
    ) -> Result<crate::telegram::ChannelIdentity, Error> {
        self.resolve_channel_identity_impl(channel_ref).await
    }
```

- [ ] **Step 6: Full suite**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: all pass (mockall regenerates `MockTelegramClientTrait` with `expect_resolve_channel_identity` automatically; existing tests set no expectation on it and never call it, so nothing breaks).

- [ ] **Step 7: Commit**

```bash
git add -A src/
git commit -m "feat: add sentinel-free channel identity resolution"
```

---

### Task 4: Link builder rework (B2 core)

`MessageLink::new` (`src/link.rs:78-94`) gains a `username: Option<&str>` parameter and two fields. The `?single`/`&single` suffix is dropped everywhere — it is a media-group hint, not part of a canonical link. This task keeps the tools' behavior otherwise unchanged (call sites pass `None`); Task 5 threads the real username through.

**Files:**
- Modify: `src/link.rs:70-94` (struct + constructor), plus its test module
- Modify: `src/mcp/server/impl_links.rs:18,46` (call sites → pass `None` for now)
- Modify: `src/mcp/tests/links.rs:41-46,70` (drop `?single`/`&single` from expected strings)

**Interfaces:**
- Consumes: `ChannelId`, `MessageId` (unchanged).
- Produces: `MessageLink { channel_id: ChannelId, message_id: MessageId, https_link: String, tg_protocol_link: String, internal_link: String, is_public: bool }`; `MessageLink::new(channel_id: ChannelId, message_id: MessageId, username: Option<&str>) -> Self`. Task 5 relies on these exact fields.

- [ ] **Step 1: Write the failing builder tests**

In `src/link.rs`'s test module, REPLACE the five `message_link_*` tests (lines ~104-158: `message_link_https_format`, `message_link_tg_protocol_format`, `message_link_stores_ids`, `message_link_serialization`, `message_link_different_ids`) with:

```rust
    #[test]
    fn message_link_public_channel_uses_username_forms() {
        let link = MessageLink::new(
            ChannelId::new(1144180066).unwrap(),
            MessageId::new(610121).unwrap(),
            Some("swodki"),
        );

        assert_eq!(link.https_link, "https://t.me/swodki/610121");
        assert_eq!(link.tg_protocol_link, "tg://resolve?domain=swodki&post=610121");
        assert_eq!(link.internal_link, "https://t.me/c/1144180066/610121");
        assert!(link.is_public);
    }

    #[test]
    fn message_link_private_channel_uses_internal_forms() {
        let link = MessageLink::new(
            ChannelId::new(123456789).unwrap(),
            MessageId::new(42).unwrap(),
            None,
        );

        assert_eq!(link.https_link, "https://t.me/c/123456789/42");
        assert_eq!(
            link.tg_protocol_link,
            "tg://privatepost?channel=123456789&post=42"
        );
        assert_eq!(link.internal_link, "https://t.me/c/123456789/42");
        assert!(!link.is_public);
    }

    #[test]
    fn message_link_never_emits_single_suffix() {
        for username in [Some("swodki"), None] {
            let link = MessageLink::new(
                ChannelId::new(9).unwrap(),
                MessageId::new(1).unwrap(),
                username,
            );
            assert!(!link.https_link.contains("single"));
            assert!(!link.tg_protocol_link.contains("single"));
            assert!(!link.internal_link.contains("single"));
        }
    }

    #[test]
    fn message_link_stores_ids() {
        let link = MessageLink::new(
            ChannelId::new(111).unwrap(),
            MessageId::new(222).unwrap(),
            None,
        );

        assert_eq!(link.channel_id.get(), 111);
        assert_eq!(link.message_id.get(), 222);
    }

    #[test]
    fn message_link_serialization() {
        let link = MessageLink::new(
            ChannelId::new(123).unwrap(),
            MessageId::new(456).unwrap(),
            Some("testchan"),
        );

        let json = serde_json::to_string(&link).expect("serializes");
        assert!(json.contains("\"https_link\":\"https://t.me/testchan/456\""));
        assert!(json.contains("\"internal_link\":\"https://t.me/c/123/456\""));
        assert!(json.contains("\"is_public\":true"));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib link`
Expected: COMPILE ERROR — `new` takes 2 arguments but 3 were supplied / missing fields.

- [ ] **Step 3: Implement the builder**

Replace the `MessageLink` struct and `impl` (lines 70-94) with:

```rust
/// Generated deep links for a Telegram message.
///
/// Public channels (with a username) get shareable `t.me/<username>` /
/// `tg://resolve` forms; private chats fall back to the members-only
/// `t.me/c/…` / `tg://privatepost` forms. `internal_link` always carries the
/// members-only https form. Single shared builder for `generate_message_link`
/// and `open_message_in_telegram` (work-order B2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageLink {
    pub channel_id: ChannelId,
    pub message_id: MessageId,
    pub https_link: String,
    pub tg_protocol_link: String,
    pub internal_link: String,
    pub is_public: bool,
}

impl MessageLink {
    /// Create links for a specific message in a channel.
    ///
    /// `username` is the channel's public username, if any — pass `None` for
    /// private chats (never pass sentinel strings like `"unknown"`).
    pub fn new(channel_id: ChannelId, message_id: MessageId, username: Option<&str>) -> Self {
        let internal_link = format!("https://t.me/c/{}/{}", channel_id, message_id);
        let (https_link, tg_protocol_link, is_public) = match username {
            Some(u) => (
                format!("https://t.me/{}/{}", u, message_id),
                format!("tg://resolve?domain={}&post={}", u, message_id),
                true,
            ),
            None => (
                internal_link.clone(),
                format!("tg://privatepost?channel={}&post={}", channel_id, message_id),
                false,
            ),
        };

        Self {
            channel_id,
            message_id,
            https_link,
            tg_protocol_link,
            internal_link,
            is_public,
        }
    }
}
```

- [ ] **Step 4: Fix the two call sites minimally**

In `src/mcp/server/impl_links.rs`, change both `MessageLink::new(channel_id, message_id)` calls (lines 18 and 46) to:

```rust
        let link = MessageLink::new(channel_id, message_id, None);
```

In `src/mcp/tests/links.rs`, update the expected strings: line 41 → `"https://t.me/c/123456789/42"`, line 45 → `"tg://privatepost?channel=123456789&post=42"`, line 70 → `"https://t.me/c/999/111"`.

- [ ] **Step 5: Full suite**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add -A src/
git commit -m "feat: message links carry public and internal forms"
```

---

### Task 5: Tool rewiring — public links, username input (B2 + D9)

`generate_message_link` and `open_message_in_telegram` resolve the channel through the new trait method (one rate-limiter token — the tools go from offline to one lookup, which is the only way to learn the username), accept usernames as `channel_id` (D9), and return the new additive fields.

**Files:**
- Modify: `src/mcp/server/impl_links.rs` (both `*_impl` bodies)
- Modify: `src/mcp/tools/types/requests.rs:51-79` (`GenerateLinkRequest`, `OpenMessageRequest` descriptions)
- Modify: `src/mcp/tools/types/responses.rs:81-93` (`MessageLinkResponse` + new fields)
- Modify: `src/mcp/server.rs:222-258` (both `#[tool(description = …)]` strings)
- Modify: `src/mcp/tests/links.rs` (rewrite tests for the resolving behavior)
- Modify: `src/mcp/tools/types/tests/responses_tests.rs:28` area (fixture gains the two new fields)

**Interfaces:**
- Consumes: `resolve_channel_identity` (Task 3), `MessageLink::new(…, Option<&str>)` (Task 4), existing `parse_message_id` (`src/mcp/tools/helpers.rs`), existing `json_response`.
- Produces: `MessageLinkResponse` with `internal_link: String` and `is_public: bool`; both tools accept `@username`, `username`, or numeric-string `channel_id`.

- [ ] **Step 1: Rewrite the failing tests**

Replace the contents of `src/mcp/tests/links.rs` below the imports with the tests below, and extend the imports with:

```rust
use crate::error::Error;
use crate::telegram::types::{ChannelId, ChannelIdentity};
```

```rust
fn identity(id: i64, username: Option<&str>) -> ChannelIdentity {
    ChannelIdentity {
        id: ChannelId::new(id).expect("valid test id"),
        username: username.map(str::to_string),
    }
}

// ============================================================================
// generate_message_link tests
// ============================================================================

#[tokio::test]
async fn generate_message_link_public_channel_returns_public_forms() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .return_once(|_| Ok(identity(1144180066, Some("swodki"))));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GenerateLinkRequest {
        channel_id: "1144180066".to_string(),
        message_id: 610121,
        include_tg_protocol: None, // defaults to true
    };

    let result = server
        .generate_message_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    let response: MessageLinkResponse =
        serde_json::from_str(&result.expect("tool must succeed")).expect("valid json");
    assert_eq!(response.channel_id, "1144180066");
    assert_eq!(response.message_id, 610121);
    assert_eq!(response.https_link, "https://t.me/swodki/610121");
    assert_eq!(
        response.tg_protocol_link.as_deref(),
        Some("tg://resolve?domain=swodki&post=610121")
    );
    assert_eq!(response.internal_link, "https://t.me/c/1144180066/610121");
    assert!(response.is_public);
}

#[tokio::test]
async fn generate_message_link_accepts_username_input() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .withf(|channel_ref| channel_ref == "swodki")
        .return_once(|_| Ok(identity(1144180066, Some("swodki"))));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GenerateLinkRequest {
        channel_id: "swodki".to_string(),
        message_id: 610121,
        include_tg_protocol: None,
    };

    let result = server
        .generate_message_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    let response: MessageLinkResponse =
        serde_json::from_str(&result.expect("username input must work")).expect("valid json");
    assert_eq!(response.https_link, "https://t.me/swodki/610121");
    assert_eq!(response.channel_id, "1144180066"); // canonical numeric id
}

#[tokio::test]
async fn generate_message_link_private_chat_returns_internal_forms() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .return_once(|_| Ok(identity(521440428, None)));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GenerateLinkRequest {
        channel_id: "521440428".to_string(),
        message_id: 7,
        include_tg_protocol: None,
    };

    let result = server
        .generate_message_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    let response: MessageLinkResponse =
        serde_json::from_str(&result.expect("tool must succeed")).expect("valid json");
    assert_eq!(response.https_link, "https://t.me/c/521440428/7");
    assert_eq!(
        response.tg_protocol_link.as_deref(),
        Some("tg://privatepost?channel=521440428&post=7")
    );
    assert!(!response.is_public);
}

#[tokio::test]
async fn generate_message_link_without_tg_protocol() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .return_once(|_| Ok(identity(999, None)));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GenerateLinkRequest {
        channel_id: "999".to_string(),
        message_id: 111,
        include_tg_protocol: Some(false),
    };

    let result = server
        .generate_message_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    let response: MessageLinkResponse =
        serde_json::from_str(&result.expect("tool must succeed")).expect("valid json");
    assert_eq!(response.https_link, "https://t.me/c/999/111");
    assert!(response.tg_protocol_link.is_none());
}

#[tokio::test]
async fn generate_message_link_unknown_channel_errors() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .return_once(|_| Err(Error::InvalidInput("Channel not found: @nope".to_string())));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GenerateLinkRequest {
        channel_id: "@nope".to_string(),
        message_id: 42,
        include_tg_protocol: None,
    };

    let result = server
        .generate_message_link(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_err());
    assert!(result.expect_err("must be error").contains("Channel not found"));
}

// ============================================================================
// open_message_in_telegram tests
// ============================================================================

#[tokio::test]
async fn open_message_in_telegram_unknown_channel_errors() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .return_once(|_| Err(Error::InvalidInput("Channel not found: invalid".to_string())));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = OpenMessageRequest {
        channel_id: "invalid".to_string(),
        message_id: 42,
        use_tg_protocol: None,
    };

    let result = server
        .open_message_in_telegram(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    assert!(result.is_err());
    assert!(result.expect_err("must be error").contains("Channel not found"));
}

#[tokio::test]
async fn open_message_in_telegram_uses_public_tg_form_by_default() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .return_once(|_| Ok(identity(1144180066, Some("swodki"))));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = OpenMessageRequest {
        channel_id: "swodki".to_string(),
        message_id: 42,
        use_tg_protocol: None, // defaults to true
    };

    let result = server
        .open_message_in_telegram(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    let response: OpenMessageResponse =
        serde_json::from_str(&result.expect("tool must succeed")).expect("valid json");
    assert_eq!(response.link_used, "tg://resolve?domain=swodki&post=42");
}

#[tokio::test]
async fn open_message_in_telegram_uses_https_when_requested() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_resolve_channel_identity()
        .return_once(|_| Ok(identity(123456, None)));
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = OpenMessageRequest {
        channel_id: "123456".to_string(),
        message_id: 42,
        use_tg_protocol: Some(false),
    };

    let result = server
        .open_message_in_telegram(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    let response: OpenMessageResponse =
        serde_json::from_str(&result.expect("tool must succeed")).expect("valid json");
    assert_eq!(response.link_used, "https://t.me/c/123456/42");
}
```

This DELETES the old `generate_message_link_returns_both_formats`, `generate_message_link_invalid_channel_id`, and `open_message_in_telegram_invalid_channel_id` tests (strict-numeric parsing is gone — that's D9) and replaces them with resolution-based equivalents. Note: the two `open_…` success tests execute the real `open` command on macOS (existing suite behavior — the old tests did too); they assert only `link_used`, not `success`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib links`
Expected: COMPILE ERROR — `MessageLinkResponse` has no `internal_link`/`is_public`.

- [ ] **Step 3: Update the response and request structs**

`src/mcp/tools/types/responses.rs` — replace `MessageLinkResponse` (lines 81-93) with:

```rust
pub struct MessageLinkResponse {
    #[schemars(description = "Canonical numeric channel ID")]
    pub channel_id: String,

    #[schemars(description = "Message ID")]
    pub message_id: i64,

    #[schemars(
        description = "Best shareable HTTPS link: https://t.me/{username}/{message_id} for public channels, https://t.me/c/{channel_id}/{message_id} for private chats"
    )]
    pub https_link: String,

    #[schemars(
        description = "tg:// protocol link (tg://resolve for public channels, tg://privatepost for private chats)"
    )]
    pub tg_protocol_link: Option<String>,

    #[schemars(description = "Members-only https://t.me/c/… form (always present)")]
    pub internal_link: String,

    #[schemars(description = "Whether the channel has a public username")]
    pub is_public: bool,
}
```

(Keep the struct's existing derives/attributes exactly as they are.)

`src/mcp/tools/types/requests.rs` — in BOTH `GenerateLinkRequest` (line 52) and `OpenMessageRequest` (line 68), change the `channel_id` description from `"Numeric channel ID"` to:

```rust
    #[schemars(description = "Channel ID or username (e.g. @channelname or 1234567890)")]
```

- [ ] **Step 4: Rewrite the two impl bodies**

Replace `src/mcp/server/impl_links.rs` contents (keeping the file docstring and the surrounding `impl` frame) with:

```rust
impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) async fn generate_message_link_impl(
        &self,
        request: GenerateLinkRequest,
    ) -> Result<String, String> {
        let message_id = parse_message_id(request.message_id)?;

        // One rate-limited peer resolution: the username is required to emit
        // the shareable t.me/<username> form for public channels (B2), and
        // it lets channel_id be a username too (D9).
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        let identity = self
            .telegram_client
            .resolve_channel_identity(&request.channel_id)
            .await
            .map_err(|e| e.to_string())?;

        let link = MessageLink::new(identity.id, message_id, identity.username.as_deref());
        let include_tg = request.include_tg_protocol.unwrap_or(true);

        let response = MessageLinkResponse {
            channel_id: identity.id.to_string(),
            message_id: request.message_id,
            https_link: link.https_link,
            tg_protocol_link: if include_tg {
                Some(link.tg_protocol_link)
            } else {
                None
            },
            internal_link: link.internal_link,
            is_public: link.is_public,
        };

        json_response(&response)
    }

    pub(super) async fn open_message_in_telegram_impl(
        &self,
        request: OpenMessageRequest,
    ) -> Result<String, String> {
        let message_id = parse_message_id(request.message_id)?;

        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        let identity = self
            .telegram_client
            .resolve_channel_identity(&request.channel_id)
            .await
            .map_err(|e| e.to_string())?;

        let link = MessageLink::new(identity.id, message_id, identity.username.as_deref());

        // Choose link type (defaults to tg:// protocol)
        let use_tg = request.use_tg_protocol.unwrap_or(true);
        let link_to_open = if use_tg {
            &link.tg_protocol_link
        } else {
            &link.https_link
        };

        // Execute open command (macOS-specific)
        #[cfg(target_os = "macos")]
        let result = tokio::process::Command::new("open")
            .arg(link_to_open)
            .output()
            .await;

        #[cfg(not(target_os = "macos"))]
        let result: Result<std::process::Output, std::io::Error> = Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "open_message_in_telegram is only supported on macOS",
        ));

        let response = match result {
            Ok(output) => {
                let success = output.status.success();
                OpenMessageResponse {
                    success,
                    message: if success {
                        "Message opened in Telegram".to_string()
                    } else {
                        format!("Failed to open: {:?}", output.status)
                    },
                    link_used: link_to_open.clone(),
                    app_opened: success,
                }
            }
            Err(e) => OpenMessageResponse {
                success: false,
                message: format!("Failed to execute open command: {}", e),
                link_used: link_to_open.clone(),
                app_opened: false,
            },
        };

        json_response(&response)
    }
}
```

If `parse_channel_id` becomes unused after this (check with `cargo clippy`), leave the helper in place if other tools still use it (`get_message_media` etc. use flexible identifiers through other paths — verify before deleting; deleting is NOT required for this task).

- [ ] **Step 5: Update tool descriptions and the response fixture**

`src/mcp/server.rs:223` →

```rust
    #[tool(
        description = "Generate shareable deep links for a Telegram message (accepts channel ID or username; public channels get https://t.me/<username> links)"
    )]
```

`src/mcp/server.rs:242` →

```rust
    #[tool(
        description = "Open a specific message in Telegram Desktop application (macOS only; accepts channel ID or username)"
    )]
```

`src/mcp/tools/types/tests/responses_tests.rs` (~line 28): add the two new fields to the `MessageLinkResponse` fixture, e.g. `internal_link: "https://t.me/c/123/456".to_string(), is_public: false,` — and fix any assertion counts in that test accordingly.

- [ ] **Step 6: Full suite**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add -A src/
git commit -m "fix: emit public t.me links and accept usernames in link tools"
```

---

### Task 6: Schema inlining + dangling-$ref guard (B3)

The published `inputSchema` for `search_messages`/`get_recent_messages` references `#/$defs/MediaFilter` with no `$defs` block, so schema-following clients can never construct a `media_filter` value. Fix by inlining the enum; lock with a schema-walk test that runs in the normal `cargo test` gate.

**Files:**
- Create: `src/mcp/tests/schema_integrity.rs`
- Modify: `src/mcp/tests.rs` (add the `#[path]` module declaration alongside the existing ones)
- Modify: `src/telegram/types/media.rs:80` area (`MediaFilter` enum attributes)

**Interfaces:**
- Consumes: `McpServer::tools_list_result()` (`pub(crate)`, `src/mcp/server.rs:109`) — returns `ListToolsResult` whose `.tools: Vec<Tool>` each carry `input_schema: Arc<JsonObject>`.
- Produces: CI-enforced invariant — no tool schema contains a `$ref` without a resolvable local `$defs` target, and `media_filter`'s variants are inline.

- [ ] **Step 1: Write the failing tests**

Create `src/mcp/tests/schema_integrity.rs`:

```rust
//! Published tool schemas must be self-contained (work-order B3).
//!
//! A dangling `#/$defs/MediaFilter` $ref shipped in 0.13.0 made media_filter
//! uncallable: schema-following clients could not construct any valid value.

use crate::mcp::server::McpServer;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use serde_json::Value;
use std::sync::Arc;

fn test_server() -> McpServer<MockTelegramClientTrait, MockRateLimiterTrait> {
    McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    )
}

/// Collect every `$ref` string value anywhere in a schema tree.
fn collect_refs(value: &Value, refs: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref") {
                refs.push(reference.clone());
            }
            for nested in map.values() {
                collect_refs(nested, refs);
            }
        }
        Value::Array(items) => {
            for nested in items {
                collect_refs(nested, refs);
            }
        }
        _ => {}
    }
}

#[test]
fn every_tool_schema_ref_resolves_locally() {
    let tools = test_server().tools_list_result().tools;
    assert_eq!(tools.len(), 12, "expected all 12 tools to be listed");

    for tool in &tools {
        let schema = Value::Object((*tool.input_schema).clone());
        let mut refs = Vec::new();
        collect_refs(&schema, &mut refs);

        for reference in refs {
            let target = reference.strip_prefix("#/$defs/").unwrap_or_else(|| {
                panic!("tool {}: non-local $ref {}", tool.name, reference)
            });
            let defs = schema.get("$defs").unwrap_or_else(|| {
                panic!("tool {}: $ref {} but no $defs block", tool.name, reference)
            });
            assert!(
                defs.get(target).is_some(),
                "tool {}: $ref {} does not resolve",
                tool.name,
                reference
            );
        }
    }
}

#[test]
fn media_filter_enum_is_inline_with_no_refs() {
    let tools = test_server().tools_list_result().tools;

    for tool_name in ["search_messages", "get_recent_messages"] {
        let tool = tools
            .iter()
            .find(|t| t.name == tool_name)
            .unwrap_or_else(|| panic!("{tool_name} tool must exist"));
        let schema = Value::Object((*tool.input_schema).clone());

        let mut refs = Vec::new();
        collect_refs(&schema, &mut refs);
        assert!(
            refs.is_empty(),
            "{tool_name}: schema must be fully inline, found $refs: {refs:?}"
        );

        let serialized = serde_json::to_string(&schema).expect("schema serializes");
        for variant in [
            "photo",
            "video",
            "photo_video",
            "document",
            "audio",
            "voice",
            "video_note",
            "gif",
            "url",
            "pinned",
        ] {
            assert!(
                serialized.contains(variant),
                "{tool_name}: media_filter variant {variant} missing from schema"
            );
        }
    }
}
```

Register it in `src/mcp/tests.rs` (alphabetical position, matching the existing style):

```rust
#[path = "tests/schema_integrity.rs"]
mod schema_integrity;
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test schema_integrity`
Expected: FAIL — at minimum `media_filter_enum_is_inline_with_no_refs` fails on the `$ref`; `every_tool_schema_ref_resolves_locally` fails too if rmcp drops the `$defs` block (the audit's observation). If BOTH pass out of the box, STOP and investigate how the published schema differs from `tools_list_result()` before proceeding — the bug was observed on a live server.

- [ ] **Step 3: Inline the enum**

In `src/telegram/types/media.rs`, on the `MediaFilter` enum (line ~80), add the schemars attribute below the serde one:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub enum MediaFilter {
```

(Only the `#[schemars(inline)]` line is new — keep the existing derives exactly.) If `#[schemars(inline)]` does not exist in the pinned schemars v1 version (compile error), the fallback is `#[schemars(with = "...")]`-free manual inlining via `SchemaSettings` at the generation site — but try the attribute first; it is the documented schemars v1 mechanism.

- [ ] **Step 4: Run to verify green**

Run: `cargo fmt --all && cargo test schema_integrity`
Expected: both tests pass — the enum now appears inline (as `"enum": […]` or `"anyOf"` with inline `"const"` values; the variant-string assertions cover either encoding).

- [ ] **Step 5: Full suite**

Run: `cargo clippy -- -D warnings && cargo test`
Expected: all pass (`deserialize_optional_media_filter` behavior is untouched — this is schema-only).

- [ ] **Step 6: Commit**

```bash
git add -A src/
git commit -m "fix: inline MediaFilter enum in tool schemas; guard against dangling \$ref"
```

---

### Task 7: Docs, stale-example sweep, gate, PR

**Files:**
- Modify: `docs/tasklist.md` (progress table + new phase row)
- Modify: `docs/memory.md` (journal entry)
- Possibly modify: any `*.md` with stale link examples

**Interfaces:** none — documentation and delivery.

- [ ] **Step 1: Sweep docs for stale link examples**

Run: `grep -rn "privatepost\|?single\|Numeric channel ID" README.md docs/ --include="*.md" | grep -v superpowers | grep -v work-order`
Update any hit that documents `generate_message_link`/`open_message_in_telegram` behavior to the new forms (public `t.me/<username>` + `internal_link`/`is_public` fields). Do NOT edit `docs/telegram-mcp-0.13.0-work-order.md` (audit record) or the spec/plan files.

- [ ] **Step 2: Update project tracking**

In `docs/tasklist.md`: add phase 28 to the Progress Report table —

```markdown
| 28 | Correctness core (work-order B1/B2/B3/D9) | ✅ Complete | NNN | MessageEmpty → not-found across all fetch paths; shared link builder emits public t.me/username + internal_link/is_public; media_filter enum inlined + $ref schema guard |
```

Replace `NNN` with the total test count printed by the Step 3 `cargo test` run (it was 456 before this branch).

Update the "Overall Progress" line to 28/28. In `docs/memory.md`, append a dated journal entry (2026-08-10) summarizing: the B1 root cause (grammers wraps `MessageEmpty` instead of returning `None`; guard at `src/telegram/client/guard.rs`), the sentinel hazard that motivated `ChannelIdentity` (never build links from `Channel.username`), and the B3 lesson (assert schema self-containment in tests; `#[schemars(inline)]` on enums referenced from request structs).

- [ ] **Step 3: Full pre-merge gate**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: clean. Also run `cargo test config -- --test-threads=1` to double-check the serial config tests.

- [ ] **Step 4: Commit docs and push**

```bash
git add docs/tasklist.md docs/memory.md README.md docs/
git commit -m "docs: record correctness-core phase in tasklist and journal"
git push -u origin fix/correctness-core
```

- [ ] **Step 5: Open the PR**

```bash
gh pr create --title "fix: correctness core — B1 fabricated messages, B2+D9 public links, B3 schema refs" --body "$(cat <<'EOF'
Implements docs/superpowers/specs/2026-08-10-correctness-core-design.md (work-order sub-project 1 of 5).

- **B1** — deleted/never-existed message ids now error (`Message {id} not found or deleted in channel {ref}`) instead of fabricating an epoch-timestamp message; guard applied to get_message_by_link, get_message_media, transcribe_voice_message fetch paths and convert_message.
- **B2** — generate_message_link / open_message_in_telegram emit public `https://t.me/<username>` + `tg://resolve` forms for public channels via one shared builder; new additive `internal_link` + `is_public` response fields; `?single` suffix dropped.
- **D9** — both link tools accept `channel_id` as username or numeric ID (one rate-limited resolve).
- **B3** — `MediaFilter` enum inlined into published schemas; new schema-walk test asserts no tool schema contains an unresolvable `$ref`.

Release: will ship as v0.13.1 after merge (separate release flow).

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 6: Request code review**

Per the repo workflow, request review before merging (superpowers:requesting-code-review). After merge, run the `release` skill for v0.13.1.

---

## Post-merge live QA (not automatable in CI — needs the live account)

Verify against the live server, mirroring the audit's fixtures:

1. `get_message_by_link("https://t.me/swodki/609784")` (deleted) and `…/999999999` (never existed) → both error; no epoch timestamp anywhere.
2. `get_message_media` / `transcribe_voice_message` on `(swodki, 609784)` → not-found error, not a media-type error.
3. `generate_message_link(channel_id="1144180066", message_id=610121)` → `https://t.me/swodki/610121`, `is_public: true`; `channel_id="swodki"` works; a private chat id (e.g. `521440428`) → `t.me/c/` forms.
4. `search_messages(media_filter="voice")` and `get_recent_messages(media_filter="photo")` validate and execute end to end (work-order §5 untested item — runtime behavior was unreachable while the schema was broken).
