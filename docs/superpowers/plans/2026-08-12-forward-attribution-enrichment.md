# Forward Attribution Enrichment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Populate `forwarded_from.channel_name` / `channel_username` / `sender_name` (+ new `post_author`) on `search_messages` and `get_recent_messages` results from the MTProto response envelope's `chats`/`users` arrays — zero additional network calls.

**Architecture:** The pinned grammers rev keeps the per-message entity map (`Message.peers`) `pub(crate)`, so the two affected fetch paths drop to raw TL invocations of the *same* requests grammers' iterators issue (`messages.GetHistory` / `messages.Search` / `messages.SearchGlobal`), keeping the envelope. A pure `EntityLookup` maps raw `chats`+`users` to names; `convert_message` splits into a raw core (`convert_raw_message`) plus a compat wrapper for unchanged call paths. Spec: `docs/superpowers/specs/2026-08-12-forward-attribution-enrichment-design.md`.

**Tech Stack:** Rust nightly (2024 edition), grammers pinned rev `9fef0bae` (Codeberg), `tl` types re-exported as `grammers_client::tl`, mockall for trait mocks.

## Global Constraints

- Zero additional network calls: conversion and enrichment must never invoke RPC; the raw pagers issue exactly the requests the grammers iterators issued before.
- Existing JSON field names, types, and shape unchanged; new fields optional with `#[serde(skip_serializing_if = "Option::is_none", default)]`.
- Entity-map miss → ids-only `forwarded_from`, never an error, never a placeholder name, never a resolve call.
- Pre-commit gate for EVERY task: `cargo fmt --all && cargo clippy -- -D warnings && cargo test` (config tests run serial: `cargo test config -- --test-threads=1` if touched).
- Never `unwrap()` in production code; `expect()` only in tests.
- TDD: failing test first for every behavior change.
- All work on branch `feature/forward-attribution-enrichment` (create in Task 1 Step 0: `git checkout -b feature/forward-attribution-enrichment`).
- grammers API facts verified against the pinned checkout at `~/.cargo/git/checkouts/grammers-8937e3b5288aa015/9fef0ba/` — consult it, never the stale GitHub mirror.

## File Structure

- `src/telegram/envelope.rs` (new): `EntityInfo`, `EntityLookup` — pure envelope entity map.
- `src/telegram/client/raw_pager.rs` (new): `RawPage`, `RawHistoryPager`, `RawChannelSearchPager`, `RawGlobalSearchPager`.
- `src/telegram/types/entities.rs`: `ForwardInfo` + `post_author`.
- `src/telegram/converters/message.rs`: `extract_forward_info(header, entities)`, `convert_raw_message` core, `convert_message` wrapper, `raw_*` field helpers.
- `src/telegram/converters/media.rs`: `matches_media_filter_raw` sharing a core with `matches_media_filter`.
- `src/telegram/converters.rs`, `src/telegram.rs`, `src/telegram/client.rs`: export wiring.
- `src/telegram/client/ops_history.rs`, `src/telegram/client/ops_search.rs`: iterator → raw pager swap.
- `src/test_helpers.rs`: raw TL fixture builders + `post_author` on the forward fixture.
- `src/mcp/tests/search.rs`: enriched-forward serialization + no-resolve guard test.
- `README.md`, `CHANGELOG.md`, `docs/memory.md`, `docs/tasklist.md`: docs.

---

### Task 1: `ForwardInfo.post_author` domain field

**Files:**
- Modify: `src/telegram/types/entities.rs:66-89` (ForwardInfo + doc comment)
- Modify: `src/test_helpers.rs:90-112` (`create_test_message_with_forward`)
- Modify: `src/mcp/tests/search.rs:445-452` (struct literal gains field)
- Modify: `src/telegram/converters/message.rs:52-62` (struct literal gains field)

**Interfaces:**
- Produces: `ForwardInfo { channel_id, channel_name, channel_username, sender_name, post_author: Option<String>, original_date, original_message_id }` — later tasks construct this shape.

- [ ] **Step 0: Create the feature branch**

```bash
git checkout -b feature/forward-attribution-enrichment
```

- [ ] **Step 1: Write the failing serialization test**

