# Get Message by Link — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add MCP tool 8 (`get_message_by_link`) that retrieves a single Telegram message by its `t.me` URL.

**Architecture:** URL parsing in `src/link.rs`, new trait method `get_message_by_id` on `TelegramClientTrait`, tool implementation in `server.rs`. Follows the existing trait-based DI pattern — mock the trait in tests, call grammers in production.

**Tech Stack:** Rust nightly, rmcp 0.15, grammers (git master), mockall, schemars v1, serde, tokio

**Spec:** `docs/superpowers/specs/2026-04-12-get-message-by-link-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `src/link.rs` | Modify | Add `ChannelRef` enum + `parse_telegram_link` function + tests |
| `src/telegram/trait_def.rs` | Modify | Add `get_message_by_id` to trait |
| `src/telegram/client.rs` | Modify | Implement `get_message_by_id` for `TelegramClient` |
| `src/mcp/tools/types/requests.rs` | Modify | Add `GetMessageByLinkRequest` |
| `src/mcp/tools/types.rs` | Modify | Re-export new request type |
| `src/mcp/server.rs` | Modify | Add tool 8 `get_message_by_link` + import |
| `src/mcp/tests/message_by_link.rs` | Create | MCP tool integration tests (mock-based) |
| `src/mcp/tests.rs` | Modify | Register new test module |

---

### Task 1: URL Parser — `parse_telegram_link` in `src/link.rs`

**Files:**
- Modify: `src/link.rs`

- [ ] **Step 1: Add `ChannelRef` enum and `parse_telegram_link` function signature**

Add these at the top of `src/link.rs`, after the existing imports:

```rust
use crate::error::Error;

/// Reference to a channel — either by username or numeric ID.
/// Used when parsing t.me links where the channel can be identified either way.
#[derive(Debug, Clone, PartialEq)]
pub enum ChannelRef {
    Username(String),
    Id(ChannelId),
}

/// Parse a Telegram message link into channel reference and message ID.
///
/// Supported formats:
/// - `https://t.me/username/12345`
/// - `https://t.me/c/12345/67890` (private channel)
/// - `http://t.me/...`
/// - `t.me/...` (no scheme)
///
/// Query parameters (e.g. `?single`) and trailing slashes are stripped.
pub fn parse_telegram_link(link: &str) -> Result<(ChannelRef, MessageId), Error> {
    // Strip scheme if present
    let path = link
        .strip_prefix("https://")
        .or_else(|| link.strip_prefix("http://"))
        .unwrap_or(link);

    // Must start with t.me/
    let path = path
        .strip_prefix("t.me/")
        .ok_or_else(|| Error::InvalidInput(format!("Not a valid t.me link: {}", link)))?;

    // Strip query parameters and trailing slashes
    let path = path.split('?').next().unwrap_or(path);
    let path = path.trim_end_matches('/');

    // Split into segments
    let segments: Vec<&str> = path.split('/').collect();

    match segments.as_slice() {
        // Private channel: t.me/c/{channel_id}/{message_id}
        ["c", channel_id_str, message_id_str] => {
            let channel_id: i64 = channel_id_str.parse().map_err(|_| {
                Error::InvalidInput(format!("Invalid channel ID in link: {}", channel_id_str))
            })?;
            let message_id: i64 = message_id_str.parse().map_err(|_| {
                Error::InvalidInput(format!("Invalid message ID in link: {}", message_id_str))
            })?;

            Ok((
                ChannelRef::Id(ChannelId::new(channel_id)?),
                MessageId::new(message_id)?,
            ))
        }
        // Public channel: t.me/{username}/{message_id}
        [username, message_id_str] => {
            let message_id: i64 = message_id_str.parse().map_err(|_| {
                Error::InvalidInput(format!("Invalid message ID in link: {}", message_id_str))
            })?;

            Ok((
                ChannelRef::Username(username.to_string()),
                MessageId::new(message_id)?,
            ))
        }
        _ => Err(Error::InvalidInput(format!(
            "Invalid t.me link format: {}. Expected t.me/username/message_id or t.me/c/channel_id/message_id",
            link
        ))),
    }
}
```

- [ ] **Step 2: Write tests for `parse_telegram_link`**

Add inside the existing `#[cfg(test)] mod tests` block in `src/link.rs`, after the last existing test:

