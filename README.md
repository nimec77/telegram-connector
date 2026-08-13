# Telegram MCP Connector

A Model Context Protocol (MCP) service that enables Claude to search and interact with Telegram channels and messages in real-time. Built in Rust using the `rmcp` SDK and `grammers` Telegram client.

## Features

- **Real-time Telegram Access** - Connect to Telegram using the MTProto protocol
- **Message Search** - Search messages across all subscribed channels or specific channels
- **Recent Messages** - Get recent messages from a channel by time window (no search query needed)
- **Media Filtering** - Filter search results by media type (photos, videos, documents, etc.)
- **Channel Management** - List subscribed channels and get channel metadata
- **Deep Linking** - Generate `tg://` and `https://t.me` links for messages
- **Native Integration** - Open messages directly in Telegram Desktop (macOS)
- **Voice Transcription** - Transcribe voice messages and video notes to text via Telegram's server-side transcription (requires Telegram Premium)
- **Rate Limiting** - Built-in token bucket rate limiter to prevent API abuse
- **Secure Credentials** - API keys and phone numbers protected with `secrecy` crate
- **File Logging** - Daily log rotation with automatic cleanup of old logs

> **Requires Telegram Premium:** `transcribe_voice_message` uses Telegram's
> server-side `messages.transcribeAudio`, which is only available on accounts
> with Telegram Premium and is subject to Telegram's weekly transcription
> quota. Without Premium the tool returns a clear error. `check_mcp_status`
> reports a `premium` flag so you can tell in advance.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    MCP Client (Claude Desktop)              │
└──────────────────────────┬──────────────────────────────────┘
                           │ JSON-RPC over stdio
┌──────────────────────────▼──────────────────────────────────┐
│                     MCP Server Layer (16 tools)              │
│                    (rmcp + server.rs)                        │
│                                                             │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────┐    │
│  │check_status │ │get_channels │ │search_messages      │    │
│  └─────────────┘ └─────────────┘ └─────────────────────┘    │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────┐    │
│  │channel_info │ │gen_link     │ │open_in_telegram     │    │
│  └─────────────┘ └─────────────┘ └─────────────────────┘    │
│  ┌─────────────────────────┐ ┌───────────────────────────┐   │
│  │get_recent_messages      │ │get_message_by_link        │   │
│  └─────────────────────────┘ └───────────────────────────┘   │
│  ┌─────────────────────────┐ ┌───────────────────────────┐   │
│  │get_last_responses       │ │get_message_media          │   │
│  └─────────────────────────┘ └───────────────────────────┘   │
│  ┌─────────────────────────┐ ┌───────────────────────────┐   │
│  │search_public_channels   │ │transcribe_voice_message   │   │
│  └─────────────────────────┘ └───────────────────────────┘   │
│  ┌─────────────────────────┐ ┌───────────────────────────┐   │
│  │get_messages_batch       │ │resolve_channels           │   │
│  └─────────────────────────┘ └───────────────────────────┘   │
│  ┌─────────────────────────┐ ┌───────────────────────────┐   │
│  │get_channel_stats        │ │get_messages_media_batch   │   │
│  └─────────────────────────┘ └───────────────────────────┘   │
└──────────────────────────┬──────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────┐
│                   Application Layer                         │
│         (config, logging, rate_limiter, link, error)        │
└──────────────────────────┬──────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────┐
│                    Telegram Layer                           │
│              (grammers client, auth, types)                 │
└──────────────────────────┬──────────────────────────────────┘
                           │ MTProto
                           ▼
                   Telegram Cloud API
```

**Key Design Patterns:**
- Library + Binary separation (`lib.rs` for core logic, `main.rs` for CLI)
- Shared state via `Arc<T>` for Telegram client and rate limiter
- Traits with `mockall` for testability
- JSON schemas via `schemars` for MCP tool parameters

## Prerequisites

- **Rust** - 2024 Edition (nightly toolchain)
- **Telegram Account** - With phone number
- **Telegram API Credentials** - From https://my.telegram.org
- **Telegram Desktop** - For `open_message_in_telegram` tool (macOS only)

## Installation

### 1. Clone the Repository

```bash
git clone https://github.com/your-username/telegram-connector.git
cd telegram-connector
```

### 2. Build the Project

```bash
cargo build --release
```

The binary will be at `target/release/telegram-mcp`.

### 3. Get Telegram API Credentials

1. Go to https://my.telegram.org
2. Log in with your phone number
3. Go to "API development tools"
4. Create a new application
5. Note the `api_id` and `api_hash`

### 4. Create Configuration File

The config file location depends on your platform:

| Platform | Config Path |
|----------|-------------|
| **Linux** | `~/.config/telegram-connector/config.toml` |
| **macOS** | `~/Library/Application Support/telegram-connector/config.toml` |
| **Windows** | `%APPDATA%\telegram-connector\config.toml` |

**Linux:**
```bash
mkdir -p ~/.config/telegram-connector
nano ~/.config/telegram-connector/config.toml
```

**macOS:**
```bash
mkdir -p ~/Library/Application\ Support/telegram-connector
nano ~/Library/Application\ Support/telegram-connector/config.toml
```

**Alternative:** Use the `--config` flag to specify a custom path:
```bash
./target/release/telegram-mcp --config ./config.toml --setup
```

**Or** set the `TELEGRAM_MCP_CONFIG` environment variable:
```bash
export TELEGRAM_MCP_CONFIG=/path/to/config.toml
./target/release/telegram-mcp --setup
```

Create the config file with the following content:

```toml
[telegram]
api_id = 12345678
api_hash = "your_api_hash_here"
phone_number = "+1234567890"

# Optional: custom session file location
# session_file = "~/.config/telegram-connector/session.bin"

[search]
# Optional: search defaults
# default_hours_back = 48
# max_results_default = 20
# max_results_limit = 100
# deadline_seconds = 20                                          # Wall-clock budget (seconds) for a search_messages accumulation loop; must be > 0 and <= 3600. On expiry the search returns what it gathered so far with query_metadata.timed_out/partial set, never an error

[rate_limiting]
# Optional: token bucket configuration
# max_tokens = 60
# refill_rate = 2.0
# media_download_cost = 3                                       # Rate-limit tokens charged per image returned by get_message_media / get_messages_media_batch (default: 3)

[limits]
# Optional: response size limits
# response_byte_budget = 40000                                  # Byte cap on a serialized message-stream response (search_messages/get_recent_messages); over-budget pages drop trailing messages and set has_more/next_cursor
# media_batch_max_total_bytes = 8388608                          # Cap on a get_messages_media_batch call's total image payload, in bytes of base64 as sent to the client (default: 8388608 / 8 MiB); images are downscaled progressively to fit

[logging]
# Optional: logging configuration
# level = "info"                                        # trace, debug, info, warn, error
# format = "compact"                                    # compact, pretty, json

# File logging (always JSON format, daily rotation)
# file_enabled = true                                   # Default: true
# file_path = "~/.config/telegram-connector/logs/"      # Default log directory
# max_log_days = 7                                      # Days to retain (old logs cleaned on startup)

[server]
# Optional: server configuration
# shutdown_timeout_seconds = 5                          # Graceful shutdown timeout
```

You can also use environment variables for sensitive values:

```toml
[telegram]
api_id = 12345678
api_hash = "${TELEGRAM_API_HASH}"
phone_number = "${TELEGRAM_PHONE}"
```

### 5. Authenticate with Telegram

Run the setup command to authenticate:

```bash
./target/release/telegram-mcp --setup
```

This will:
1. Connect to Telegram
2. Send a verification code to your Telegram app
3. Prompt you to enter the code
4. If 2FA is enabled, prompt for your password
5. Save the session for future use

## Usage

### Running the MCP Server

After authentication, start the server:

```bash
./target/release/telegram-mcp
```

The server communicates via stdio (stdin/stdout) using JSON-RPC.

### CLI Options

```
telegram-mcp [OPTIONS]

Options:
  -s, --setup                Run interactive setup to authenticate
      --session-file <FILE>  Path to session file (overrides config)
  -c, --config <FILE>        Path to configuration file
  -h, --help                 Print help
  -V, --version              Print version