In `src/telegram/types/entities.rs`, add at the bottom (file has no test module yet):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_info_serializes_post_author_and_skips_absent_fields() {
        let info = ForwardInfo {
            channel_id: Some(ChannelId::new(1783384254).expect("valid id")),
            channel_name: None,
            channel_username: None,
            sender_name: None,
            post_author: Some("Иван Петров".to_string()),
            original_date: None,
            original_message_id: None,
        };
        let json = serde_json::to_value(&info).expect("serializes");
        assert_eq!(json["post_author"], "Иван Петров");
        // Absent optionals must be skipped, not null (backward-compatible shape).
        assert!(json.get("channel_name").is_none());
        assert!(json.get("sender_name").is_none());

        let bare = ForwardInfo {
            channel_id: None,
            channel_name: None,
            channel_username: None,
            sender_name: None,
            post_author: None,
            original_date: None,
            original_message_id: None,
        };
        let json = serde_json::to_value(&bare).expect("serializes");
        assert!(json.get("post_author").is_none());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test forward_info_serializes_post_author`
Expected: COMPILE FAIL — `ForwardInfo` has no field `post_author`.

- [ ] **Step 3: Add the field and rewrite the stale doc comment**

In `src/telegram/types/entities.rs`, replace the ForwardInfo doc comment (lines 66-74, the "intentionally never populated" text) and add the field after `sender_name`:

```rust
/// Attribution for a forwarded message.
///
/// `channel_name` / `channel_username` / `sender_name` are resolved from the
/// same response envelope the message arrived in (its `chats` + `users`
/// arrays) — never from an extra resolve call. When the envelope does not
/// contain the source peer, the ids-only form is emitted instead; nothing is
/// fabricated (zero-extra-call enrichment invariant).
```

```rust
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sender_name: Option<String>,
    /// Author signature on signed channel posts (fwd header `post_author`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post_author: Option<String>,
```

- [ ] **Step 4: Fix the three existing struct literals (compiler-driven)**

Add `post_author: None,` to the `ForwardInfo` literals in:
- `src/telegram/converters/message.rs` (`extract_forward_info`)
- `src/test_helpers.rs` (`create_test_message_with_forward`)
- `src/mcp/tests/search.rs` (`search_messages_serializes_enrichment_fields`)

- [ ] **Step 5: Run gate**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: PASS (new test green, no other changes).

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat: add post_author to ForwardInfo (additive, skip-when-absent)"
```

---

### Task 2: `EntityLookup` envelope entity map

**Files:**
- Create: `src/telegram/envelope.rs`
- Modify: `src/telegram.rs:1-8` (add `pub(crate) mod envelope;`)
- Modify: `src/test_helpers.rs` (raw TL fixture builders)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces (used by Tasks 3, 4, 6, 7, 8):
  - `pub(crate) struct EntityInfo { pub display_name: Option<String>, pub first_name: Option<String>, pub username: Option<String> }` with `pub(crate) fn sender_name(&self) -> Option<String>` (= `first_name` falling back to `display_name`)
  - `pub(crate) struct EntityLookup` with:
    - `pub(crate) fn empty() -> Self`
    - `pub(crate) fn from_envelope(chats: &[tl::enums::Chat], users: &[tl::enums::User]) -> Self`
    - `pub(crate) fn get(&self, peer: &tl::enums::Peer) -> Option<&EntityInfo>`
    - `pub(crate) fn insert_peer(&mut self, peer: &grammers_client::peer::Peer)`
- Test-helper fixtures (used by Tasks 3, 4, 6): `pub fn raw_tl_channel(id: i64, title: &str, username: Option<&str>) -> tl::types::Channel`, `pub fn raw_tl_user(id: i64, first: Option<&str>, last: Option<&str>, username: Option<&str>) -> tl::types::User`

**Key grammers facts (verified in pinned rev):**
- `grammers_session::types::PeerId` is `Copy + Hash + Eq`; public constructors `PeerId::user_unchecked(i64)`, `PeerId::chat_unchecked(i64)`, `PeerId::channel_unchecked(i64)`; `impl From<tl::enums::Peer> for PeerId` maps `PeerUser`/`PeerChat`/`PeerChannel` to the matching namespace.
- `tl::enums::Chat` variants: `Empty`, `Chat`, `Forbidden`, `Channel`, `ChannelForbidden`, `Community`, `CommunityForbidden`. **Community is channel-namespace** (`Community::id()` uses `channel_unchecked` — verified in `grammers-client/src/peer/community.rs`).
- `tl::enums::User` variants: `Empty`, `User`.
- High-level `grammers_client::peer::Peer` variants: `User`, `Group`, `Channel`, `Community`; `peer.id() -> PeerId`; per-kind accessors: `User::{first_name(), last_name(), username()}`, `Group::{title() -> Option<&str>, username()}`, `Channel::{title() -> &str, username()}`, `Community::title() -> &str` (no username accessor).

- [ ] **Step 1: Add raw TL fixture builders to `src/test_helpers.rs`**

Model on the existing `tl::types::Channel` literal in `src/telegram/converters/message.rs` tests (`public_channel_peer`). Add `use grammers_client::tl;` to the imports.

```rust
/// Raw TL channel for envelope fixtures. All flag booleans false, all
/// remaining optionals None (compiler-guided — copy the full field list from
/// `public_channel_peer` in converters/message.rs tests).
pub fn raw_tl_channel(id: i64, title: &str, username: Option<&str>) -> tl::types::Channel {
    tl::types::Channel {
        id,
        access_hash: Some(0),
        title: title.to_string(),
        username: username.map(|u| u.to_string()),
        photo: tl::enums::ChatPhoto::Empty,
        date: 0,
        // every other bool flag: false; every other Option: None
        ..
    }
}

/// Raw TL user for envelope fixtures. Same convention: remaining flags false,
/// remaining optionals None (the `self` TL field is named `is_self` in Rust).
pub fn raw_tl_user(
    id: i64,
    first: Option<&str>,
    last: Option<&str>,
    username: Option<&str>,
) -> tl::types::User {
    tl::types::User {
        id,
        access_hash: Some(0),
        first_name: first.map(|s| s.to_string()),
        last_name: last.map(|s| s.to_string()),
        username: username.map(|s| s.to_string()),
        ..
    }
}
```

(`..` is not valid without `Default` — spell out every remaining field literally, exactly as `public_channel_peer` does. The compiler lists what's missing; set bools to `false`, Options to `None`.)

- [ ] **Step 2: Write the failing tests**

Create `src/telegram/envelope.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{raw_tl_channel, raw_tl_user};
    use grammers_client::tl;

    fn channel_peer(id: i64) -> tl::enums::Peer {
        tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: id })
    }
    fn user_peer(id: i64) -> tl::enums::Peer {
        tl::enums::Peer::User(tl::types::PeerUser { user_id: id })
    }
    fn chat_peer(id: i64) -> tl::enums::Peer {
        tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: id })
    }

    #[test]
    fn resolves_channel_title_and_username_from_envelope() {
        let chats = vec![tl::enums::Chat::Channel(raw_tl_channel(
            1783384254,
            "Военкор",
            Some("voenkor_ru"),
        ))];
        let lookup = EntityLookup::from_envelope(&chats, &[]);
        let info = lookup.get(&channel_peer(1783384254)).expect("hit");
        assert_eq!(info.display_name.as_deref(), Some("Военкор"));
        assert_eq!(info.username.as_deref(), Some("voenkor_ru"));
        assert_eq!(info.first_name, None);
    }

    #[test]
    fn channel_without_username_resolves_title_only() {
        let chats = vec![tl::enums::Chat::Channel(raw_tl_channel(77, "Приватный", None))];
        let lookup = EntityLookup::from_envelope(&chats, &[]);
        let info = lookup.get(&channel_peer(77)).expect("hit");
        assert_eq!(info.display_name.as_deref(), Some("Приватный"));
        assert_eq!(info.username, None);
    }

    #[test]
    fn resolves_user_full_name_and_first_name() {
        let users = vec![tl::enums::User::User(raw_tl_user(
            42,
            Some("Иван"),
            Some("Петров"),
            Some("ivanp"),
        ))];
        let lookup = EntityLookup::from_envelope(&[], &users);
        let info = lookup.get(&user_peer(42)).expect("hit");
        assert_eq!(info.display_name.as_deref(), Some("Иван Петров"));
        assert_eq!(info.first_name.as_deref(), Some("Иван"));
        assert_eq!(info.username.as_deref(), Some("ivanp"));
        assert_eq!(info.sender_name().as_deref(), Some("Иван"));
    }

    #[test]
    fn user_without_last_name_uses_first_name_as_display() {
        let users = vec![tl::enums::User::User(raw_tl_user(7, Some("Анна"), None, None))];
        let lookup = EntityLookup::from_envelope(&[], &users);
        let info = lookup.get(&user_peer(7)).expect("hit");
        assert_eq!(info.display_name.as_deref(), Some("Анна"));
    }

    #[test]
    fn same_bare_id_in_different_namespaces_does_not_collide() {
        let chats = vec![tl::enums::Chat::Channel(raw_tl_channel(5, "Канал", None))];
        let users = vec![tl::enums::User::User(raw_tl_user(5, Some("Юзер"), None, None))];
        let lookup = EntityLookup::from_envelope(&chats, &users);
        assert_eq!(
            lookup.get(&channel_peer(5)).expect("channel").display_name.as_deref(),
            Some("Канал")
        );
        assert_eq!(
            lookup.get(&user_peer(5)).expect("user").first_name.as_deref(),
            Some("Юзер")
        );
    }

    #[test]
    fn miss_and_empty_variants_return_none() {
        let chats = vec![tl::enums::Chat::Empty(tl::types::ChatEmpty { id: 9 })];
        let users = vec![tl::enums::User::Empty(tl::types::UserEmpty { id: 9 })];
        let lookup = EntityLookup::from_envelope(&chats, &users);
        assert!(lookup.get(&chat_peer(9)).is_none());
        assert!(lookup.get(&user_peer(9)).is_none());
        assert!(EntityLookup::empty().get(&channel_peer(1)).is_none());
    }
}
```

- [ ] **Step 3: Wire the module and run tests to verify they fail**

Add `pub(crate) mod envelope;` to `src/telegram.rs` (after `pub(crate) mod albums;`).
Run: `cargo test envelope`
Expected: COMPILE FAIL — `EntityLookup` not found.

- [ ] **Step 4: Implement `EntityLookup`**

Top of `src/telegram/envelope.rs`:

```rust
//! Entity map built from a raw MTProto response envelope.
//!
//! History/search responses (`messages.Messages` family) carry `chats` and
//! `users` arrays with every entity their messages reference — including
//! forward sources the account is not subscribed to. This module turns those
//! arrays into a pure, client-free lookup used to attribute forwards (and
//! resolve senders) with zero extra network calls. Required because the
//! pinned grammers rev keeps `Message.peers` crate-private.

use grammers_client::tl;
use grammers_session::types::PeerId;
use std::collections::HashMap;

/// Display data for one peer out of a response envelope.
#[derive(Debug, Clone)]
pub(crate) struct EntityInfo {
    /// Title for chats/channels; "first last" for users.
    pub display_name: Option<String>,
    /// Users only — message-level `sender_name` has always been first-name-only.
    pub first_name: Option<String>,
    /// Public @username, without the prefix.
    pub username: Option<String>,
}

impl EntityInfo {
    /// Name for message-level `sender_name` (first name for users, title otherwise).
    pub(crate) fn sender_name(&self) -> Option<String> {
        self.first_name.clone().or_else(|| self.display_name.clone())
    }
}

/// Pure lookup from a raw peer to its envelope entity.
///
/// Keyed by [`PeerId`], which bit-packs the peer namespace — a user and a
/// channel sharing a bare id never collide.
#[derive(Debug, Default)]
pub(crate) struct EntityLookup {
    map: HashMap<PeerId, EntityInfo>,
}

impl EntityLookup {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    /// Build from a response envelope's `chats` + `users` arrays.
    pub(crate) fn from_envelope(chats: &[tl::enums::Chat], users: &[tl::enums::User]) -> Self {
        let mut map = HashMap::new();
        for chat in chats {
            match chat {
                tl::enums::Chat::Channel(c) => {
                    map.insert(
                        PeerId::channel_unchecked(c.id),
                        EntityInfo {
                            display_name: Some(c.title.clone()),
                            first_name: None,
                            username: c.username.clone(),
                        },
                    );
                }
                tl::enums::Chat::ChannelForbidden(c) => {
                    map.insert(
                        PeerId::channel_unchecked(c.id),
                        EntityInfo {
                            display_name: Some(c.title.clone()),
                            first_name: None,
                            username: None,
                        },
                    );
                }
                // Communities live in the channel namespace (Community::id()
                // uses channel_unchecked in the pinned grammers rev).
                tl::enums::Chat::Community(c) => {
                    map.insert(
                        PeerId::channel_unchecked(c.id),
                        EntityInfo {
                            display_name: Some(c.title.clone()),
                            first_name: None,
                            username: None,
                        },
                    );
                }
                tl::enums::Chat::CommunityForbidden(c) => {
                    map.insert(
                        PeerId::channel_unchecked(c.id),
                        EntityInfo {
                            display_name: Some(c.title.clone()),
                            first_name: None,
                            username: None,
                        },
                    );
                }
                tl::enums::Chat::Chat(c) => {
                    map.insert(
                        PeerId::chat_unchecked(c.id),
                        EntityInfo {
                            display_name: Some(c.title.clone()),
                            first_name: None,
                            username: None,
                        },
                    );
                }
                tl::enums::Chat::Forbidden(c) => {
                    map.insert(
                        PeerId::chat_unchecked(c.id),
                        EntityInfo {
                            display_name: Some(c.title.clone()),
                            first_name: None,
                            username: None,
                        },
                    );
                }
                tl::enums::Chat::Empty(_) => {}
            }
        }
        for user in users {
            if let tl::enums::User::User(u) = user {
                let display_name = match (&u.first_name, &u.last_name) {
                    (Some(f), Some(l)) => Some(format!("{f} {l}")),
                    (Some(f), None) => Some(f.clone()),
                    (None, Some(l)) => Some(l.clone()),
                    (None, None) => None,
                };
                map.insert(
                    PeerId::user_unchecked(u.id),
                    EntityInfo {
                        display_name,
                        first_name: u.first_name.clone(),
                        username: u.username.clone(),
                    },
                );
            }
        }
        Self { map }
    }

    /// Resolve a raw `Peer` reference (e.g. a fwd header's `from_id`).
    pub(crate) fn get(&self, peer: &tl::enums::Peer) -> Option<&EntityInfo> {
        self.map.get(&PeerId::from(peer.clone()))
    }

    /// Seed from a high-level peer (used by the grammers-`Message` wrapper so
    /// call paths without an envelope keep their sender resolution).
    pub(crate) fn insert_peer(&mut self, peer: &grammers_client::peer::Peer) {
        use grammers_client::peer::Peer;
        let info = match peer {
            Peer::User(u) => EntityInfo {
                display_name: match (u.first_name(), u.last_name()) {
                    (Some(f), Some(l)) => Some(format!("{f} {l}")),
                    (Some(f), None) => Some(f.to_string()),
                    (None, Some(l)) => Some(l.to_string()),
                    (None, None) => None,
                },
                first_name: u.first_name().map(|s| s.to_string()),
                username: u.username().map(|s| s.to_string()),
            },
            Peer::Group(g) => EntityInfo {
                display_name: g.title().map(|s| s.to_string()),
                first_name: None,
                username: g.username().map(|s| s.to_string()),
            },
            Peer::Channel(c) => EntityInfo {
                display_name: Some(c.title().to_string()),
                first_name: None,
                username: c.username().map(|s| s.to_string()),
            },
            Peer::Community(c) => EntityInfo {
                display_name: Some(c.title().to_string()),
                first_name: None,
                username: None,
            },
        };
        self.map.insert(peer.id(), info);
    }
}
```

(If `User::last_name()` does not exist on the high-level `User` in the pinned rev, check `grammers-client/src/peer/user.rs` — it does: `pub fn last_name(&self) -> Option<&str>` at line 133.)

- [ ] **Step 5: Run gate**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat: EntityLookup — pure entity map over raw response envelopes"
```

---

### Task 3: Enrich `extract_forward_info`

**Files:**
- Modify: `src/telegram/converters/message.rs:38-63` (`extract_forward_info`) + its test module
- Modify: `src/telegram/converters.rs` (no export change needed — `extract_forward_info` is `pub(crate)` within converters)

**Interfaces:**
- Consumes: `EntityLookup::{from_envelope, empty, get}`, `EntityInfo.display_name/username` (Task 2); `ForwardInfo.post_author` (Task 1).
- Produces: `pub(crate) fn extract_forward_info(header: &tl::types::MessageFwdHeader, entities: &EntityLookup) -> ForwardInfo` — Task 4's core calls this signature.

- [ ] **Step 1: Write the failing tests**

Add to the test module of `src/telegram/converters/message.rs`:

```rust
    use crate::telegram::envelope::EntityLookup;
    use crate::test_helpers::{raw_tl_channel, raw_tl_user};

    /// Fwd header fixture: unset fields None/false, `date` fixed.
    fn fwd_header(
        from_id: Option<tl::enums::Peer>,
        from_name: Option<&str>,
        post_author: Option<&str>,
    ) -> tl::types::MessageFwdHeader {
        tl::types::MessageFwdHeader {
            imported: false,
            saved_out: false,
            from_id,
            from_name: from_name.map(|s| s.to_string()),
            date: 1_700_000_000,
            channel_post: Some(1863),
            post_author: post_author.map(|s| s.to_string()),
            saved_from_peer: None,
            saved_from_msg_id: None,
            saved_from_id: None,
            saved_from_name: None,
            saved_date: None,
            psa_type: None,
        }
    }

    fn channel_fwd_peer(id: i64) -> Option<tl::enums::Peer> {
        Some(tl::enums::Peer::Channel(tl::types::PeerChannel {
            channel_id: id,
        }))
    }

    #[test]
    fn forward_from_enveloped_channel_carries_name_and_username() {
        let entities = EntityLookup::from_envelope(
            &[tl::enums::Chat::Channel(raw_tl_channel(
                1783384254,
                "Военкор",
                Some("voenkor_ru"),
            ))],
            &[],
        );
        let info = extract_forward_info(&fwd_header(channel_fwd_peer(1783384254), None, None), &entities);
        assert_eq!(info.channel_id.map(|c| c.get()), Some(1783384254));
        assert_eq!(info.channel_name.as_ref().map(|n| n.as_str()), Some("Военкор"));
        assert_eq!(
            info.channel_username.as_ref().map(|u| u.as_str()),
            Some("voenkor_ru")
        );
        assert_eq!(info.sender_name, None);
        assert_eq!(info.original_message_id.map(|m| m.get()), Some(1863));
    }

    #[test]
    fn forward_from_private_channel_carries_name_without_username() {
        let entities = EntityLookup::from_envelope(
            &[tl::enums::Chat::Channel(raw_tl_channel(77, "Приватный", None))],
            &[],
        );
        let info = extract_forward_info(&fwd_header(channel_fwd_peer(77), None, None), &entities);
        assert_eq!(info.channel_name.as_ref().map(|n| n.as_str()), Some("Приватный"));
        assert_eq!(info.channel_username, None);
    }

    #[test]
    fn forward_from_user_populates_sender_name_only() {
        let entities = EntityLookup::from_envelope(
            &[],
            &[tl::enums::User::User(raw_tl_user(
                42,
                Some("Иван"),
                Some("Петров"),
                None,
            ))],
        );
        let from = Some(tl::enums::Peer::User(tl::types::PeerUser { user_id: 42 }));
        let info = extract_forward_info(&fwd_header(from, None, None), &entities);
        assert_eq!(info.sender_name.as_deref(), Some("Иван Петров"));
        assert_eq!(info.channel_id, None);
        assert_eq!(info.channel_name, None);
        assert_eq!(info.channel_username, None);
    }

    #[test]
    fn forward_from_hidden_sender_uses_from_name() {
        let info = extract_forward_info(
            &fwd_header(None, Some("Скрытый Автор"), None),
            &EntityLookup::empty(),
        );
        assert_eq!(info.sender_name.as_deref(), Some("Скрытый Автор"));
        assert_eq!(info.channel_id, None);
        assert_eq!(info.channel_name, None);
    }

    #[test]
    fn forward_carries_post_author_for_signed_posts() {
        let entities = EntityLookup::from_envelope(
            &[tl::enums::Chat::Channel(raw_tl_channel(9, "Канал", None))],
            &[],
        );
        let info =
            extract_forward_info(&fwd_header(channel_fwd_peer(9), None, Some("И. Петров")), &entities);
        assert_eq!(info.post_author.as_deref(), Some("И. Петров"));
    }

    #[test]
    fn envelope_miss_degrades_to_ids_only() {
        let info = extract_forward_info(
            &fwd_header(channel_fwd_peer(1783384254), None, None),
            &EntityLookup::empty(),
        );
        assert_eq!(info.channel_id.map(|c| c.get()), Some(1783384254));
        assert_eq!(info.channel_name, None);
        assert_eq!(info.channel_username, None);
        assert_eq!(info.sender_name, None);
        assert_eq!(info.original_message_id.map(|m| m.get()), Some(1863));
    }

    #[test]
    fn forward_from_legacy_group_carries_title_without_id() {
        let entities = EntityLookup::from_envelope(
            &[tl::enums::Chat::Chat(/* raw legacy chat, id 31 */ tl::types::Chat {
                // remaining bool flags false, remaining Options None (compiler-guided)
                id: 31,
                title: "Группа".to_string(),
                ..
            })],
            &[],
        );
        let from = Some(tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: 31 }));
        let info = extract_forward_info(&fwd_header(from, None, None), &entities);
        assert_eq!(info.channel_id, None, "chat-namespace ids stay unemitted, as today");
        assert_eq!(info.channel_name.as_ref().map(|n| n.as_str()), Some("Группа"));
    }
