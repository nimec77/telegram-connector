Enrich the message metadata returned by `search_messages` and
`get_recent_messages` in this repository (telegram-connector: Rust 2024
nightly, rmcp SDK, grammers Telegram client). No new tools — extend the
existing `Message` domain type and its JSON response.

## Problem
The current `Message` type drops server-side metadata that grammers already
exposes, which breaks downstream analysis by the MCP client (Claude):
1. **Forward attribution is lost.** When channel A forwards a post from
   channel B, the result looks like original content of channel A. This
   breaks deduplication and source-credibility attribution in news-digest
   workflows.
2. **Link-preview content is lost.** Many channel posts are just a URL; the
   substance lives in Telegram's server-side webpage preview (title +
   description), which is already attached to the message — pure text,
   zero extra API calls.
3. **Engagement signals are lost.** `views` and `forwards` counts are cheap
   significance signals for ranking stories.

All of this is available on the grammers `Message` object (or its raw
`tl::types::Message`) — no additional network requests are required.

## Specification
Extend the message response schema with the following optional fields
(omit from JSON when absent, via `skip_serializing_if`):

- `forwarded_from` (object, optional):
  - `channel_id` (i64, optional) — source channel ID if forwarded from a
    channel
  - `channel_name` (string, optional) — source channel title
  - `channel_username` (string, optional)
  - `sender_name` (string, optional) — for forwards from users / hidden
    users (Telegram exposes only a display name when the user hides their
    account; handle that case without erroring)
  - `original_date` (string, optional, RFC 3339) — timestamp of the
    original post
  - `original_message_id` (i64, optional) — for building a link to the
    original (pairs with the existing `generate_message_link` tool)
- `link_preview` (object, optional):
  - `url` (string)
  - `site_name` (string, optional)
  - `title` (string, optional)
  - `description` (string, optional)
  Truncate `description` to 500 chars to keep responses bounded.
- `views` (u64, optional)
- `forwards` (u64, optional)
- `reply_to_message_id` (i64, optional) — when the post is a reply/comment
  thread anchor, expose the parent ID.

## Integration requirements (follow existing project conventions)
- Domain types: add `ForwardInfo` and `LinkPreview` under
  `src/telegram/types/entities.rs` (or a new module if entities.rs grows
  too large); response DTOs in `src/mcp/tools/types/responses.rs` with
  `schemars` schemas.
- Extraction logic goes in `src/telegram/converters.rs` (grammers → domain).
  If grammers' high-level API doesn't expose a field, drop down to the raw
  TL types (`message.raw`) rather than skipping the field — note in code
  comments which path was needed.
- This is a pure enrichment: both search tools reuse `SearchResult`, so the
  change should land in exactly one conversion path. Existing response
  fields and their JSON names must remain unchanged (backward compatible).
- No new rate-limiter cost — zero extra API calls by design. CI must verify
  this: tests should assert the converter works from a single Message
  object with no client calls (mockall: expect zero downloads/requests).

## Quality gates
- `cargo fmt --check && cargo clippy -- -D warnings && cargo test` must pass.
- Unit tests via the existing test_helpers fixtures:
  - forward from a channel (full attribution)
  - forward from a hidden user (name only, no IDs)
  - message with link preview (incl. description truncation at 500 chars)
  - plain message — all new fields absent from serialized JSON
  - views/forwards populated vs absent
- Update README.md: extend the response examples for `search_messages` and
  `get_recent_messages` with the new optional fields, and add a short
  "Forward attribution & link previews" note. Update CHANGELOG.md.

## Non-goals
- No media download, no fetching of the linked webpage itself (only what
  Telegram's preview already contains), no reactions/comments retrieval,
  no schema changes to other tools.