```

### Connecting with Claude Desktop

Add to your Claude Desktop configuration (`~/.config/claude-desktop/config.json`):

```json
{
  "mcpServers": {
    "telegram": {
      "command": "/path/to/telegram-mcp",
      "args": []
    }
  }
}
```

Restart Claude Desktop to load the MCP server.

## MCP Tools Reference

### 1. check_mcp_status

Check the connection status and rate limiter state.

**Parameters:** None

**Response:**
```json
{
  "telegram_connected": true,
  "rate_limiter": {
    "tokens": 54.0,
    "capacity": 60.0,
    "refill_per_sec": 2.0,
    "costs": {
      "search": 1,
      "media_download": 3,
      "transcription": 5
    }
  },
  "media": {
    "batch_max_ids": 10,
    "max_total_bytes": 8388608,
    "per_image_max_bytes": 1572864,
    "default_max_dimension": 1280,
    "max_dimension_limit": 2048
  },
  "server_version": "0.1.0"
}
```

> **Note:** `rate_limiter` reports the live token-bucket budget: `tokens`
> (currently available), `capacity` (bucket size — `[rate_limiting]
> max_tokens`, default 60), `refill_per_sec` (`[rate_limiting] refill_rate`,
> default 2.0), and `costs` (tokens charged per call kind — `media_download`
> and `transcription` are configurable under `[rate_limiting]`; `search` is
> the `1`-token cost for the other metered calls — search/history/
> channel-info/link-generation calls, i.e. `search_messages`,
> `get_recent_messages`, `get_message_by_link`, `search_public_channels`,
> `generate_message_link`, `open_message_in_telegram`, `get_messages_batch`,
> `resolve_channels`, `get_channel_stats`, and `get_channel_info` when called
> with `include_full: true`). The remaining tools — `check_mcp_status`,
> `get_subscribed_channels`, `get_last_responses`, and `get_channel_info`
> without `include_full` — never call `acquire` and cost nothing. **Exception:**
> `search_messages`/`get_recent_messages` called with `channel_ids` (multi-channel
> fan-out) acquire `N` tokens — one per deduped channel in the list — instead of
> the flat `1`, so the deficit error stays accurate for a large fan-out.
> `get_messages_media_batch` acquires `media_download_cost × requested ids` up
> front and refunds the ids that produced no image (see its own section below).
> When a call is rejected for insufficient tokens, the error states the deficit,
> e.g. `rate limit exceeded: requested 5 tokens, 2.40 available, retry after 2
> seconds` (Telegram flood-wait rejections keep their existing wording, with no
> token arithmetic to show). `media` is a purely additive block of configured
> media limits — `batch_max_ids`/`max_total_bytes`/`per_image_max_bytes`/
> `default_max_dimension`/`max_dimension_limit` — so a caller can plan a
> `get_messages_media_batch` run instead of discovering the limits by hitting
> them.

**Usage:** Use this tool to verify the connection before performing other operations.

---

### 2. get_subscribed_channels

List all Telegram channels you're subscribed to.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `limit` | integer | No | 50 | Maximum channels to return (max: 500) |
| `offset` | integer | No | 0 | Pagination offset |

**Response:**
```json
{
  "channels": [
    {
      "id": 1234567890,
      "name": "Tech News",
      "username": "technews",
      "chat_type": "channel",
      "description": null,
      "member_count": null,
      "is_verified": true,
      "is_public": true,
      "is_subscribed": true,
      "last_message_date": "2025-12-28T10:30:00Z"
    }
  ],
  "returned": 1,
  "total": 25,
  "has_more": true
}
```

> **Note:** `description` and `member_count` are `null` from this endpoint — the
> channel list is built from basic dialog info and does not fetch them. `null`
> means "not fetched", not "no description" / "empty channel". `username` is
> `null` when the chat has no public username (never a fabricated placeholder);
> `chat_type` is one of `channel` (broadcast), `supergroup`, or `group` (basic
> group). `returned` is the page size (`channels.len()`); `total` is the genuine
> subscription count across your whole dialog list, not the page size — the
> server walks the full list on every call to compute it (work-order B6a).
> `has_more` is derived straight from that true total (`offset + returned <
> total`) and is always a known `true`/`false`, never `null`, for this tool.
> `last_message_date` is populated from the dialog's top message (work-order B8)
> — when a channel has no messages, it is `null`.

**Usage:** Get a list of available channels before searching or filtering.

---

### 3. get_channel_info

Get detailed information about a specific channel.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `channel_identifier` | string | Yes | — | Channel username (e.g., `technews`) or numeric ID |
| `include_full` | boolean | No | `false` | Fetch full channel info (`description`, `member_count`, `last_message_date`) with extra Telegram RPCs. `description` and `member_count` come from `channels.getFullChannel` (channel-kind peers only: broadcasts and megagroups); `last_message_date` comes from a one-message history peek (any peer kind). Other peer kinds silently fall back to basic info for the RPC-dependent fields. |

**Response (default, `include_full: false`):**
```json
{
  "id": 1144180066,
  "name": "Сводки",
  "username": "swodki",
  "chat_type": "channel",
  "description": null,
  "member_count": null,
  "is_verified": true,
  "is_public": true,
  "is_subscribed": true,
  "last_message_date": "2025-12-28T10:30:00Z"
}
```

> **Note:** `description` and `member_count` are `null` by default —
> `get_channel_info` resolves the channel from basic peer info and does not perform
> a full-channel fetch, so it reports `null` ("not fetched") rather than a
> misleading `0` / empty description. Pass `include_full: true` (below) to fetch them.

**Response (`include_full: true`):**
```json
{
  "id": 1144180066,
  "name": "Сводки",
  "username": "swodki",
  "chat_type": "channel",
  "description": "Daily technology news and analysis.",
  "member_count": 48213,
  "is_verified": true,
  "is_public": true,
  "is_subscribed": true,
  "last_message_date": "2026-08-10T05:55:12Z"
}
```

> **Note:** `include_full` populates `description` and `member_count` via
> `channels.GetFullChannel` (channel-kind peers — broadcasts and megagroups — only;
> other peer kinds report these as `null`), and `last_message_date` via a one-message
> history peek (any peer kind with readable history). If either fetch fails, the
> affected field(s) stay `null` and the call still succeeds. It costs exactly one
> rate-limiter token (on top of the basic lookup), regardless of how many of the
> up to two extra Telegram RPCs it issues.

**Response (private group, no public username):**
```json
{
  "id": 521440428,
  "name": "Семейный чатик",
  "username": null,
  "chat_type": "group",
  "description": null,
  "member_count": null,
  "is_verified": false,
  "is_public": false,
  "is_subscribed": true,
  "last_message_date": null
}
```

> **Note:** `username` is `null` for chats with no public username — private
> groups and basic (non-mega) groups typically have none. No placeholder value
> is fabricated. `chat_type` distinguishes `channel` (broadcast), `supergroup`,
> and `group` (basic group, including Telegram's `Community` peer kind).

**Usage:** Verify channel details or get the numeric ID for other operations. Use `include_full: true` when you specifically need the description or member count — it costs 1 rate-limiter token, and up to two extra Telegram RPC round-trips.

---

### 4. generate_message_link

Generate shareable links for a specific message.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `channel_id` | string | Yes | - | Channel ID or username (e.g. `@channelname` or `1234567890`) |
| `message_id` | integer | Yes | - | Message ID within the channel |
| `include_tg_protocol` | boolean | No | true | Include `tg://` protocol link |

**Response (public channel):**
```json
{
  "channel_id": "1144180066",
  "message_id": 610121,
  "https_link": "https://t.me/swodki/610121",
  "tg_protocol_link": "tg://resolve?domain=swodki&post=610121",
  "internal_link": "https://t.me/c/1144180066/610121",
  "is_public": true
}
```

**Response (private chat, no public username):**
```json
{
  "channel_id": "1234567890",
  "message_id": 42,
  "https_link": "https://t.me/c/1234567890/42",
  "tg_protocol_link": "tg://privatepost?channel=1234567890&post=42",
  "internal_link": "https://t.me/c/1234567890/42",
  "is_public": false
}
```

> **Note:** For public channels, `https_link`/`tg_protocol_link` use the shareable
> `t.me/<username>` / `tg://resolve` forms; `internal_link` always carries the
> members-only `t.me/c/…` form regardless of `is_public`. `channel_id` accepts
> either a username or a numeric ID — the response always echoes back the
> canonical numeric ID. No link ever carries a `?single`/`&single` suffix.
> The tool performs one channel resolution to build the link (costs one
> rate-limiter token), and the channel must be resolvable by the connected
> account (a subscribed dialog or a public username) or the call errors.