```

(As before: `..` placeholders in TL literals must be spelled out; the compiler lists the remaining fields — bools `false`, Options `None`. `tl::types::Chat` fields per schema: `creator, left, deactivated, call_active, call_not_empty, noforwards, id, title, photo (tl::enums::ChatPhoto::Empty), participants_count: 0, date: 0, version: 0, migrated_to: None, admin_rights: None, default_banned_rights: None`.)

Check the newtype accessor names before running: `ChannelId::get()`, `MessageId::get()` exist (used in ops files); `ChannelName`/`Username` — if `as_str()` does not exist, check `src/telegram/types/` for the actual accessor (e.g. `.0`, `.as_ref()`, or `Display`) and adjust the assertions to it.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test forward_from -- --nocapture`
Expected: COMPILE FAIL — `extract_forward_info` takes 1 argument.

- [ ] **Step 3: Implement enrichment**

Replace `extract_forward_info` in `src/telegram/converters/message.rs`:

```rust
/// Extract forward attribution from a raw forward header, resolving the
/// source's display data from the response envelope's entity map.
///
/// Zero network calls: `entities` is built from the same response the message
/// arrived in. A map miss degrades to the ids-only form — nothing is
/// fabricated and nothing is resolved on demand. Raw TL is required here
/// because the pinned grammers rev keeps `Message.peers` crate-private (see
/// `envelope.rs` module docs).
pub(crate) fn extract_forward_info(
    header: &tl::types::MessageFwdHeader,
    entities: &EntityLookup,
) -> ForwardInfo {
    let info = header.from_id.as_ref().and_then(|peer| entities.get(peer));

    let (channel_id, channel_name, channel_username, user_sender_name) = match &header.from_id {
        Some(tl::enums::Peer::Channel(ch)) => (
            ChannelId::new(ch.channel_id).ok(),
            info.and_then(|i| i.display_name.as_deref())
                .and_then(|n| ChannelName::new(n).ok()),
            info.and_then(|i| i.username.as_deref())
                .and_then(|u| Username::new(u).ok()),
            None,
        ),
        // Legacy groups: chat-namespace ids were never emitted; keep that,
        // but surface the title now that the envelope provides it.
        Some(tl::enums::Peer::Chat(_)) => (
            None,
            info.and_then(|i| i.display_name.as_deref())
                .and_then(|n| ChannelName::new(n).ok()),
            None,
            None,
        ),
        Some(tl::enums::Peer::User(_)) => {
            (None, None, None, info.and_then(|i| i.display_name.clone()))
        }
        None => (None, None, None, None),
    };

    ForwardInfo {
        channel_id,
        channel_name,
        channel_username,
        // Hidden senders (`from_name`, no `from_id`) win, as they always have;
        // otherwise a user-source forward carries the user's display name.
        sender_name: header.from_name.clone().or(user_sender_name),
        post_author: header.post_author.clone(),
        original_date: DateTime::<Utc>::from_timestamp(header.date as i64, 0)
            .filter(|dt| dt.timestamp() > 0),
        original_message_id: header
            .channel_post
            .and_then(|id| MessageId::new(id as i64).ok()),
    }
}
```

Add imports: `use crate::telegram::envelope::EntityLookup;` and `ChannelName`, `Username` to the existing `crate::telegram::types` import list. Update the one call site (`convert_message`) to pass `&EntityLookup::empty()` for now — Task 4 rewires it properly.

