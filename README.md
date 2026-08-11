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
│                     MCP Server Layer (12 tools)              │
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

[rate_limiting]
# Optional: token bucket configuration
# max_tokens = 50
# refill_rate = 2.0
# media_download_cost = 5                                       # Rate-limit tokens charged per get_message_media call (default: 5)

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
  "rate_limiter_tokens": 45.5,
  "server_version": "0.1.0"
}
```

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
| `include_full` | boolean | No | `false` | Fetch full channel info (`description`, `member_count`) with one extra Telegram RPC (`channels.getFullChannel`). Channel-kind peers only (broadcasts and megagroups); other peer kinds (small groups, communities) silently fall back to basic info. |

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

> **Note:** `include_full` fills in `description`, `member_count`, and
> `last_message_date`. The extra RPC applies to channel-kind peers (broadcasts
> and megagroups); if it fails (e.g. a private or forbidden channel) the basic
> channel info is still returned, with `description`/`member_count`/`last_message_date`
> left as `null`. It costs one rate-limiter
> token on top of the basic lookup.

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

**Usage:** Verify channel details or get the numeric ID for other operations. Use `include_full: true` when you specifically need the description or member count — it costs one extra Telegram RPC round-trip.

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
| `channel_id` | string | No | - | Filter by specific channel ID |
| `hours_back` | integer | No | 48 | How many hours back to search (max: 72) |
| `limit` | integer | No | 20 | Maximum results to return (max: 100) |
| `media_filter` | string | No | - | Filter by media type (see below) |
| `from_date` | string | No | - | Inclusive start of the time window as RFC 3339 UTC (e.g. `2026-08-01T00:00:00Z`). Overrides `hours_back`. Reaching far back works best on low-traffic channels; on active channels prefer a narrower recent window, since deep windows are paged client-side and may time out |
| `to_date` | string | No | - | Inclusive end of the time window as RFC 3339 UTC. Messages newer than this are excluded. Set without `from_date`, it must fall inside the `hours_back` window — otherwise the window is empty and the call is rejected |

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

**Response:**
```json
{
  "messages": [
    {
      "id": 42,
      "channel_id": 1234567890,
      "channel_name": "Tech News",
      "channel_username": "technews",
      "text": "Breaking: New AI model released...",
      "timestamp": "2025-12-28T10:30:00Z",
      "sender_id": 987654321,
      "sender_name": "John Doe",
      "has_media": false,
      "media_type": "none",
      "views": 15000,
      "forwards": 230,
      "forwarded_from": {
        "channel_id": 1009988776,
        "original_message_id": 8123,
        "original_date": "2025-12-28T09:00:00Z"
      },
      "link_preview": {
        "url": "https://example.com/ai-model",
        "site_name": "Example",
        "title": "New AI model released",
        "description": "A short summary pulled from Telegram's server-side preview..."
      },
      "reply_to_message_id": 41,
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
        "duration_seconds": 8,
        "file_size_bytes": 41200,
        "kind": "voice",
        "mime_type": "audio/ogg"
      }
    }
  ],
  "returned": 15,
  "search_time_ms": 250,
  "query_metadata": {
    "query": "AI model",
    "window_from": "2025-12-26T10:30:00Z",
    "channels_scanned": 10,
    "channels_in_results": 6
  }
}
```

**Response fields:** `returned` is the number of messages in this response (the
page size — not a total-match count; there may be more matches beyond it).
`query_metadata.window_from` is the effective window start actually applied
(`from_date`, or `now - hours_back`); `window_to` is the effective upper bound
and is omitted entirely when the window is open-ended. `channels_scanned` is
the number of channels the search actually scanned — `null` for a global
search (server-side, scan scope is unknowable) and a concrete count when
`channel_id` is set. `channels_in_results` is the number of distinct channels
present in `messages`, always a number.

**Forward attribution & link previews:** Messages carry optional enrichment derived
from the same Telegram response — no extra API calls. `forwarded_from` attributes a
forwarded post to its source (`channel_id`, `original_message_id`, `original_date`,
and `sender_name` for hidden senders); the source channel's **title and username are
not included** — Telegram does not expose them per message without an extra lookup,
so pair `channel_id` with `generate_message_link` if you need to reach the source.
`link_preview` surfaces Telegram's server-side webpage preview (`url`, `site_name`,
`title`, `description`, truncated to 500 characters). `views`, `forwards`, and
`reply_to_message_id` are included when present. All of these fields are omitted
entirely when absent, so existing consumers are unaffected.

**Video & audio metadata:** Messages with video-class media carry an optional
`video_info` object — `duration_seconds`, `width`, `height`, `file_size_bytes`,
`kind` (`video` | `video_note` | `animation`), `has_thumbnail`, and `mime_type` —
and audio-class media carry an optional `audio_info` object (`duration_seconds`,
`file_size_bytes`, `kind` (`audio` | `voice`), `mime_type`). Both are derived from
the message's document attributes with **no extra API calls** (the full video is
never downloaded), so the client can judge a clip's length and shape — and whether
fetching its thumbnail via `get_message_media`, or transcribing a voice message, is
worthwhile — before spending a request. Rare GIF-class animations without a video
attribute report `duration_seconds`/`width`/`height` as `0`. Both objects are
omitted when the message has no video/audio media.

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
| `channel_id` | string | Yes | - | Channel ID or username (e.g., `technews` or `1234567890`) |
| `hours_back` | integer | No | 48 | Hours of history to retrieve (max: 168) |
| `limit` | integer | No | 20 | Maximum messages to return (max: 100) |
| `media_filter` | string | No | - | Filter by media type (same options as `search_messages`) |
| `from_date` | string | No | - | Inclusive start of the time window as RFC 3339 UTC (e.g. `2026-08-01T00:00:00Z`). Overrides `hours_back`. Reaching far back works best on low-traffic channels; on active channels prefer a narrower recent window, since deep windows are paged client-side and may time out |
| `to_date` | string | No | - | Inclusive end of the time window as RFC 3339 UTC. Messages newer than this are excluded. Set without `from_date`, it must fall inside the `hours_back` window — otherwise the window is empty and the call is rejected |

**Key Difference from `search_messages`:**

| Feature | search_messages | get_recent_messages |
|---------|-----------------|---------------------|
| Query required | Yes (or media_filter) | No |
| Channel required | No (global search) | Yes (single channel) |
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
      "media_type": "none"
    }
  ],
  "returned": 5,
  "search_time_ms": 150,
  "query_metadata": {
    "query": "",
    "window_from": "2025-12-27T10:30:00Z",
    "channels_scanned": 1,
    "channels_in_results": 1
  }
}
```