**Link Formats:**
- **HTTPS:** Opens in browser or Telegram Web
- **tg://** : Opens directly in Telegram Desktop (macOS)

**Usage:** Generate links to share or open specific messages.

---

### 5. open_message_in_telegram

Open a message directly in Telegram Desktop.

**Platform Support:** macOS only. Returns an error on other platforms.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `channel_id` | string | Yes | - | Channel ID or username (e.g. `@channelname` or `1234567890`) |
| `message_id` | integer | Yes | - | Message ID |
| `use_tg_protocol` | boolean | No | true | Use `tg://` (true) or `https://` (false) |

**Response:**
```json
{
  "success": true,
  "message": "Opened message in Telegram",
  "link_used": "tg://resolve?domain=swodki&post=42",
  "app_opened": true
}
```

> **Note:** `link_used` reflects the same public-vs-private form rules as
> `generate_message_link` — a public channel opens via `tg://resolve`
> (or `https://t.me/<username>/...`), a private chat via `tg://privatepost`
> (or the members-only `https://t.me/c/...` form). Never a `?single`/`&single`
> suffix. Like `generate_message_link`, this tool performs one channel
> resolution (costs one rate-limiter token), and the channel must be
> resolvable by the connected account (subscribed dialog or public username)
> or the call errors.

**Usage:** Quickly navigate to a message in the native Telegram app.

---

### 6. search_messages

Search for messages across channels with optional media type filtering.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `query` | string | Conditional | - | Search query. Required unless `media_filter` is specified |
| `channel_id` | string | No | - | Filter by a specific channel — numeric ID or `@username`/`username` (a username spends one extra resolve call). Mutually exclusive with `channel_ids` |
| `channel_ids` | array of strings | No | - | Fan out over up to 20 channels (IDs or usernames) in one call with bounded concurrency (4 in flight); results are merged newest-first and `limit` counts the merged total. Mutually exclusive with `channel_id`; incompatible with `before_id`/`after_id`. Omit both `channel_id` and `channel_ids` for a global search |
| `hours_back` | integer | No | 48 | How many hours back to search (max: 72) |
| `limit` | integer | No | 20 | Maximum results to return (max: 100) |
| `media_filter` | string | No | - | Filter by media type (see below) |
| `from_date` | string | No | - | Inclusive start of the time window as RFC 3339 UTC (e.g. `2026-08-01T00:00:00Z`). Overrides `hours_back`. Reaching far back works best on low-traffic channels; on active channels prefer a narrower recent window, since deep windows are paged client-side and may time out |
| `to_date` | string | No | - | Inclusive end of the time window as RFC 3339 UTC. Messages newer than this are excluded. Set without `from_date`, it must fall inside the `hours_back` window — otherwise the window is empty and the call is rejected |
| `collapse_albums` | boolean | No | `true` | Collapse album (grouped media) siblings into one post-level result. When `true`, `limit` counts posts (not raw messages) and each collapsed post carries an `album` object. When `false`, every sibling is returned as its own message and `limit` counts raw messages, matching pre-0.15 behavior |
| `before_id` | integer | No | - | Exclusive upper message-id bound — return only messages with `id < before_id`. Use `next_cursor.before_id` from a previous response to fetch the next (older) page without offset drift. Requires `channel_id` |
| `after_id` | integer | No | - | Exclusive lower message-id bound — stop before messages with `id <= after_id`. Bounds a page at ids newer than a known message. Requires `channel_id` |
| `max_text_length` | integer | No | 2000 | Maximum text length in characters per message. Longer texts are cut and flagged with `text_truncated` plus `text_full_length`; refetch the single message to get full text |
| `format` | string | No | `full` | Response shape — `full` repeats channel fields on every message; `compact` hoists them into one response-level `channel` header (single-channel scope) or a `channels` map keyed by channel id (multi-channel `channel_ids` scope). `compact` requires `channel_id` or `channel_ids` — rejected on a global search |

**Media Filter Options:**
| Value | Description |
|-------|-------------|
| `photo` | Messages with photos attached |
| `video` | Messages with videos attached |
| `photo_video` | Messages with photos OR videos |
| `document` | Messages with documents/files attached |
| `audio` | Messages with music/audio files |
| `voice` | Messages with voice messages |
| `video_note` | Messages with video notes (round videos) |
| `gif` | Messages with GIF animations |
| `url` | Messages containing URLs |
| `pinned` | Pinned messages only |

**Important:** The `media_filter` is metadata-based filtering, NOT content recognition. It filters by attachment type, not by what's inside the media.

> **Note:** `channel_id` accepts `@username`, `username`, or a numeric ID —
> the same flexibility `get_recent_messages`/`get_channel_info` have always
> had. A username is resolved to a numeric ID via one extra channel-resolve
> call before the search runs; a plain numeric string is parsed locally with
> no extra round trip.

**Response:**

*This example is a field reference — it shows every possible field on one
message for illustration. A real message never carries `video_info`,
`audio_info`, `document_info`, and `poll_info` together; each is exclusive to
its own `media_type` (see "Video & audio metadata" / "Document metadata" /
"Poll metadata" below).*

```json
{
  "messages": [
    {
      "id": 610047,
      "channel_id": 1234567890,
      "channel_name": "Tech News",
      "channel_username": "technews",
      "text": "Breaking: New AI model released...",
      "timestamp": "2025-12-28T10:30:00Z",
      "sender_id": 987654321,
      "sender_name": "John Doe",
      "has_media": true,
      "media_type": "photo",
      "link": "https://t.me/technews/610047",
      "views": 15000,
      "forwards": 230,
      "forwarded_from": {
        "channel_id": 1009988776,
        "channel_name": "Военкор",
        "channel_username": "voenkor_ru",
        "post_author": "И. Петров",
        "original_message_id": 8123,
        "original_date": "2025-12-28T09:00:00Z"
      },
      "link_preview": {
        "url": "https://example.com/ai-model",
        "site_name": "Example",
        "title": "New AI model released",
        "description": "A short summary pulled from Telegram's server-side preview..."
      },
      "reply_to_message_id": 610046,
      "video_info": {
        "duration_seconds": 95,
        "width": 1280,
        "height": 720,
        "file_size_bytes": 12485760,
        "kind": "video",
        "has_thumbnail": true,
        "mime_type": "video/mp4"
      },
      "audio_info": {
        "duration_seconds": 215,
        "file_size_bytes": 5242880,
        "kind": "audio",
        "mime_type": "audio/mpeg",
        "title": "Around the World",
        "performer": "Daft Punk"
      },
      "document_info": {
        "file_name": "Как мы строим RAG.pdf",
        "file_size_bytes": 2411008,
        "mime_type": "application/pdf"
      },
      "poll_info": {
        "question": "Какой стек выбрать?",
        "options": [
          {"text": "Rust", "voters": 287},
          {"text": "Go", "voters": 125}
        ],
        "total_voters": 412,
        "closed": true,
        "multiple_choice": false,
        "quiz": false
      },
      "grouped_id": 13579246801357,
      "reactions": [
        { "emoji": "🔥", "count": 41 },
        { "emoji": "👍", "count": 12 }
      ],
      "reactions_total": 55,
      "album": {
        "media_count": 8,
        "media_types": ["photo", "photo", "photo", "photo", "photo", "photo", "photo", "photo"],
        "message_ids": [610047, 610048, 610049, 610050, 610051, 610052, 610053, 610054]
      }
    }
  ],
  "returned": 15,
  "has_more": false,
  "search_time_ms": 250,
  "query_metadata": {
    "query": "AI model",
    "window_from": "2025-12-26T10:30:00Z",
    "channels_scanned": 10,
    "channels_in_results": 6,
    "pages_fetched": 3,
    "messages_scanned": 245
  }
}
```

**Response fields:** `returned` is the number of messages in this response (the
page size — not a total-match count; there may be more matches beyond it).
`query_metadata.window_from` is the effective window start actually applied
(`from_date`, or `now - hours_back`); `window_to` is the effective upper bound
and is omitted entirely when the window is open-ended. `channels_scanned` is
the number of channels the search actually scanned — `null` for a global
search (server-side, scan scope is unknowable); the attempted (not just
successful) channel count when `channel_ids` fans out over multiple channels;
and, when `channel_id` is set, a concrete count that differs by tool —
`get_recent_messages` always reports `1`, while `search_messages` walks your
subscribed dialogs looking for a channel matching the numeric id and reports
`0` (not an error — just an empty result) if it isn't among them, which is
possible for a syntactically valid `channel_id` you haven't joined (e.g. one
sourced from `forwarded_from.channel_id` or a `resolve_channels` result).
`channels_in_results` is the number of distinct channels present in
`messages`, always a number.