- [ ] **Step 4: Run gate**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: PASS — all seven new tests green, existing suite green.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: resolve forward attribution from the response envelope entity map"
```

---

### Task 4: `convert_raw_message` core + `convert_message` wrapper

**Files:**
- Modify: `src/telegram/converters/message.rs` (split `convert_message`, add `raw_*` helpers) + tests
- Modify: `src/telegram/converters.rs` (export `convert_raw_message`, `timestamp_from_raw`)

**Interfaces:**
- Consumes: `EntityLookup` (Task 2), `extract_forward_info(header, entities)` (Task 3).
- Produces (used by Tasks 7-8):
  - `pub fn convert_raw_message(raw: &tl::enums::Message, peer: &grammers_client::peer::Peer, entities: &EntityLookup) -> Option<Message>`
  - `pub(crate) fn timestamp_from_raw(raw: &tl::enums::Message) -> Option<DateTime<Utc>>` (promote the existing private fn; re-export from `converters.rs`)
  - `pub fn convert_message(msg: &grammers_client::message::Message, peer: &Peer) -> Option<Message>` — unchanged signature, now a seeding wrapper (call sites in `ops_message.rs`/`ops_stats.rs` untouched).

**Verified raw accessor facts (all mirror grammers' own thin readers):** `raw.id()` exists on `tl::enums::Message`; text = `Message(m).message` else `""`; media = `Message(m).media.clone().and_then(Media::from_raw)` (public, client-free); views/forwards = `m.views`/`m.forwards` (Message only); reply-to = `Message(m).reply_to: Some(tl::enums::MessageReplyHeader::Header(h)) → h.reply_to_msg_id`; grouped_id = `Message(m).grouped_id`; fwd = `Message(m).fwd_from`; `PeerId::from(peer.clone()).bare_id() -> Option<i64>`.

- [ ] **Step 1: Write the failing test — full raw conversion with enrichment**

Add to the message.rs test module:

```rust
    /// Raw channel-post message with a forward header, as it arrives inside a
    /// `messages.Messages` envelope.
    fn raw_forwarded_message(id: i32, fwd: tl::types::MessageFwdHeader) -> tl::enums::Message {
        tl::enums::Message::Message(tl::types::Message {
            id,
            peer_id: tl::enums::Peer::Channel(tl::types::PeerChannel {
                channel_id: 1144180066,
            }),
            date: 1_700_000_100,
            message: "переслано".to_string(),
            fwd_from: Some(tl::enums::MessageFwdHeader::Header(fwd)),
            post: true,
            from_id: None,
            media: None,
            views: Some(10),
            forwards: Some(2),
            reply_to: None,
            grouped_id: None,
            reactions: None,
            // remaining bool flags false, remaining Options None (compiler-guided)
            ..
        })
    }

    #[test]
    fn convert_raw_message_enriches_forward_from_envelope() {
        let peer = public_channel_peer(1144180066, "swodki");
        let entities = EntityLookup::from_envelope(
            &[tl::enums::Chat::Channel(raw_tl_channel(
                1783384254,
                "Военкор",
                Some("voenkor_ru"),
            ))],
            &[],
        );
        let raw = raw_forwarded_message(610121, fwd_header(channel_fwd_peer(1783384254), None, None));

        let msg = convert_raw_message(&raw, &peer, &entities).expect("converts");
        let fwd = msg.forwarded_from.expect("forward attribution present");
        assert_eq!(fwd.channel_name.as_ref().map(|n| n.as_str()), Some("Военкор"));
        assert_eq!(fwd.channel_username.as_ref().map(|u| u.as_str()), Some("voenkor_ru"));
        assert_eq!(msg.text, "переслано");
        assert_eq!(msg.views, Some(10));
        assert_eq!(msg.link, "https://t.me/swodki/610121");
    }

    #[test]
    fn convert_raw_message_without_forward_leaves_field_absent_in_json() {
        let peer = public_channel_peer(1144180066, "swodki");
        let mut raw_inner = match raw_forwarded_message(610122, fwd_header(None, None, None)) {
            tl::enums::Message::Message(m) => m,
            _ => unreachable!(),
        };
        raw_inner.fwd_from = None;
        let raw = tl::enums::Message::Message(raw_inner);

        let msg = convert_raw_message(&raw, &peer, &EntityLookup::empty()).expect("converts");
        assert!(msg.forwarded_from.is_none());
        let json = serde_json::to_value(&msg).expect("serializes");
        assert!(json.get("forwarded_from").is_none(), "absent, not null");
    }

    #[test]
    fn convert_raw_message_resolves_sender_from_envelope() {
        let peer = public_channel_peer(1144180066, "swodki");
        let entities = EntityLookup::from_envelope(
            &[],
            &[tl::enums::User::User(raw_tl_user(42, Some("Иван"), Some("Петров"), None))],
        );
        let mut raw_inner = match raw_forwarded_message(610123, fwd_header(None, None, None)) {
            tl::enums::Message::Message(m) => m,
            _ => unreachable!(),
        };
        raw_inner.fwd_from = None;
        raw_inner.from_id = Some(tl::enums::Peer::User(tl::types::PeerUser { user_id: 42 }));
        let raw = tl::enums::Message::Message(raw_inner);

        let msg = convert_raw_message(&raw, &peer, &entities).expect("converts");
        assert_eq!(msg.sender_id.map(|u| u.get()), Some(42));
        assert_eq!(msg.sender_name.as_deref(), Some("Иван"), "first-name-only parity");
    }

    #[test]
    fn convert_raw_message_refuses_empty_placeholder() {
        let peer = public_channel_peer(1144180066, "swodki");
        let raw = tl::enums::Message::Empty(tl::types::MessageEmpty { id: 1, peer_id: None });
        assert!(convert_raw_message(&raw, &peer, &EntityLookup::empty()).is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test convert_raw_message`
Expected: COMPILE FAIL — `convert_raw_message` not found.

- [ ] **Step 3: Implement — move the body, add raw helpers, shrink the wrapper**

In `src/telegram/converters/message.rs`:

```rust
/// Raw-field readers mirroring grammers' own thin accessors (each is a
/// `match` on the three message variants; Service/Empty yield the neutral
/// value exactly as the high-level methods do).
fn raw_text(raw: &tl::enums::Message) -> &str {
    match raw {
        tl::enums::Message::Message(m) => &m.message,
        _ => "",
    }
}

fn raw_from_id(raw: &tl::enums::Message) -> Option<&tl::enums::Peer> {
    match raw {
        tl::enums::Message::Message(m) => m.from_id.as_ref(),
        tl::enums::Message::Service(m) => m.from_id.as_ref(),
        tl::enums::Message::Empty(_) => None,
    }
}

fn raw_media(raw: &tl::enums::Message) -> Option<Media> {
    match raw {
        tl::enums::Message::Message(m) => m.media.clone().and_then(Media::from_raw),
        _ => None,
    }
}

fn raw_forward_header(raw: &tl::enums::Message) -> Option<&tl::types::MessageFwdHeader> {
    match raw {
        tl::enums::Message::Message(m) => m
            .fwd_from
            .as_ref()
            .map(|tl::enums::MessageFwdHeader::Header(h)| h),
        _ => None,
    }
}

fn raw_views(raw: &tl::enums::Message) -> Option<i32> {
    match raw {
        tl::enums::Message::Message(m) => m.views,
        _ => None,
    }
}

fn raw_forwards(raw: &tl::enums::Message) -> Option<i32> {
    match raw {
        tl::enums::Message::Message(m) => m.forwards,
        _ => None,
    }
}

fn raw_reply_to_message_id(raw: &tl::enums::Message) -> Option<i32> {
    match raw {
        tl::enums::Message::Message(tl::types::Message {
            reply_to: Some(tl::enums::MessageReplyHeader::Header(header)),
            ..
        }) => header.reply_to_msg_id,
        _ => None,
    }
}

fn raw_grouped_id(raw: &tl::enums::Message) -> Option<i64> {
    match raw {
        tl::enums::Message::Message(m) => m.grouped_id,
        _ => None,
    }
}
```

(If `MessageReplyHeader` has more than the `Header` variant in the pinned rev, the single-pattern closure/match will not compile — switch the two affected helpers to a `match` with `_ => None`.)

Core conversion (replaces the body of `convert_message`):

```rust
/// Convert a raw TL message to our domain Message, resolving senders and
/// forward attribution from the response envelope's entity map.
///
/// This is the single conversion path for every fetch route; the raw pagers
/// call it with the full envelope, the grammers-`Message` wrapper below calls
/// it with what the high-level API exposes. Pure function of its inputs — no
/// client, no network (zero-extra-call invariant is structural).
pub fn convert_raw_message(
    raw: &tl::enums::Message,
    peer: &grammers_client::peer::Peer,
    entities: &EntityLookup,
) -> Option<Message> {
    // A MessageEmpty placeholder (deleted / never-existed id) must never map
    // to a domain Message — it has an epoch-0 date and empty text (B1).
    if matches!(raw, tl::enums::Message::Empty(_)) {
        return None;
    }

    let (channel_id, channel_name, channel_username) = peer_identity(peer)?;
    let message_id = MessageId::new(raw.id() as i64).ok()?;

    // Sender from the raw from_id + envelope. grammers' private-DM fallback
    // (peer-as-sender when from_id is absent) is unreachable here: every
    // fetch path targets channel/group peers, where absent from_id means an
    // anonymous post — (None, None), as before.
    let (sender_id, sender_name) = match raw_from_id(raw) {
        Some(from) => (
            grammers_session::types::PeerId::from(from.clone())
                .bare_id()
                .and_then(|i| UserId::new(i).ok()),
            entities.get(from).and_then(|info| info.sender_name()),
        ),
        None => (None, None),
    };

    let media = raw_media(raw);
    let (has_media, media_type) = match &media {
        Some(m) => (true, convert_media_to_type(m)),
        None => (false, MediaType::None),
    };

    let link_preview = match &media {
        Some(Media::WebPage(wp)) => extract_link_preview(&wp.raw),
        _ => None,
    };
    let video_info = media.as_ref().and_then(extract_video_info);
    let audio_info = media.as_ref().and_then(extract_audio_info);

    let forwarded_from = raw_forward_header(raw).map(|h| extract_forward_info(h, entities));

    let views = raw_views(raw).and_then(|v| u64::try_from(v).ok());
    let forwards = raw_forwards(raw).and_then(|v| u64::try_from(v).ok());
    let reply_to_message_id =
        raw_reply_to_message_id(raw).and_then(|id| MessageId::new(id as i64).ok());

    let raw_reactions = match raw {
        tl::enums::Message::Message(m) => m.reactions.as_ref(),
        _ => None,
    };
    let (reactions, reactions_total) = extract_reactions(raw_reactions);
    let link = build_message_link(peer, message_id)?;

    Some(Message {
        id: message_id,
        channel_id,
        channel_name,
        channel_username,
        text: raw_text(raw).to_string(),
        timestamp: timestamp_from_raw(raw)?,
        sender_id,
        sender_name,
        has_media,
        media_type,
        forwarded_from,
        link_preview,
        views,
        forwards,
        reply_to_message_id,
        video_info,
        audio_info,
        grouped_id: raw_grouped_id(raw),
        link,
        reactions,
        reactions_total,
        album: None,
    })
}

/// Convert a grammers high-level Message (call paths that never see the raw
/// envelope: get_message_by_id, get_messages_batch, get_channel_stats).
///
/// Seeds the entity map from the only peers the high-level API exposes —
/// sender and chat — so sender resolution is unchanged; forwards on these
/// paths stay ids-only because `Message.peers` is crate-private in the
/// pinned grammers rev (see `envelope.rs`).
pub fn convert_message(
    msg: &grammers_client::message::Message,
    peer: &grammers_client::peer::Peer,
) -> Option<Message> {
    let mut entities = EntityLookup::empty();
    if let Some(sender) = msg.sender() {
        entities.insert_peer(sender);
    }
    if let Some(chat) = msg.peer() {
        entities.insert_peer(chat);
    }
    convert_raw_message(&msg.raw, peer, &entities)
}
```

Promote the timestamp seam and exports:
- Change `fn timestamp_from_raw` to `pub(crate) fn timestamp_from_raw`.
- In `src/telegram/converters.rs`: `pub use message::{convert_message, convert_raw_message};` and `pub(crate) use message::{message_timestamp, timestamp_from_raw};`.

Sender-parity note (documented, deliberate): previously `sender_id` was only set when the envelope contained the sender; now it is set whenever `from_id` is present. More data, never less — message-level `sender_name` semantics (first-name-only) are unchanged via `EntityInfo::sender_name()`.

- [ ] **Step 4: Run gate**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "refactor: split conversion into raw core + grammers wrapper, enrich via envelope"
```

---

### Task 5: `matches_media_filter_raw`

**Files:**
- Modify: `src/telegram/converters/media.rs:189-219` + tests
- Modify: `src/telegram/converters.rs:14-17` (add export)

**Interfaces:**
- Consumes: `raw_text`/`raw_media` logic pattern (self-contained here).
- Produces: `pub fn matches_media_filter_raw(raw: &tl::enums::Message, filter: &MediaFilter) -> bool` — Task 7 uses it.

- [ ] **Step 1: Write the failing tests**

In the media.rs test module (create `#[cfg(test)] mod tests` at the bottom if absent — check first):

```rust
    #[test]
    fn raw_filter_matches_photo_media() {
        let raw = tl::enums::Message::Message(tl::types::Message {
            id: 1,
            peer_id: tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 1 }),
            date: 1_700_000_000,
            message: String::new(),
            media: Some(tl::enums::MessageMedia::Photo(tl::types::MessageMediaPhoto {
                spoiler: false,
                photo: Some(tl::enums::Photo::Empty(tl::types::PhotoEmpty { id: 1 })),
                ttl_seconds: None,
                live_photo: false,
                video: None,
            })),
            // remaining bool flags false, remaining Options None (compiler-guided)
            ..
        });
        assert!(matches_media_filter_raw(&raw, &MediaFilter::Photo));
        assert!(!matches_media_filter_raw(&raw, &MediaFilter::Video));
    }

    #[test]
    fn raw_filter_url_matches_text_without_media() {
        let raw = tl::enums::Message::Message(tl::types::Message {
            id: 2,
            peer_id: tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 1 }),
            date: 1_700_000_000,
            message: "see https://example.com".to_string(),
            media: None,
            ..
        });
        assert!(matches_media_filter_raw(&raw, &MediaFilter::Url));
        assert!(!matches_media_filter_raw(&raw, &MediaFilter::Photo));
    }
```

(If `tl::enums::Photo::Empty` / `PhotoEmpty` differ in the pinned rev, check `grep "photoEmpty#" grammers-tl-types/tl/api.tl`; any minimal `MessageMedia` variant that maps to `MediaType::Photo` works — `convert_media_to_type`'s match arms show what's accepted.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test raw_filter`
Expected: COMPILE FAIL.

- [ ] **Step 3: Implement via a shared core**

Refactor in `src/telegram/converters/media.rs`:

```rust
/// Shared core for the high-level and raw filter entry points.
fn media_matches_filter(
    media: Option<&Media>,
    text: &str,
    pinned: bool,
    filter: &MediaFilter,
) -> bool {
    let Some(media) = media else {
        return match filter {
            MediaFilter::Url => text.contains("http://") || text.contains("https://"),
            MediaFilter::Pinned => pinned,
            _ => false,
        };
    };

    let media_type = convert_media_to_type(media);

    match filter {
        MediaFilter::Photo => media_type == MediaType::Photo,
        MediaFilter::Video => media_type == MediaType::Video,
        MediaFilter::PhotoVideo => media_type == MediaType::Photo || media_type == MediaType::Video,
        MediaFilter::Document => media_type == MediaType::Document,
        MediaFilter::Audio => media_type == MediaType::Audio,
        MediaFilter::Voice => media_type == MediaType::Voice,
        MediaFilter::VideoNote => media_type == MediaType::VideoNote,
        MediaFilter::Gif => media_type == MediaType::Animation,
        MediaFilter::Url => text.contains("http://") || text.contains("https://"),
        MediaFilter::Pinned => pinned,
    }
}

/// Check if a message's media matches the given filter (for client-side filtering)
///
/// Used by `get_recent_messages` since `iter_messages` doesn't support server-side filtering.
pub fn matches_media_filter(msg: &grammers_client::message::Message, filter: &MediaFilter) -> bool {
    media_matches_filter(msg.media().as_ref(), msg.text(), msg.pinned(), filter)
}

/// Raw-message twin of [`matches_media_filter`], for the raw history pager path.
pub fn matches_media_filter_raw(raw: &tl::enums::Message, filter: &MediaFilter) -> bool {
    let (media, text, pinned) = match raw {
        tl::enums::Message::Message(m) => (
            m.media.clone().and_then(Media::from_raw),
            m.message.as_str(),
            m.pinned,
        ),
        _ => (None, "", false),
    };
    media_matches_filter(media.as_ref(), text, pinned, filter)
}
```

Add `matches_media_filter_raw` to the `pub use media::{...}` list in `src/telegram/converters.rs`. Add `use grammers_client::tl;` to media.rs imports if absent.

- [ ] **Step 4: Run gate**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "refactor: share media-filter core, add raw-message variant"
```

---

### Task 6: Raw pagers

**Files:**
- Create: `src/telegram/client/raw_pager.rs`
- Modify: `src/telegram/client.rs:31-42` (add `mod raw_pager;`)

**Interfaces:**
- Consumes: `EntityLookup::from_envelope` (Task 2).
- Produces (used by Tasks 7-8):
  - `pub(super) struct RawHistoryPager` — `fn new(client: &Client, peer: PeerRef) -> Self`, `fn offset_id(self, i32) -> Self`, `async fn next(&mut self) -> Result<Option<(tl::enums::Message, Arc<EntityLookup>)>, InvocationError>`
  - `pub(super) struct RawChannelSearchPager` — `new(client, peer)`, `query(self, &str)`, `filter(self, tl::enums::MessagesFilter)`, `offset_id(self, i32)`, same `next`
  - `pub(super) struct RawGlobalSearchPager` — `new(client)`, `query`, `filter`, and `async fn next(&mut self) -> Result<Option<(tl::enums::Message, Arc<EntityLookup>, Option<grammers_client::peer::Peer>)>, InvocationError>` (third element = the message's own chat, built from the envelope)

**Pagination parity contract (copied from grammers `client/messages.rs`, pinned rev — the plan's source of truth for every rule below):**
- Page limit 100 (`MAX_LIMIT`).
- `last_chunk`: `Messages::Messages` → always; `Slice`/`ChannelMessages` → `messages.is_empty() || messages[0].id() <= limit`; `NotModified` cannot occur with `hash: 0`.
- History advance: `offset_id = last.id()`, `offset_date = raw date of last` (0 for Empty).
- Channel-search advance: `offset_id = last.id()`, `max_date = raw date of last`.
- Global advance: `offset_rate = next_rate.unwrap_or(0)`, `offset_id = last.id()`, `offset_peer` = InputPeer for the last message's `peer_id` resolved from the same page's chats/users (access hash from the envelope; `InputPeer::Empty` on miss — grammers falls back the same way).

- [ ] **Step 1: Write the failing tests (pure logic only — no network)**

In `src/telegram/client/raw_pager.rs`, tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::raw_tl_channel;
    use grammers_client::tl;

    fn raw_msg(id: i32, date: i32, channel_id: i64) -> tl::enums::Message {
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
            peer_id: tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id }),
            saved_peer_id: None,
            reply_to: None,
            date,
            action: tl::enums::MessageAction::Empty,
            reactions: None,
            ttl_period: None,
        })
    }

    fn slice(messages: Vec<tl::enums::Message>, next_rate: Option<i32>) -> tl::enums::messages::Messages {
        tl::enums::messages::Messages::Slice(tl::types::messages::MessagesSlice {
            inexact: false,
            count: 1000,
            next_rate,
            offset_id_offset: None,
            search_flood: None,
            messages,
            topics: vec![],
            chats: vec![tl::enums::Chat::Channel(raw_tl_channel(11, "Канал", None))],
            users: vec![],
        })
    }

    #[test]
    fn unpack_slice_computes_last_chunk_and_keeps_envelope() {
        let page = unpack_page(slice(vec![raw_msg(500, 1_700_000_000, 11)], Some(7)), 100)
            .expect("unpacks");
        assert!(!page.last_chunk, "id 500 > limit 100 → more pages may exist");
        assert_eq!(page.next_rate, Some(7));
        assert_eq!(page.messages.len(), 1);
        assert_eq!(page.chats.len(), 1);

        let tail = unpack_page(slice(vec![raw_msg(90, 1_700_000_000, 11)], None), 100)
            .expect("unpacks");
        assert!(tail.last_chunk, "highest id <= limit → nothing older can exist");

        let empty = unpack_page(slice(vec![], None), 100).expect("unpacks");
        assert!(empty.last_chunk);
    }

    #[test]
    fn unpack_messages_variant_is_always_last_chunk() {
        let res = tl::enums::messages::Messages::Messages(tl::types::messages::Messages {
            messages: vec![raw_msg(3, 1_700_000_000, 11)],
            topics: vec![],
            chats: vec![],
            users: vec![],
        });
        assert!(unpack_page(res, 100).expect("unpacks").last_chunk);
    }

    #[test]
    fn history_offsets_advance_from_last_message() {
        let mut request = history_request(tl::enums::InputPeer::Empty, 0);
        let page = unpack_page(
            slice(
                vec![raw_msg(500, 1_700_000_500, 11), raw_msg(499, 1_700_000_400, 11)],
                None,
            ),
            100,
        )
        .expect("unpacks");
        advance_history_offsets(&mut request, &page);
        assert_eq!(request.offset_id, 499);
        assert_eq!(request.offset_date, 1_700_000_400);
    }

    #[test]
    fn global_offset_peer_resolves_access_hash_from_envelope() {
        let page = unpack_page(slice(vec![raw_msg(500, 1_700_000_000, 11)], Some(9)), 100)
            .expect("unpacks");
        let input = input_peer_for_message(&page.messages[0], &page);
        match input {
            tl::enums::InputPeer::Channel(c) => assert_eq!(c.channel_id, 11),
            other => panic!("expected InputPeer::Channel, got {other:?}"),
        }

        // Envelope miss → Empty, mirroring grammers' unwrap_or fallback.
        let missing = raw_msg(501, 1_700_000_000, 999);
        assert!(matches!(
            input_peer_for_message(&missing, &page),
            tl::enums::InputPeer::Empty
        ));
    }
}
```

(`tl::types::messages::MessagesSlice` — the `messages.` namespace maps to `tl::types::messages::`/`tl::enums::messages::` in generated code, exactly as `unpack_messages` in grammers does. `raw_tl_channel(11, ...)` has `access_hash: Some(0)`, so the InputPeer assertion needs no extra fixture.)

- [ ] **Step 2: Wire module, run tests to verify they fail**

Add `mod raw_pager;` to `src/telegram/client.rs` (after `mod ops_transcribe;`).
Run: `cargo test raw_pager`
Expected: COMPILE FAIL.

- [ ] **Step 3: Implement the pagers**

```rust
//! Raw-TL history/search pagers.
//!
//! Replicates the pagination of grammers' `MessageIter` / `SearchIter` /
//! `GlobalSearchIter` (pinned rev, `client/messages.rs`) while keeping the
//! response envelope's `chats`+`users`, which the high-level iterators
//! discard behind a crate-private `PeerMap`. Same requests, same order, same
//! stop conditions — zero additional network calls; the envelope feeds
//! forward attribution (see `telegram/envelope.rs`).

