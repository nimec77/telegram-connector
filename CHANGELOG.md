# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.22.1] - 2026-08-14

### Fixed
- `get_messages_media_batch` now reports an image that downloaded successfully
  but could not be shrunk under its remaining payload allowance as
  `payload_cap_reached`, not `download_failed`. The cause was one error variant
  (`Error::DownloadFailed`) serving two distinct failure modes; cap exhaustion
  now has its own `Error::PayloadCapExceeded`. A client branching on the token
  was being told to retry something that could never succeed.
- The batch refund now uses saturating multiplication, matching the charge it
  reverses. With a large configured `media_download_cost` the old unchecked
  multiply could panic in debug builds or wrap in release.
- Rate-limit costs are validated at startup: a `media_download_cost` or
  `transcription_cost` above `max_tokens` is rejected, since such a call could
  never be served even from a full bucket.

### Changed
- `get_message_media`'s error text for a payload-cap failure lost its
  `"media download failed: "` prefix: it now reads `"image could not be
  reduced below the N-byte payload cap"` instead of `"media download failed:
  image could not be reduced below the N-byte payload cap"`. Deliberate — the
  old text announced a download failure when the download had actually
  succeeded. This is prose returned to the client on the single-message path,
  not one of the batch tool's machine-readable `reason` tokens.
- Image encoding runs on a blocking thread instead of the async worker. The
  batch loop stays sequential and in request order, so payload-budget
  allocation is unchanged and deterministic.
- A metadata-serialization failure inside a batch, or a panicked/cancelled
  image-encode task, is reported as `internal_error: <detail>` rather than
  being mislabelled a download failure. Not reachable in normal operation.

### Internal
- Removed both module-layering inversions: `src/telegram/` no longer reaches up
  into `crate::mcp` for its download concurrency, and `config.rs` no longer
  imports a validation bound from the MCP layer.
- `McpServer::new` builds its defaults from `config::defaults` instead of six
  hand-copied numbers; a test now fails if the two desync.
- Shared id dedupe/validation between the two batch tools; explicit success
  counter in place of `content.len() / 2`; grammers' slot-count contract is
  asserted rather than padded for.

### Documentation
- Live acceptance closure (2026-08-14, real session): the `[search] deadline_seconds`
  expiry path was exercised live for the first time (forced with a 1 s deadline: graceful
  partial result with `timed_out`/`partial` set, no error), and `poll_info` was verified
  against a real poll — full question/options/per-option voters/`total_voters`/flags,
  identical through `get_messages_batch` (raw envelope path) and `get_recent_messages`.
  The work-order-A standing note (forward-attribution parity, `document_info`) is closed:
  all criteria pass. Recorded in `docs/memory.md` and `docs/tasklist.md`.
- Docs-vs-code audit fixes: README gains the missing `get_message_by_link` tool section
  (tool 8) and a current project-structure tree; the stale `server_version` example is
  bumped; `config.example.toml` documents the missing `[server] shutdown_timeout_seconds`,
  `[telegram.timeouts] download_secs`, and `[observability] max_buffered_payload_bytes`
  keys; CLAUDE.md notes both media tools return `CallToolResult` and lists the full serde
  helper set; `docs/conventions.md` examples match the real constructor signatures and
  error variants; stale "11 tools" references in `.claude/` rules/skills corrected to 16;
  phase-36 test count corrected to 709; `docs/phase-20-plan.md` verification checklist
  closed retroactively.

## [0.22.0] - 2026-08-14

### Added
- `get_messages_media_batch` returns the images of up to 10 messages from one
  channel in a single call — image block plus metadata block per message, then
  a summary carrying `requested`/`returned`/`failed`/`total_base64_bytes`.
  The batch resolves the channel once and issues one `get_messages_by_id` for
  every id, then downloads with bounded concurrency (4), so N images cost one
  channel resolution and one fetch round trip instead of N of each. For a
  numeric `channel_id` a resolution is a full dialog walk, which is what made
  the per-call path expensive.

  Measured on a live session, 10 photos per run, each run in a fresh process
  (so a full token bucket, and rate limiting cannot distort the timing):

  | channel | 10 × `get_message_media` | 1 × `get_messages_media_batch` |
  |---|---|---|
  | `1556054753` | 13.10 s | **2.99 s** |
  | `1583175062` | 10.34 s | **1.44 s** |

  The mechanism is visible in the debug log: the batch spends ~2.0 s on its one
  resolve-and-fetch, then completes all ten downloads in ~1.0 s at concurrency
  4. The sequential path pays that ~1.0 s resolve-and-fetch on every single
  call, and its downloads never overlap.

  One caveat worth recording, because it inverts the result: a batch issued
  immediately after ten sequential downloads took **19.59 s**, because its
  single `iter_dialogs` walk took ~18 s instead of ~2 s. Telegram appears to
  throttle dialog enumeration after burst activity. The dialog walk is the
  variable-cost part of this path, and it degrades exactly when a digest run
  has just been hammering the same account — which strengthens rather than
  weakens the case for performing it once.

  Per-id failures — `not_found`, `no_visual_media`, `payload_cap_reached`,
  `download_failed` — are reported in `failed` and never fail the batch.