`query_metadata.pages_fetched` and `query_metadata.messages_scanned` report the
result pages fetched from Telegram and the raw messages walked (including ones
later filtered out or outside the window) to produce this result — both are
always present, on every `search_messages` and `get_recent_messages` response,
so an expensive call is legible to its caller. They count result pages, not
every round trip the call made: when `channel_id` is set, `search_messages`
walks your dialogs to find the channel first, and that walk — often most of the
call's cost — is not in `pages_fetched`. `query_metadata.timed_out` and
`query_metadata.partial` are set together, on `search_messages` only, when the
search hit `[search] deadline_seconds` (default 20) and stopped early with
whatever it had gathered so far — never an error, because partial results beat
a failed workflow. Both flags are omitted from the JSON entirely when `false`,
so a healthy response's shape on the wire is unchanged — the example above
never carries them. A deadline-truncated result does not set `has_more`:
expiry proves nothing about what lies beyond the page, unlike an overflowed
`limit`, which does (see "Paging and size" below). `get_recent_messages`
reports `pages_fetched`/`messages_scanned` too (it pages through history the
same way) but never `timed_out`/`partial` — the deadline is scoped to
`search_messages`.

**Multi-channel fan-out (`channel_ids`):** pass up to 20 channel references
(IDs or usernames) to search or fetch them in one call instead of N round
trips. Each channel is fetched concurrently (4 in flight at a time) with the
full requested `limit`, then results are merged newest-first (timestamp desc,
id desc tiebreak) and truncated to `limit` overall — so `has_more` is `true`
whenever the merge truncated, or any individual channel reported more of its
own. `before_id`/`after_id` are rejected with `channel_ids` (cursors are
per-channel and would be ambiguous across a merged, multi-channel page); no
`next_cursor` is ever emitted in this scope. The rate-limiter cost is
`acquire(N)` for the deduped channel count — one atomic acquire, so a
too-large `channel_ids` list surfaces the same "requested N tokens, X.XX
available" deficit message as any other call. A channel that fails to
resolve or fetch does not fail the whole call: it lands in
`channel_errors: [{"channel": "<as passed>", "error": "..."}]` and the other
channels' results still come back; the call only errors when **every**
requested channel failed. `channel_id` and `channel_ids` are mutually
exclusive (both set is a validation error); omitting both keeps
`search_messages`'s existing global-search behavior — `get_recent_messages`
has no global mode, so it requires one or the other. The merged response's
`query_metadata.pages_fetched`/`messages_scanned` are the sum across every
fetched channel, and `timed_out`/`partial` are `true` if any one channel's
search hit its deadline — so a single slow channel degrades the whole
fan-out's flags without hiding which channels actually returned data.

**Paging and size (`has_more`, `next_cursor`, byte budget):** every
`search_messages` / `get_recent_messages` response carries `has_more`. It is
`true` only when more qualifying messages exist in the requested window —
because `limit` was reached with messages left over, or because the serialized
response hit the `[limits] response_byte_budget` cap (default 40 000 bytes) and
trailing messages were dropped. In single-channel scope the response then also
carries `next_cursor: {"before_id": <oldest included id>}`; pass it as
`before_id` on the next call to page strictly older messages with no overlap
and no drift from new posts. `after_id` can likewise cut an album at the page
boundary — its siblings with `id <= after_id` are excluded from the fetch —
so `album.message_ids` lets a caller detect a partial album and re-fetch the
missing siblings. Global search reports `has_more` without a cursor
(message ids are per-channel). At least one message is always returned even if
it alone exceeds the byte budget. Long texts are independently cut at
`max_text_length` characters (default 2000) and flagged `text_truncated: true`
with `text_full_length`.

**Compact format (`format: "compact"`):** in single-channel scope
(`get_recent_messages`, or `search_messages` with `channel_id`), hoists
`channel_id` / `channel_name` / `channel_username` into one response-level
`channel` object and removes them from each message — at `limit: 100` this
saves kilobytes of repetition; on an empty result the `channel` key is
omitted entirely (same as full format), not serialized as `null`. In
multi-channel scope (`channel_ids`), the single `channel` header doesn't
apply — instead `channel_name`/`channel_username` are stripped into a
response-level `channels` object, a map from decimal channel id to
`{"id", "name", "username"}`, one entry per channel actually present in the
merged results; each message **keeps** its own `channel_id` so it stays
attributable to the right map entry. `compact` is rejected outside both
scopes (i.e. on a global search with neither `channel_id` nor `channel_ids`
set).

**Wire note:** `channel_username` is now omitted (rather than serialized as
`null`) when a message's channel has no public username; full format is
otherwise unchanged (`channel_id`/`channel_name` remain always-present).

**Forward attribution & link previews:** Messages carry optional enrichment derived
from the same Telegram response — no extra API calls. `forwarded_from` attributes a
forwarded post to its source: `channel_id`, `original_message_id`, `original_date`,
plus the source's **`channel_name` and `channel_username`** resolved from the same
response envelope (works even for channels you are not subscribed to), `sender_name`
(the user's display name for user-source forwards, or the hidden-sender name), and
`post_author` for signed channel posts. When Telegram's envelope happens not to
carry the source entity, the ids-only form is emitted — nothing is fabricated and
no resolution call is made. This enrichment is identical across every
message-returning tool — `search_messages`, `get_recent_messages`,
`get_message_by_link`, and `get_messages_batch` all convert messages through
the same step, each one handed a genuine Telegram response envelope from its
own raw TL call (`messages.GetHistory`/`Search`/`SearchGlobal` for search and
history, a raw `getMessages` call for by-link and batch) rather than a
grammers high-level helper that would discard it — so re-fetching a forward
by id (e.g. for its full untruncated text) carries the same
`channel_name`/`channel_username` the original search or history call
already showed.
`link_preview` surfaces Telegram's server-side webpage preview (`url`, `site_name`,
`title`, `description`, truncated to 500 characters). `views`, `forwards`, and
`reply_to_message_id` are included when present. All of these fields are omitted
entirely when absent, so existing consumers are unaffected.

**Permalink, reactions, and album id:** every message carries a `link` — the same
public `t.me/<username>/<id>` or members-only `t.me/c/<channel_id>/<id>` permalink
`generate_message_link` returns — computed with **no extra API call**. `reactions`
itemizes standard-emoji reactions (`emoji`, `count`) sorted as Telegram returns them;
custom-emoji and paid reactions are not individually renderable and are omitted from
the list, but `reactions_total` always counts every reaction of every kind. Both are
omitted when the message has no reactions. `grouped_id` is Telegram's album (media
group) id — present and identical across sibling messages that were posted together
as an album, `null`/omitted otherwise.

**Album collapsing (`collapse_albums`, default `true`):** by default, sibling messages
that share a `grouped_id` (an album/media group posted together) are collapsed into a
single post-level result before `limit` is applied — so `limit` counts **posts**, not
raw messages, and an album is never cut in half at the boundary (its trailing siblings
are always admitted once the album has started). The collapsed post is represented by
its lowest-id sibling; `text` is taken from whichever sibling actually carries a
caption (Telegram puts the caption on an arbitrary member of the group, not always the
first). The post carries an `album` object: `media_count` (sibling count),
`media_types` (one entry per sibling, ascending id order), and `message_ids` (every
sibling's id, ascending) — but these three fields describe only the siblings **present
in this result set**, not necessarily the whole album Telegram holds: a `media_filter`,
a `from_date`/`to_date` bound, or global-search adjacency can drop siblings that fall
outside the fetched window, so an album straddling that boundary is partially
represented. If only one sibling of an album survives into the result set, it is
indistinguishable from a genuine non-album post: it is returned as a plain message with
no `album` object at all, same as an "album" that was genuinely just one message. Pass
`"collapse_albums": false` to get the pre-0.15 behavior: every sibling returned as its
own message (all sharing `grouped_id`, none carrying `album`), and `limit` counting
raw messages again. Because `limit` counts posts, on very album-heavy channels
reaching a high `limit` may require walking up to ~10x as many raw messages within the
same timeout budget, so the call may hit the timeout sooner than expected — lower
`limit` or pass `"collapse_albums": false` if that happens.

**Video & audio metadata:** Messages with video-class media carry an optional
`video_info` object — `duration_seconds`, `width`, `height`, `file_size_bytes`,
`kind` (`video` | `video_note` | `animation`), `has_thumbnail`, and `mime_type` —
and audio-class media carry an optional `audio_info` object (`duration_seconds`,
`file_size_bytes`, `kind` (`audio` | `voice`), `mime_type`, plus `title` and
`performer` read from `DocumentAttributeAudio`'s ID3-style metadata — populated
for music tracks, omitted for the common case of voice messages). Both are
derived from the message's document attributes with **no extra API calls** (the
full video is never downloaded), so the client can judge a clip's length and
shape — and whether fetching its thumbnail via `get_message_media`, or
transcribing a voice message, is worthwhile — before spending a request. Rare
GIF-class animations without a video attribute report
`duration_seconds`/`width`/`height` as `0`. Both objects are omitted when the
message has no video/audio media.