use crate::telegram::envelope::EntityLookup;
use grammers_client::Client;
use grammers_client::tl;
use grammers_mtsender::InvocationError;
use grammers_session::types::PeerRef;
use std::collections::VecDeque;
use std::sync::Arc;

/// grammers' MAX_LIMIT: server pages cap at 100 messages.
const PAGE_LIMIT: i32 = 100;

/// One decoded page: raw messages plus the envelope entities they came with.
struct RawPage {
    messages: Vec<tl::enums::Message>,
    chats: Vec<tl::enums::Chat>,
    users: Vec<tl::enums::User>,
    next_rate: Option<i32>,
    last_chunk: bool,
}

/// Raw date of a message (0 for Empty — mirrors grammers `date_timestamp`).
fn raw_date(raw: &tl::enums::Message) -> i32 {
    match raw {
        tl::enums::Message::Message(m) => m.date,
        tl::enums::Message::Service(m) => m.date,
        tl::enums::Message::Empty(_) => 0,
    }
}

/// Raw peer the message lives in (mirrors grammers `utils::peer_from_message`).
fn raw_peer_id(raw: &tl::enums::Message) -> Option<&tl::enums::Peer> {
    match raw {
        tl::enums::Message::Message(m) => Some(&m.peer_id),
        tl::enums::Message::Service(m) => Some(&m.peer_id),
        tl::enums::Message::Empty(m) => m.peer_id.as_ref(),
    }
}

