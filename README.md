# Telegram MCP Connector

A Model Context Protocol (MCP) service that enables Codex and other MCP clients to search
and interact with Telegram channels and messages in real-time. Built in Rust using
the `rmcp` SDK and the `grammers` Telegram client.

## Features

- **Real-time Telegram access** over MTProto — search across all subscribed channels, one
  channel, or a fan-out of up to 20 channels
- **Recent history by time window** (no search query needed), with media-type filtering,
  date ranges, album collapsing, and drift-free pagination cursors
- **Media retrieval** — photos and video thumbnails returned as MCP image content blocks,
  downscaled to fit configurable byte budgets
- **Voice transcription** via Telegram's server-side API (requires Telegram Premium;
  `check_mcp_status` reports a `premium` flag so you can tell in advance)
- **Channel tools** — subscribed-channel listing, public directory search, batch identifier
  resolution, per-channel posting/engagement stats
- **Deep linking** — `https://t.me` / `tg://` links, plus opening messages directly in
  Telegram Desktop (macOS)
- **Built-in token-bucket rate limiter**, secure credential handling (`secrecy`), and daily
  rotated file logging

## Architecture

```
MCP Client (Codex or another stdio client)
        │ JSON-RPC over stdio
MCP Server Layer — 16 tools (rmcp, src/mcp/)
        │
Application Layer (config, logging, rate_limiter, link, error)
        │
Telegram Layer (grammers client, auth, converters, src/telegram/)
        │ MTProto
Telegram Cloud API
```

**Key design patterns:** library + binary separation (`lib.rs` / `main.rs`); shared state
via `Arc<T>`; traits + `mockall` for testability; JSON schemas via `schemars` for MCP tool
parameters and canonical tool outputs.

## Prerequisites

- **Rust** — 2024 edition; `rust-toolchain.toml` selects nightly with rustfmt and clippy
- **Telegram account** with phone number, and API credentials from https://my.telegram.org
- **Telegram Desktop** — only for the `open_message_in_telegram` tool (macOS only)

## Installation

```bash
git clone https://github.com/nimec77/telegram-connector.git
cd telegram-connector
cargo build --locked --release  # binary at target/release/telegram-mcp
```

Get API credentials at https://my.telegram.org → "API development tools" → create an
application → note `api_id` and `api_hash`.

### Configuration

Config file location by platform (or pass `--config <FILE>`, or set `TELEGRAM_MCP_CONFIG`):

| Platform | Config path |
|----------|-------------|
| Linux | `~/.config/telegram-connector/config.toml` |
| macOS | `~/Library/Application Support/telegram-connector/config.toml` |
| Windows | `%APPDATA%\telegram-connector\config.toml` |

```toml
[telegram]
api_id = 12345678
api_hash = "your_api_hash_here"
phone_number = "+1234567890"
# session_file = "~/.config/telegram-connector/session.bin"

[search]
# deadline_seconds = 20          # Wall-clock budget for a search accumulation loop (1–3600).
                                 # On expiry the search returns what it has, with
                                 # query_metadata.timed_out/partial set — never an error.

[rate_limiting]
# max_tokens = 60
# refill_rate = 2.0                # Tokens per second; must be > 0.
# media_download_cost = 3        # Tokens charged per image returned by media tools.

[limits]
# response_byte_budget = 40000           # Byte cap per message-stream response; over-budget
                                         # pages drop trailing messages and set has_more.
# media_batch_max_total_bytes = 8388608  # Cap (8 MiB) on a media batch's total base64 image
                                         # payload; images are downscaled progressively.

[logging]
# level = "info"                 # trace, debug, info, warn, error
# format = "compact"             # compact, pretty, json
# file_enabled = true            # File logging: always JSON, daily rotation
# file_path = "~/.config/telegram-connector/logs/"
# max_log_days = 7

[server]
# output_format = "json_v1"       # Default; use "legacy" for historical text responses.
# shutdown_timeout_seconds = 5
```

Sensitive values support env-var expansion: `api_hash = "${TELEGRAM_API_HASH}"`.