- `[limits] media_batch_max_total_bytes` (default 8 MiB) caps a batch's total
  image payload, counted in bytes of base64 as sent to the client. Images are
  downscaled progressively to fit; ids that still do not fit are reported as
  `payload_cap_reached`. Verified live: the same 10-photo batch that returns
  1 365 692 bytes uncapped returns 5 images totalling 390 228 bytes under a
  400 000-byte cap, with the remaining 5 ids reported as `payload_cap_reached`
  in request order.

- `check_mcp_status` gains a `media` block (`batch_max_ids`, `max_total_bytes`,
  `per_image_max_bytes`, `default_max_dimension`, `max_dimension_limit`).

### Changed
- `[rate_limiting]` defaults retuned for batch media: `max_tokens` 50 → 60 and
  `media_download_cost` 5 → 3. At the unchanged `refill_rate` of 2.0/sec that
  is 20 images in a burst then one per 1.5 s, up from 10 then one per 2.5 s.
  Batches acquire for every requested id up front and refund the ids that
  produced no image, so admission control stays real while the net charge is
  per image returned. On a whole-call failure (unresolvable channel, fetch RPC
  error, `FLOOD_WAIT`) the batch refunds all but one token, since the call did
  perform a channel resolution and a fetch RPC. Both values are conservative
  estimates, not calibrated against Telegram's flood thresholds.

  These are **defaults**, so a config that sets `max_tokens` explicitly is
  unaffected by the 50 → 60 change. Note that at a pinned `max_tokens = 30`,
  one full 10-image batch costs exactly 30 tokens — the whole bucket. Raise
  `max_tokens` if you intend to run batches back to back.

## [0.21.0] - 2026-08-13

### Fixed
- Unscoped `search_messages` no longer walks Telegram's entire global message
  index to enforce its own time window. `messages.SearchGlobal` carries
  `min_date`/`max_date` parameters that the pager hardcoded to `0`, so the
  window was applied client-side while Telegram streamed results backwards at
  100 per round trip — a rare media filter over a narrow window paged to
  exhaustion. Both bounds are now sent with the request. Measured on a live
  session, limit 20:

  | search | before | after |
  |---|---|---|
  | global `document`, 24h | 44.86 s | 0.449 s |
  | global `video_note`, 24h | 12.93 s | 0.411 s |
  | global `voice`, 72h | 8.07 s | 0.468 s |
  | global `url`, 24h | 0.46 s | 0.500 s |

  Both bounds are widened by a second at each end, so the window Telegram
  applies is a provable superset of the requested one whether it reads
  `min_date`/`max_date` as inclusive or exclusive — neither the TL schema nor
  grammers says which. The client-side window checks are retained as defense in
  depth and still decide the exact edges. Result sets are unchanged; only the
  work done to produce them is.

### Added
- `[search] deadline_seconds` (default 20) bounds a search's accumulation loop.
  On expiry the search returns the results gathered so far with
  `query_metadata.timed_out` and `query_metadata.partial` set — never an error,
  because partial results beat a failed workflow. Both flags are omitted from
  JSON when false, so a healthy response's shape on the wire is unchanged.
  Keep the deadline below `[telegram.timeouts] search_secs` (default 120),
  which still fails the call. The default is a conservative starting point,
  not a measured value.
- `query_metadata.pages_fetched` and `query_metadata.messages_scanned` report
  result pages fetched from Telegram and the raw messages walked, so an expensive search is
  legible to its caller. These replace the work order's suggested
  `dialogs_scanned`: the global path issues one paginated `searchGlobal` and
  sweeps no dialogs. Both are new fields, always present on every
  `search_messages` and `get_recent_messages` response — additive (no
  existing field is renamed, retyped, or removed), but responses are not
  byte-identical: every search and history call now carries two more fields
  than before.

## [0.20.0] - 2026-08-13

### Fixed
- `forwarded_from` now carries `channel_name`, `channel_username`, `sender_name`,
  and `post_author` on `get_message_by_link` and `get_messages_batch`, matching
  `get_recent_messages` / `search_messages`. Both tools fetched through
  grammers' high-level `get_messages_by_id`, which discards the MTProto
  response envelope forward attribution reads — so re-fetching a forward by id
  for its full untruncated text (the designated use of `get_messages_batch`)
  silently dropped attribution the original search/history call already
  showed.
- `poll_info.options[].voters` no longer reports a fabricated `0` for an
  option Telegram has not disclosed a count for. `PollAnswerVoters.voters`
  carries its own disclosure flag independent of whether results exist at
  all — Telegram's partial-disclosure case can reveal which option is
  chosen/correct while withholding that option's count; that case now
  degrades the option to `voters` omitted, same as an entirely undisclosed
  poll.

### Added
- `document_info` (`file_name`, `file_size_bytes`, `mime_type`) on messages
  whose `media_type` is `document`, read from the document's attributes with
  no extra API call. Video/audio/voice/animation media keep their own
  `video_info`/`audio_info` objects instead, so nothing is duplicated.
- `poll_info` (`question`, `options` (each `{text, voters}`), `total_voters`,
  `closed`, `multiple_choice`, `quiz`) on poll messages, read directly from
  the message's poll media with no extra API call. Deviates from the
  original work order's plain-array-of-strings shape: a per-option vote
  breakdown is what tells a caller what the poll actually concluded.
