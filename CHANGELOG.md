# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed (breaking)
- `search_messages` and `get_recent_messages` now collapse album (grouped-media)
  siblings into a single post-level result by default (new `collapse_albums`
  parameter, default `true`, work-orders B5/A2): `limit` counts **posts**, not raw
  messages, and an album is never split across the limit boundary (once a post's
  first sibling is admitted, the rest of that album is always admitted too, even
  past `limit`). The collapsed post is represented by its lowest-id sibling and
  carries a new `album` object (`media_count`, `media_types`, `message_ids` — every
  sibling id stays reachable even though only the representative appears in
  `messages`); `text` is taken from whichever sibling actually carries the caption.
  This changes result shapes on album-heavy channels: a page that previously
  returned N raw sibling messages for one album now returns 1 collapsed post plus
  whatever else fit under `limit`. Pass `"collapse_albums": false` to get the exact
  pre-0.15 behavior — every sibling returned as its own message, `limit` counting
  raw messages again.
- `SearchResult.total_found` (surfaced on the wire as `SearchResponse.total_found`) is renamed to `returned`: the field was always the page size (`messages.len()`), never a true match count, and the old name invited callers to read it as "total matches available." `QueryMetadata` no longer echoes the requested `hours_back` or an ambiguous `channels_searched` count; it now reports the window and scope a query actually executed with: `window_from` (effective window start — `from_date`, or `now - hours_back`) and `window_to` (effective upper bound, omitted entirely when the window is open-ended) replace `hours_back`; `channels_scanned` (the number of channels actually scanned — `null` for a global search, where the scan scope is unknowable server-side) and `channels_in_results` (distinct channels present in `messages`, always a number) replace the single overloaded `channels_searched`. Affects `search_messages` and `get_recent_messages` responses.
- `Channel.username` and `Message.channel_username` are now `Option<Username>`, serialized as `null` when the chat has no public username — the `"unknown"`/`"group"` sentinel strings are gone. Both were syntactically valid Telegram usernames that could collide with a real channel (`@premium` exists in the wild). Clients matching on the literal sentinel strings must switch to checking for `null`. New field `chat_type` (`"channel"` | `"supergroup"` | `"group"`) on every `Channel` object identifies the kind of chat (broadcast channel, megagroup, or basic group/community) without inferring it from `username`'s presence.
- `ChannelsResponse` (`get_subscribed_channels` and `search_public_channels`) gains `returned` (the page size, `channels.len()`); `total` is now `Option<usize>` and `has_more` is now `Option<bool>`, both `null` when the source cannot know the answer, rather than a value that looked authoritative but wasn't. `get_subscribed_channels` now walks its entire dialog list on every call (not just the requested page) so `total` is a genuine subscription count instead of the page size, and the previous over-fetch-one-row trick for detecting a next page is gone — `has_more` is derived straight from the true total and is always `Some(...)` for this tool. `search_public_channels`'s `contacts.search` has no global match count, so its `total` is always `null`; its `has_more` is `null` when the page came back full (`returned == limit`, work-order D10 — a full page says nothing about what lies beyond it) and `Some(false)` when it came back short of `limit`.
- `get_message_media` metadata response: `original_width`, `original_height`, `original_size_bytes` are renamed to `source_variant_width`, `source_variant_height`, `source_variant_size_bytes` to clarify that these are the dimensions and size of the selected variant Telegram served, not necessarily the original file (work-order D3). Two new fields `largest_available_width` and `largest_available_height` report the largest variant Telegram offers for the media, letting clients detect when a better (higher-resolution) version exists and re-fetch with relaxed `max_dimension` constraints (work-order D3).

### Added
- `Channel.last_message_date` is now populated on `get_subscribed_channels` (from the dialog's top message, work-order B8) and on `get_channel_info` when `include_full: true` (via a one-message history peek; if the peek fails, the field is left `null` and the call still succeeds). Everywhere else it remains `null`. When a channel has no messages, `last_message_date` is `null`.
- Every message now carries a `link` (the same permalink `generate_message_link` returns, public `t.me/<username>/<id>` form when the channel has a username, members-only `t.me/c/<channel_id>/<id>` otherwise), `reactions` (itemized standard-emoji reactions as `{emoji, count}`, omitted when the message has none), `reactions_total` (count across every reaction kind including custom-emoji and paid, which aren't individually itemized), and `grouped_id` (Telegram's album/media-group id, shared by sibling messages posted as an album, `null` otherwise). All four are zero-extra-RPC — derived from data already present in the message the server returned (work-orders D1, D2). `grouped_id` is also what the new `collapse_albums` post-collapsing feature (above) groups siblings by.
- `search_messages` and `get_recent_messages` accept a new optional `collapse_albums` boolean (default `true`, see Changed above) and, when a post is a collapsed album, its `album` object (`media_count: u32`, `media_types: Vec<MediaType>`, `message_ids: Vec<MessageId>`), work-orders B5/A2.

### Known limitations
- Forward attribution (`forwarded_from.channel_name` and `forwarded_from.channel_username`) stays id-only: `channel_id` and `original_message_id` are populated, but `channel_name` and `channel_username` remain `null` (resolving them would require an extra RPC per message, breaking the zero-extra-call invariant). Batch attribution — resolving multiple channel ids in one call — is the planned `resolve_channels` tool for v0.18 (roadmap A7).

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