/// Decode a `messages.Messages` response, preserving the envelope and
/// computing grammers' last-chunk rule: `Messages` is always final;
/// `Slice`/`ChannelMessages` are final when empty or when the newest id in
/// the page is within `limit` of the absolute lowest message id (1).
fn unpack_page(
    res: tl::enums::messages::Messages,
    limit: i32,
) -> Result<RawPage, InvocationError> {
    use tl::enums::messages::Messages;
    let (messages, chats, users, next_rate, last_chunk) = match res {
        Messages::Messages(m) => (m.messages, m.chats, m.users, None, true),
        Messages::Slice(m) => {
            let last = m.messages.is_empty() || m.messages[0].id() <= limit;
            (m.messages, m.chats, m.users, m.next_rate, last)
        }
        Messages::ChannelMessages(m) => {
            let last = m.messages.is_empty() || m.messages[0].id() <= limit;
            (m.messages, m.chats, m.users, None, last)
        }
        // hash is always 0 in our requests; NotModified cannot occur. Treat
        // it as an empty final page rather than panicking (never unwrap).
        Messages::NotModified(_) => (vec![], vec![], vec![], None, true),
    };
    Ok(RawPage {
        messages,
        chats,
        users,
        next_rate,
        last_chunk,
    })
}

fn history_request(peer: tl::enums::InputPeer, offset_id: i32) -> tl::functions::messages::GetHistory {
    tl::functions::messages::GetHistory {
        peer,
        offset_id,
        offset_date: 0,
        add_offset: 0,
        limit: PAGE_LIMIT,
        max_id: 0,
        min_id: 0,
        hash: 0,
    }
}

fn advance_history_offsets(request: &mut tl::functions::messages::GetHistory, page: &RawPage) {
    if let Some(last) = page.messages.last() {
        request.offset_id = last.id();
        request.offset_date = raw_date(last);
    }
}

/// InputPeer for a message's chat, with the access hash taken from the same
/// page's envelope. Envelope miss → `InputPeer::Empty` (grammers' fallback).
fn input_peer_for_message(raw: &tl::enums::Message, page: &RawPage) -> tl::enums::InputPeer {
    let Some(peer) = raw_peer_id(raw) else {
        return tl::enums::InputPeer::Empty;
    };
    match peer {
        tl::enums::Peer::Channel(p) => page
            .chats
            .iter()
            .find_map(|chat| match chat {
                tl::enums::Chat::Channel(c) if c.id == p.channel_id => Some(
                    tl::enums::InputPeer::Channel(tl::types::InputPeerChannel {
                        channel_id: c.id,
                        access_hash: c.access_hash.unwrap_or(0),
                    }),
                ),
                _ => None,
            })
            .unwrap_or(tl::enums::InputPeer::Empty),
        tl::enums::Peer::User(p) => page
            .users
            .iter()
            .find_map(|user| match user {
                tl::enums::User::User(u) if u.id == p.user_id => {
                    Some(tl::enums::InputPeer::User(tl::types::InputPeerUser {
                        user_id: u.id,
                        access_hash: u.access_hash.unwrap_or(0),
                    }))
                }
                _ => None,
            })
            .unwrap_or(tl::enums::InputPeer::Empty),
        tl::enums::Peer::Chat(p) => {
            tl::enums::InputPeer::Chat(tl::types::InputPeerChat { chat_id: p.chat_id })
        }
    }
}

/// Raw `messages.GetHistory` pager (get_recent_messages path).
pub(super) struct RawHistoryPager {
    client: Client,
    request: tl::functions::messages::GetHistory,
    buffer: VecDeque<(tl::enums::Message, Arc<EntityLookup>)>,
    last_chunk: bool,
}

impl RawHistoryPager {
    pub(super) fn new(client: &Client, peer: PeerRef) -> Self {
        Self {
            client: client.clone(),
            request: history_request(peer.into(), 0),
            buffer: VecDeque::new(),
            last_chunk: false,
        }
    }

    pub(super) fn offset_id(mut self, offset: i32) -> Self {
        self.request.offset_id = offset;
        self
    }

    pub(super) async fn next(
        &mut self,
    ) -> Result<Option<(tl::enums::Message, Arc<EntityLookup>)>, InvocationError> {
        if let Some(item) = self.buffer.pop_front() {
            return Ok(Some(item));
        }
        if self.last_chunk {
            return Ok(None);
        }
        let page = unpack_page(self.client.invoke(&self.request).await?, self.request.limit)?;
        self.last_chunk = page.last_chunk;
        advance_history_offsets(&mut self.request, &page);
        fill_buffer(&mut self.buffer, page);
        Ok(self.buffer.pop_front())
    }
}

/// Move a page's messages into a pager buffer, pairing each with the page's
/// (shared) entity lookup.
fn fill_buffer(
    buffer: &mut VecDeque<(tl::enums::Message, Arc<EntityLookup>)>,
    page: RawPage,
) {
    let entities = Arc::new(EntityLookup::from_envelope(&page.chats, &page.users));
    buffer.extend(
        page.messages
            .into_iter()
            .map(|message| (message, Arc::clone(&entities))),
    );
}
```

Channel search pager (same shape; request template copied from grammers `SearchIter::new`):

```rust
/// Raw `messages.Search` pager (search_messages single-channel path).
pub(super) struct RawChannelSearchPager {
    client: Client,
    request: tl::functions::messages::Search,
    buffer: VecDeque<(tl::enums::Message, Arc<EntityLookup>)>,
    last_chunk: bool,
}

