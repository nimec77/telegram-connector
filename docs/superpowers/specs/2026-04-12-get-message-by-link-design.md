# Design: Get Message by Link (Tool 8)

## Summary

Add an MCP tool that retrieves a single Telegram message by its `t.me` link. Currently the server can search messages and fetch recent history, but there is no way to fetch a specific message when you have its URL.

## Approach

**Approach A (selected):** New trait method `get_message_by_id` on `TelegramClientTrait`, with URL parsing in the MCP layer and Telegram API logic in the client layer. Clean separation, mockable, follows all existing patterns.

## URL Parsing

Add `parse_telegram_link` to `src/link.rs`. Supported formats:

| Format | Example | Extracted |
|--------|---------|-----------|
| Public (https) | `https://t.me/swodki/575403` | username=`swodki`, msg_id=`575403` |
| Private (https) | `https://t.me/c/1234567/575403` | channel_id=`1234567`, msg_id=`575403` |
| No scheme | `t.me/swodki/575403` | username=`swodki`, msg_id=`575403` |
| HTTP | `http://t.me/swodki/575403` | username=`swodki`, msg_id=`575403` |

**Not supported (by design):** `tg://resolve?domain=...&post=...` protocol links. Rarely copy-pasted, can be added later.

Query parameters (e.g., `?single`) and trailing slashes are stripped during parsing.

New enum in `link.rs`:

```rust
pub enum ChannelRef {
    Username(String),
    Id(ChannelId),
}
```

Return type: `Result<(ChannelRef, MessageId), Error>` using `Error::InvalidInput`.

## Trait Method

New method on `TelegramClientTrait` in `trait_def.rs`:

```rust
async fn get_message_by_id(
    &self,
    channel_ref: &str,  // username or numeric ID string
    message_id: i32,
) -> Result<Message, Error>;
```

Implementation in `client.rs`:

1. Resolve peer: `resolve_username` for usernames, dialog iteration for numeric IDs (same logic as `get_channel_info`)
2. Call `grammers_client::Client::get_messages_by_id(peer, &[message_id])`
3. Convert via `convert_message` to domain `Message`
4. Return `Error::InvalidInput` if message not found

Returns a single `Message`, not `Vec` — always fetching exactly one.

**Note on ID types:** grammers `get_messages_by_id` takes `&[i32]`, but our domain `MessageId` wraps `i64`. The conversion `message_id.get() as i32` happens at the client layer boundary. This is safe because Telegram message IDs fit in i32 (Telegram API uses int32 for message IDs).

## MCP Tool

Tool 8: `get_message_by_link` in `server.rs`.

### Request

```rust
pub struct GetMessageByLinkRequest {
    /// Telegram message link (e.g., https://t.me/swodki/575403)
    pub link: String,
}
```

Single required field. The link contains all needed information.

### Response

Reuses existing `Message` domain type serialized to JSON. Same format as messages from `search_messages` and `get_recent_messages`. No new response type.

### Flow

1. Parse `link` via `parse_telegram_link` -> `(ChannelRef, MessageId)`
2. Convert `ChannelRef` to string identifier (username or numeric ID)
3. Acquire 1 rate limiter token
4. Call `self.telegram_client.get_message_by_id(identifier, message_id)`
5. Serialize and return

### Errors

- Invalid/unparseable link: immediate parse error, no API call
- Channel not found: error from trait method
- Message not found: error from trait method

## Testing

### Unit tests (no mocks)

- `link.rs`: `parse_telegram_link` — public link, private link, http, no scheme, trailing slash, query params, invalid URLs, missing parts
- `requests.rs`: `GetMessageByLinkRequest` deserialization

### Mock-based tests

- `src/mcp/tests/message_by_link.rs` (new file):
  - Happy path: valid link, mock returns message, JSON response
  - Invalid link: parse error before mock called
  - Channel not found: mock returns error
  - Message not found: mock returns error
  - Rate limiter: token acquired before API call
- `src/telegram/tests/client_tests.rs`: `get_message_by_id` mock trait tests

### Existing helpers

`create_test_message()` from `test_helpers.rs` covers what is needed.

## Files Changed

| File | Change |
|------|--------|
| `src/link.rs` | Add `ChannelRef` enum, `parse_telegram_link` function, tests |
| `src/telegram/trait_def.rs` | Add `get_message_by_id` method |
| `src/telegram/client.rs` | Implement `get_message_by_id` |
| `src/mcp/tools/types/requests.rs` | Add `GetMessageByLinkRequest` |
| `src/mcp/tools/types.rs` | Re-export `GetMessageByLinkRequest` |
| `src/mcp/server.rs` | Add tool 8 `get_message_by_link` |
| `src/mcp/tests/message_by_link.rs` | New test module |
| `src/mcp/tests.rs` | Register `message_by_link` submodule |