### Authenticate

```bash
./target/release/telegram-mcp --setup
```

Connects to Telegram, sends a verification code to your Telegram app, prompts for it (and
your 2FA cloud password if enabled), then saves the session for future runs.

## Usage

Start the server (communicates via JSON-RPC over stdio):

```bash
./target/release/telegram-mcp
```

```
Options:
  -s, --setup                Run interactive setup to authenticate
      --session-file <FILE>  Path to session file (overrides config)
  -c, --config <FILE>        Path to configuration file
  -h, --help                 Print help
  -V, --version              Print version
```

### Connect Codex

After authenticating with `--setup`, merge this table into `~/.codex/config.toml`,
replacing both paths with absolute paths:

```toml
[mcp_servers.telegram]
command = "/absolute/path/to/telegram-connector/target/release/telegram-mcp"
args = ["--config", "/absolute/path/to/telegram-connector-config.toml"]
startup_timeout_sec = 90
tool_timeout_sec = 180
```

Restart the Codex session and check `/mcp` for the active server. Startup allows
one `resolve_secs` budget each for authorization and best-effort Premium lookup
(up to 60 seconds with defaults); the 90-second client budget leaves headroom.
The longer tool timeout accommodates voice transcription, which can take up to 120 seconds.
See [the configuration example](.codex/config.toml.example) and
[Codex development guide](docs/codex.md) for CLI registration, project setup,
and recommended tools. These settings follow the
[official Codex MCP documentation](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).

Other MCP clients can launch the same binary with the same `--config` argument
over stdio using their own MCP configuration format.

## MCP Tools

Required parameters are marked `*`. Tool schemas are self-describing over MCP; this table is
the quick reference.

| Tool | Purpose | Key parameters |
|------|---------|----------------|
| `check_mcp_status` | Report cached authorization/Premium observations, live rate-limiter token budget/costs, and configured media limits; performs no Telegram RPCs. | (none) |
| `get_subscribed_channels` | List subscribed channels with genuine `total` and `has_more`; one token per call, with a total history timeout. | `limit`, `offset` |
| `get_channel_info` | Get details for one channel; `include_full` adds `description`/`member_count`/`last_message_date`. Both paths cost 1 token total. | `channel_identifier`*, `include_full` |
| `generate_message_link` | Build shareable `https://t.me/...` and `tg://` links for a message (public vs. members-only `t.me/c/...` forms). | `channel_id`*, `message_id`*, `include_tg_protocol` |
| `open_message_in_telegram` | Open a message directly in Telegram Desktop (macOS only; errors elsewhere). | `channel_id`*, `message_id`*, `use_tg_protocol` |
| `search_messages` | Search messages globally, in one channel, or fanned out over up to 20 channels, with time windows and media-type filtering. | `query` (required unless `media_filter` set), `channel_id`, `channel_ids`, `hours_back`, `limit`, `media_filter`, `from_date`, `to_date`, `collapse_albums`, `before_id`, `after_id`, `max_text_length`, `format` (`full`/`compact`) |
| `get_recent_messages` | Fetch recent channel history by time window without a query (history iteration, not search); same response shape as `search_messages`. | `channel_id` (required unless `channel_ids` set), `channel_ids`, `hours_back`, `limit`, `media_filter`, `from_date`, `to_date`, `collapse_albums`, `before_id`, `after_id`, `max_text_length`, `format` |
| `get_message_by_link` | Resolve a t.me message link straight to the full message object (costs 1 token). | `link`* (`https://t.me/username/12345`, `https://t.me/c/channel_id/12345`, `t.me/username/12345`) |
| `get_last_responses` | Recover only explicit caller keys in json_v1; last-N remains a legacy diagnostic. Images are omitted unless requested. No Telegram RPCs. | `recovery_keys` (required in json_v1), `n` (legacy), `include_binary` |
| `get_message_media` | Return a message's photo (or video thumbnail) as an MCP image content block (JPEG, downscaled) plus a JSON metadata text block. | `channel_id`*, `message_id`*, `max_dimension` (longest side px, clamped 64–2048) |
| `get_messages_media_batch` | Fetch photos/thumbnails for up to 10 messages from one channel in one call; returns a JSON envelope followed by indexed image blocks (legacy mode retains image/metadata pairs). | `channel_id`*, `message_ids`* (1–10, deduped), `max_dimension` |
| `transcribe_voice_message` | Transcribe a voice message or video note via Telegram's server-side API (requires Telegram Premium; costs 5 tokens by default). | `channel_id`*, `message_id`*, `timeout_seconds` (clamped 1–120) |
| `search_public_channels` | Search Telegram's public directory (`contacts.search`) for channels/groups beyond your subscriptions. | `query`*, `limit` |
| `get_messages_batch` | Fetch up to 50 specific messages from one channel in one RPC — the designated path to re-fetch full untruncated text. | `channel_id`*, `message_ids`* (1–50, deduped), `max_text_length` |
| `resolve_channels` | Batch-resolve up to 20 identifiers (numeric ID, `@username`, or exact subscribed-chat title) to full channel entities, with per-entry errors. | `identifiers`* (1–20) |
| `get_channel_stats` | Posting-rate and engagement summary (posts/day, median views, media/album share) over a bounded recent-history sample. | `channel_id`*, `days_back` (max 30; sweep also capped at 500 messages) |

