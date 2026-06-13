# Message Metadata Enrichment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enrich the message metadata returned by `search_messages` and `get_recent_messages` with forward attribution, link previews, view/forward counts, and the reply-parent message ID — extracted from the grammers `Message` already in hand, with zero extra API calls.

**Architecture:** Extraction lands in the single shared `convert_message` (grammers → domain) via two pure, client-free helpers, populating five new optional fields on the domain `Message`. The fields reach the wire through a new `MessageResponse`/`SearchResponse` DTO layer (`From` mappings) that the result-emitting tools serialize. All new fields are `Option` with `skip_serializing_if`, so plain messages serialize byte-identically to today.

**Tech Stack:** Rust 2024 nightly, `grammers-client` (high-level accessors + raw `grammers_client::tl` types), `serde` + `schemars` v1, `chrono`.

**Source spec:** `docs/superpowers/specs/2026-06-13-message-metadata-enrichment-design.md`

---

## File Structure

- **`src/telegram/types/entities.rs`** — add `ForwardInfo` and `LinkPreview` domain structs; add five optional fields to `Message`; serialization tests.
- **`src/telegram/types.rs`** — re-export `ForwardInfo`, `LinkPreview`.
- **`src/telegram/converters.rs`** — add pure helpers `extract_forward_info` / `extract_link_preview` (+ unit tests); wire them and the count accessors into `convert_message`.
- **`src/mcp/tools/types/responses.rs`** — add `MessageResponse` + `SearchResponse` DTOs with `From` impls (+ unit tests).
- **`src/mcp/tools/types.rs`** — re-export `MessageResponse`, `SearchResponse`.
- **`src/mcp/server.rs`** — serialize the DTOs from `search_messages_impl`, `get_recent_messages_impl`, `get_message_by_link_impl`; import the DTOs.
- **`src/test_helpers.rs`**, **`src/mcp/tests/{search,history}.rs`**, **`src/telegram/tests/client_tests.rs`** — default the five new fields to `None` in `Message` literals; add two builder helpers.
- **`README.md`**, **`CHANGELOG.md`** — document the new fields.

**Pre-flight (run once before Task 1):** `ast-index update` so symbol search reflects the branch.

---

## Task 1: Domain types + new Message fields

**Files:**
- Modify: `src/telegram/types/entities.rs`
- Modify: `src/telegram/types.rs`
- Modify: `src/test_helpers.rs:24-37`
- Modify: `src/mcp/tests/history.rs:16-29` (the local `create_test_message`)
- Modify: `src/mcp/tests/search.rs:21-32` and `:235-246` (two inline `Message` literals)
- Modify: `src/telegram/tests/client_tests.rs:26-39` (the local `create_test_message`)

- [ ] **Step 1: Write the failing serialization tests**

In `src/telegram/types/entities.rs`, inside `#[cfg(test)] mod tests`, add these two tests (the inline `create_test_message()` already exists there):

```rust
    #[test]
    fn message_omits_new_fields_when_absent() {
        let msg = create_test_message();
        let json = serde_json::to_value(&msg).unwrap();
        assert!(json.get("forwarded_from").is_none());
        assert!(json.get("link_preview").is_none());
        assert!(json.get("views").is_none());
        assert!(json.get("forwards").is_none());
        assert!(json.get("reply_to_message_id").is_none());
    }

    #[test]
    fn message_includes_new_fields_when_present() {
        let mut msg = create_test_message();
        msg.views = Some(1234);
        msg.forwards = Some(56);
        msg.reply_to_message_id = Some(MessageId::new(99).unwrap());
        msg.forwarded_from = Some(ForwardInfo {
            channel_id: Some(ChannelId::new(100).unwrap()),
            channel_name: None,
            channel_username: None,
            sender_name: None,
            original_date: None,
            original_message_id: Some(MessageId::new(7).unwrap()),
        });
        msg.link_preview = Some(LinkPreview {
            url: "https://example.com".to_string(),
            site_name: Some("Example".to_string()),
            title: Some("Title".to_string()),
            description: Some("Desc".to_string()),
        });

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["views"], 1234);
        assert_eq!(json["forwards"], 56);
        assert_eq!(json["reply_to_message_id"], 99);
        assert_eq!(json["forwarded_from"]["channel_id"], 100);
        assert_eq!(json["forwarded_from"]["original_message_id"], 7);
        assert!(json["forwarded_from"].get("channel_name").is_none());
        assert_eq!(json["link_preview"]["url"], "https://example.com");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib entities 2>&1 | head -30`