**Document metadata:** Messages whose `media_type` is `document` carry an
optional `document_info` object (`file_name`, `file_size_bytes`, `mime_type`),
derived from the document's attributes with **no extra API call**. `file_name`
is itself omitted when Telegram carries no `DocumentAttributeFilename` for the
file. Video, audio, voice, and animation media keep their own `video_info` /
`audio_info` objects instead — `document_info` is never emitted alongside
them, so nothing is duplicated across two keys.

**Poll metadata:** Poll messages (`media_type: "poll"`) carry an optional
`poll_info` object — `question`, `options` (each `{"text", "voters"}`),
`total_voters`, `closed`, `multiple_choice`, and `quiz` — read directly from the
message's poll media with **no extra API call**. `total_voters` and each
option's `voters` are independently optional: Telegram can disclose which
option is winning while withholding one option's individual count, or withhold
results entirely. An undisclosed count is omitted (`voters` absent on that
option, or `total_voters` absent for the whole poll) — never fabricated as `0`.

**Media Types:** `none`, `photo`, `video`, `document`, `audio`, `voice`, `video_note`, `animation`, `sticker`, `contact`, `location`, `venue`, `poll`, `dice`

**Usage Examples:**

| Query | media_filter | Result |
|-------|--------------|--------|
| `"AI news"` | (none) | Messages containing "AI news" |
| `"AI news"` | `photo` | Messages with "AI news" AND a photo attached |
| `""` (empty) | `document` | All documents (no text filtering) |
| `""` (empty) | (none) | ❌ Error (too broad) |

**Date-range example:** search a fixed window instead of a rolling `hours_back` lookback,
reaching arbitrarily far into the past:
```json
{"query": "AI news", "from_date": "2026-07-01T00:00:00Z", "to_date": "2026-07-31T23:59:59Z"}
```

**Usage:** Search for specific topics across your subscribed channels, optionally filtering by media type.

---

### 7. get_recent_messages

Get recent messages from a channel by time window, without requiring a search query. Uses message history iteration instead of search.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `channel_id` | string | Conditional | - | Channel ID or username (e.g., `technews` or `1234567890`). Required unless `channel_ids` is set; mutually exclusive with it |
| `channel_ids` | array of strings | No | - | Fan out over up to 20 channels (IDs or usernames) in one call with bounded concurrency (4 in flight); results are merged newest-first and `limit` counts the merged total. Mutually exclusive with `channel_id`; incompatible with `before_id`/`after_id`. See `search_messages` above for the full fan-out behavior — `get_recent_messages` has no global mode, so one of `channel_id`/`channel_ids` is always required |
| `hours_back` | integer | No | 48 | Hours of history to retrieve (max: 168) |
| `limit` | integer | No | 20 | Maximum messages to return (max: 100) |
| `media_filter` | string | No | - | Filter by media type (same options as `search_messages`) |
| `from_date` | string | No | - | Inclusive start of the time window as RFC 3339 UTC (e.g. `2026-08-01T00:00:00Z`). Overrides `hours_back`. Reaching far back works best on low-traffic channels; on active channels prefer a narrower recent window, since deep windows are paged client-side and may time out |
| `to_date` | string | No | - | Inclusive end of the time window as RFC 3339 UTC. Messages newer than this are excluded. Set without `from_date`, it must fall inside the `hours_back` window — otherwise the window is empty and the call is rejected |
| `collapse_albums` | boolean | No | `true` | Collapse album (grouped media) siblings into one post-level result. When `true`, `limit` counts posts (not raw messages) and each collapsed post carries an `album` object. When `false`, every sibling is returned as its own message and `limit` counts raw messages, matching pre-0.15 behavior. See `search_messages` above for the full behavior description |
| `before_id` | integer | No | - | Exclusive upper message-id bound — return only messages with `id < before_id`. Use `next_cursor.before_id` from a previous response to fetch the next (older) page without offset drift |
| `after_id` | integer | No | - | Exclusive lower message-id bound — stop before messages with `id <= after_id`. Bounds a page at ids newer than a known message |
| `max_text_length` | integer | No | 2000 | Maximum text length in characters per message. Longer texts are cut and flagged with `text_truncated` plus `text_full_length`; refetch the single message to get full text |
| `format` | string | No | `full` | Response shape — `full` repeats channel fields on every message; `compact` hoists them into one response-level `channel` header (single `channel_id`) or a `channels` map keyed by channel id (`channel_ids` fan-out). See `search_messages` above |

**Key Difference from `search_messages`:**

| Feature | search_messages | get_recent_messages |
|---------|-----------------|---------------------|
| Query required | Yes (or media_filter) | No |
| Channel required | No (global search) | Yes (`channel_id` or `channel_ids`) |
| Underlying API | `search_messages()` / `search_all_messages()` | `iter_messages()` |
| Use case | Find specific content | Get all recent activity |

**Response:** Same format as `search_messages` (same `SearchResponse` JSON shape).

```json
{
  "messages": [
    {
      "id": 99,
      "channel_id": 1234567890,
      "channel_name": "Tech News",
      "channel_username": "technews",
      "text": "Latest update from the team...",
      "timestamp": "2025-12-28T10:30:00Z",
      "sender_id": 0,
      "sender_name": "Tech News",
      "has_media": false,
      "media_type": "none",
      "link": "https://t.me/technews/99"
    }
  ],
  "returned": 5,
  "has_more": false,
  "search_time_ms": 150,
  "query_metadata": {
    "query": "",
    "window_from": "2025-12-27T10:30:00Z",
    "channels_scanned": 1,
    "channels_in_results": 1,
    "pages_fetched": 1,
    "messages_scanned": 5
  }
}
```

**Date-range example:** fetch a fixed window instead of a rolling `hours_back` lookback:
```json
{"channel_id": "durov", "from_date": "2026-07-01T00:00:00Z", "to_date": "2026-07-31T23:59:59Z"}
```

**Usage:** Get all recent activity from a channel without needing a search query. Ideal for monitoring or catching up on channel content.

---

### 9. get_last_responses

Debug/recovery tool: replay the last N tool responses that were written to stdout, so a response lost in transit (client crash, truncated read, etc.) can be recovered without re-querying Telegram or spending rate-limit budget.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `n` | integer | No | all buffered | How many recent responses to return, newest first |
| `include_binary` | boolean | No | `false` | Include full base64 image payloads in replayed responses. When `false` (default), image content blocks are replaced with `{omitted, mime_type, size_bytes}` stubs |

**Response (default, `include_binary: false`):**
```json
{
  "buffered": 12,
  "responses": [
    {
      "request_id": "7",
      "tool_name": "get_message_media",
      "written_at": "2026-08-12T09:15:03Z",
      "size_bytes": 88211,
      "response": {
        "jsonrpc": "2.0",
        "id": 7,
        "result": {
          "content": [
            {
              "type": "image",
              "omitted": true,
              "mime_type": "image/jpeg",
              "size_bytes": 65890
            },
            {
              "type": "text",
              "text": "{\"channel_id\":\"@swodki\",\"message_id\":610119,...}"
            }
          ]
        }
      }
    }
  ]
}
```

> **Note:** `buffered` is the total number of responses currently held in the
> ring buffer (sized by `[observability] response_buffer_size`, default 10;
> `0` disables buffering entirely). `responses` is the requested page,
> newest first. Each entry's `response` is the exact JSON-RPC envelope that
> was written to stdout — with `include_binary: false` (the default), any
> `image` content block inside it is replaced by a `{omitted: true,
> mime_type, size_bytes}` stub instead of its base64 payload, since this
> tool exists for when context is already tight or damaged; pass
> `include_binary: true` to get the full base64 data back (work-order D6).

