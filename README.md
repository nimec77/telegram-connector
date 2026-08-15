# Telegram MCP Connector

A Model Context Protocol (MCP) service that enables Claude to search and interact with
Telegram channels and messages in real-time. Built in Rust using the `rmcp` SDK and the
`grammers` Telegram client.

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
MCP Client (Claude Desktop)
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
parameters.

## Prerequisites

- **Rust** — 2024 edition (nightly toolchain)
- **Telegram account** with phone number, and API credentials from https://my.telegram.org
- **Telegram Desktop** — only for the `open_message_in_telegram` tool (macOS only)

## Installation

```bash
git clone https://github.com/nimec77/telegram-connector.git
cd telegram-connector
cargo build --release        # binary at target/release/telegram-mcp
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
# default_hours_back = 48
# max_results_default = 20
# max_results_limit = 100
# deadline_seconds = 20          # Wall-clock budget for a search accumulation loop (1–3600).
                                 # On expiry the search returns what it has, with
                                 # query_metadata.timed_out/partial set — never an error.

[rate_limiting]
# max_tokens = 60
# refill_rate = 2.0
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

Claude Desktop config (`~/.config/claude-desktop/config.json`), then restart Claude Desktop:

```json
{
  "mcpServers": {
    "telegram": { "command": "/path/to/telegram-mcp", "args": [] }
  }
}
```

## MCP Tools

Required parameters are marked `*`. Tool schemas are self-describing over MCP; this table is
the quick reference.

| Tool | Purpose | Key parameters |
|------|---------|----------------|
| `check_mcp_status` | Report connection status, live rate-limiter token budget/costs, and configured media limits so callers can plan batches. | (none) |
| `get_subscribed_channels` | List all channels the account is subscribed to, with genuine `total` and derived `has_more`. | `limit`, `offset` |
| `get_channel_info` | Get details for one channel; `include_full` adds `description`/`member_count`/`last_message_date` via extra RPCs for 1 token. | `channel_identifier`*, `include_full` |
| `generate_message_link` | Build shareable `https://t.me/...` and `tg://` links for a message (public vs. members-only `t.me/c/...` forms). | `channel_id`*, `message_id`*, `include_tg_protocol` |
| `open_message_in_telegram` | Open a message directly in Telegram Desktop (macOS only; errors elsewhere). | `channel_id`*, `message_id`*, `use_tg_protocol` |
| `search_messages` | Search messages globally, in one channel, or fanned out over up to 20 channels, with time windows and media-type filtering. | `query` (required unless `media_filter` set), `channel_id`, `channel_ids`, `hours_back`, `limit`, `media_filter`, `from_date`, `to_date`, `collapse_albums`, `before_id`, `after_id`, `max_text_length`, `format` (`full`/`compact`) |
| `get_recent_messages` | Fetch recent channel history by time window without a query (history iteration, not search); same response shape as `search_messages`. | `channel_id` (required unless `channel_ids` set), `channel_ids`, `hours_back`, `limit`, `media_filter`, `from_date`, `to_date`, `collapse_albums`, `before_id`, `after_id`, `max_text_length`, `format` |
| `get_message_by_link` | Resolve a t.me message link straight to the full message object (costs 1 token). | `link`* (`https://t.me/username/12345`, `https://t.me/c/channel_id/12345`, `t.me/username/12345`) |
| `get_last_responses` | Replay the last N buffered stdout responses to recover lost/truncated replies without re-querying Telegram; image payloads stubbed unless requested. | `n`, `include_binary` |
| `get_message_media` | Return a message's photo (or video thumbnail) as an MCP image content block (JPEG, downscaled) plus a JSON metadata text block. | `channel_id`*, `message_id`*, `max_dimension` (longest side px, clamped 64–2048) |
| `get_messages_media_batch` | Fetch photos/thumbnails for up to 10 messages from one channel in one call; returns `[image, metadata, …, summary]` content blocks. | `channel_id`*, `message_ids`* (1–10, deduped), `max_dimension` |
| `transcribe_voice_message` | Transcribe a voice message or video note via Telegram's server-side API (requires Telegram Premium; costs 5 tokens by default). | `channel_id`*, `message_id`*, `timeout_seconds` (clamped 1–120) |
| `search_public_channels` | Search Telegram's public directory (`contacts.search`) for channels/groups beyond your subscriptions. | `query`*, `limit` |
| `get_messages_batch` | Fetch up to 50 specific messages from one channel in one RPC — the designated path to re-fetch full untruncated text. | `channel_id`*, `message_ids`* (1–50, deduped), `max_text_length` |
| `resolve_channels` | Batch-resolve up to 20 identifiers (numeric ID, `@username`, or exact subscribed-chat title) to full channel entities, with per-entry errors. | `identifiers`* (1–20) |
| `get_channel_stats` | Posting-rate and engagement summary (posts/day, median views, media/album share) over a bounded recent-history sample. | `channel_id`*, `days_back` (max 30; sweep also capped at 500 messages) |

### Behavior notes

- **Rate-limit costs:** metered search/history/info/link calls cost 1 token; media downloads
  default 3; transcription defaults 5; `check_mcp_status`, `get_subscribed_channels`,
  `get_last_responses`, and `get_channel_info` without `include_full` are free.
- **Fan-out token cost:** `channel_ids` fan-out atomically acquires one token per deduped
  channel; `get_messages_media_batch` acquires `media_download_cost × ids` up front and
  refunds ids that produced no image (whole-call failure refunds all but 1 token).
- **Pagination:** single-channel responses with more data carry
  `next_cursor: {"before_id": ...}` — pass it back as `before_id` for drift-free older
  pages; global search reports `has_more` without a cursor; cursors are rejected in
  `channel_ids` scope.
- **Byte budgets:** responses are capped at `[limits] response_byte_budget` (default
  40 000 bytes; trailing messages dropped, `has_more: true`); `get_messages_batch` lists
  budget-dropped-but-existing ids in `omitted_ids` (distinct from `missing`).
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
- **Tool not found** — verify the tool name and restart Claude Desktop after config changes.

## Development

```bash
cargo test                                              # all tests
cargo fmt --check && cargo clippy -- -D warnings && cargo test   # pre-commit gate
```

The same commands are available as `just` recipes (`just` to list, `just check` for the full
gate). Project docs for contributors live in `CLAUDE.md` (architecture, workflow) and
`docs/` (`conventions.md`, `memory.md`).