```rust
    // =========================================================================
    // parse_telegram_link Tests
    // =========================================================================

    #[test]
    fn parse_public_link_https() {
        let (channel_ref, msg_id) =
            parse_telegram_link("https://t.me/swodki/575403").unwrap();
        assert_eq!(channel_ref, ChannelRef::Username("swodki".to_string()));
        assert_eq!(msg_id.get(), 575403);
    }

    #[test]
    fn parse_public_link_http() {
        let (channel_ref, msg_id) =
            parse_telegram_link("http://t.me/swodki/575403").unwrap();
        assert_eq!(channel_ref, ChannelRef::Username("swodki".to_string()));
        assert_eq!(msg_id.get(), 575403);
    }

    #[test]
    fn parse_public_link_no_scheme() {
        let (channel_ref, msg_id) =
            parse_telegram_link("t.me/swodki/575403").unwrap();
        assert_eq!(channel_ref, ChannelRef::Username("swodki".to_string()));
        assert_eq!(msg_id.get(), 575403);
    }

    #[test]
    fn parse_private_link() {
        let (channel_ref, msg_id) =
            parse_telegram_link("https://t.me/c/1234567/575403").unwrap();
        assert_eq!(
            channel_ref,
            ChannelRef::Id(ChannelId::new(1234567).unwrap())
        );
        assert_eq!(msg_id.get(), 575403);
    }

    #[test]
    fn parse_link_with_query_params() {
        let (channel_ref, msg_id) =
            parse_telegram_link("https://t.me/swodki/575403?single").unwrap();
        assert_eq!(channel_ref, ChannelRef::Username("swodki".to_string()));
        assert_eq!(msg_id.get(), 575403);
    }

    #[test]
    fn parse_link_with_trailing_slash() {
        let (channel_ref, msg_id) =
            parse_telegram_link("https://t.me/swodki/575403/").unwrap();
        assert_eq!(channel_ref, ChannelRef::Username("swodki".to_string()));
        assert_eq!(msg_id.get(), 575403);
    }

    #[test]
    fn parse_link_invalid_domain() {
        let result = parse_telegram_link("https://example.com/swodki/575403");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Not a valid t.me link"));
    }

    #[test]
    fn parse_link_missing_message_id() {
        let result = parse_telegram_link("https://t.me/swodki");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid t.me link format"));
    }

    #[test]
    fn parse_link_non_numeric_message_id() {
        let result = parse_telegram_link("https://t.me/swodki/abc");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid message ID"));
    }

    #[test]
    fn parse_link_empty_string() {
        let result = parse_telegram_link("");
        assert!(result.is_err());
    }

    #[test]
    fn parse_private_link_invalid_channel_id() {
        let result = parse_telegram_link("https://t.me/c/abc/575403");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid channel ID"));
    }
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test link::tests::parse_ -- --nocapture`

Expected: All 11 new `parse_*` tests PASS. Existing `message_link_*` tests still PASS.

- [ ] **Step 4: Run full pre-commit checks**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test link`

Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/link.rs
git commit -m "feat: add parse_telegram_link for t.me URL parsing

Adds ChannelRef enum and parse_telegram_link function to extract
channel reference and message ID from t.me links. Supports public
(username-based) and private (numeric ID) link formats, with or
without scheme, query params, and trailing slashes."
```

---

### Task 2: Trait Method — `get_message_by_id` on `TelegramClientTrait`

**Files:**
- Modify: `src/telegram/trait_def.rs`

- [ ] **Step 1: Add the method to the trait**

In `src/telegram/trait_def.rs`, add the new method to the `TelegramClientTrait` trait, after the `is_connected` method:

```rust
    /// Get a single message by its ID from a specific channel.
    ///
    /// The `channel_ref` can be a username (e.g. "swodki") or a numeric ID string (e.g. "1234567").
    /// Uses grammers' `get_messages_by_id` under the hood.
    async fn get_message_by_id(&self, channel_ref: &str, message_id: i32) -> Result<Message, Error>;
```

Also update the import at the top of the file — add `Message` to the existing use statement:

```rust
use crate::telegram::types::{Channel, HistoryParams, Message, SearchParams, SearchResult};
```

- [ ] **Step 2: Verify compilation (will fail until client impl is added)**

Run: `cargo check 2>&1 | head -20`

Expected: Compilation error about `TelegramClient` not implementing `get_message_by_id`. This confirms the trait now requires it.

- [ ] **Step 3: Commit**

```bash
git add src/telegram/trait_def.rs
git commit -m "feat: add get_message_by_id to TelegramClientTrait

New trait method for fetching a single message by channel reference
and message ID. Channel ref can be username or numeric ID string."
```

---

### Task 3: Client Implementation — `get_message_by_id` for `TelegramClient`

**Files:**
- Modify: `src/telegram/client.rs`

- [ ] **Step 1: Implement `get_message_by_id`**

In `src/telegram/client.rs`, add this method inside the `#[async_trait::async_trait] impl TelegramClientTrait for TelegramClient` block, after `get_recent_messages`:

```rust
    async fn get_message_by_id(
        &self,
        channel_ref: &str,
        message_id: i32,
    ) -> Result<crate::telegram::Message, Error> {
        // Validate input
        if channel_ref.is_empty() {
            return Err(Error::InvalidInput(
                "Channel reference cannot be empty".to_string(),
            ));
        }

        // Resolve the channel peer (same pattern as get_channel_info)
        let peer = if let Ok(id) = channel_ref.parse::<i64>() {
            // Numeric ID — search through dialogs
            let mut dialogs = self.client.iter_dialogs();
            let mut found = None;

            while let Some(dialog) = dialogs.next().await.map_err(|e| {
                tracing::error!(error = %e, "Failed to iterate dialogs in get_message_by_id");
                Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
            })? {
                if dialog.peer().id().bare_id() == id {
                    found = Some(dialog.peer().clone());
                    break;
                }
            }

            found.ok_or_else(|| {
                tracing::warn!(id, "Channel not found in dialogs by ID");
                Error::InvalidInput(format!("Channel not found: {}", channel_ref))
            })?
        } else {
            // Username — resolve directly
            let username = channel_ref.strip_prefix('@').unwrap_or(channel_ref);
            self.client
                .resolve_username(username)
                .await
                .map_err(|e| {
                    tracing::error!(username = %username, error = %e, "Failed to resolve username");
                    Error::TelegramApi(format!("Failed to resolve username: {}", e))
                })?
                .ok_or_else(|| {
                    tracing::warn!(username = %username, "Username not found");
                    Error::InvalidInput(format!("Channel not found: {}", channel_ref))
                })?
        };

        // Get message by ID using grammers API
        let peer_ref = peer
            .to_ref()
            .await
            .ok_or_else(|| Error::TelegramApi("Failed to convert peer to PeerRef".to_string()))?;

        let messages = self
            .client
            .get_messages_by_id(peer_ref, &[message_id])
            .await
            .map_err(|e| {
                tracing::error!(
                    channel_ref = %channel_ref,
                    message_id,
                    error = %e,
                    "Failed to get message by ID"
                );
                Error::TelegramApi(format!("Failed to get message: {}", e))
            })?;

        // get_messages_by_id returns Vec<Option<Message>> — extract the single result
        let grammers_msg = messages
            .into_iter()
            .next()
            .flatten()
            .ok_or_else(|| {
                tracing::warn!(
                    channel_ref = %channel_ref,
                    message_id,
                    "Message not found"
                );
                Error::InvalidInput(format!(
                    "Message {} not found in channel {}",
                    message_id, channel_ref
                ))
            })?;

        // Convert to our domain type
        convert_message(&grammers_msg, &peer).ok_or_else(|| {
            tracing::error!(
                channel_ref = %channel_ref,
                message_id,
                "Failed to convert message to domain type"
            );
            Error::TelegramApi("Failed to convert message".to_string())
        })
    }
```