Expected: FAIL — compile error, `ForwardInfo`/`LinkPreview` not found and unknown `Message` fields.

- [ ] **Step 3: Add the domain types and fields**

In `src/telegram/types/entities.rs`, append the `forwarded_from`/`link_preview`/`views`/`forwards`/`reply_to_message_id` fields to `struct Message` (after `media_type`):

```rust
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub forwarded_from: Option<ForwardInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub link_preview: Option<LinkPreview>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub views: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub forwards: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reply_to_message_id: Option<MessageId>,
```

Then add the two structs after the `impl Message { ... }` block:

```rust
/// Attribution for a forwarded message.
///
/// `channel_name` / `channel_username` are intentionally never populated: the
/// grammers forward header carries only the source's numeric `from_id`, and the
/// resolved title/username live in the response's peer map (not exposed per
/// message). Filling them would require an extra resolve call, which the
/// zero-extra-call enrichment path must avoid.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ForwardInfo {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub channel_id: Option<ChannelId>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub channel_name: Option<ChannelName>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub channel_username: Option<Username>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sender_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub original_date: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub original_message_id: Option<MessageId>,
}

/// Telegram's server-side webpage preview attached to a message.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LinkPreview {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub site_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
}
```

The top-of-file imports (`ChannelId, MessageId, UserId`, `ChannelName, Username`, `DateTime, Utc`, `JsonSchema`, `Serialize, Deserialize`) already cover everything these structs need.

Update the inline `create_test_message()` in the same test module (after `media_type: MediaType::None,`):

```rust
            forwarded_from: None,
            link_preview: None,
            views: None,
            forwards: None,
            reply_to_message_id: None,
```

- [ ] **Step 4: Re-export the new types**

In `src/telegram/types.rs`, change the entities re-export line:

```rust
pub use entities::{Channel, ForwardInfo, LinkPreview, Message};
```

- [ ] **Step 5: Fix the remaining `Message` literals so the crate compiles**

Append the same five `None` fields (after `media_type: ...,`) to each of these `Message { ... }` literals:

- `src/test_helpers.rs:25` (in `create_test_message`)
- `src/mcp/tests/history.rs:17` (in the local `create_test_message`)
- `src/mcp/tests/search.rs:21` (inside `messages: vec![Message {`)
- `src/mcp/tests/search.rs:235` (inside `messages: vec![Message {`)
- `src/telegram/tests/client_tests.rs:27` (in the local `create_test_message`)

The lines to add in each:

```rust
        forwarded_from: None,
        link_preview: None,
        views: None,
        forwards: None,
        reply_to_message_id: None,
```

(`src/telegram/converters.rs:276` is also a `Message` literal but is rewritten in Task 3 — leave it for now; it will not compile until Task 3, so this task ends with the converter literal temporarily updated too: add the same five `None` fields there in this step, to keep the build green. Task 3 replaces the `None`s with real extraction.)

Add the five `None` fields to `src/telegram/converters.rs:276` (`Some(Message { ... })`) as well, after `media_type,`.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --lib entities 2>&1 | tail -20`
Expected: PASS, including `message_omits_new_fields_when_absent` and `message_includes_new_fields_when_present`.

- [ ] **Step 7: Confirm the whole crate still builds and tests pass**

Run: `cargo test 2>&1 | tail -20`
Expected: PASS (all existing tests green — the new optional fields default to `None` everywhere).

- [ ] **Step 8: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy -- -D warnings
git add src/telegram/types/entities.rs src/telegram/types.rs src/test_helpers.rs \
        src/mcp/tests/history.rs src/mcp/tests/search.rs \
        src/telegram/tests/client_tests.rs src/telegram/converters.rs
git commit -m "feat: add ForwardInfo, LinkPreview and enrichment fields to Message"
```

---

## Task 2: Pure extraction helpers

