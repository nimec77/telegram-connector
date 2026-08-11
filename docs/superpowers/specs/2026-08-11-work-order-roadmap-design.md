# Work-Order Remainder Roadmap (v0.15 – v0.18) — Design

**Date:** 2026-08-11
**Source:** `docs/telegram-mcp-0.13.0-work-order.md` (black-box audit of v0.13.0)
**Releases:** four minor releases, v0.15.0 → v0.18.0, one branch + PR + plan each
**Status:** APPROVED

## Context

The audit's B1, B2+D9, and B3 shipped in v0.14.0 (correctness core, PR #32).
This spec sequences **everything else** in the work order — B4–B10, D1–D8, D10,
A1–A8 — into four releases. It supersedes the five-sub-project decomposition
sketched in `2026-08-10-correctness-core-design.md` §Context.

Decisions fixed during brainstorming:

- **Break cleanly.** 0.x server, single known consumer (`news-digest`, owned by
  us). Fields are renamed/removed outright, no compatibility aliases (one
  exception: `rate_limiter_tokens`, D5, kept one release). All response-shape
  breaks are batched into v0.15 so the consumer resyncs once.
- **Everything is in scope**, including A5 (`get_channel_stats`) in a minimal,
  classifier-free form.
- **Roadmap + per-release plans.** This spec covers all four releases at design
  level. Each release gets its own implementation plan
  (`docs/superpowers/plans/`), written when that release starts — the v0.15 plan
  immediately after this spec is approved.
- §1.3 of the work order (clamping, date-window accuracy, media guards,
  identifier flexibility, normalized unknown-channel error) must not regress in
  any release, and each plan restates it as a constraint.

---

## v0.15 — "post shape": all breaking changes + message enrichment

Covers **B5+A2, B6, B7, B8, B9, B10, D1, D2, D3, D10**.

### Albums (B5 + A2)

- `Message` gains `grouped_id: Option<i64>` (serialized only when present).
- `get_recent_messages` and `search_messages` gain `collapse_albums:
  Option<bool>`, **default `true`**.
- Collapsing happens in the client fetch loops (`ops_history` / `ops_search`):
  siblings are grouped by `grouped_id` within the fetched window — grouping does
  not rely on adjacency. **`limit` counts collapsed posts**, so the fetch loops
  count posts, not raw records.
- A collapsed album is the same `Message` struct plus one optional field:

  ```
  album: Option<AlbumInfo {
      media_count: u32,
      media_types: Vec<MediaType>,
      message_ids: Vec<MessageId>,   // all siblings, ascending
  }>
  ```

- Representative sibling = **lowest id** (stable referencing). `text` comes from
  whichever sibling carries it; `views`/`forwards`/`media_type`/`timestamp`
  come from the representative; `has_media: true`.
- With `collapse_albums: false`, every sibling is emitted as today, each carrying
  its `grouped_id`.

### Metadata honesty (B6 – B10)

- **B6/B7 — `QueryMetadata` replaced** with:

  ```
  { query, window_from, window_to,           // RFC 3339; the window actually run
    channels_scanned: Option<u32>,           // 1 for single-channel; null for global
                                             // (server-side search — scope unknowable)
    channels_in_results: u32 }
  ```

  `hours_back` is gone from metadata. `total_found` → `returned` in
  message-stream responses. `get_subscribed_channels.total` keeps its name but
  always means the full subscription count (knowable — dialogs are enumerated),
  and `has_more` is computed from it.
- **B8 — `last_message_date`** is populated from dialog data in
  `get_subscribed_channels` (grammers dialogs carry the last message; verify the
  accessor at plan time), and under `include_full` on `get_channel_info` via a
  one-message history peek (`include_full` already means "extra RPC accepted").
  Elsewhere it stays `null`, honestly.
- **B9 —** `Channel.username` becomes `Option<Username>`; the `"unknown"` /
  `"group"` sentinels in `src/telegram/converters/channel.rs` are deleted. New
  field on every channel object: `chat_type: "channel" | "group" |
  "supergroup"` (broadcast → `channel`, small group → `group`, megagroup →
  `supergroup`).
- **B10 —** no per-message resolve RPCs (preserves the zero-extra-call
  enrichment invariant). At plan time, check whether grammers exposes the
  response peer map; if yes, fill `ForwardInfo.channel_name/username` from it
  for free. If no, the fields stay unpopulated and the documented batch path is
  `resolve_channels` (A7, v0.18).

### Message enrichment (D1, D2)

- **D1 —** every `Message` gains `link: String`, built by the v0.14 shared link
  builder (public `t.me/<username>` when the channel has a username, `t.me/c/`
  otherwise). No extra RPC — the converter already sees the chat.
- **D2 —** `reactions: Option<Vec<{emoji: String, count: u64}>>` plus
  `reactions_total: Option<u64>`, read from the raw TL message. Standard emoji
  only; custom-emoji reactions are counted in `reactions_total` but not
  itemized.

### Riding along (breaking, so batched here)

- **D3 —** `get_message_media`: `original_*` → `source_variant_width` /
  `source_variant_height` / `source_variant_size_bytes`; new
  `largest_available_width` / `largest_available_height`.
- **D10 —** `search_public_channels.has_more` becomes `Option<bool>`: `null`
  when `returned == limit` (unknown), `false` otherwise.

---