- `audio_info` gains `title` and `performer`, read from
  `DocumentAttributeAudio`'s ID3-style metadata — populated for music
  tracks, absent for the common case of voice messages.

### Changed
- `get_channel_stats` now sweeps via the same raw `RawHistoryPager` the
  other history/search paths use, instead of the grammers iterator —
  behavior-neutral; retires the last caller of the envelope-less conversion
  path.
- Internal: `convert_message` and `EntityLookup::insert_peer` are removed.
  `EntityLookup` no longer derives `Default`, and `empty()` (still
  `#[cfg(test)]`-only) has a direct body instead of delegating to it — a
  derived `Default` would have stayed reachable from production code as an
  ungated equivalent to the gated `empty()`, defeating the guard's purpose.
  `from_envelope` is now the sole production constructor, so message
  conversion cannot compile without a real response envelope. `require_found`
  gained a `require_found_raw` twin, both delegating to a shared `not_found`
  error constructor, for the two callers that now fetch raw TL rather than
  grammers' high-level wrapper; `get_message_media` and
  `transcribe_voice_message` are unaffected and still use the original
  (they need `.media()`, which only the high-level wrapper exposes).

All additions above are zero extra network calls and are omitted from JSON
when absent.

## [0.19.0] - 2026-08-12

### Added
- Forward attribution: `forwarded_from` on `search_messages` /
  `get_recent_messages` now carries `channel_name`, `channel_username`,
  `sender_name`, and `post_author`, resolved from the same MTProto response
  envelope (`chats`/`users`) — zero additional API calls; sources the account
  is not subscribed to are attributed too. The ids-only form is kept when the
  envelope lacks the peer. Additive and backward compatible.

### Changed
- `get_recent_messages` / `search_messages` fetch via raw
  `messages.GetHistory` / `messages.Search` / `messages.SearchGlobal`
  invocations (same requests and pagination as the grammers iterators, which
  discard the envelope behind a crate-private `PeerMap`).

## [0.18.0] - 2026-08-12

### Added
- `get_messages_batch`: fetch up to 50 specific messages from one channel in a
  single call (one `channels.GetMessages` RPC); deleted/missing ids are
  reported per-id in `missing` instead of failing the whole batch; ids
  dropped to fit the response byte budget are reported in `omitted_ids`,
  distinct from `missing` — which means "does not exist" (A1)
- `resolve_channels`: batch-resolve up to 20 identifiers (numeric ID,
  `@username`, or the exact title of a subscribed chat) to full channel
  entities in one call — one dialog walk plus at most one `resolve_username`
  RPC per unmatched username-shaped identifier; per-identifier failures come
  back as data (`error` on that entry) rather than failing the call (A7)
- `get_channel_stats`: posting-rate and engagement statistics (`post_count`,
  `posts_per_day`, `median_views`, `media_share`, `album_share`) over a
  bounded, album-collapsed history sample (default 7 days, max 30, capped at
  500 raw messages scanned); `sample.complete` reports whether the full
  window was covered (A5)
- `channel_ids` fan-out on `get_recent_messages` and `search_messages`:
  fetch/search up to 20 channels in one call with bounded concurrency (4 in
  flight), merged newest-first and truncated to `limit`; partial per-channel
  failures land in `channel_errors` instead of failing the whole call.
  `format: "compact"` now also supports multi-channel scope via a
  response-level `channels` map keyed by decimal channel id, with each
  message's own `channel_id` retained for attribution (A3, A6)

### Changed
- Validation order on `search_messages` shifted slightly: a username
  `channel_id` is now resolved later in the request-validation sequence
  (after limit/window/cursor checks, immediately before the rate-limiter
  acquire) instead of right after the query check. A request that combines a
  missing/invalid `channel_id` with another invalid field may now surface
  the other field's error first, and an invalid numeric `channel_id` is
  validated later than before. Behaviorally benign for any request with a
  single problem, but error-string consumers that depend on which error
  comes first for a multi-problem request should be aware of the reorder.

### Fixed
- `search_messages.channel_id` accepts `@username`/`username` again, not
  just a numeric ID — restores work-order §1.3 identifier flexibility (a
  username spends one extra `resolve_channel_identity` call; this had
  regressed relative to the already-flexible `get_recent_messages` and
  `get_channel_info`)

### Removed (breaking)
- `StatusResponse.rate_limiter_tokens` — the deprecated alias of
  `rate_limiter.tokens` introduced in v0.17, kept for exactly one release
  per its documented grace period (D5 grace period ended). Read
  `rate_limiter.tokens` instead.

## [0.17.0] - 2026-08-12

### Added
- `check_mcp_status` gains a `rate_limiter` budget block (`tokens`,
  `capacity`, `refill_per_sec`, `costs`); `rate_limiter_tokens` is kept as a
  deprecated alias, scheduled for removal in v0.18; rate-limit rejections now
  state the requested and available token counts (D5)
- `get_last_responses` accepts optional `include_binary` (default `false`):
  image payloads are replaced with size-annotated stubs in the replay unless
  explicitly requested (D6)