**Files:**
- Modify: `src/telegram/converters.rs` (imports, two helpers, tests in the existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing extraction tests**

In `src/telegram/converters.rs`, inside `#[cfg(test)] mod tests` (which already starts with `use super::*;`), add a TL builder helper and the tests:

```rust
    fn fwd_header() -> tl::types::MessageFwdHeader {
        tl::types::MessageFwdHeader {
            imported: false,
            saved_out: false,
            from_id: None,
            from_name: None,
            date: 1_700_000_000,
            channel_post: None,
            post_author: None,
            saved_from_peer: None,
            saved_from_msg_id: None,
            saved_from_id: None,
            saved_from_name: None,
            saved_date: None,
            psa_type: None,
        }
    }

    fn webpage_media(
        url: &str,
        site_name: Option<&str>,
        title: Option<&str>,
        description: Option<String>,
    ) -> tl::types::MessageMediaWebPage {
        tl::types::MessageMediaWebPage {
            force_large_media: false,
            force_small_media: false,
            manual: false,
            safe: false,
            webpage: tl::enums::WebPage::Page(tl::types::WebPage {
                has_large_media: false,
                video_cover_photo: false,
                id: 1,
                url: url.to_string(),
                display_url: url.to_string(),
                hash: 0,
                r#type: None,
                site_name: site_name.map(|s| s.to_string()),
                title: title.map(|s| s.to_string()),
                description,
                photo: None,
                embed_url: None,
                embed_type: None,
                embed_width: None,
                embed_height: None,
                duration: None,
                author: None,
                document: None,
                cached_page: None,
                attributes: None,
            }),
        }
    }

    #[test]
    fn forward_from_channel_extracts_ids_not_names() {
        let mut header = fwd_header();
        header.from_id = Some(tl::enums::Peer::Channel(tl::types::PeerChannel {
            channel_id: 555,
        }));
        header.channel_post = Some(42);

        let info = extract_forward_info(&header);
        assert_eq!(info.channel_id.map(|c| c.get()), Some(555));
        assert_eq!(info.original_message_id.map(|m| m.get()), Some(42));
        assert!(info.original_date.is_some());
        assert!(info.channel_name.is_none());
        assert!(info.channel_username.is_none());
        assert!(info.sender_name.is_none());
    }

    #[test]
    fn forward_from_hidden_user_has_name_only() {
        let mut header = fwd_header();
        header.from_name = Some("Hidden User".to_string());

        let info = extract_forward_info(&header);
        assert_eq!(info.sender_name.as_deref(), Some("Hidden User"));
        assert!(info.channel_id.is_none());
        assert!(info.original_message_id.is_none());
    }

    #[test]
    fn link_preview_extracted_and_description_truncated_to_500_chars() {
        let media = webpage_media(
            "https://example.com/article",
            Some("Example"),
            Some("Headline"),
            Some("я".repeat(600)),
        );

        let preview = extract_link_preview(&media).unwrap();
        assert_eq!(preview.url, "https://example.com/article");
        assert_eq!(preview.site_name.as_deref(), Some("Example"));
        assert_eq!(preview.title.as_deref(), Some("Headline"));
        assert_eq!(preview.description.as_ref().unwrap().chars().count(), 500);
    }

    #[test]
    fn link_preview_empty_webpage_returns_none() {
        let media = tl::types::MessageMediaWebPage {
            force_large_media: false,
            force_small_media: false,
            manual: false,
            safe: false,
            webpage: tl::enums::WebPage::Empty(tl::types::WebPageEmpty {
                id: 0,
                url: None,
            }),
        };
        assert!(extract_link_preview(&media).is_none());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib converters 2>&1 | head -20`
Expected: FAIL — compile error, `extract_forward_info` / `extract_link_preview` not found.

- [ ] **Step 3: Add the helpers and imports**

In `src/telegram/converters.rs`, extend the domain-types import to include the two new types:

```rust
use crate::telegram::types::{
    Channel, ChannelId, ChannelName, ForwardInfo, LinkPreview, MediaFilter, MediaType, Message,
    MessageId, SizeCandidate, UserId, Username,
};
```

Add a chrono import below the existing `use grammers_client::tl;` line:

```rust
use chrono::{DateTime, Utc};
```

Add the two helpers (place them just above `pub fn convert_message`):

```rust
/// Extract forward attribution from a raw forward header.
///
/// Drops down to the raw TL `MessageFwdHeader` because grammers' high-level API
/// exposes only `forward_header()` (the raw enum). `channel_name`/`channel_username`
/// are left `None`: `from_id` is an ID-only TL `Peer`, and the resolved
/// title/username are not available without an extra resolve call.
fn extract_forward_info(header: &tl::types::MessageFwdHeader) -> ForwardInfo {
    let channel_id = match &header.from_id {
        Some(tl::enums::Peer::Channel(ch)) => ChannelId::new(ch.channel_id).ok(),
        _ => None,
    };

    ForwardInfo {
        channel_id,
        channel_name: None,
        channel_username: None,
        sender_name: header.from_name.clone(),
        original_date: DateTime::<Utc>::from_timestamp(header.date as i64, 0),
        original_message_id: header
            .channel_post
            .and_then(|id| MessageId::new(id as i64).ok()),
    }
}

/// Extract a link preview from a raw webpage media block.
///
/// Only the `WebPage::Page` variant carries content; `Empty`/`Pending`/`NotModified`
/// yield `None`. `description` is truncated to 500 Unicode scalar values so multi-byte
/// (e.g. Cyrillic) text is never split mid-codepoint.
fn extract_link_preview(media: &tl::types::MessageMediaWebPage) -> Option<LinkPreview> {
    match &media.webpage {
        tl::enums::WebPage::Page(page) => Some(LinkPreview {
            url: page.url.clone(),
            site_name: page.site_name.clone(),
            title: page.title.clone(),
            description: page
                .description
                .as_ref()
                .map(|d| d.chars().take(500).collect()),
        }),
        _ => None,
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib converters 2>&1 | tail -20`
Expected: PASS — all four new tests plus the existing `select_size_candidate` tests.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy -- -D warnings
git add src/telegram/converters.rs
git commit -m "feat: add pure extract_forward_info / extract_link_preview helpers"
```

---

## Task 3: Wire extraction into convert_message

**Files:**
- Modify: `src/telegram/converters.rs:259-288` (the `convert_message` body)

> This step is integration glue. `convert_message` takes a concrete grammers `Message`, which cannot be constructed in a unit test without a live `Client`; the extraction logic it calls is already unit-tested in Task 2, and serialization is covered in Tasks 1 and 5. Verification here is "the whole suite stays green and clippy is clean."

- [ ] **Step 1: Replace the media match and add the enrichment bindings**

In `src/telegram/converters.rs`, replace the existing media block:

```rust
    // Check for media and detect its type
    let (has_media, media_type) = match msg.media() {
        Some(media) => (true, convert_media_to_type(&media)),
        None => (false, MediaType::None),
    };
```

with:

```rust
    // Check for media and detect its type (computed once; reused for link preview)
    let media = msg.media();
    let (has_media, media_type) = match &media {
        Some(m) => (true, convert_media_to_type(m)),
        None => (false, MediaType::None),
    };

    // Enrichment (all derived from data already in `msg` — no network calls):
    let link_preview = match &media {
        Some(Media::WebPage(wp)) => extract_link_preview(&wp.raw),
        _ => None,
    };

    let forwarded_from = match msg.forward_header() {
        Some(tl::enums::MessageFwdHeader::Header(header)) => Some(extract_forward_info(&header)),
        None => None,
    };

    let views = msg.view_count().and_then(|v| u64::try_from(v).ok());
    let forwards = msg.forward_count().and_then(|v| u64::try_from(v).ok());
    let reply_to_message_id = msg
        .reply_to_message_id()
        .and_then(|id| MessageId::new(id as i64).ok());
```

- [ ] **Step 2: Populate the new fields in the returned `Message`**

In the same function, replace the five temporary `None` lines added in Task 1 Step 5 (inside `Some(Message { ... })`) so the tail reads:

```rust
    Some(Message {
        id: message_id,
        channel_id,
        channel_name,
        channel_username,
        text: msg.text().to_string(),
        timestamp: msg.date(),
        sender_id,
        sender_name,
        has_media,
        media_type,
        forwarded_from,
        link_preview,
        views,
        forwards,
        reply_to_message_id,
    })
```

- [ ] **Step 3: Build and run the full suite**

Run: `cargo test 2>&1 | tail -20`
Expected: PASS (no behavior change visible to existing tests; the mock client path is unaffected).

- [ ] **Step 4: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy -- -D warnings
git add src/telegram/converters.rs
git commit -m "feat: populate forward/link-preview/views/forwards/reply in convert_message"
```

---

## Task 4: Response DTO layer

**Files:**
- Modify: `src/mcp/tools/types/responses.rs` (imports, two DTOs, two `From` impls, tests)
- Modify: `src/mcp/tools/types.rs` (re-export)

- [ ] **Step 1: Write the failing DTO tests**

In `src/mcp/tools/types/responses.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn message_response_maps_and_omits_absent_fields() {
        use crate::telegram::types::{ChannelId, ChannelName, MediaType, Message, MessageId, Username};

        let msg = Message {
            id: MessageId::new(1).unwrap(),
            channel_id: ChannelId::new(100).unwrap(),
            channel_name: ChannelName::new("Test").unwrap(),
            channel_username: Username::new("testchan").unwrap(),
            text: "hi".to_string(),
            timestamp: chrono::Utc::now(),
            sender_id: None,
            sender_name: None,
            has_media: false,
            media_type: MediaType::None,
            forwarded_from: None,
            link_preview: None,
            views: Some(10),
            forwards: None,
            reply_to_message_id: None,
        };

        let dto = MessageResponse::from(msg);
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["views"], 10);
        assert!(json.get("forwards").is_none());
        assert!(json.get("forwarded_from").is_none());
        // sender_id mirrors the domain type: present as null, not skipped.
        assert!(json.get("sender_id").is_some());
        assert!(json["sender_id"].is_null());
    }

    #[test]
    fn search_response_maps_from_search_result() {
        use crate::telegram::types::{
            ChannelId, ChannelName, MediaType, Message, MessageId, QueryMetadata, SearchResult,
            Username,
        };

        let result = SearchResult {
            messages: vec![Message {
                id: MessageId::new(1).unwrap(),
                channel_id: ChannelId::new(100).unwrap(),
                channel_name: ChannelName::new("Test").unwrap(),
                channel_username: Username::new("testchan").unwrap(),
                text: "hi".to_string(),
                timestamp: chrono::Utc::now(),
                sender_id: None,
                sender_name: None,
                has_media: false,
                media_type: MediaType::None,
                forwarded_from: None,
                link_preview: None,
                views: None,
                forwards: None,
                reply_to_message_id: None,
            }],
            total_found: 1,
            search_time_ms: 5,
            query_metadata: QueryMetadata {
                query: "x".to_string(),
                hours_back: 48,
                channels_searched: 1,
            },
        };

        let dto = SearchResponse::from(result);
        assert_eq!(dto.messages.len(), 1);
        assert_eq!(dto.total_found, 1);
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["query_metadata"]["query"], "x");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib responses 2>&1 | head -20`
Expected: FAIL — `MessageResponse` / `SearchResponse` not found.

- [ ] **Step 3: Add imports and the DTOs**

In `src/mcp/tools/types/responses.rs`, replace the import header:

```rust
use crate::telegram::types::{Channel, MediaType};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
```

with:

```rust
use crate::telegram::types::{
    Channel, ChannelId, ChannelName, ForwardInfo, LinkPreview, MediaType, Message, MessageId,
    QueryMetadata, SearchResult, UserId, Username,
};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
```

Append the DTOs (at the end of the file, before `#[cfg(test)] mod tests`):

```rust
/// Wire representation of a single message (mirrors the domain `Message`).
///
/// `sender_id` / `sender_name` mirror the domain type exactly (serialized as `null`
/// when absent, no `skip_serializing_if`) to preserve the existing wire format; the
/// enrichment fields are omitted when absent.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MessageResponse {
    pub id: MessageId,
    pub channel_id: ChannelId,
    pub channel_name: ChannelName,
    pub channel_username: Username,
    pub text: String,
    pub timestamp: DateTime<Utc>,
    pub sender_id: Option<UserId>,
    pub sender_name: Option<String>,
    pub has_media: bool,
    pub media_type: MediaType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub forwarded_from: Option<ForwardInfo>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub link_preview: Option<LinkPreview>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub views: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub forwards: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reply_to_message_id: Option<MessageId>,
}

impl From<Message> for MessageResponse {
    fn from(m: Message) -> Self {
        Self {
            id: m.id,
            channel_id: m.channel_id,
            channel_name: m.channel_name,
            channel_username: m.channel_username,
            text: m.text,
            timestamp: m.timestamp,
            sender_id: m.sender_id,
            sender_name: m.sender_name,
            has_media: m.has_media,
            media_type: m.media_type,
            forwarded_from: m.forwarded_from,
            link_preview: m.link_preview,
            views: m.views,
            forwards: m.forwards,
            reply_to_message_id: m.reply_to_message_id,
        }
    }
}

/// Wire representation of a search/history result set.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchResponse {
    pub messages: Vec<MessageResponse>,
    pub total_found: u64,
    pub search_time_ms: u64,
    pub query_metadata: QueryMetadata,
}

impl From<SearchResult> for SearchResponse {
    fn from(r: SearchResult) -> Self {
        Self {
            messages: r.messages.into_iter().map(MessageResponse::from).collect(),
            total_found: r.total_found,
            search_time_ms: r.search_time_ms,
            query_metadata: r.query_metadata,
        }
    }
}
```

- [ ] **Step 4: Re-export the DTOs**

In `src/mcp/tools/types.rs`, extend the `pub use responses::{ ... };` list to include `MessageResponse` and `SearchResponse`:

```rust
pub use responses::{
    BufferedResponseEntry, ChannelsResponse, GetMessageMediaResponse, LastResponsesResponse,
    MessageLinkResponse, MessageResponse, OpenMessageResponse, SearchResponse, StatusResponse,
};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib responses 2>&1 | tail -20`
Expected: PASS — both new tests plus existing response tests.

- [ ] **Step 6: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy -- -D warnings
git add src/mcp/tools/types/responses.rs src/mcp/tools/types.rs
git commit -m "feat: add MessageResponse / SearchResponse DTOs with From mappings"
```

---

## Task 5: Serialize the DTOs from the tools

**Files:**
- Modify: `src/mcp/server.rs:5-11` (import block)
- Modify: `src/mcp/server.rs:303`, `:383`, `:420` (the three serialize calls)
- Modify: `src/mcp/tests/search.rs` (add one end-to-end contract test)

> The DTO produces JSON identical to today's domain serialization (the domain `Message`/`SearchResult` already serialize these fields after Tasks 1–3), so this is a structural refactor to the response boundary, not a wire-format change. The contract test below pins the enriched-message output through the real tool path.

- [ ] **Step 1: Write the end-to-end contract test**

In `src/mcp/tests/search.rs`, add (the module already imports `Message`, `SearchResult`, the newtypes, `MediaType`, `QueryMetadata`):

```rust
#[tokio::test]
async fn search_messages_serializes_enrichment_fields() {
    use crate::telegram::types::{ForwardInfo, LinkPreview};

    let mut mock_client = MockTelegramClientTrait::new();
    let enriched = SearchResult {
        messages: vec![Message {
            id: MessageId::new(1).unwrap(),
            channel_id: ChannelId::new(123).unwrap(),
            channel_name: ChannelName::new("Test Channel").unwrap(),
            channel_username: Username::new("testchannel").unwrap(),
            text: "forwarded post".to_string(),
            timestamp: chrono::Utc::now(),
            sender_id: None,
            sender_name: None,
            has_media: false,
            media_type: MediaType::None,
            forwarded_from: Some(ForwardInfo {
                channel_id: Some(ChannelId::new(555).unwrap()),
                channel_name: None,
                channel_username: None,
                sender_name: None,
                original_date: None,
                original_message_id: Some(MessageId::new(42).unwrap()),
            }),
            link_preview: Some(LinkPreview {
                url: "https://example.com".to_string(),
                site_name: Some("Example".to_string()),
                title: Some("Title".to_string()),
                description: Some("Desc".to_string()),
            }),
            views: Some(999),
            forwards: Some(12),
            reply_to_message_id: None,
        }],
        total_found: 1,
        search_time_ms: 10,
        query_metadata: QueryMetadata {
            query: "x".to_string(),
            hours_back: 48,
            channels_searched: 1,
        },
    };

    mock_client
        .expect_search_messages()
        .returning(move |_| Ok(enriched.clone()));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = SearchRequest {
        query: "x".to_string(),
        channel_id: None,
        hours_back: None,
        limit: None,
        media_filter: None,
    };

    let result = server
        .search_messages(Parameters(request), RequestId(NumberOrString::Number(1)))
        .await
        .unwrap();

    let json: serde_json::Value = serde_json::from_str(&result).unwrap();
    let msg = &json["messages"][0];
    assert_eq!(msg["views"], 999);
    assert_eq!(msg["forwards"], 12);
    assert_eq!(msg["forwarded_from"]["channel_id"], 555);
    assert_eq!(msg["forwarded_from"]["original_message_id"], 42);
    assert!(msg["forwarded_from"].get("channel_name").is_none());
    assert_eq!(msg["link_preview"]["url"], "https://example.com");
    // Plain-message backward compat: sender_id still present as null.
    assert!(msg["sender_id"].is_null());
}
```

- [ ] **Step 2: Run the test (it passes via domain serialization first)**

Run: `cargo test --lib search_messages_serializes_enrichment_fields 2>&1 | tail -20`
Expected: PASS — confirms the enriched contract before the refactor (domain `SearchResult` already serializes these fields). This is the baseline the refactor must preserve.

- [ ] **Step 3: Import the DTOs in server.rs**

In `src/mcp/server.rs`, add `MessageResponse` and `SearchResponse` to the `use crate::mcp::tools::{ ... };` block (keep alphabetical grouping):

```rust
use crate::mcp::tools::{
    BufferedResponseEntry, ChannelsResponse, GenerateLinkRequest, GetChannelInfoRequest,
    GetChannelsRequest, GetLastResponsesRequest, GetMessageByLinkRequest, GetMessageMediaRequest,
    GetMessageMediaResponse, GetRecentMessagesRequest, LastResponsesResponse, MessageLinkResponse,
    MessageResponse, OpenMessageRequest, OpenMessageResponse, SearchRequest, SearchResponse,
    StatusResponse, parse_channel_id, parse_message_id, parse_optional_channel_id,
};
```

- [ ] **Step 4: Swap the three serialize calls to the DTOs**

`src/mcp/server.rs:303` (end of `search_messages_impl`):

```rust
        serde_json::to_string(&SearchResponse::from(result)).map_err(|e| e.to_string())
```

`src/mcp/server.rs:383` (end of `get_recent_messages_impl`):

```rust
        serde_json::to_string(&SearchResponse::from(result)).map_err(|e| e.to_string())
```

`src/mcp/server.rs:420` (end of `get_message_by_link_impl`):

```rust
        serde_json::to_string(&MessageResponse::from(message)).map_err(|e| e.to_string())
```

(The per-result logging at lines 288 and 370 borrows `result` before these moves and is unaffected.)

- [ ] **Step 5: Run the full suite**

Run: `cargo test 2>&1 | tail -25`
Expected: PASS — the new contract test, all existing `search`/`history`/`message_by_link` tests (they deserialize into the domain `SearchResult`/`Message`, which ignores nothing new and matches all existing keys).

- [ ] **Step 6: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy -- -D warnings
git add src/mcp/server.rs src/mcp/tests/search.rs
git commit -m "feat: serialize MessageResponse/SearchResponse DTOs from message tools"
```

---

## Task 6: test_helpers builders + documentation

**Files:**
- Modify: `src/test_helpers.rs` (two new builders)
- Modify: `README.md` (response example + note)
- Modify: `CHANGELOG.md` (`[Unreleased]` entry)

- [ ] **Step 1: Add forward/link-preview test builders**

In `src/test_helpers.rs`, first extend the imports to include the new types:

```rust
use crate::telegram::types::{
    Channel, ChannelId, ChannelName, ForwardInfo, LinkPreview, MediaType, Message, MessageId,
    QueryMetadata, SearchResult, UserId, Username,
};
```

Then append after `create_test_message_with_sender`:

```rust
/// Create a test message carrying forward attribution (channel forward).
pub fn create_test_message_with_forward(
    id: i64,
    text: &str,
    channel_id: i64,
    forwarded_channel_id: i64,
    original_message_id: i64,
) -> Message {
    let mut msg = create_test_message(id, text, channel_id);
    msg.forwarded_from = Some(ForwardInfo {
        channel_id: Some(ChannelId::new(forwarded_channel_id).expect("valid channel id")),
        channel_name: None,
        channel_username: None,
        sender_name: None,
        original_date: None,
        original_message_id: Some(MessageId::new(original_message_id).expect("valid message id")),
    });
    msg
}

/// Create a test message carrying a link preview.
pub fn create_test_message_with_link_preview(
    id: i64,
    text: &str,
    channel_id: i64,
    url: &str,
) -> Message {
    let mut msg = create_test_message(id, text, channel_id);
    msg.link_preview = Some(LinkPreview {
        url: url.to_string(),
        site_name: None,
        title: None,
        description: None,
    });
    msg
}
```

- [ ] **Step 2: Verify the builders compile (used by the test crate)**

Run: `cargo test --lib test_helpers 2>&1 | tail -15`
Expected: PASS or "no tests" — the point is a clean compile (these helpers are referenced by the test build; a warning about unused functions is acceptable since they mirror the existing `create_test_message_with_*` style).

Note: if clippy flags the new builders as `dead_code`, mirror whatever the existing `create_test_message_with_sender` uses (it is already part of the test surface, so no attribute is needed). Do not add `#[allow(dead_code)]` unless the existing helpers have it.

- [ ] **Step 3: Extend the README response example**

In `README.md`, the `search_messages` response example (the message object at lines ~405-416) currently ends at `"media_type": "none"`. Replace that single example message object with two, showing the enrichment:

```json
    {
      "id": 42,
      "channel_id": 1234567890,
      "channel_name": "Tech News",
      "channel_username": "technews",
      "text": "Breaking: New AI model released...",
      "timestamp": "2025-12-28T10:30:00Z",
      "sender_id": 987654321,
      "sender_name": "John Doe",
      "has_media": false,
      "media_type": "none",
      "views": 15000,
      "forwards": 230,
      "forwarded_from": {
        "channel_id": 1009988776,
        "original_message_id": 8123,
        "original_date": "2025-12-28T09:00:00Z"
      },
      "link_preview": {
        "url": "https://example.com/ai-model",
        "site_name": "Example",
        "title": "New AI model released",
        "description": "A short summary pulled from Telegram's server-side preview..."
      },
      "reply_to_message_id": 41
    }
```

Immediately after that response code block (before the "Media Types:" line), add the note:

```markdown
**Forward attribution & link previews:** Messages carry optional enrichment derived
from the same Telegram response — no extra API calls. `forwarded_from` attributes a
forwarded post to its source (`channel_id`, `original_message_id`, `original_date`,
and `sender_name` for hidden senders); the source channel's **title and username are
not included** — Telegram does not expose them per message without an extra lookup,
so pair `channel_id` with `generate_message_link` if you need to reach the source.
`link_preview` surfaces Telegram's server-side webpage preview (`url`, `site_name`,
`title`, `description`, truncated to 500 characters). `views`, `forwards`, and
`reply_to_message_id` are included when present. All of these fields are omitted
entirely when absent, so existing consumers are unaffected.
```

The `get_recent_messages` section already says its response reuses the same format — no separate example edit is required, but confirm its "Same format as `search_messages`" note (line ~464) still reads correctly.

- [ ] **Step 4: Add the CHANGELOG entry**

In `CHANGELOG.md`, under `## [Unreleased]`, add an `### Added` block:

```markdown
### Added
- `search_messages` and `get_recent_messages` now enrich each message with optional, zero-extra-API-call metadata extracted from the Telegram response: `forwarded_from` (forward attribution — source `channel_id`, `original_message_id`, `original_date`, and `sender_name` for hidden senders; the source channel's title/username are not exposed per message and are intentionally omitted), `link_preview` (Telegram's server-side webpage preview — `url`, `site_name`, `title`, `description` truncated to 500 characters), `views`, `forwards`, and `reply_to_message_id`. All fields are omitted when absent, so existing response consumers are unaffected. `get_message_by_link` returns the same enriched message shape. Internally the message wire format moved to dedicated `MessageResponse`/`SearchResponse` DTOs mapped from the domain types.
```

- [ ] **Step 5: Final full gate**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test 2>&1 | tail -25`
Expected: all three pass; test summary shows the new tests green.

- [ ] **Step 6: Commit**

```bash
git add src/test_helpers.rs README.md CHANGELOG.md
git commit -m "docs: document forward attribution, link previews, and engagement fields"
```

---

## Self-Review

**Spec coverage** (each spec section → task):
- ForwardInfo/LinkPreview domain types + Message fields → Task 1.
- Pure extraction helpers (zero-call by construction) + the five required test cases → Task 2 (channel forward, hidden-user forward, link preview + 500-char truncation, empty webpage) and Task 1 (plain message omits fields; views/forwards present-vs-absent).
- Wire extraction into the single `convert_message` path → Task 3 (also auto-enriches `get_message_by_link`).
- DTO layer + `From` mappings → Task 4; tools serialize DTOs → Task 5.
- Backward compatibility (skip_serializing_if; `sender_id`/`sender_name` still null; existing `from_str::<SearchResult>` tests pass) → preserved in Tasks 1, 4, 5 and verified by the full suite.
- README + CHANGELOG → Task 6.
- Non-goals (no download, no webpage fetch, no resolve calls) → respected; no task adds a client call.

**Placeholder scan:** No TBD/TODO; every code step shows complete code; every test shows assertions; exact file paths and line anchors throughout.

**Type consistency:** `ForwardInfo`/`LinkPreview` field names and types are identical across entities.rs (Task 1), the helpers (Task 2), the DTO (Task 4), and the builders/tests (Tasks 5–6). `extract_forward_info` returns `ForwardInfo` (non-Option) and is wrapped in `Some(..)` at the single forward-header call site (Task 3). `extract_link_preview` returns `Option<LinkPreview>`. Counts use `u64::try_from` (drops negatives); `original_date` uses `DateTime::<Utc>::from_timestamp`; IDs use the `MessageId`/`ChannelId` newtype constructors via `.ok()`.

**Note on line anchors:** line numbers reflect the branch at planning time; if they have drifted, locate the symbol named in each step (`ast-index outline <file>`) and apply the edit there.