impl RawChannelSearchPager {
    pub(super) fn new(client: &Client, peer: PeerRef) -> Self {
        Self {
            client: client.clone(),
            request: tl::functions::messages::Search {
                peer: peer.into(),
                q: String::new(),
                from_id: None,
                saved_peer_id: None,
                saved_reaction: None,
                top_msg_id: None,
                filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
                min_date: 0,
                max_date: 0,
                offset_id: 0,
                add_offset: 0,
                limit: PAGE_LIMIT,
                max_id: 0,
                min_id: 0,
                hash: 0,
            },
            buffer: VecDeque::new(),
            last_chunk: false,
        }
    }

    pub(super) fn query(mut self, query: &str) -> Self {
        self.request.q = query.to_string();
        self
    }

    pub(super) fn filter(mut self, filter: tl::enums::MessagesFilter) -> Self {
        self.request.filter = filter;
        self
    }

    pub(super) fn offset_id(mut self, offset: i32) -> Self {
        self.request.offset_id = offset;
        self
    }

    pub(super) async fn next(
        &mut self,
    ) -> Result<Option<(tl::enums::Message, Arc<EntityLookup>)>, InvocationError> {
        if let Some(item) = self.buffer.pop_front() {
            return Ok(Some(item));
        }
        if self.last_chunk {
            return Ok(None);
        }
        let page = unpack_page(self.client.invoke(&self.request).await?, self.request.limit)?;
        self.last_chunk = page.last_chunk;
        if let Some(last) = page.messages.last() {
            self.request.offset_id = last.id();
            self.request.max_date = raw_date(last);
        }
        fill_buffer(&mut self.buffer, page);
        Ok(self.buffer.pop_front())
    }
}
```

Global pager (request template from grammers `GlobalSearchIter::new`; yields the chat `Peer` too):

```rust
/// Raw `messages.SearchGlobal` pager (search_messages all-channels path).
/// Yields each message with its envelope entities and its own chat as a
/// high-level `Peer` (built from the same envelope), which the ops layer
/// needs for identity and link building.
pub(super) struct RawGlobalSearchPager {
    client: Client,
    request: tl::functions::messages::SearchGlobal,
    buffer: VecDeque<(
        tl::enums::Message,
        Arc<EntityLookup>,
        Option<grammers_client::peer::Peer>,
    )>,
    last_chunk: bool,
}

impl RawGlobalSearchPager {
    pub(super) fn new(client: &Client) -> Self {
        Self {
            client: client.clone(),
            request: tl::functions::messages::SearchGlobal {
                folder_id: None,
                q: String::new(),
                filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
                min_date: 0,
                max_date: 0,
                offset_rate: 0,
                offset_peer: tl::enums::InputPeer::Empty,
                offset_id: 0,
                limit: PAGE_LIMIT,
                broadcasts_only: false,
                groups_only: false,
                users_only: false,
                community: None,
            },
            buffer: VecDeque::new(),
            last_chunk: false,
        }
    }

    pub(super) fn query(mut self, query: &str) -> Self {
        self.request.q = query.to_string();
        self
    }

    pub(super) fn filter(mut self, filter: tl::enums::MessagesFilter) -> Self {
        self.request.filter = filter;
        self
    }

    pub(super) async fn next(
        &mut self,
    ) -> Result<
        Option<(
            tl::enums::Message,
            Arc<EntityLookup>,
            Option<grammers_client::peer::Peer>,
        )>,
        InvocationError,
    > {
        if let Some(item) = self.buffer.pop_front() {
            return Ok(Some(item));
        }
        if self.last_chunk {
            return Ok(None);
        }
        // Order matters: advance offsets while `page` is whole, then
        // destructure — the message vec is consumed by the buffer fill while
        // chats/users are still needed for per-message chat peers.
        let page = unpack_page(self.client.invoke(&self.request).await?, self.request.limit)?;
        self.last_chunk = page.last_chunk;
        if let Some(last) = page.messages.last() {
            self.request.offset_rate = page.next_rate.unwrap_or(0);
            self.request.offset_id = last.id();
            self.request.offset_peer = input_peer_for_message(last, &page);
        }
        let RawPage {
            messages, chats, users, ..
        } = page;
        let entities = Arc::new(EntityLookup::from_envelope(&chats, &users));
        for message in messages {
            let chat = chat_peer_for_message(&self.client, &message, &chats, &users);
            self.buffer.push_back((message, Arc::clone(&entities), chat));
        }
        Ok(self.buffer.pop_front())
    }
}
```

```rust
/// The message's own chat as a high-level Peer, built from the envelope
/// (`Peer::from_raw` / `User::from_raw` are public; the crate-private
/// `PeerMap` is not — this is the raw-TL replacement for `msg.peer()`).
fn chat_peer_for_message(
    client: &Client,
    raw: &tl::enums::Message,
    chats: &[tl::enums::Chat],
    users: &[tl::enums::User],
) -> Option<grammers_client::peer::Peer> {
    use grammers_client::peer::{Peer, User};
    let peer = raw_peer_id(raw)?;
    match peer {
        tl::enums::Peer::Channel(p) => chats.iter().find_map(|chat| match chat {
            tl::enums::Chat::Channel(c) if c.id == p.channel_id => {
                Some(Peer::from_raw(client, chat.clone()))
            }
            tl::enums::Chat::Community(c) if c.id == p.channel_id => {
                Some(Peer::from_raw(client, chat.clone()))
            }
            _ => None,
        }),
        tl::enums::Peer::Chat(p) => chats.iter().find_map(|chat| match chat {
            tl::enums::Chat::Chat(c) if c.id == p.chat_id => {
                Some(Peer::from_raw(client, chat.clone()))
            }
            _ => None,
        }),
        tl::enums::Peer::User(p) => users.iter().find_map(|user| match user {
            tl::enums::User::User(u) if u.id == p.user_id => {
                Some(Peer::User(User::from_raw(client, user.clone())))
            }
            _ => None,
        }),
    }
}
```

(`Peer::from_raw(client, tl::enums::Chat)` routes Channel/Community/Chat variants itself — verified public in `peer/mod.rs:82`. If it panics on some variant per its implementation, restrict the match arms to the variants `peer/mod.rs`'s `from_raw` actually accepts — read it before implementing.)

- [ ] **Step 4: Run gate**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: PASS — the four pure-logic tests green.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat: raw TL pagers preserving the response envelope (grammers pagination parity)"
```

---

### Task 7: `get_recent_messages` on the raw history pager

**Files:**
- Modify: `src/telegram/client/ops_history.rs:101-167`

**Interfaces:**
- Consumes: `RawHistoryPager` (Task 6), `convert_raw_message` + `timestamp_from_raw` (Task 4), `matches_media_filter_raw` (Task 5).
- Produces: no new interfaces — behavioral upgrade of `get_recent_messages_impl`.

- [ ] **Step 1: Swap the iterator loop**

Replace the `with_timeout("iter_messages", ...)` block body (keep the timeout name for log continuity):

```rust
        let (messages, has_more) =
            with_timeout("iter_messages", self.timeouts.history_secs, async {
                let mut messages = Vec::new();
                let mut has_more = false;
                let mut counter = PostCounter::default();
                let mut pager = RawHistoryPager::new(&self.client, peer_ref);
                if let Some(before) = before_offset {
                    pager = pager.offset_id(before);
                }

                while let Some((raw_msg, entities)) = pager
                    .next()
                    .await
                    .map_err(|e| Error::TelegramApi(format!("Failed to iterate messages: {}", e)))?
                {
                    if let Some(to) = params.to_date
                        && timestamp_from_raw(&raw_msg).is_some_and(|t| t > to)
                    {
                        continue; // newer than the requested window; keep iterating toward it
                    }

                    // Check time filter - messages are in reverse chronological order
                    if timestamp_from_raw(&raw_msg).is_none_or(|t| t < cutoff_time) {
                        break;
                    }

                    // Exclusive lower cursor bound: everything from here on
                    // is older (reverse chronological), so stop (A8).
                    if let Some(after) = after_bound
                        && raw_msg.id() <= after
                    {
                        break;
                    }

                    // Apply media filter client-side (GetHistory has no server-side filtering)
                    if params
                        .media_filter
                        .as_ref()
                        .is_some_and(|filter| !matches_media_filter_raw(&raw_msg, filter))
                    {
                        continue;
                    }

                    if let Some(converted) = convert_raw_message(&raw_msg, &peer, &entities) {
                        if params.collapse_albums {
                            // Post-level limit: stop only when a NEW post would
                            // overflow; trailing siblings of admitted albums pass.
                            if !counter.admit(album_key(&converted), params.limit as usize) {
                                has_more = true;
                                break;
                            }
                            messages.push(converted);
                        } else {
                            // Refuse the overflow message instead of pushing the
                            // limit-th and breaking blind: refusing proves a
                            // qualifying message exists beyond the page (A8).
                            if messages.len() >= params.limit as usize {
                                has_more = true;
                                break;
                            }
                            messages.push(converted);
                        }
                    }
                }
                Ok((messages, has_more))
            })
            .await?;
```

Imports at the top of the file (via `use super::*;` most arrive already — add explicitly): `use super::raw_pager::RawHistoryPager;`. Ensure `client.rs`'s converter import list gains `convert_raw_message`, `matches_media_filter_raw`, `timestamp_from_raw` (ops files use `use super::*`).