### Fixed
- `get_message_media` returns the original bytes, byte-identical, when the
  source JPEG already fits `max_dimension` — no re-encode, no quality loss,
  no size inflation (D4)
- Tool-level error prefixes (`parse_channel_id` / `parse_message_id`) apply
  exactly once instead of stacking with the inner `invalid input:` prefix
  (D7)
- Malformed channel usernames are rejected locally, before spending a
  resolve RPC, surfacing the same clean not-found error as any other
  unresolvable channel; the local shape check rejects only the empty string,
  names over 32 characters, non-`[A-Za-z0-9_]` characters, and a leading
  digit — there is no minimum-length floor, since short legacy usernames
  (3 chars, e.g. `@gif`) and Fragment-auction usernames (4 chars, e.g.
  `@bank`) are real and resolvable (D8)

## [0.16.0] - 2026-08-12

### Added
- `before_id` / `after_id` cursor pagination on `search_messages` and
  `get_recent_messages` (single-channel scope), keyed on message id so pages
  don't drift on active channels; responses carry `next_cursor` (A8)
- `has_more` on message-stream responses — true only when more qualifying
  messages exist beyond the returned page (A8/B4)
- Response byte budget (`[limits] response_byte_budget`, default 40 000
  bytes): oversized pages are truncated truthfully instead of overflowing the
  client (B4)
- `max_text_length` (default 2000 chars) with `text_truncated` /
  `text_full_length` flags (B4)
- `format: "compact"`: response-level `channel` header instead of per-message
  channel fields (A4)

### Changed
- `MessageResponse.channel_id` / `channel_name` / `channel_username` are now
  omitted (rather than serialized as `null`) when unset. In `format: "full"`
  this affects only `channel_username` (`channel_id`/`channel_name` stay
  always-present); `format: "compact"` strips all three from every message
  and hoists them into the response-level `channel` header instead.

## [0.15.0] - 2026-08-11

### Changed (breaking)
- `search_messages` and `get_recent_messages` now collapse album (grouped-media)
  siblings into a single post-level result by default (new `collapse_albums`
  parameter, default `true`, work-orders B5/A2): `limit` counts **posts**, not raw
  messages, and an album is never split across the limit boundary (once a post's
  first sibling is admitted, the rest of that album is always admitted too, even
  past `limit`). The collapsed post is represented by its lowest-id sibling and
  carries a new `album` object (`media_count`, `media_types`, `message_ids` — every
  sibling *present in this result set* stays reachable even though only the
  representative appears in `messages`; a sibling excluded by `media_filter`, a
  date bound, or window/adjacency effects is not counted or listed — see Known
  limitations); `text` is taken from whichever sibling actually carries the caption.
  This changes result shapes on album-heavy channels: a page that previously
  returned N raw sibling messages for one album now returns 1 collapsed post plus
  whatever else fit under `limit`. Pass `"collapse_albums": false` to get the exact
  pre-0.15 behavior — every sibling returned as its own message, `limit` counting
  raw messages again.
- `SearchResult.total_found` (surfaced on the wire as `SearchResponse.total_found`) is renamed to `returned` (work-order B6): the field was always the page size (`messages.len()`), never a true match count, and the old name invited callers to read it as "total matches available." `QueryMetadata` no longer echoes the requested `hours_back` or an ambiguous `channels_searched` count; it now reports the window and scope a query actually executed with (work-order B7): `window_from` (effective window start — `from_date`, or `now - hours_back`) and `window_to` (effective upper bound, omitted entirely when the window is open-ended) replace `hours_back`; `channels_scanned` (the number of channels actually scanned — `null` for a global search, where the scan scope is unknowable server-side) and `channels_in_results` (distinct channels present in `messages`, always a number) replace the single overloaded `channels_searched`. Affects `search_messages` and `get_recent_messages` responses.
- `Channel.username` and `Message.channel_username` are now `Option<Username>`, serialized as `null` when the chat has no public username — the `"unknown"`/`"group"` sentinel strings are gone (work-order B9). Both were syntactically valid Telegram usernames that could collide with a real channel (`@premium` exists in the wild). Clients matching on the literal sentinel strings must switch to checking for `null`. New field `chat_type` (`"channel"` | `"supergroup"` | `"group"`) on every `Channel` object identifies the kind of chat (broadcast channel, megagroup, or basic group/community) without inferring it from `username`'s presence.
- `ChannelsResponse` (`get_subscribed_channels` and `search_public_channels`) gains `returned` (the page size, `channels.len()`); `total` is now `Option<usize>` and `has_more` is now `Option<bool>`, both `null` when the source cannot know the answer, rather than a value that looked authoritative but wasn't (work-order B6a). `get_subscribed_channels` now walks its entire dialog list on every call (not just the requested page) so `total` is a genuine subscription count instead of the page size, and the previous over-fetch-one-row trick for detecting a next page is gone — `has_more` is derived straight from the true total and is always `Some(...)` for this tool. `search_public_channels`'s `contacts.search` has no global match count, so its `total` is always `null`; its `has_more` is `null` when the page came back full (`returned == limit`, work-order D10 — a full page says nothing about what lies beyond it) and `Some(false)` when it came back short of `limit`.
- `get_message_media` metadata response: `original_width`, `original_height`, `original_size_bytes` are renamed to `source_variant_width`, `source_variant_height`, `source_variant_size_bytes` to clarify that these are the dimensions and size of the selected variant Telegram served, not necessarily the original file (work-order D3). Two new fields `largest_available_width` and `largest_available_height` report the largest variant Telegram offers for the media, letting clients detect when a better (higher-resolution) version exists and re-fetch with relaxed `max_dimension` constraints (work-order D3).