## v0.16 — "capacity": response size + cursors

Covers **B4, A4, A8**.

- **Byte budget (B4).** New `[limits] response_byte_budget` config table entry
  (default 40 000 bytes, `#[serde(default)]`). The response assembler
  serializes messages incrementally; when the next message would exceed the
  budget it stops, sets `has_more: true`, and emits a cursor. `limit` remains
  the documented max; the budget is what actually caps verbose channels.
- **Cursors (A8).** `before_id` / `after_id` params on `get_recent_messages`
  and single-channel `search_messages`, keyed on `message_id` so pages don't
  drift on active channels. Responses gain `next_cursor: {before_id:
  <last-included-id>}` whenever `has_more` is true (from budget, limit, or
  window).
- **Text truncation (B4).** `max_text_length` param, default 2000. When it
  bites: `text_truncated: true`, `text_full_length: <n>`. Full text is one
  `get_message_by_link` (or, from v0.18, `get_messages_batch`) call away.
- **Compact mode (A4).** `format: "compact"` on the message-returning tools
  hoists `channel_id`/`channel_name`/`channel_username` into one
  response-level `channel` header and drops them from each message. Chosen
  over a `fields` projection: one switch, no schema combinatorics.

---

## v0.17 — "polish"

Covers **D4, D5, D6, D7, D8** (all non-breaking except the D5 alias removal
scheduled one release later).

- **D4 —** `get_message_media` passes original bytes through when the selected
  variant is already JPEG and no downscale happened.
- **D5 —** `check_mcp_status` gains `rate_limiter: {tokens, capacity,
  refill_per_sec, costs: {…}}` with real values from the limiter;
  `rate_limiter_tokens` stays one release as a deprecated alias. Rate-limit
  rejection errors state the token deficit and estimated wait seconds.
- **D6 —** `get_last_responses` gains `include_binary` (default `false`);
  image blocks replay as `{type: "image", omitted: true, mime_type,
  size_bytes}`.
- **D7 —** audit the error-wrapping layer so prefixes apply exactly once.
- **D8 —** normalize remaining raw MTProto error strings to the clean
  `invalid input: …` template; validate username shape locally (5–32 chars,
  `[A-Za-z0-9_]`, no leading digit) before spending an RPC.

---

## v0.18 — "surface": new tools and batch params

Covers **A1, A3, A5, A6, A7** (A8 shipped in v0.16). All additive.

- **A3 + A6 merged —** `get_recent_messages` and `search_messages` accept
  `channel_ids: [..]` alongside the existing single `channel_id`. Supplying
  both is an error; `get_recent_messages` requires one of them, while
  `search_messages` with neither stays a global search, as today. The client fans out with bounded concurrency (4 parallel
  fetches, each under its existing timeout); results are flat, and
  `query_metadata.channels_scanned` reports the real count. In multi-channel
  compact mode the single-channel header becomes a response-level `channels`
  map plus per-message `channel_id`.
- **A1 —** `get_messages_batch(channel_id, message_ids[])`, one RPC
  (`channels.GetMessages` accepts an id vector), capped at 50 ids. Deleted or
  missing ids are reported per-id as `{id, error: "not found"}` (the v0.14
  `MessageEmpty` guard detects them) instead of failing the batch.
- **A7 —** `resolve_channels(identifiers[])`: batch username/id/title →
  channel entities, capped at 20. The explicit path for forward attribution
  (B10) and title-only private channels.
- **A5 —** `get_channel_stats(channel_id, days_back?)`: one bounded history
  sweep (default 7 days, max 500 messages) computing `post_count`
  (album-collapsed), `posts_per_day`, `median_views`, `media_share`,
  `album_share`, plus `sample: {messages_scanned, window_from, window_to,
  complete: bool}` so the caller can tell when the sweep hit the cap. No promo
  classification.

---

## Testing strategy (every release)

- Existing patterns: mockall at the trait boundary; offline raw-TL fixtures for
  converters (the `MessageEmpty` seam from v0.14 extends to albums and
  reactions); schema-walk tests extended to cover new fields.
- Standing regression suite (work order §7):
  - no epoch-0 timestamp in any response *(exists since v0.14)*;
  - no `$ref` without resolvable `$defs` in any tool schema *(exists)*;
  - link-form correctness public vs private *(exists; extended to
    `Message.link`)*;
  - `username` is never `"unknown"`/`"group"` *(new, v0.15)*;
  - album fixture — 8 siblings + 2 singles collapse to 3 posts, `limit`
    honored *(new, v0.15)*;
  - oversized fixture stays under the byte budget with truthful `has_more` +
    cursor *(new, v0.16)*;
  - golden date-window test against a recorded fixture asserting the exact
    message-id set *(new, v0.16 — locks §1.3 behaviour)*.
- Untested items from work order §5 get explicit coverage as their areas are
  touched: `media_filter` runtime behaviour end-to-end (v0.15 album work
  touches the same loops); deleted-id behaviour of `get_message_media` /
  `transcribe_voice_message` (v0.15 regression tests); rate-limit exhaustion
  error path (v0.17, D5).

## Consumer follow-up

After v0.15 ships, resync the `news-digest` skill once against the new response
shapes (albums default-collapsed, renamed metadata fields, nullable username,
`chat_type`, `link`, reactions). Later releases are additive for the consumer.