- [ ] **Step 2: Run gate**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: PASS (mcp mock-level tests unaffected; converter tests unaffected). `matches_media_filter` (high-level) may now be unused by ops_history — it is still used elsewhere or exported `pub`; if clippy flags an unused import in this file, drop only the import.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "feat: get_recent_messages fetches via raw GetHistory, enriching forwards from the envelope"
```

---

### Task 8: `search_messages` on the raw search pagers

**Files:**
- Modify: `src/telegram/client/ops_search.rs:52-204`

**Interfaces:**
- Consumes: `RawChannelSearchPager`, `RawGlobalSearchPager` (Task 6), `convert_raw_message`, `timestamp_from_raw` (Task 4).
- Produces: no new interfaces.

- [ ] **Step 1: Swap the channel-path loop**

Inside the `with_timeout("search_messages_channel", ...)` dialog walk, replace from `let peer_ref = peer_to_ref(peer).await?;` through the inner `while` loop:

```rust
                            // Search in this specific channel
                            let peer_ref = peer_to_ref(peer).await?;
                            let mut pager = RawChannelSearchPager::new(&self.client, peer_ref)
                                .query(&params.query);
                            if let Some(before) = before_offset {
                                pager = pager.offset_id(before);
                            }

                            // Apply media filter if specified
                            if let Some(ref media_filter) = params.media_filter {
                                pager = pager.filter(convert_media_filter(media_filter));
                            }

                            while let Some((raw_msg, entities)) = pager
                                .next()
                                .await
                                .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?
                            {
                                if let Some(to) = params.to_date
                                    && timestamp_from_raw(&raw_msg).is_some_and(|t| t > to)
                                {
                                    continue; // newer than the requested window; keep iterating toward it
                                }
                                if timestamp_from_raw(&raw_msg).is_none_or(|t| t < cutoff_time) {
                                    break; // reverse chronological order
                                }
                                // Exclusive lower cursor bound: everything from here on
                                // is older (reverse chronological), so stop (A8).
                                if let Some(after) = after_bound
                                    && raw_msg.id() <= after
                                {
                                    break;
                                }
                                if let Some(converted) =
                                    convert_raw_message(&raw_msg, peer, &entities)
                                {
                                    // ... album/limit admission block UNCHANGED from the
                                    // current file (copy verbatim, it operates on `converted`)
                                }
                            }
                            break;
```

(The album/limit admission block is copied unchanged — it only touches `converted`, `counter`, `messages`, `has_more`, `params`.)

- [ ] **Step 2: Swap the global-path loop**

Replace the `search_all_messages` block body:

```rust
                    let mut pager = RawGlobalSearchPager::new(&self.client).query(&params.query);

                    if let Some(ref media_filter) = params.media_filter {
                        pager = pager.filter(convert_media_filter(media_filter));
                    }

                    while let Some((raw_msg, entities, chat_peer)) = pager
                        .next()
                        .await
                        .map_err(|e| Error::TelegramApi(format!("Search failed: {}", e)))?
                    {
                        if let Some(to) = params.to_date
                            && timestamp_from_raw(&raw_msg).is_some_and(|t| t > to)
                        {
                            continue; // newer than the requested window; keep iterating toward it
                        }
                        if timestamp_from_raw(&raw_msg).is_none_or(|t| t < cutoff_time) {
                            continue; // Skip old messages but keep searching
                        }
                        if let Some(peer) = chat_peer.as_ref()
                            && let Some(converted) = convert_raw_message(&raw_msg, peer, &entities)
                        {
                            // ... album/limit admission block UNCHANGED (copy verbatim)
                        }
                    }
                    Ok((messages, has_more))
```

Imports: `use super::raw_pager::{RawChannelSearchPager, RawGlobalSearchPager};`.

- [ ] **Step 3: Run gate**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: search_messages fetches via raw Search/SearchGlobal, enriching forwards"
```

---

### Task 9: Server-level guard test (enrichment serialized, zero resolve calls)

**Files:**
- Modify: `src/mcp/tests/search.rs` (extend the enrichment serialization test region, ~line 428)
- Modify: `src/test_helpers.rs` (enriched-forward fixture)

**Interfaces:**
- Consumes: `ForwardInfo` with `post_author` (Task 1).

- [ ] **Step 1: Extend the fixture**

In `src/test_helpers.rs` add (keep the existing `create_test_message_with_forward` untouched):

```rust
/// Forward fixture with full attribution, as the envelope-enriched
/// conversion now produces it.
pub fn create_test_message_with_enriched_forward(
    id: i64,
    text: &str,
    channel_id: i64,
    forwarded_channel_id: i64,
) -> Message {
    let mut msg = create_test_message(id, text, channel_id);
    msg.forwarded_from = Some(ForwardInfo {
        channel_id: Some(
            ChannelId::new(forwarded_channel_id)
                .expect("Test forwarded channel ID must be positive"),
        ),
        channel_name: Some(ChannelName::new("Военкор").expect("valid name")),
        channel_username: Some(Username::new("voenkor_ru").expect("valid username")),
        sender_name: None,
        post_author: Some("И. Петров".to_string()),
        original_date: None,
        original_message_id: Some(MessageId::new(1863).expect("valid id")),
    });
    msg
}
```

- [ ] **Step 2: Write the failing server test**

In `src/mcp/tests/search.rs`, after `search_messages_serializes_enrichment_fields`, model setup/assertion style on that test (same mock plumbing, same `Parameters(...)`/`RequestId` invocation, extract the JSON text from the result the same way it does):

```rust
#[tokio::test]
async fn search_messages_serializes_enriched_forward_without_resolve_calls() {
    let mut mock_client = MockTelegramClientTrait::new();
    let enriched = SearchResult {
        messages: vec![crate::test_helpers::create_test_message_with_enriched_forward(
            1, "переслано", 123, 1783384254,
        )],
        returned: 1,
        has_more: false,
        search_time_ms: 10,
        query_metadata: QueryMetadata {
            query: "x".to_string(),
            window_from: chrono::Utc::now() - chrono::Duration::hours(48),
            window_to: None,
            channels_scanned: Some(1),
            channels_in_results: 1,
        },
    };
    mock_client
        .expect_search_messages()
        .times(1)
        .return_once(move |_| Ok(enriched));
    // No expectation on resolve_channels / resolve_channel_identity /
    // get_channel_info: mockall panics if any of them is called — the
    // zero-resolve guarantee for the enrichment path.

    let (server, _rx) = test_server(mock_client); // reuse this file's existing constructor helper
    let request = SearchMessagesRequest {
        query: "x".to_string(),
        ..Default::default() // match how other tests in this file build it
    };
    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await;

    let text = result.expect("search succeeds");
    assert!(text.contains("\"channel_name\":\"Военкор\""), "response: {text}");
    assert!(text.contains("\"channel_username\":\"voenkor_ru\""));
    assert!(text.contains("\"post_author\":\"И. Петров\""));
}
```

(Adapt the server construction + request literal to the exact helpers this file already uses — copy from `search_messages_serializes_enrichment_fields` a few lines above; if `SearchMessagesRequest` has no `Default`, spell out its fields exactly as that test does. If serialized JSON uses spaces after colons, adjust the `contains` needles to the actual output — print it with `{text}` on first failure.)

- [ ] **Step 3: Run tests to verify the new test fails, then goes green**

Run: `cargo test search_messages_serializes_enriched_forward`
Expected: first COMPILE FAIL (missing fixture) → after Step 1-2 complete: PASS.

- [ ] **Step 4: Run gate**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "test: enriched forward serializes end-to-end with zero resolve calls"
```

---

### Task 10: Documentation

**Files:**
- Modify: `README.md:582` region (`forwarded_from` example) and the `resolve_channels` framing at lines ~647, ~708, ~1104, ~1156
- Modify: `CHANGELOG.md` (`## [Unreleased]`)
- Modify: `docs/memory.md`, `docs/tasklist.md`

- [ ] **Step 1: README**

Update the `forwarded_from` response example (~line 582) to the enriched shape:

```json
      "forwarded_from": {
        "channel_id": 1783384254,
        "channel_name": "Военкор",
        "channel_username": "voenkor_ru",
        "post_author": "И. Петров",
        "original_date": "2026-08-10T14:00:00Z",
        "original_message_id": 1863
      }
```

Then fix the now-stale claims: the text at ~647/1104/1156 says search/history results "never carry the forward source's name" and routes readers to `resolve_channels` — rewrite those sentences: attribution now comes inline from the same response envelope (`channel_name`/`channel_username`/`sender_name`/`post_author` when Telegram's envelope carries the source; ids-only otherwise), and `resolve_channels` remains for full channel entities of *subscribed* chats. Keep the ~708 sentence ("from the same Telegram response — no extra API calls") — it is now true for forwards too; extend it to say so. Grep first: `grep -n "forwarded_from\|forward source" README.md` and reconcile every hit.

- [ ] **Step 2: CHANGELOG under `## [Unreleased]`**

```markdown
### Added
- Forward attribution: `forwarded_from` on `search_messages` /
  `get_recent_messages` now carries `channel_name`, `channel_username`,
  `sender_name`, and `post_author`, resolved from the same MTProto response
  envelope (`chats`/`users`) — zero additional API calls; sources the account
  is not subscribed to are attributed too. Ids-only form is kept when the
  envelope lacks the peer. Additive and backward compatible.

### Changed
- `get_recent_messages` / `search_messages` fetch via raw
  `messages.GetHistory` / `messages.Search` / `messages.SearchGlobal`
  invocations (same requests, same pagination as the grammers iterators,
  which discard the envelope behind a crate-private `PeerMap`).
```

- [ ] **Step 3: docs/memory.md + docs/tasklist.md**

Append to `docs/memory.md` (follow its existing entry format — read the tail first): the pinned grammers rev (= upstream HEAD) keeps `Message.peers` crate-private, so envelope enrichment requires raw TL fetches; `EntityLookup`/raw pagers are the seam; a future grammers rev exposing the peer map would let the pagers collapse back to the iterators. Mark the enrichment work done in `docs/tasklist.md` if it tracks it.

- [ ] **Step 4: Full gate + commit**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test && cargo test config -- --test-threads=1`
Expected: PASS.

```bash
git add -A && git commit -m "docs: forward attribution enrichment — README example, changelog, memory"
```

---

## Verification (post-plan)

1. Full gate: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`.
2. Manual acceptance (live): build, run against live Telegram, fetch a channel that forwards from an unsubscribed source, confirm `forwarded_from.channel_name` is human-readable (the ID-only output was the bug).
3. `superpowers:requesting-code-review` before merge; then `superpowers:finishing-a-development-branch`.