### Added
- `Channel.last_message_date` is now populated on `get_subscribed_channels` (from the dialog's top message, work-order B8) and on `get_channel_info` when `include_full: true` (via a one-message history peek; if the peek fails, the field is left `null` and the call still succeeds). Everywhere else it remains `null`. When a channel has no messages, `last_message_date` is `null`.
- Every message now carries a `link` (the same permalink `generate_message_link` returns, public `t.me/<username>/<id>` form when the channel has a username, members-only `t.me/c/<channel_id>/<id>` otherwise), `reactions` (itemized standard-emoji reactions as `{emoji, count}`, omitted when the message has none), `reactions_total` (count across every reaction kind including custom-emoji and paid, which aren't individually itemized), and `grouped_id` (Telegram's album/media-group id, shared by sibling messages posted as an album, `null` otherwise). All four are zero-extra-RPC — derived from data already present in the message the server returned (work-orders D1, D2). `grouped_id` is also what the new `collapse_albums` post-collapsing feature (above) groups siblings by.
- `search_messages` and `get_recent_messages` accept a new optional `collapse_albums` boolean (default `true`, see Changed above) and, when a post is a collapsed album, its `album` object (`media_count: u32`, `media_types: Vec<MediaType>`, `message_ids: Vec<MessageId>`), work-orders B5/A2.

### Known limitations
- Forward attribution (`forwarded_from.channel_name` and `forwarded_from.channel_username`) stays id-only (work-order B10): `channel_id` and `original_message_id` are populated, but `channel_name` and `channel_username` remain `null` (resolving them would require an extra RPC per message, breaking the zero-extra-call invariant). Batch attribution — resolving multiple channel ids in one call — is the planned `resolve_channels` tool for v0.18 (roadmap A7).
- An album that straddles the fetched result window (cut off by `limit`, `media_filter`, a `from_date`/`to_date` bound, or global-search adjacency) is collapsed only from the siblings actually present in that window — `album.media_count` and `album.message_ids` describe the fetched subset, not necessarily the full album Telegram holds. A single surviving sibling is indistinguishable from a genuine non-album post and is returned as a plain message with no `album` field.

## [0.14.0] - 2026-08-10

### Added
- `generate_message_link` and `open_message_in_telegram` now emit shareable public links for public channels: when the channel has a username, `https_link` is `https://t.me/<username>/<id>` and `tg_protocol_link` is `tg://resolve?domain=<username>&post=<id>`; chats without a username keep the members-only `https://t.me/c/<channel_id>/<id>` / `tg://privatepost` forms (previously every channel got the members-only forms, which don't resolve for non-members). Two additive fields on the link response: `internal_link` (always the members-only `t.me/c/` https form) and `is_public`. The stray `?single`/`&single` suffix — a media-group hint, not part of a canonical message link — is no longer appended. To learn the username, both tools now resolve the peer once through a shared link builder, so they are no longer offline: each call charges 1 rate-limiter token and requires a connected session, and the response's `channel_id` now carries the canonical numeric id rather than echoing the raw input.
- `generate_message_link` accepts a channel username (`"swodki"`, `"@swodki"`) as well as a numeric id in `channel_id` — it was previously the only tool restricted to strictly numeric ids.

### Fixed
- Deleted or never-existent message ids no longer yield fabricated success responses (empty `text`, epoch `"1970-01-01T00:00:00Z"` timestamp, missing `views`/`forwards`). Telegram returns a `MessageEmpty` placeholder for such ids, which was previously converted as if it were a real message; `get_message_by_link`, `get_message_media`, and `transcribe_voice_message` now fail with `Message {id} not found or deleted in channel {channel}`, and the iteration paths (`search_messages`, `get_recent_messages`) skip the placeholder instead of emitting it.
- The published `inputSchema` for `search_messages` and `get_recent_messages` referenced `#/$defs/MediaFilter` without emitting a `$defs` block, so schema-following clients could not construct a valid `media_filter` value — the feature was unusable. The enum is now inlined into both schemas, and a new schema-walk test asserts that every `$ref` in every published tool schema has a resolvable target, so the pre-merge gate catches any future dangling reference.
- `open_message_in_telegram` on non-macOS platforms now returns a proper tool error (`open_message_in_telegram is only supported on macOS`) without charging a rate-limiter token, instead of a success-shaped response with `success: false` buried inside.

## [0.13.0] - 2026-08-10

### Added
- `search_messages` and `get_recent_messages` now accept optional `from_date`/`to_date` (RFC 3339 UTC) to filter by a fixed time window instead of a rolling `hours_back` lookback. `from_date`, when set, overrides `hours_back` as the window start and — unlike `hours_back` — is not clamped, so it can reach back beyond the `hours_back` cap; how far is practical depends on channel traffic, since deep windows are paged client-side and may time out on busy channels. `to_date` excludes messages newer than the given instant; set without `from_date` it must fall inside the `hours_back` window, otherwise the window is empty and the call is rejected with a message pointing at `from_date`. Both bounds are inclusive, so `from_date == to_date` is a valid single-instant window; a present-but-blank date is rejected rather than silently ignored.
- `get_channel_info` now accepts optional `include_full` (default `false`); when `true`, it performs one extra `channels.getFullChannel` RPC to populate `description` and `member_count`, which are otherwise always `null`. Channel-kind peers only (broadcasts and megagroups); other peer kinds (small groups, communities) silently fall back to basic info, as does a channel whose full-info RPC fails. The extra RPC costs one rate-limiter token on top of the basic lookup. `last_message_date` is not populated by either path.
- New tool `search_public_channels` (tool 12): keyword search over Telegram's public directory (`contacts.search`), returning the same `Channel` shape as `get_subscribed_channels`. `is_subscribed` reflects whether you're already subscribed to each found channel (Telegram's `contacts.search` mixes directory matches with matches from your own dialogs in one result set); `true` is reliable, `false` is best-effort, since the dialog side of that result set is server-capped and prefix-matched. At most `limit` results are returned. Drill down into an unsubscribed result with `get_channel_info` and the result's real `@username` — its numeric `id` will not resolve until you join the channel. Closes the "find sources" gap — previously the connector could only work with channels the user already knew about.
- `tools/list` now carries SEP-2549 cache hints (`ttlMs`/`cacheScope`) per MCP 2026-07-28: a 1-hour `ttlMs` (the tool list is static per build) and `cacheScope: private` (single-user server). Hints are withheld from clients that didn't negotiate protocol revision 2026-07-28 or later, since SEP-2549 doesn't exist in earlier revisions.

### Fixed
- `search_messages`'s `hours_back` parameter description advertised "max: 168"; the actual enforced cap (`SearchParams::MAX_HOURS_BACK`) is 72. The schema now matches the enforced limit.

## [0.12.0] - 2026-08-10

### Changed
- Upgraded `rmcp` 1.8 → 3.1, adopting the SDK line for MCP protocol revision 2026-07-28 (stateless lifecycle, `server/discover`, Multi Round-Trip Request plumbing, `resultType` on results). rmcp negotiates per client: peers speaking 2026-07-28 get the new lifecycle automatically, while current clients continue to use the legacy `initialize` handshake unchanged — no MCP-visible behavior change for existing consumers. Code impact was confined to the rmcp 3 model rename `Content` → `ContentBlock` (now a plain enum without the `Annotated`/`RawContent` wrapper) in `get_message_media` and its tests; the tool router, `InstrumentedTransport`, and all tool schemas were source-compatible.
- The three `grammers-*` git dependencies now point at the new upstream home, `https://codeberg.org/Lonami/grammers`, pinned to a specific rev rather than tracking `master` — Codeberg master is active and ships breaking changes, so upgrades are now a deliberate rev bump instead of whatever `cargo update` happens to pull. (The project migrated off GitHub in February 2026; the GitHub repository is a stale mirror slated for deletion — do not point the dependencies back at it.) This carries grammers 0.8.1 → 0.10.0: message dates are now jiff `Timestamp`s at the grammers boundary (converted to the domain's chrono `DateTime<Utc>` at second precision — Telegram dates carry no sub-second component, so nothing is lost), `Peer::to_ref` is fallible, `PeerId::bare_id` returns `Option<i64>` (`None` only for the self-user sentinel), the new `Peer::Community` kind is mapped like a group, and the account Premium flag is read from the raw TL user object (`User::is_premium` was removed). No MCP-visible schema changes.
- Refreshed all semver-compatible dependencies via a full `cargo update` (notably rmcp 1.7.0 → 1.8.0 and tokio → 1.53.1).

### Fixed
- Fresh dependency resolution (bare `cargo update`, or any build without the committed `Cargo.lock`) no longer fails. Every version of the transitive crate `core2` was yanked from crates.io, and `glass_pumpkin` 1.7–1.9 (required as `^1.7` by grammers-crypto 0.8) depend on it, while the core2-free 1.10.0 was also yanked — so no `^1.7` version was resolvable and the lockfile was the only thing keeping builds alive. grammers 0.10 requires `glass_pumpkin 2.0.0-rc0`, which drops core2 entirely.

## [0.11.1] - 2026-06-20

### Changed
- **Breaking:** `Channel.member_count` is now `Option<u64>` (JSON `null` when not fetched) instead of always-`0`. Both the channel list (`get_subscribed_channels`) and the single-channel lookup (`get_channel_info`) derive channels from basic dialog/peer info, which carries no member count — so they previously reported a misleading `0`, indistinguishable from a genuinely empty channel. They now report `null` to mean "not fetched." Clients keying on `member_count` being a number must tolerate `null`.

### Fixed
- `get_subscribed_channels` no longer reports `has_more: true` when the page exactly fills the requested `limit` (or a multiple of it). The server now over-fetches one extra channel to detect a real next page, so consumers no longer make a wasted round-trip that returns zero channels at exact page boundaries.

## [0.11.0] - 2026-06-20

### Added
- `search_messages`, `get_recent_messages`, and `get_message_by_link` now enrich messages with optional, zero-extra-API-call media metadata: `video_info` for video-class media (`duration_seconds`, `width`, `height`, `file_size_bytes`, `kind` — `video`/`video_note`/`animation` —, `has_thumbnail`, `mime_type`) and `audio_info` for audio-class media (`duration_seconds`, `file_size_bytes`, `kind` — `audio`/`voice` —, `mime_type`). Both are derived from the message's document attributes — the full video/audio is never downloaded — and are omitted when absent, so existing consumers are unaffected. `get_message_media` now also includes `video_info` in its metadata block.

## [0.10.1] - 2026-06-20

### Changed
- **Breaking:** message `media_type` for round videos is now reported as `"video_note"` (was `"videonote"`) by `search_messages`, `get_recent_messages`, `get_message_info`, and `get_message_by_link`. This aligns the value with `transcribe_voice_message`, the `media_filter` request parameter, and error messages — `MediaType` now serializes as `snake_case`. Clients keying on the literal `"videonote"` must update.

## [0.10.0] - 2026-06-20

### Added
- New `transcribe_voice_message` tool (tool 11): transcribes voice messages and video notes (round videos) to text via Telegram's server-side `messages.transcribeAudio` (no local ML). Resolves the peer once, validates the media type (rejecting non-voice/non-video_note), then polls by re-invoking until the transcription completes or `timeout_seconds` (default 30, clamped to 1–120) elapses, returning partial text with `partial: true` on timeout. **Requires Telegram Premium** on the connected account, and is subject to Telegram's weekly transcription quota; without Premium the tool fast-fails before spending a rate-limit token or making a transcription call. Charged `rate_limiting.transcription_cost` tokens (default 5, versus 1 for searches).
- `premium` flag in `check_mcp_status` output: reports whether the connected account has Telegram Premium (`null` if it could not be determined). Detected eagerly at startup and cached, so it is available from the first request.
- `[rate_limiting] transcription_cost` config option (default 5): rate-limiter tokens charged per `transcribe_voice_message` call.

## [0.9.0] - 2026-06-13

### Added
- `search_messages` and `get_recent_messages` now enrich each message with optional, zero-extra-API-call metadata extracted from the Telegram response: `forwarded_from` (forward attribution — source `channel_id`, `original_message_id`, `original_date`, and `sender_name` for hidden senders; the source channel's title/username are not exposed per message and are intentionally omitted), `link_preview` (Telegram's server-side webpage preview — `url`, `site_name`, `title`, `description` truncated to 500 characters), `views`, `forwards`, and `reply_to_message_id`. All fields are omitted when absent, so existing response consumers are unaffected. `get_message_by_link` returns the same enriched message shape. Internally the message wire format moved to dedicated `MessageResponse`/`SearchResponse` DTOs mapped from the domain types.

## [0.8.0] - 2026-06-12

### Added
- New `get_message_media` tool (tool 10): returns a message's photo — or the server-side thumbnail of its video/animation/video note (`is_thumbnail: true`) — as an MCP image content block (base64 JPEG, quality 80) plus a JSON metadata text block (media type, caption, original/returned dimensions and byte sizes). Images are downscaled so the longest side fits `max_dimension` (default 1280, clamped to 64–2048), with the smallest sufficient server-side size variant chosen before downloading; photos whose selected variant exceeds 20 MB are refused; the base64 payload is capped at ~1.5 MB with automatic further downscaling. Downloads are charged `media_download_cost` rate-limiter tokens (`[rate_limiting]`, default 5) and bounded by a new `download_secs` timeout (`[telegram.timeouts]`, default 120).
- Responses larger than `max_buffered_payload_bytes` (`[observability]`, default 256 KiB) are stored in the `get_last_responses` ring buffer as a stub instead of the full payload, so megabyte-sized image responses don't pin memory or get replayed as text.

## [0.7.0] - 2026-06-12

### Changed
- Updated `chrono` from 0.4.44 to 0.4.45; audited all other direct dependencies against crates.io and confirmed they are already at their latest stable versions. A full `cargo update` remains blocked upstream (`grammers-crypto` requires `glass_pumpkin` ^1.7, whose non-yanked releases depend on the fully-yanked `core2` crate) — use targeted `cargo update -p <crate>` until that is fixed.

## [0.6.0] - 2026-06-12

### Added
- `[observability]` config table (`slow_write_threshold_ms`, default 500; `response_buffer_size`, default 10) and an instrumented stdio transport: every response write to stdout is logged with the JSON-RPC request id, tool name, payload size and write+flush duration; writes slower than the threshold emit a WARN (a stalling pipe means the peer stopped reading); stdin EOF and signal shutdown log a session summary (uptime, request/response counters, age of last write, abandoned in-flight requests). Built after the 2026-06-12 lost-response incident (`docs/connetion-issue.md`).
- `check_mcp_status` now reports `requests_received`, `responses_written`, `last_response_write_age_secs`, `session_started_at`, and `session_uptime_secs`, making a zombie bridge session visible from the client side.
- New `get_last_responses` debug/recovery tool (tool 9): returns the last N responses written to stdout from an in-memory ring buffer, so a response lost in transit can be re-fetched without re-querying Telegram or spending rate-limit budget.

### Changed
- All MCP tools now emit symmetric `Tool invocation started` / `Tool invocation completed` / `Tool invocation failed` log entries correlated by JSON-RPC `request_id` and carrying `duration_ms` (previously 5 of 8 tools logged only `started`, and no entry carried the request id).

## [0.5.2] - 2026-05-31

### Changed
- MCP tool request parameters now accept both JSON scalar forms, so clients that send numbers as strings (or vice-versa) are no longer rejected. Numeric fields (`limit`, `offset`, `hours_back`, `message_id`) accept either a JSON number or a numeric string (trimmed; `"10"` and `" 10 "` → `10`); string fields (`channel_id`, `channel_identifier`, `query`, `link`) accept an integer number and stringify it (`123` → `"123"`); boolean fields (`include_tg_protocol`, `use_tg_protocol`) accept `true`/`false`, `1`/`0`, and their string forms (case-insensitive). Empty strings on optional fields become `None`; floats and other non-coercible values still return a clear error. Implemented as five reusable `serde` `deserialize_with` helpers in `src/mcp/tools/types/serde_helpers.rs`; field types and the advertised JSON schema are unchanged.

## [0.5.1] - 2026-05-23

### Changed
- Updated `rmcp` from 1.6.0 to 1.7.0; routine patch bumps for `serde_json` 1.0.150, `anyhow` 1.0.102, `chrono` 0.4.44, `dashmap` 6.2.1, `tempfile` 3.27.0, `filetime` 0.2.29.

## [0.5.0] - 2026-05-22

### Added
- `[telegram.timeouts]` config section with `resolve_secs` / `history_secs` / `search_secs` (defaults 30 / 60 / 120). All grammers network calls in `TelegramClient` are now bounded by these budgets via the new `with_timeout` helper, surfacing as `Error::Timeout { operation, secs }` instead of hanging indefinitely.
- `tracing::info!("Tool invocation started")` entry log at the top of all 8 MCP tools so hung requests can be diagnosed from the log file without changing log level.

### Fixed
- MCP requests no longer hang when a single grammers call (`resolve_username`, `iter_messages.next()`, `search_iter.next()`) stalls — the offending operation now times out and returns a typed `Error::Timeout` to the client.

## [0.4.1] - 2026-05-09

### Changed
- Upgraded `rmcp` from 0.15.0 to 1.6.0 and migrated `McpServer::get_info` to the new builder API (`InitializeResult::new` + `with_server_info` / `with_instructions`)
- Updated dependencies: `tokio` 1.49.0 → 1.52.3, `clap` 4.5.58 → 4.6.1, `toml` 0.9.8 → 1.1.2, `tracing-appender` 0.2 → 0.2.5, `filetime` 0.2.27 → 0.2.28

## [0.4.0] - 2026-04-12

### Added
- `get_message_by_link` MCP tool to retrieve a specific Telegram message by its `t.me` link (public and private channel links supported)
- `parse_telegram_link` function in `link.rs` for parsing `https://t.me/...` URLs into channel reference and message ID
- `get_message_by_id` method on `TelegramClientTrait` for fetching a single message by channel and message ID

### Fixed
- Updated `proptest` dev-dependency from 1.6.0 to 1.11.0

## [0.3.1] - 2026-03-22

### Fixed
- Environment variable expansion is now skipped for TOML comment lines (lines starting with `#`)

## [0.3.0] - 2026-03-22

### Changed
- Environment variable expansion now returns an error for missing variables instead of leaving the placeholder unexpanded
- Recursive environment variable expansion (variables referencing other variables) is now prevented with a clear error

## [0.2.0] - 2026-03-22

### Added
- MCP server with 7 tools exposed via JSON-RPC/stdio using `rmcp` SDK
- Telegram client integration via `grammers` (MTProto) with session persistence
- Channel search and listing by username or channel ID
- Full-text message search across channels with optional media filtering
- Recent messages retrieval with configurable time window and client-side media filtering
- Rate limiting with trait-based dependency injection for testability
- Type-safe domain model with newtype wrappers (`ChannelId`, `MessageId`, `UserId`, `Username`, `ChannelName`)
- File logging with daily rotation and configurable retention (`max_log_days`)
- Log cleanup on startup to enforce retention policy
- Config loading from TOML with environment variable expansion (including numeric fields)
- Interactive setup mode (`--setup`) for first-time Telegram authentication
- Telegram deep-link generation for channels and messages (`tg://` scheme)
- `MockTelegramClientTrait` and `MockRateLimiterTrait` via `mockall` for unit testing
- Comprehensive unit and integration tests across MCP tools and Telegram client

### Changed
- Adapted to breaking `grammers-client` API changes for session management, peer access, and message handling
- Serialized MCP tool responses as raw JSON strings for spec compliance
- Renamed log files to `<prefix>-<date>.<suffix>` format
- Migrated test modules to file-as-module pattern (no `mod.rs`)
- Downgraded verbose `info!` logs to `debug!` to reduce noise

### Fixed
- Environment variable expansion for numeric TOML config fields
- Telegram deep-link scheme corrected from `tg://resolve` to `tg://privatepost`
- Empty `media_filter` strings now treated as `None` in search requests
- MCP server errors now propagate and produce a clean exit