**Usage:** Ask Claude to "show me the last response" or "recover response 7" after a dropped or truncated reply, without paying for another Telegram round-trip.

---

### 10. get_message_media

Retrieve the visual media from a Telegram message as an MCP **image content block** (base64-encoded JPEG, quality 80) plus a JSON metadata text block. Useful for reading photos posted in channels without leaving the conversation.

**What it returns:**
- **Photos:** the photo is downscaled so its longest side fits `max_dimension`, re-encoded as JPEG, and returned as an MCP image block. The metadata block contains `media_type`, `is_thumbnail` (always `false` for photos), `caption`, source variant dimensions (what was actually fetched) and byte size, largest available variant dimensions (if better exists), and the returned dimensions and byte size.
- **Already-fitting JPEGs pass through byte-identical:** if the fetched source variant is already a JPEG whose longest side is `<= max_dimension` (and its base64 size fits the payload cap), it is returned as-is — the source is still decoded (its dimensions are read from that decode), but the re-encode is skipped, so no quality loss and no size inflation from a needless re-encode. `returned_width`/`returned_height`/`returned_size_bytes` then equal the source variant's own dimensions and byte size.
- **Videos, animations, video notes:** only the server-side thumbnail is available; it is returned as an image block with `is_thumbnail: true` and a `video_info` object (duration, dimensions, kind) in the metadata.
- **Messages without visual media:** a structured error is returned (no image block).
- **Photos whose selected size variant exceeds 20 MB:** refused with an error.
- **Payload cap:** the base64 payload is capped at ~1.5 MB; if the image is still too large after the initial downscale it is downscaled further automatically.

**Size variant selection:** the smallest server-side size variant whose longest side is >= `max_dimension` is chosen before downloading, minimising transfer bytes while guaranteeing the requested resolution.

**Rate limiting:** each call charges `media_download_cost` tokens from the rate limiter (default **3**, versus 1 for searches). Configure under `[rate_limiting] media_download_cost`.

**Timeout:** bounded by `[telegram.timeouts] download_secs` (default 120 s).

**Fetching more than one image from this channel?** Use `get_messages_media_batch`
below instead of looping this tool — it resolves the channel once and issues one
fetch round trip for the whole batch, instead of one of each per image.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `channel_id` | string | Yes | - | Channel ID or username |
| `message_id` | integer | Yes | - | Message ID within the channel |
| `max_dimension` | integer | No | 1280 | Longest image side in pixels after downscaling (clamped to 64–2048) |

**Metadata response (text block):**
```json
{
  "channel_id": "@technews",
  "message_id": 42,
  "media_type": "video",
  "is_thumbnail": true,
  "caption": "Optional caption text",
  "source_variant_width": 2560,
  "source_variant_height": 1440,
  "source_variant_size_bytes": 400000,
  "largest_available_width": 3840,
  "largest_available_height": 2160,
  "returned_width": 1280,
  "returned_height": 720,
  "returned_size_bytes": 98304,
  "mime_type": "image/jpeg",
  "video_info": {
    "duration_seconds": 95,
    "width": 1280,
    "height": 720,
    "file_size_bytes": 12485760,
    "kind": "video",
    "has_thumbnail": true,
    "mime_type": "video/mp4"
  }
}
```

**Usage:** Ask Claude to "show me the photo from message 42 in channel technews" or "what does the chart in message 100 of channel @data say?"

---

### 16. get_messages_media_batch

Retrieve the photos (or video/animation/video-note thumbnails) of up to 10 messages from **one** channel in a single call. **This is the preferred path whenever you want more than one image:** the batch resolves the channel once and issues one `get_messages_by_id` fetch for every id, then downloads with bounded concurrency (4) — one channel resolution and one fetch round trip for the whole batch, instead of one of each per `get_message_media` call. For a numeric `channel_id` a resolution is a full dialog walk with no cache, which is exactly what makes the per-call path expensive as the count grows.

**What it returns:** content blocks in request order —
`[image, metadata, image, metadata, …, summary]`. Each `metadata` block is the
same `GetMessageMediaResponse` shape `get_message_media` emits (byte-identical
to it for a batch of 1), positioned immediately after its image so the pairing
stays unambiguous even when some ids fail. The trailing `summary` block is
always last, regardless of how many ids failed.

**Per-id failures never fail the batch.** An id with no visual media, a deleted
id, or an id dropped because the batch's payload cap was reached is reported in
the summary's `failed` array with a machine-readable `reason` — `not_found`,
`no_visual_media`, `payload_cap_reached`, or `download_failed: <detail>` —
never as a call error. Only a channel-level failure (channel not found, a
resolve or fetch RPC error) fails the whole call, since in that case no id
could have succeeded.

**Payload cap:** the batch's total base64 payload (all images combined, the
quantity that actually consumes context) is capped by `[limits]
media_batch_max_total_bytes` (default 8 MiB), counted in bytes of base64 as
sent to the client. Images are downscaled progressively to fit the remaining
budget; ids that still don't fit once the budget is exhausted are reported as
`payload_cap_reached` rather than shrunk to uselessness.