Also add `convert_message` to the imports at the top of `client.rs` if not already there. The existing import line is:

```rust
use crate::telegram::converters::{
    convert_media_filter, convert_message, convert_peer_to_channel, matches_media_filter,
};
```

`convert_message` is already imported, so no change needed.

- [ ] **Step 2: Verify compilation**

Run: `cargo check`

Expected: Clean compilation, no errors.

- [ ] **Step 3: Run full pre-commit checks**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`

Expected: All pass (the new trait method now has an implementation so mockall can also generate the mock).

- [ ] **Step 4: Commit**

```bash
git add src/telegram/client.rs
git commit -m "feat: implement get_message_by_id for TelegramClient

Resolves channel by username or numeric ID, fetches a single message
via grammers get_messages_by_id, converts to domain Message type."
```

---

### Task 4: MCP Request Type — `GetMessageByLinkRequest`

**Files:**
- Modify: `src/mcp/tools/types/requests.rs`
- Modify: `src/mcp/tools/types.rs`

- [ ] **Step 1: Write the deserialization test**

Add to the `#[cfg(test)] mod tests` block in `src/mcp/tools/types/requests.rs`:

```rust
    #[test]
    fn get_message_by_link_request_deserializes() {
        let json = r#"{"link": "https://t.me/swodki/575403"}"#;
        let request: GetMessageByLinkRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.link, "https://t.me/swodki/575403");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test get_message_by_link_request_deserializes`

Expected: FAIL — `GetMessageByLinkRequest` not found.

- [ ] **Step 3: Add the request type**

Add to `src/mcp/tools/types/requests.rs`, after the `GetRecentMessagesRequest` struct (before the `#[cfg(test)]` block):

```rust
/// Request for get_message_by_link tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetMessageByLinkRequest {
    #[schemars(
        description = "Telegram message link. Supported formats: https://t.me/username/12345, https://t.me/c/channel_id/12345, t.me/username/12345"
    )]
    pub link: String,
}
```

- [ ] **Step 4: Add re-export in `types.rs`**

In `src/mcp/tools/types.rs`, add `GetMessageByLinkRequest` to the re-export:

```rust
pub use requests::{
    GenerateLinkRequest, GetChannelInfoRequest, GetChannelsRequest, GetMessageByLinkRequest,
    GetRecentMessagesRequest, OpenMessageRequest, SearchRequest,
};
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test get_message_by_link_request_deserializes`

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/mcp/tools/types/requests.rs src/mcp/tools/types.rs
git commit -m "feat: add GetMessageByLinkRequest type

Request type for the new get_message_by_link MCP tool. Single required
field: the t.me link string."
```

---

### Task 5: MCP Tool 8 — `get_message_by_link` in `server.rs`

**Files:**
- Modify: `src/mcp/server.rs`

- [ ] **Step 1: Add the import for the new request type and link parser**

Update the imports at the top of `src/mcp/server.rs`. Change the `use crate::mcp::tools` line to include `GetMessageByLinkRequest`:

```rust
use crate::mcp::tools::{
    ChannelsResponse, GenerateLinkRequest, GetChannelInfoRequest, GetChannelsRequest,
    GetMessageByLinkRequest, GetRecentMessagesRequest, MessageLinkResponse, OpenMessageRequest,
    OpenMessageResponse, SearchRequest, StatusResponse, parse_channel_id, parse_message_id,
    parse_optional_channel_id,
};
```

Add the link parser import:

```rust
use crate::link::{ChannelRef, parse_telegram_link};
```

Update the existing `use crate::link::MessageLink;` to:

```rust
use crate::link::{ChannelRef, MessageLink, parse_telegram_link};
```

- [ ] **Step 2: Add tool 8 method**

Add inside the `#[tool_router] impl` block in `server.rs`, after the `get_recent_messages` method (before the closing `}`):