Every tool except `get_last_responses` also accepts the optional wire-only `recovery_key`.

### Behavior notes

- **Tool output migration:** all 16 tools default to a versioned JSON envelope in
  `structuredContent` and matching text: `schema_version: "1"`, with exactly one of
  `data` or `error` non-null. Channel/user IDs are decimal strings; message IDs remain
  integers. Tool errors carry stable codes and set `isError: true`; partial successes
  retain per-item errors. Set `[server] output_format = "legacy"` to retain historical
  text shapes. This setting is independent of MCP protocol negotiation. See the
  [contract and migration guide](docs/tool-contract.md) for schemas, media indices,
  compatibility changes, serialized budgets and keyed recovery.
- **Channel identity:** bare numeric IDs that match distinct eligible Telegram peer
  namespaces fail with `ambiguous_channel`. Use an unambiguous public username where
  available. Numeric lookup scans the bounded dialog traversal before accepting a match.
- **Rate-limit costs:** search/history/channel-list/info/link calls cost 1 token; media
  downloads default 3; transcription defaults 5. `check_mcp_status` and
  `get_last_responses` are free. Local validation precedes acquisition. Channel-info
  resolution and optional full enrichment share one operation charge.
- **Account observations:** status reads cached authorization/Premium values. Startup
  refreshes them using bounded RPCs; authorization transport failures are errors and
  preserve prior observations. Premium lookup failure is nonfatal and preserves its last
  successful value (or `null` if unknown). Transcription refreshes an unknown Premium
  cache only after acquiring its operation charge; a cached non-Premium result rejects
  without charge. Attempted account RPCs are not refunded.
- **Transcription deadline:** `timeout_seconds` is one total budget after token
  acquisition, including cold account lookup, peer resolution, message validation,
  the initial transcription RPC, sleeps and polls. Configured RPC caps still apply.
  Expiry before the first transcription state returns a timeout error; after a state
  exists, it returns the latest text with `partial: true`. Completions at or after
  expiry cannot replace that partial. Cancellation stops scheduling further RPCs.
- **Channel listing:** `limit` defaults to 20 and accepts 1–500. One `history_secs` budget
  covers the entire dialog walk across all pages; timeout returns an error.
- **Fan-out token cost:** `channel_ids` fan-out atomically acquires one token per deduped
  channel; `get_messages_media_batch` acquires `media_download_cost × ids` up front and
  refunds ids that produced no image (whole-call failure refunds all but 1 token).
- **Pagination:** single-channel responses with more data carry
  `next_cursor: {"before_id": ...}` — pass it back as `before_id` for drift-free older
  pages; global search reports `has_more` without a cursor; cursors are rejected in
  `channel_ids` scope.
