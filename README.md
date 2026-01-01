# Telegram MCP Connector

A Model Context Protocol (MCP) service that enables Claude to search and interact with Telegram channels and messages in real-time. Built in Rust using the `rmcp` SDK and `grammers` Telegram client.

## Features

- **Real-time Telegram Access** - Connect to Telegram using the MTProto protocol
- **Message Search** - Search messages across all subscribed channels or specific channels
- **Channel Management** - List subscribed channels and get channel metadata
- **Deep Linking** - Generate `tg://` and `https://t.me` links for messages
- **Native Integration** - Open messages directly in Telegram Desktop (macOS)
- **Rate Limiting** - Built-in token bucket rate limiter to prevent API abuse
- **Secure Credentials** - API keys and phone numbers protected with `secrecy` crate

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    MCP Client (Claude Desktop)              │
└──────────────────────────┬──────────────────────────────────┘
                           │ JSON-RPC over stdio
┌──────────────────────────▼──────────────────────────────────┐
│                     MCP Server Layer                        │
│                    (rmcp + tools.rs)                        │
│                                                             │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────┐    │
│  │check_status │ │get_channels │ │search_messages      │    │
│  └─────────────┘ └─────────────┘ └─────────────────────┘    │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────┐    │
│  │channel_info │ │gen_link     │ │open_in_telegram     │    │
│  └─────────────┘ └─────────────┘ └─────────────────────┘    │
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
# session_file = "~/.config/telegram-connector/session.db"

[search]
# Optional: search defaults
# default_hours_back = 48
# max_results_default = 20
# max_results_limit = 100

[rate_limiting]
# Optional: token bucket configuration
# max_tokens = 50
# refill_rate = 2.0

[logging]
# Optional: logging configuration
# level = "info"
# format = "compact"
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

### Connecting with Comet Browser

[Comet](https://www.comet.wiki/) is an MCP-compatible browser that can connect to MCP servers.

**Step 1:** Open Comet and go to Settings (gear icon)

**Step 2:** Navigate to "MCP Servers" section

**Step 3:** Click "Add Server" and configure:

| Field | Value |
|-------|-------|
| Name | `Telegram` |
| Command | `/path/to/telegram-mcp` |
| Arguments | (leave empty) |

**Step 4:** Click "Save" and enable the server

**Step 5:** The Telegram tools will now be available in Comet's chat interface

**Verify Connection:**
- Open a new chat in Comet
- Ask: "Check the Telegram connection status"
- Comet will use the `check_mcp_status` tool to verify the connection

**Example Prompts for Comet:**
- "List my Telegram channels"
- "Search for messages about AI in my Telegram"
- "Get info about the @durov channel"
- "Generate a link for message 42 in channel 1234567890"

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
      "description": "Latest tech updates",
      "member_count": 50000,
      "is_verified": true,
      "is_public": true,
      "is_subscribed": true,
      "last_message_date": "2025-12-28T10:30:00Z"
    }
  ],
  "total": 25,
  "has_more": false
}
```

**Usage:** Get a list of available channels before searching or filtering.

---

### 3. get_channel_info

Get detailed information about a specific channel.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `channel_identifier` | string | Yes | Channel username (e.g., `technews`) or numeric ID |

**Response:**
```json
{
  "id": 1234567890,
  "name": "Tech News",
  "username": "technews",
  "description": "Latest tech updates and analysis",
  "member_count": 50000,
  "is_verified": true,
  "is_public": true,
  "is_subscribed": true,
  "last_message_date": "2025-12-28T10:30:00Z"
}
```

**Usage:** Verify channel details or get the numeric ID for other operations.

---

### 4. generate_message_link

Generate shareable links for a specific message.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `channel_id` | string | Yes | - | Numeric channel ID |
| `message_id` | integer | Yes | - | Message ID within the channel |
| `include_tg_protocol` | boolean | No | true | Include `tg://` protocol link |

**Response:**
```json
{
  "channel_id": "1234567890",
  "message_id": 42,
  "https_link": "https://t.me/c/1234567890/42?single",
  "tg_protocol_link": "tg://privatepost?channel=1234567890&post=42&single"
}
```

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
| `channel_id` | string | Yes | - | Numeric channel ID |
| `message_id` | integer | Yes | - | Message ID |
| `use_tg_protocol` | boolean | No | true | Use `tg://` (true) or `https://` (false) |