```rust
    /// Tool 8: get_message_by_link - Get a specific message by its t.me link
    #[tool(
        description = "Get a specific Telegram message by its t.me link (e.g. https://t.me/swodki/575403)"
    )]
    pub async fn get_message_by_link(
        &self,
        Parameters(request): Parameters<GetMessageByLinkRequest>,
    ) -> Result<String, String> {
        // Parse the link
        let (channel_ref, message_id) =
            parse_telegram_link(&request.link).map_err(|e| e.to_string())?;

        // Convert ChannelRef to string identifier for the trait method
        let channel_identifier = match &channel_ref {
            ChannelRef::Username(username) => username.clone(),
            ChannelRef::Id(id) => id.get().to_string(),
        };

        // Acquire rate limiter token
        self.rate_limiter
            .acquire(1)
            .await
            .map_err(|e| e.to_string())?;

        // Fetch the message
        let message = self
            .telegram_client
            .get_message_by_id(&channel_identifier, message_id.get() as i32)
            .await
            .map_err(|e| e.to_string())?;

        tracing::info!(
            link = %request.link,
            channel = %channel_identifier,
            message_id = message_id.get(),
            "Get message by link completed"
        );

        serde_json::to_string(&message).map_err(|e| e.to_string())
    }
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`

Expected: Clean compilation.

- [ ] **Step 4: Run pre-commit checks**

Run: `cargo fmt --all && cargo clippy -- -D warnings`

Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/mcp/server.rs
git commit -m "feat: add get_message_by_link MCP tool (tool 8)

Parses t.me links (public and private), resolves channel, fetches
the specific message via get_message_by_id, returns as JSON."
```

---

### Task 6: MCP Tool Tests — `message_by_link.rs`

**Files:**
- Create: `src/mcp/tests/message_by_link.rs`
- Modify: `src/mcp/tests.rs`

- [ ] **Step 1: Register the new test module**

In `src/mcp/tests.rs`, add the new module after the existing ones:

```rust
#[path = "tests/message_by_link.rs"]
mod message_by_link;
```

- [ ] **Step 2: Create the test file with all tests**

Create `src/mcp/tests/message_by_link.rs`:

```rust
//! Tests for get_message_by_link tool

use crate::error::Error;
use crate::mcp::server::McpServer;
use crate::mcp::tools::GetMessageByLinkRequest;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{
    ChannelId, ChannelName, MediaType, Message, MessageId, Username,
};
use rmcp::handler::server::wrapper::Parameters;
use std::sync::Arc;

fn create_test_message(id: i64, text: &str, channel_id: i64) -> Message {
    Message {
        id: MessageId::new(id).unwrap(),
        channel_id: ChannelId::new(channel_id).unwrap(),
        channel_name: ChannelName::new("Test Channel").unwrap(),
        channel_username: Username::new("testchannel").unwrap(),
        text: text.to_string(),
        timestamp: chrono::Utc::now(),
        sender_id: None,
        sender_name: None,
        has_media: false,
        media_type: MediaType::None,
    }
}

#[tokio::test]
async fn get_message_by_link_public_link_returns_message() {
    // Given: Mock client that returns a message for username + message ID
    let mut mock_client = MockTelegramClientTrait::new();
    let expected_message = create_test_message(575403, "Hello from Telegram", 999);
    let expected = expected_message.clone();

    mock_client
        .expect_get_message_by_id()
        .withf(|channel_ref, msg_id| channel_ref == "swodki" && *msg_id == 575403)
        .return_once(move |_, _| Ok(expected));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Get message by public link
    let request = GetMessageByLinkRequest {
        link: "https://t.me/swodki/575403".to_string(),
    };

    let result = server.get_message_by_link(Parameters(request)).await;

    // Then: Returns the message
    assert!(result.is_ok());
    let message: Message = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(message.text, "Hello from Telegram");
    assert_eq!(message.id.get(), 575403);
}