- **Byte budgets:** search/history and message batches count the complete serialized
  JSON-RPC response, including the structured envelope and matching JSON fallback, against
  `[limits] response_byte_budget` (default 40 000). Oldest messages are dropped with honest
  pagination; batches list existing budget-dropped IDs in `omitted_ids`. One oversized
  message survives when metadata fits; metadata-only overflow returns `response_too_large`.
  Media has independent base64 limits and a 65 536-byte non-binary overhead cap.
- **Targeted recovery:** supply a unique `recovery_key` (1–128 printable ASCII characters)
  before a recoverable call, then select it with `get_last_responses.recovery_keys`. Keys
  expire ten minutes after completion, capacity eviction or restart. Pending/unavailable
  results are explicit. Keys prevent accidental mixing and provide no authentication.
- **Media memory and deadlines:** production MCP shares four raw-buffer slots and an
  80 MiB raw reservation allowance across calls, plus two image workers. The configured
  download timeout covers setup, queues, transfer and processing. Source images must fit
  8192 pixels per side, 8 388 608 pixels total and a 64 MiB decoder allowance. Decode/resize
  and allocator memory are separate from raw admission; see status and the validation report.
- **Session files on macOS:** newly created session directories use 0700; the database,
  WAL/SHM and journal use 0600. Session/sidecar symlinks, hardlinks, nonregular files and
  an immediate parent writable by group/others are refused before SQLite opens them.
  Existing parent-directory modes are preserved.
- **Partial results:** `search_messages` sets `query_metadata.timed_out`/`partial` (never an
  error) when `[search] deadline_seconds` expires; a deadline-truncated result does not set
  `has_more`.
- **Text truncation:** texts longer than `max_text_length` (default 2000) are cut with
  `text_truncated: true` + `text_full_length`; refetch via `get_messages_batch` with a large
  `max_text_length`.
- **Multi-channel fan-out:** up to 20 channels, 4 concurrent, merged newest-first and
  truncated to `limit`; per-channel failures land in `channel_errors` and only an
  all-channel failure errors the call.
- **Album collapsing (default on):** `limit` counts posts, not raw messages; the `album`
  object describes only siblings present in the result set, so filters/windows can leave
  partial or invisible albums.
- **Free enrichment everywhere:** `forwarded_from`, `link` permalinks, reactions, and
  video/audio/document/poll metadata are derived with no extra API calls, identically across
  all message-returning tools.
- **Unsubscribed channels:** `search_public_channels` results are only reachable by their
  public `@username` — numeric-id follow-up requires joining first, and username-less
  results can't be drilled into at all.

## Troubleshooting

- **"Session expired"** — delete `~/.config/telegram-connector/session.bin` and re-run
  `--setup`.
- **"Invalid phone number format"** — use international format with country code
  (`+1234567890`).
- **"2FA password required"** — enter the Telegram *cloud* password at the setup prompt, not
  a local passcode.
- **"Failed to connect to Telegram"** — check internet, verify `api_id`/`api_hash`, or wait
  out a Telegram outage.
- **"Rate limited"** — wait the reported `retry_after_seconds`, reduce request frequency, or
  raise `refill_rate` in config.
- **Server not responding** — confirm the binary is running (`ps aux | grep telegram-mcp`),
  verify stdio isn't blocked, check the logs.
- **"Config file not found"** — put `config.toml` in the platform config dir (see table
  above), or pass `--config` / set `TELEGRAM_MCP_CONFIG`.
- **Tool not found** — verify the tool name, restart the MCP client after config changes,
  and check `/mcp` in Codex.

## Development

```bash
just check          # format check, all-target clippy, and tests (matches CI)
cargo test --locked # all tests; no Telegram credentials required
```

The same commands are available as `just` recipes (`just` to list, `just check` for the full
gate). Start Codex in the repository to load [AGENTS.md](AGENTS.md) and discover the
repository skills in `.agents/skills/`. See [docs/codex.md](docs/codex.md) for the
migration analysis and optional integrations, and `docs/conventions.md` /
`docs/memory.md` for conventions and durable project knowledge.
