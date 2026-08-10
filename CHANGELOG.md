# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