#[tokio::test]
async fn get_message_by_link_private_link_returns_message() {
    // Given: Mock client that returns a message for numeric channel ID
    let mut mock_client = MockTelegramClientTrait::new();
    let expected_message = create_test_message(42, "Private channel post", 1234567);
    let expected = expected_message.clone();

    mock_client
        .expect_get_message_by_id()
        .withf(|channel_ref, msg_id| channel_ref == "1234567" && *msg_id == 42)
        .return_once(move |_, _| Ok(expected));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Get message by private link
    let request = GetMessageByLinkRequest {
        link: "https://t.me/c/1234567/42".to_string(),
    };

    let result = server.get_message_by_link(Parameters(request)).await;

    // Then: Returns the message
    assert!(result.is_ok());
    let message: Message = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(message.text, "Private channel post");
}

#[tokio::test]
async fn get_message_by_link_invalid_link_returns_error() {
    // Given: Server (no mock expectations — parse should fail before API call)
    let mock_client = MockTelegramClientTrait::new();
    let mock_limiter = MockRateLimiterTrait::new();
    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Invalid link
    let request = GetMessageByLinkRequest {
        link: "https://example.com/not/telegram".to_string(),
    };

    let result = server.get_message_by_link(Parameters(request)).await;

    // Then: Returns parse error (no API call made)
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Not a valid t.me link"));
}

#[tokio::test]
async fn get_message_by_link_channel_not_found() {
    // Given: Mock client that returns channel-not-found error
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_get_message_by_id()
        .return_once(|_, _| {
            Err(Error::InvalidInput("Channel not found: nonexistent".to_string()))
        });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Link to non-existent channel
    let request = GetMessageByLinkRequest {
        link: "https://t.me/nonexistent/123".to_string(),
    };

    let result = server.get_message_by_link(Parameters(request)).await;

    // Then: Returns error from client
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Channel not found"));
}

#[tokio::test]
async fn get_message_by_link_message_not_found() {
    // Given: Mock client that returns message-not-found error
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_get_message_by_id()
        .return_once(|_, _| {
            Err(Error::InvalidInput(
                "Message 999999 not found in channel swodki".to_string(),
            ))
        });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    // When: Link to non-existent message
    let request = GetMessageByLinkRequest {
        link: "https://t.me/swodki/999999".to_string(),
    };

    let result = server.get_message_by_link(Parameters(request)).await;

    // Then: Returns error
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Message 999999 not found"));
}

#[tokio::test]
async fn get_message_by_link_rate_limited() {
    // Given: Rate limiter that rejects
    let mock_client = MockTelegramClientTrait::new();
    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| {
        Err(Error::RateLimit {
            retry_after_seconds: 5,
        })
    });

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));

    let request = GetMessageByLinkRequest {
        link: "https://t.me/swodki/575403".to_string(),
    };

    // When: Rate limited
    let result = server.get_message_by_link(Parameters(request)).await;

    // Then: Returns rate limit error (no API call made)
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("rate limit"));
}
```

- [ ] **Step 3: Run all tests**

Run: `cargo test message_by_link -- --nocapture`

Expected: All 6 tests PASS.

- [ ] **Step 4: Run full pre-commit checks**

Run: `cargo fmt --all && cargo clippy -- -D warnings && cargo test`

Expected: All pass. All existing tests still pass.

- [ ] **Step 5: Commit**

```bash
git add src/mcp/tests.rs src/mcp/tests/message_by_link.rs
git commit -m "test: add mock-based tests for get_message_by_link tool

Covers: happy path (public + private links), invalid link parsing,
channel not found, message not found, rate limiting."
```

---

### Task 7: Final Verification

- [ ] **Step 1: Run the full test suite**

Run: `cargo test`

Expected: All tests pass, including all existing tests.

- [ ] **Step 2: Run full pre-commit checks**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`

Expected: All pass — clean formatting, no warnings, all tests green.

- [ ] **Step 3: Count tools to verify we now have 8**

Run: `grep -c '#\[tool(' src/mcp/server.rs`

Expected: `8`