**Date-range example:** fetch a fixed window instead of a rolling `hours_back` lookback:
```json
{"channel_id": "durov", "from_date": "2026-07-01T00:00:00Z", "to_date": "2026-07-31T23:59:59Z"}
```

**Usage:** Get all recent activity from a channel without needing a search query. Ideal for monitoring or catching up on channel content.

---

### 10. get_message_media

Retrieve the visual media from a Telegram message as an MCP **image content block** (base64-encoded JPEG, quality 80) plus a JSON metadata text block. Useful for reading photos posted in channels without leaving the conversation.

**What it returns:**
- **Photos:** the photo is downscaled so its longest side fits `max_dimension`, re-encoded as JPEG, and returned as an MCP image block. The metadata block contains `media_type`, `is_thumbnail` (always `false` for photos), `caption`, original dimensions and byte size, and the returned dimensions and byte size.
- **Videos, animations, video notes:** only the server-side thumbnail is available; it is returned as an image block with `is_thumbnail: true` and a `video_info` object (duration, dimensions, kind) in the metadata.
- **Messages without visual media:** a structured error is returned (no image block).
- **Photos whose selected size variant exceeds 20 MB:** refused with an error.
- **Payload cap:** the base64 payload is capped at ~1.5 MB; if the image is still too large after the initial downscale it is downscaled further automatically.

**Size variant selection:** the smallest server-side size variant whose longest side is >= `max_dimension` is chosen before downloading, minimising transfer bytes while guaranteeing the requested resolution.

**Rate limiting:** each call charges `media_download_cost` tokens from the rate limiter (default **5**, versus 1 for searches). Configure under `[rate_limiting] media_download_cost`.

**Timeout:** bounded by `[telegram.timeouts] download_secs` (default 120 s).

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
  "original_width": 2560,
  "original_height": 1440,
  "original_size_bytes": 400000,
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
> — `get_channel_info` (and `get_recent_messages`, which also resolves usernames
> server-side) reach a channel you have not joined. Two things do **not** work on
> an unsubscribed result: following up by the returned numeric `id` (id lookups
> walk your dialog list, and `contacts.search` does not add its results to the
> peer cache), and `search_messages`, whose `channel_id` is numeric-only. A result
> with no public username reports `username: null` and cannot be drilled into at
> all. To keyword-search a newly discovered channel, join it first.

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

**Expected:** `telegram_connected: true`, `rate_limiter_tokens: 50.0`

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
│   ├── server.rs       # MCP server + all 7 tool handlers
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