**Response:**
```json
{
  "success": true,
  "message": "Opened message in Telegram",
  "link_used": "tg://privatepost?channel=1234567890&post=42&single",
  "app_opened": true
}
```

**Usage:** Quickly navigate to a message in the native Telegram app.

---

### 6. search_messages

Search for messages across channels with optional media type filtering.

**Parameters:**
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `query` | string | Conditional | - | Search query. Required unless `media_filter` is specified |
| `channel_id` | string | No | - | Filter by specific channel ID |
| `hours_back` | integer | No | 48 | How many hours back to search (max: 168) |
| `limit` | integer | No | 20 | Maximum results to return (max: 100) |
| `media_filter` | string | No | - | Filter by media type (see below) |

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
      "media_type": "none"
    }
  ],
  "total_found": 15,
  "search_time_ms": 250,
  "query_metadata": {
    "query": "AI model",
    "hours_back": 48,
    "channels_searched": 10
  }
}
```

**Media Types:** `none`, `photo`, `video`, `document`, `audio`, `voice`, `videonote`, `animation`, `sticker`, `contact`, `location`, `venue`, `poll`, `dice`

**Usage Examples:**

| Query | media_filter | Result |
|-------|--------------|--------|
| `"AI news"` | (none) | Messages containing "AI news" |
| `"AI news"` | `photo` | Messages with "AI news" AND a photo attached |
| `""` (empty) | `document` | All documents (no text filtering) |
| `""` (empty) | (none) | ❌ Error (too broad) |

**Usage:** Search for specific topics across your subscribed channels, optionally filtering by media type.

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

### Testing with Claude Desktop

1. Configure Claude Desktop (see [Usage](#connecting-with-claude-desktop))
2. Restart Claude Desktop
3. Ask Claude: "Check the Telegram connection status"
4. Ask Claude: "List my Telegram channels"
5. Ask Claude: "Search for messages about AI in my Telegram channels"

### Testing with Comet Browser

1. Configure Comet (see [Usage](#connecting-with-comet-browser))
2. Open a new chat in Comet
3. Ask: "Check the Telegram connection status"
   - **Expected:** Shows `telegram_connected: true` and available tokens
4. Ask: "List my Telegram channels"
   - **Expected:** Returns list of your subscribed channels
5. Ask: "Search for messages about [topic] in my Telegram"
   - **Expected:** Returns matching messages with metadata
6. Ask: "Find all photos in my Telegram channels from the last 24 hours"
   - **Expected:** Returns messages with photos attached (uses media_filter=photo)
7. Ask: "Search for documents containing 'report' in my Telegram"
   - **Expected:** Returns messages with query "report" and document attachments
8. Ask: "Get info about the @durov channel"
   - **Expected:** Returns channel details (if public)
9. Ask: "Generate a link for message [id] in channel [channel_id]"
   - **Expected:** Returns both HTTPS and tg:// links

**Troubleshooting Comet:**
- If tools don't appear, check that the server is enabled in MCP Servers settings
- Verify the binary path is correct and the file is executable
- Check Comet's console for connection errors

## Troubleshooting

### Authentication Issues

**"Session expired"**
- Delete the session file and re-authenticate:
  ```bash
  rm ~/.config/telegram-connector/session.db
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
├── lib.rs              # Library root
├── main.rs             # CLI entry point
├── cli.rs              # CLI argument parsing
├── config.rs           # Configuration loading
├── error.rs            # Error types
├── logging.rs          # Logging setup
├── rate_limiter.rs     # Rate limiting
├── link.rs             # Link generation
├── mcp.rs              # MCP module root
├── mcp/
│   ├── server.rs       # MCP server + tools
│   └── tools/
│       └── types.rs    # Tool request/response types
├── telegram.rs         # Telegram module root
└── telegram/
    ├── client.rs       # Telegram client
    ├── auth.rs         # Authentication
    └── types.rs        # Domain types
```

## License

MIT License - see LICENSE file for details.

## Acknowledgments

- [grammers](https://github.com/Lonami/grammers) - Rust MTProto implementation
- [rmcp](https://github.com/anthropics/mcp) - Model Context Protocol SDK
- [Telegram](https://telegram.org) - Messaging platform