**Rate limiting:** acquires `media_download_cost × requested ids` up front,
before any network work, then refunds the cost of every id that produced no
image — so the net charge is per image actually returned, while admission
control stays real (the limiter can still refuse a batch it can't afford). On a
whole-call failure the batch refunds all but **1 token**: the call still
performed a channel resolution and a fetch RPC before failing, the same cost
`get_messages_batch` charges for that identical shape of work.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `channel_id` | string | Yes | - | Channel ID or username. Every id in `message_ids` must belong to it |
| `message_ids` | array of integers | Yes | - | Message IDs to fetch media for in one call (1-10). Duplicates are deduped silently (order preserved) before the 10-id cap is checked |
| `max_dimension` | integer | No | 1280 | Longest image side in pixels after downscaling (clamped to 64–2048). Images may be downscaled further to fit the batch payload cap |

**Response shape** (four requested ids, one with no visual media, one deleted):
```
content: [
  image   (message 101),
  text    (GetMessageMediaResponse for message 101),
  image   (message 102),
  text    (GetMessageMediaResponse for message 102),
  text    (MediaBatchSummary — always last)
]
```

**Summary (trailing text block):**
```json
{
  "channel_id": "@technews",
  "requested": 4,
  "returned": 2,
  "failed": [
    { "id": 103, "reason": "no_visual_media" },
    { "id": 104, "reason": "not_found" }
  ],
  "total_base64_bytes": 611820,
  "max_total_bytes": 8388608
}
```

> **Note:** `requested` counts ids after de-duplication; `returned` is the
> number of image blocks actually produced, always `content.len() / 2`
> (every image has exactly one metadata block, and the summary is the one
> extra block). Every requested id ends up counted in exactly one of
> `returned` / `failed`. `max_total_bytes` echoes the configured cap so a
> caller can tell a near-miss from comfortable headroom without a separate
> `check_mcp_status` round trip. `check_mcp_status` also reports the same cap
> — plus `batch_max_ids`, `per_image_max_bytes`, `default_max_dimension`, and
> `max_dimension_limit` — under its `media` block (see `check_mcp_status`
> above), so a caller can plan a run instead of discovering the limits by
> hitting them.

**Usage:** Ask Claude to "show me the photos from messages 101, 102, 103, and 104 in channel technews" — a single `get_messages_media_batch` call replaces four separate `get_message_media` calls, at the cost of one channel resolution and one fetch round trip instead of four of each.

---

### 11. transcribe_voice_message

Transcribe a voice message or video note (round video) to text using Telegram's server-side `messages.transcribeAudio` (no local ML).

> **Requires Telegram Premium.** Transcription is only available on accounts with Telegram Premium and is subject to Telegram's weekly transcription quota. Without Premium the tool returns a clear error; `check_mcp_status` reports a `premium` flag so you can check in advance.

**What it returns:**
- The transcription `text` (may be partial). Only voice messages and video notes are transcribable; other media types are rejected with a structured error.
- `partial: true` when the wait elapsed before Telegram finished transcribing — the latest accumulated text is returned. The tool re-invokes (`polls`) the transcription until it completes or `timeout_seconds` elapses.
- `duration_seconds` of the source audio when available (from message metadata).
- `media_type`: `"voice"` or `"video_note"`.

**Rate limiting:** each call charges `transcription_cost` tokens from the rate limiter (default **5**, versus 1 for searches — Telegram's weekly quota makes these calls precious). Configure under `[rate_limiting] transcription_cost`.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `channel_id` | string | Yes | - | Channel ID or username |
| `message_id` | integer | Yes | - | Message ID within the channel |
| `timeout_seconds` | integer | No | 30 | Seconds to wait for transcription to complete (clamped to 1–120) |

**Response:**
```json
{
  "text": "привет, это голосовое сообщение",
  "partial": false,
  "duration_seconds": 7,
  "media_type": "voice"
}
```

**Usage:** Ask Claude to "transcribe the voice message in message 42 of channel @podcast".

---

### 12. search_public_channels

Search Telegram's public directory (`contacts.search`) for channels and groups by keyword — not limited to channels you're already subscribed to. Closes the "find sources" gap: use it to discover new channels, then follow up with `get_channel_info` using a result's real `@username`.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `query` | string | Yes | - | Keyword or name to search Telegram's public directory for |
| `limit` | integer | No | 10 | Maximum results to return (max: 50) |

**Response:** Same `ChannelsResponse` shape as `get_subscribed_channels`, with at most `limit` entries. Each returned channel's `is_subscribed` reflects whether you're already subscribed to it — Telegram's `contacts.search` returns matches from both the public directory and your own dialogs in the same result set, so a search can surface a mix of new and already-subscribed channels. `is_subscribed: true` is reliable; `is_subscribed: false` is best-effort, because the already-subscribed side of the result set is server-capped and prefix-matched. Unlike `get_subscribed_channels`, `contacts.search` reports no global match count, so `total` is always `null` here; `has_more` is `null` too when the page came back full (`returned == limit`) — a full page says nothing about what lies beyond it (work-order D10) — and a known `false` when fewer than `limit` results came back:

```json
{
  "channels": [
    {
      "id": 987654321,
      "name": "Rust Programming News",
      "username": "rustnews",
      "chat_type": "channel",
      "description": null,
      "member_count": null,
      "is_verified": false,
      "is_public": true,
      "is_subscribed": false,
      "last_message_date": null
    }
  ],
  "returned": 1,
  "total": null,
  "has_more": false
}
```

**Usage:** Ask Claude to "find public Telegram channels about Rust programming" — Claude calls `search_public_channels` with `query: "Rust programming"`, then drills into a result with `get_channel_info` using its `@username`.

> **Drill-down limits:** follow-ups go through a result's real public `@username`
> — `get_channel_info`, `get_recent_messages`, and `search_messages`/
> `resolve_channels` (all of which now resolve a username `channel_id`/
> identifier server-side) reach a channel you have not joined. What does **not**
> work on an unsubscribed result: following up by the returned numeric `id` (id
> lookups walk your dialog list, and `contacts.search` does not add its results
> to the peer cache). A result with no public username reports `username: null`
> and cannot be drilled into at all by username either — numeric `id`, and
> therefore any follow-up, requires joining first.

---

### 13. get_messages_batch

Fetch up to 50 specific messages from one channel in a single call — one `channels.GetMessages` RPC regardless of how many ids you ask for. The designated way to re-fetch full text after `text_truncated`, and to verify or deduplicate specific posts without N round trips.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `channel_id` | string | Yes | - | Channel ID or username (required) |
| `message_ids` | array of integers | Yes | - | Message IDs to fetch in one call (1-50). Deleted/missing ids are reported per-id in `missing`, not as an error |
| `max_text_length` | integer | No | 2000 | Maximum text length in characters per message. This tool is the designated full-text path: pass a large value with few ids to fetch untruncated text |

**Response:**
```json
{
  "channel_id": "swodki",
  "messages": [
    {
      "id": 610119,
      "channel_id": 1144180066,
      "channel_name": "Сводки",
      "channel_username": "swodki",
      "text": "Full untruncated post text...",
      "timestamp": "2026-08-10T05:55:12Z",
      "sender_id": 0,
      "sender_name": "Сводки",
      "has_media": false,
      "media_type": "none",
      "link": "https://t.me/swodki/610119"
    }
  ],
  "returned": 1,
  "missing": [
    { "id": 609784, "error": "not found or deleted" }
  ]
}
```

> **Note:** `message_ids` are deduped silently (order preserved) before the
> 50-id cap is checked, so `[7, 3, 7]` counts as 2 ids. Every requested id
> ends up in exactly one of `messages` / `missing` — never both, never
> neither. `missing` covers both a genuinely absent/deleted id and the rare
> case of a real Telegram message this client could not represent
> domain-side (logged as a warning server-side, reported to the caller as
> missing rather than silently dropped). `missing` is unrelated to
> `omitted_ids`: `omitted_ids` (present only when populated) lists ids whose
> messages were found but got popped from the tail to fit the response byte
> budget — re-request exactly those ids, since (unlike `missing`) they
> genuinely exist. Costs one rate-limiter token regardless of batch size.

**Usage:** Ask Claude to "fetch the full text of messages 610119 and 609784 from @swodki" — a single `get_messages_batch` call replaces two separate lookups and reports 609784 as missing if it was deleted.

---

### 14. resolve_channels

Batch-resolve up to 20 channel identifiers — numeric ID, `@username`, or the exact title of a subscribed chat — to full channel entities in one call. Use it for full channel entities (subscriber counts, verification flags, chat type) of reachable chats, and to look up title-only private channels. Forward attribution no longer needs it: `forwarded_from` carries the source's name/username inline, resolved from the same response envelope — including sources you are not subscribed to, which this tool cannot resolve.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `identifiers` | array of strings | Yes | - | Identifiers to resolve (1-20): numeric channel ID, `@username`, or the exact title of a subscribed chat. Each entry resolves independently; failures come back per-entry, not as a call error |

**Response:**
```json
{
  "resolutions": [
    {
      "identifier": "swodki",
      "channel": {
        "id": 1144180066,
        "name": "Сводки",
        "username": "swodki",
        "chat_type": "channel",
        "description": null,
        "member_count": null,
        "is_verified": true,
        "is_public": true,
        "is_subscribed": true,
        "last_message_date": "2025-12-28T10:30:00Z"
      }
    },
    {
      "identifier": "999",
      "error": "Channel not found: 999"
    }
  ],
  "returned": 2,
  "resolved": 1
}
```

> **Note:** Exactly one of `channel` / `error` is present per resolution
> (the other is omitted, not `null`). Classification per identifier: a
> numeric string matches a subscribed dialog by id; a valid-shaped username
> (after stripping a leading `@`) first matches a subscribed dialog by
> username (free), then falls back to one `resolve_username` RPC; anything
> else is matched as an exact, trimmed, case-insensitive **title** against
> your subscribed chats. A title matching zero chats reports `Channel not
> found: {identifier}`; a title matching more than one reports `ambiguous
> title: N subscribed chats are named '{identifier}'` — it never guesses.
> One dialog walk serves the id/username/title paths for the whole batch;
> only unmatched username-shaped identifiers spend an extra RPC (at most one
> each). The call itself only fails on a transport-level error (the dialog
> walk); per-identifier misses are data, not call failures. Blank entries in
> `identifiers` are rejected up front as a whole-call error. Costs one
> rate-limiter token regardless of batch size.

**Usage:** Ask Claude to "get the full entity for channel 1009988776 — subscriber count and verification" after spotting it in results, or "look up my 'Семейный чатик' group" for a title-only private chat with no public username. (Forward sources already arrive named in `forwarded_from` — no resolve round-trip needed.)

---

### 15. get_channel_stats

Posting-rate and engagement statistics for one channel, computed over a bounded recent-history sample — a minimal, classifier-free "how active/popular is this channel" summary (no promo/content classification).

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `channel_id` | string | Yes | - | Channel ID or username (required) |
| `days_back` | integer | No | 7 | Days of history to sample (max: 30). The sweep also caps at 500 raw messages; `sample.complete` reports whether the full window was covered |

**Response:**
```json
{
  "channel_id": 1144180066,
  "post_count": 42,
  "posts_per_day": 6.0,
  "median_views": 15000,
  "media_share": 0.71,
  "album_share": 0.12,
  "sample": {
    "messages_scanned": 187,
    "window_from": "2026-08-05T00:00:00Z",
    "window_to": "2026-08-12T00:00:00Z",
    "complete": true
  }
}
```

> **Note:** `post_count` is album-collapsed — the same collapsing
> `search_messages`/`get_recent_messages` apply by default — so a
> multi-photo album counts as one post, not N. `posts_per_day` divides
> `post_count` by the **sampled** span (`sample.window_to -
> sample.window_from`), floored at one hour, not by `days_back`; when the
> 500-message cap cuts the sweep short (`sample.complete: false`),
> `window_from` becomes the oldest scanned message's timestamp instead of
> the requested `days_back` boundary, so the rate still reflects what was
> actually sampled rather than understating it. `median_views` is the
> lower-middle median over sampled posts that carry a view count; `null`
> when none do. `media_share`/`album_share` are `0.0`-`1.0` fractions of
> sampled posts. A channel with zero posts in the window reports all of
> `post_count`/`posts_per_day`/`media_share`/`album_share` as `0`/`0.0` and
> `median_views: null`, never `NaN`. Treat `sample.complete: false` as a
> partial-window result — the sweep hit the 500-message scan cap before
> covering the full `days_back` request. Costs one rate-limiter token.

**Usage:** Ask Claude to "how active is @swodki — how many posts per day and what's the median view count" — `get_channel_stats` answers in one call instead of paging through `get_recent_messages` and computing it client-side.

---

## Manual Testing Guide

### Prerequisites for Testing

1. Complete the [Installation](#installation) steps
2. Authenticate with `--setup`
3. Have at least one subscribed Telegram channel

### Test 1: Verify Connection

```bash
# Start the server
./target/release/telegram-mcp
```

Then send via stdin:
```json
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"check_mcp_status","arguments":{}}}
```

**Expected:** `telegram_connected: true`, `rate_limiter.tokens: 60.0`

### Test 2: List Channels

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_subscribed_channels","arguments":{"limit":5}}}
```

**Expected:** List of your subscribed channels with metadata

### Test 3: Get Channel Info

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_channel_info","arguments":{"channel_identifier":"durov"}}}
```

**Expected:** Details about the specified channel (if public/subscribed)

### Test 4: Search Messages

```json
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"search_messages","arguments":{"query":"hello","limit":5}}}
```

**Expected:** Recent messages containing "hello"

### Test 4b: Search with Media Filter

Search for photos only:
```json
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"search_messages","arguments":{"media_filter":"photo","limit":5}}}
```

**Expected:** Recent messages with photos attached (no text query required)

Search for documents with text:
```json
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"search_messages","arguments":{"query":"report","media_filter":"document","limit":5}}}
```

**Expected:** Messages containing "report" with a document attached

### Test 5: Generate Link

Use a `channel_id` and `message_id` from the search results:

```json
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"generate_message_link","arguments":{"channel_id":"1234567890","message_id":42}}}
```

**Expected:** Both HTTPS and tg:// links

### Test 6: Open in Telegram (macOS only)

```json
{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"open_message_in_telegram","arguments":{"channel_id":"1234567890","message_id":42}}}
```

**Expected:** Telegram Desktop opens to the specified message

### Test 7: Get Recent Messages

Get recent messages from a channel without a search query:
```json
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"get_recent_messages","arguments":{"channel_id":"durov","hours_back":24,"limit":10}}}
```

**Expected:** Recent messages from the channel within the last 24 hours

With media filter:
```json
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"get_recent_messages","arguments":{"channel_id":"durov","hours_back":48,"limit":5,"media_filter":"photo"}}}
```

**Expected:** Recent messages with photos from the channel

### Testing with Claude Desktop

1. Configure Claude Desktop (see [Usage](#connecting-with-claude-desktop))
2. Restart Claude Desktop
3. Ask Claude: "Check the Telegram connection status"
   - **Expected:** Shows `telegram_connected: true` and available tokens
4. Ask Claude: "List my Telegram channels"
   - **Expected:** Returns list of your subscribed channels
5. Ask Claude: "Search for messages about AI in my Telegram channels"
   - **Expected:** Returns matching messages with metadata
6. Ask Claude: "Find all photos in my Telegram channels from the last 24 hours"
   - **Expected:** Returns messages with photos attached (uses media_filter=photo)
7. Ask Claude: "Search for documents containing 'report' in my Telegram"
   - **Expected:** Returns messages with query "report" and document attachments
8. Ask Claude: "Get the last 10 messages from the @durov channel in the past 24 hours"
   - **Expected:** Returns recent messages from the channel (uses `get_recent_messages`)
9. Ask Claude: "Get info about the @durov channel"
   - **Expected:** Returns channel details (if public)
10. Ask Claude: "Generate a link for message [id] in channel [channel_id]"
   - **Expected:** Returns both HTTPS and tg:// links

## Troubleshooting

### Authentication Issues

**"Session expired"**
- Delete the session file and re-authenticate:
  ```bash
  rm ~/.config/telegram-connector/session.bin
  ./target/release/telegram-mcp --setup
  ```

**"Invalid phone number format"**
- Use international format with country code: `+1234567890`

**"2FA password required"**
- The setup will prompt for your 2FA password
- Make sure you enter the cloud password, not the local passcode

### Connection Issues

**"Failed to connect to Telegram"**
- Check your internet connection
- Verify `api_id` and `api_hash` are correct
- Telegram may be temporarily unavailable

**"Rate limited"**
- Wait for the specified `retry_after_seconds`
- Reduce request frequency
- Increase `refill_rate` in config

### MCP Issues

**Server not responding**
- Check if the binary is running: `ps aux | grep telegram-mcp`
- Verify stdio is not blocked
- Check logs for errors

**Tool not found**
- Ensure you're using the correct tool name
- Restart Claude Desktop after config changes

### Configuration Issues

**"Config file not found"**
- Config location depends on your platform:
  - **Linux:** `~/.config/telegram-connector/config.toml`
  - **macOS:** `~/Library/Application Support/telegram-connector/config.toml`
  - **Windows:** `%APPDATA%\telegram-connector\config.toml`
- Create the directory and copy the example config
- Or use `--config` flag: `./telegram-mcp --config ./config.toml`
- Or set `TELEGRAM_MCP_CONFIG` environment variable

**"Environment variable not found"**
- Set the variable: `export TELEGRAM_API_HASH="your_hash"`
- Or use plain text values in config

## Development

### Running Tests

```bash
# All tests
cargo test

# Specific module
cargo test mcp
cargo test telegram
cargo test config -- --test-threads=1

# With output
cargo test -- --nocapture
```

### Code Quality

```bash
# Format check
cargo fmt --check

# Linting
cargo clippy -- -D warnings

# All checks (pre-commit)
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

### Project Structure

```
src/
├── lib.rs              # Library root, public API exports
├── main.rs             # CLI entry point, signal handling
├── cli.rs              # CLI argument parsing (clap)
├── config.rs           # Configuration loading & validation
├── error.rs            # Error types (thiserror)
├── logging.rs          # Dual-layer tracing (stderr + file), log cleanup
├── rate_limiter.rs     # Token bucket rate limiting
├── link.rs             # Deep link generation (tg://, https://t.me)
├── test_helpers.rs     # Test fixture factories (cfg(test))
├── mcp.rs              # MCP module root
├── mcp/
│   ├── server.rs       # MCP server + all 16 tool handlers
│   ├── tools.rs        # Re-exports tools + helpers
│   └── tools/
│       ├── helpers.rs  # ID parsing helpers
│       └── types/
│           ├── requests.rs     # Tool request types
│           ├── responses.rs    # Tool response types
│           └── serde_helpers.rs # Custom deserializers
├── telegram.rs         # Telegram module root
└── telegram/
    ├── client.rs       # Telegram client (grammers wrapper)
    ├── trait_def.rs     # TelegramClientTrait + mock generation
    ├── converters.rs    # Type converters (grammers → domain)
    ├── auth.rs          # Authentication & 2FA flow
    └── types/
        ├── ids.rs       # ChannelId, MessageId, UserId
        ├── names.rs     # Username, ChannelName
        ├── media.rs     # MediaType, MediaFilter
        ├── entities.rs  # Message, Channel
        └── params.rs    # SearchParams, HistoryParams, SearchResult
```

## License

MIT License - see LICENSE file for details.

## Acknowledgments

- [grammers](https://codeberg.org/Lonami/grammers) - Rust MTProto implementation
- [rmcp](https://github.com/anthropics/mcp) - Model Context Protocol SDK
- [Telegram](https://telegram.org) - Messaging platform
