
# Telegram Channel & Message Search MCP Service
## Comprehensive Specification & Implementation Guide

**Version:** 1.0.0
**Status:** Production Ready
**Last Updated:** December 14, 2025
**Target Platform:** macOS (extensible to Linux/Windows)
**Methodology:** TDD + KISS + DDD  

---

## 📋 Table of Contents

1. [Executive Summary](#executive-summary)
2. [Project Vision & Scope](#project-vision--scope)
3. [Architecture Overview](#architecture-overview)
4. [Development Stack](#development-stack)
5. [MCP Tool Schema](#mcp-tool-schema)
6. [Configuration Guide](#configuration-guide)
7. [Error Handling Strategy](#error-handling-strategy)
8. [Project Structure](#project-structure)
9. [Implementation Checklist](#implementation-checklist)
10. [Testing Strategy (TDD)](#testing-strategy-tdd)
11. [Documentation Examples](#documentation-examples)
12. [Future Roadmap](#future-roadmap)

---

## Executive Summary

### What is This Project?

A **Model Context Protocol (MCP) service** that enables the Comet browser (powered by Claude) to search Russian-language Telegram channels and messages in real-time. Users can discover relevant conversations, retrieve message metadata, and seamlessly open messages in Telegram Desktop on macOS.

### Key Features (V1)

- ✅ **Full-text search** across subscribed Russian Telegram channels
- ✅ **Time-window search** (default: last 48 hours, extensible to 72 hours)
- ✅ **Deep link generation** for opening messages in Telegram Desktop
- ✅ **Channel metadata retrieval** (members, descriptions, verification status)
- ✅ **Adaptive rate limiting** (burst-friendly for Claude's multi-query workflows)
- ✅ **macOS integration** (via `tg://` protocol handlers)
- ✅ **TDD-first development** (all tests written before implementation)

### Use Cases

1. **Research & Discovery:** Claude searches Russian Telegram channels for recent news, announcements, discussions
2. **Content Monitoring:** Track specific topics across subscribed channels
3. **Verification:** Click search results to verify content directly in Telegram Desktop
4. **Integration:** Seamlessly merge Telegram insights into Claude's reasoning

---

## Project Vision & Scope

### MVP Scope (V1)

**In Scope:**
- Message search (text + date filters) in subscribed channels only
- Channel information retrieval
- Deep link generation for macOS
- Adaptive rate limiting (Option B: token bucket)
- Configuration-based authentication (single Telegram account)
- Interactive 2FA flow on first run
- Comprehensive error handling and logging

**Out of Scope (V2+):**
- Channel discovery/search
- File/media/image/video filters
- Multi-account support
- Session expiration re-authentication
- Advanced integrations (export, webhooks, caching)
- Non-macOS platforms (will be added)

### Target Users

- **Primary:** Researchers, analysts, content creators using Comet browser with Claude
- **Technical Level:** Comfortable with terminal setup, config files
- **Hardware:** macOS (Intel/Apple Silicon), Telegram Desktop installed

---

## Architecture Overview

### High-Level Flow

```
┌─────────────────────────────────────────────────────────────┐
│                    Comet Browser + Claude                   │
│                  (MCP Client, Native JS)                    │
└──────────────────────────┬──────────────────────────────────┘
                           │ (JSON-RPC over stdio)
                           │
┌──────────────────────────▼──────────────────────────────────┐
│           Telegram MCP Service (Rust Binary)                │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  MCP Server Layer (rmcp SDK)                           │ │
│  │  - Request handling (JSON-RPC 2.0)                     │ │
│  │  - Tool registration & discovery                       │ │
│  │  - Response serialization                              │ │
│  └────────────────────────────────────────────────────────┘ │
│                           │                                  │
│  ┌────────────────────────▼────────────────────────────────┐ │
│  │  Tool Implementation Layer                              │ │
│  │  - search_messages()                                    │ │
│  │  - get_channel_info()                                   │ │
│  │  - generate_message_link()                              │ │
│  │  - open_message_in_telegram()                           │ │
│  │  - get_subscribed_channels()                            │ │
│  │  - check_mcp_status()                                   │ │
│  └────────────────────────────────────────────────────────┘ │
│                           │                                  │
│  ┌────────────────────────▼────────────────────────────────┐ │
│  │  Telegram Layer (grammers client)                       │ │
│  │  - User session authentication                          │ │
│  │  - MTProto API calls                                    │ │
│  │  - Message fetching & filtering                         │ │
│  │  - Channel metadata retrieval                           │ │
│  └────────────────────────────────────────────────────────┘ │
│                           │                                  │
│  ┌────────────────────────▼────────────────────────────────┐ │
│  │  Support Layers                                         │ │
│  │  - Rate Limiter (adaptive token bucket)                 │ │
│  │  - Logger (structured tracing)                          │ │
│  │  - Config Manager (TOML parsing)                        │ │
│  │  - Error Handler (anyhow + thiserror)                   │ │
│  └────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────┘
                           │ (Telegram MTProto)
                           ▼
                  Telegram Cloud API
```

### Component Responsibilities

| Component | Responsibility | Technology |
|-----------|-----------------|------------|
| **MCP Server** | Expose tools via JSON-RPC, handle Comet requests | `rmcp` (official Rust SDK) |
| **Tool Layer** | Implement search/metadata/link logic | Custom async Rust code |
| **Telegram Layer** | Authenticate user, fetch messages, manage session | `grammers` client library |
| **Rate Limiter** | Track API quota, implement adaptive backpressure | Custom async token bucket |
| **Logger** | Structured event logging for debugging | `tracing` + `tracing-subscriber` |
| **Config** | Parse auth credentials, search parameters | `serde` + `toml` |
| **Error Handling** | Type-safe error types and recovery | `anyhow` + `thiserror` |

---

## Development Stack

### Core Technology Choices

| Layer | Technology | Version | Rationale |
|-------|-----------|---------|-----------|
| **Language** | Rust | 2024 Edition | Memory safety, async-first, MCP ecosystem support |
| **Async Runtime** | Tokio | Latest stable | Industry standard, full `async/await` support, excellent scheduler |
| **MCP Implementation** | `rmcp` (official SDK) | 0.8.0+ | Official protocol implementation, stdio transport, type-safe |
| **Telegram Client** | `grammers` | Latest | Full MTProto implementation, user authentication, no restrictions |
| **Config Format** | TOML | Standard | Human-readable, Rust-native parsing |
| **Logging** | `tracing` + `tracing-subscriber` | Latest | Structured logging, async-aware, granular filtering |
| **Error Handling** | `anyhow` + `thiserror` | Latest | Pragmatic combo: easy propagation + custom error types |
| **Serialization** | `serde_json` | Standard | Universal JSON handling, MCP compliance |
| **Testing** | `tokio::test` + `proptest` | Standard | TDD-first, property-based testing for search logic |

### Cargo.toml Dependencies

```toml
[package]
name = "telegram-connector"
version = "0.1.0"
edition = "2024"

[lib]
name = "telegram_connector"
path = "src/lib.rs"

[[bin]]
name = "telegram-mcp"
path = "src/main.rs"

[dependencies]
# MCP
rmcp = { version = "0.8", features = ["server"] }

# Telegram
grammers-client = { git = "https://github.com/Lonami/grammers", branch = "main" }
grammers-session = { git = "https://github.com/Lonami/grammers", branch = "main" }

# Async
tokio = { version = "1", features = ["full"] }

# Config & Serialization
toml = "0.8"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
directories = "5.0"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "json"] }
tracing-appender = "0.2"

# Errors
anyhow = "1.0"
thiserror = "1.0"

# Utilities
chrono = { version = "0.4", features = ["serde"] }
dashmap = "5.5"

# Security
secrecy = { version = "0.10", features = ["serde"] }

[dev-dependencies]
tokio-test = "0.4"
proptest = "1.4"
mockall = "0.13"
```

### Build & Runtime Requirements

- **Rust Toolchain:** rustup with latest stable (2024 edition)
- **Runtime Dependencies:**
  - Telegram Desktop (macOS) for `open tg://` links to work
  - Valid Telegram API credentials (API ID & hash from https://my.telegram.org)
  - 2GB+ RAM, 500MB disk space
- **Network:**
  - TCP/IP connection to Telegram servers
  - Support for IPv4 and IPv6
  - Can handle network disruptions gracefully

---

## MCP Tool Schema

### MCP Server Capabilities Discovery

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "protocolVersion": "2025-06-18",
    "capabilities": {},
    "clientInfo": {
      "name": "Comet",
      "version": "1.0.0"
    }
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": "2025-06-18",
    "capabilities": {
      "tools": {}
    },
    "serverInfo": {
      "name": "telegram-mcp-service",
      "version": "0.1.0"
    }
  }
}
```

### Tool Discovery

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/list",
  "params": {}
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "tools": [
      {
        "name": "search_messages",
        "description": "Search for messages in subscribed Russian Telegram channels from the last 1-2 days",
        "inputSchema": { /* see below */ }
      },
      {
        "name": "get_channel_info",
        "description": "Get metadata about a Telegram channel",
        "inputSchema": { /* see below */ }
      },
      {
        "name": "generate_message_link",
        "description": "Generate a Telegram message deep link (t.me format) that opens in Telegram Desktop on macOS",
        "inputSchema": { /* see below */ }
      },
      {
        "name": "open_message_in_telegram",
        "description": "Programmatically open a Telegram message in Telegram Desktop on macOS",
        "inputSchema": { /* see below */ }
      },
      {
        "name": "get_subscribed_channels",
        "description": "List all subscribed channels",
        "inputSchema": { /* see below */ }
      },
      {
        "name": "check_mcp_status",
        "description": "Health check: verify Telegram session is active, rate limits, and config",
        "inputSchema": { /* see below */ }
      }
    ]
  }
}
```

---

### TOOL 1: `search_messages`

**Purpose:** Full-text search for messages across subscribed channels

#### Request Schema

```json
{
  "name": "search_messages",
  "description": "Search for messages in subscribed Russian Telegram channels from the last 1-2 days",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "Search text in Russian (required). Example: 'искусственный интеллект', 'криптовалюта'"
      },
      "channel_id": {
        "type": "string",
        "description": "Optional: Specific channel ID or username to search in. Omit to search all subscribed channels. Format: numeric ID (e.g., '123456789') or username without @ (e.g., 'durov')"
      },
      "hours_back": {
        "type": "integer",
        "description": "Optional: Search window in hours (default: 48, max: 72). Defines how far back in time to search.",
        "minimum": 1,
        "maximum": 72,
        "default": 48
      },
      "limit": {
        "type": "integer",
        "description": "Optional: Max results to return (default: 20, max: 100). More results = slower but more comprehensive.",
        "minimum": 1,
        "maximum": 100,
        "default": 20
      }
    },
    "required": ["query"]
  }
}
```

#### Request Examples

**Example 1: Simple search across all channels**
```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "method": "tools/call",
  "params": {
    "name": "search_messages",
    "arguments": {
      "query": "искусственный интеллект"
    }
  }
}
```

**Example 2: Search in specific channel with date range**
```json
{
  "jsonrpc": "2.0",
  "id": 11,
  "method": "tools/call",
  "params": {
    "name": "search_messages",
    "arguments": {
      "query": "ChatGPT",
      "channel_id": "habrhabr",
      "hours_back": 24,
      "limit": 50
    }
  }
}
```

#### Response Schema

```json
{
  "type": "object",
  "properties": {
    "results": {
      "type": "array",
      "description": "Array of matching messages, ordered by recency (newest first)",
      "items": {
        "type": "object",
        "properties": {
          "message_id": {
            "type": "integer",
            "description": "Unique message ID within the channel"
          },
          "text": {
            "type": "string",
            "description": "Full message text (truncated to 2000 chars if longer)"
          },
          "channel_name": {
            "type": "string",
            "description": "Human-readable channel name"
          },
          "channel_username": {
            "type": "string",
            "description": "Channel username without @"
          },
          "channel_id": {
            "type": "string",
            "description": "Numeric channel ID"
          },
          "timestamp": {
            "type": "string",
            "format": "date-time",
            "description": "ISO 8601 format, e.g., '2025-12-14T16:30:00Z'"
          },
          "message_link": {
            "type": "string",
            "description": "tg:// deep link for macOS. Example: 'tg://resolve?channel=123456789&post=42&single'"
          },
          "sender_id": {
            "type": "string",
            "description": "User ID of message author (empty if forwarded/channel message)"
          },
          "sender_name": {
            "type": "string",
            "description": "Display name of message author"
          },
          "has_media": {
            "type": "boolean",
            "description": "Whether message contains media (V2: will add file_type)"
          },
          "media_type": {
            "type": "string",
            "enum": ["none", "photo", "video", "document", "audio", "animation"],
            "description": "Type of media attached (none = text only)"
          }
        }
      }
    },
    "total_found": {
      "type": "integer",
      "description": "Total number of messages matching query (may exceed limit)"
    },
    "search_time_ms": {
      "type": "integer",
      "description": "Server-side search duration in milliseconds"
    },
    "query_metadata": {
      "type": "object",
      "properties": {
        "query": { "type": "string" },
        "hours_back": { "type": "integer" },
        "channels_searched": { "type": "integer" },
        "language": { "type": "string", "description": "Search language filter (always 'ru' in V1)" }
      }
    }
  }
}
```

#### Response Example

```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "result": {
    "results": [
      {
        "message_id": 12345,
        "text": "Новая модель GPT-4o показала впечатляющие результаты в тестировании. Искусственный интеллект становится все более продвинутым...",
        "channel_name": "Habr - полезные статьи",
        "channel_username": "habrhabr",
        "channel_id": "1087968824",
        "timestamp": "2025-12-14T16:30:00Z",
        "message_link": "tg://resolve?channel=1087968824&post=12345&single",
        "sender_id": "0",
        "sender_name": "Habr",
        "has_media": true,
        "media_type": "photo"
      },
      {
        "message_id": 12344,
        "text": "Обсуждаем последние тренды в AI. Какие инструменты вы используете?",
        "channel_name": "AI Developers RU",
        "channel_username": "ai_dev_ru",
        "channel_id": "1234567890",
        "timestamp": "2025-12-14T15:45:00Z",
        "message_link": "tg://resolve?channel=1234567890&post=12344&single",
        "sender_id": "123456",
        "sender_name": "Ivan Petrov",
        "has_media": false,
        "media_type": "none"
      }
    ],
    "total_found": 247,
    "search_time_ms": 342,
    "query_metadata": {
      "query": "искусственный интеллект",
      "hours_back": 48,
      "channels_searched": 12,
      "language": "ru"
    }
  }
}
```

---

### TOOL 2: `get_channel_info`

**Purpose:** Retrieve metadata about a specific channel

#### Request Schema

```json
{
  "name": "get_channel_info",
  "description": "Get metadata about a Telegram channel",
  "inputSchema": {
    "type": "object",
    "properties": {
      "channel_identifier": {
        "type": "string",
        "description": "Channel username without @ (e.g., 'durov') OR numeric channel ID (e.g., '1087968824')"
      }
    },
    "required": ["channel_identifier"]
  }
}
```

#### Request Example

```json
{
  "jsonrpc": "2.0",
  "id": 20,
  "method": "tools/call",
  "params": {
    "name": "get_channel_info",
    "arguments": {
      "channel_identifier": "habrhabr"
    }
  }
}
```

#### Response Schema

```json
{
  "type": "object",
  "properties": {
    "channel_id": { "type": "string" },
    "channel_name": { "type": "string" },
    "channel_username": { "type": "string" },
    "description": { "type": "string" },
    "member_count": { "type": "integer" },
    "language": {
      "type": "string",
      "description": "ISO 639-1 code detected from description, e.g., 'ru'"
    },
    "is_verified": { "type": "boolean" },
    "is_public": { "type": "boolean" },
    "is_subscribed": { "type": "boolean" },
    "channel_link": {
      "type": "string",
      "description": "Full t.me URL"
    },
    "last_message_date": {
      "type": "string",
      "format": "date-time"
    },
    "profile_photo_url": { "type": "string" }
  }
}
```

#### Response Example

```json
{
  "jsonrpc": "2.0",
  "id": 20,
  "result": {
    "channel_id": "1087968824",
    "channel_name": "Habr - полезные статьи",
    "channel_username": "habrhabr",
    "description": "Лучшие статьи с Habr.com - технологии, программирование, стартапы",
    "member_count": 850000,
    "language": "ru",
    "is_verified": true,
    "is_public": true,
    "is_subscribed": true,
    "channel_link": "https://t.me/habrhabr",
    "last_message_date": "2025-12-14T16:45:00Z",
    "profile_photo_url": "https://cdn4.telegram-cdn.org/..."
  }
}
```

---

### TOOL 3: `generate_message_link`

**Purpose:** Create clickable links to open messages in Telegram Desktop

#### Request Schema

```json
{
  "name": "generate_message_link",
  "description": "Generate a Telegram message deep link that opens in Telegram Desktop on macOS",
  "inputSchema": {
    "type": "object",
    "properties": {
      "channel_id": {
        "type": "string",
        "description": "Numeric channel ID (required)"
      },
      "message_id": {
        "type": "integer",
        "description": "Message ID within channel (required)"
      },
      "include_tg_protocol": {
        "type": "boolean",
        "description": "Also return tg:// format for native macOS handling (default: true)",
        "default": true
      }
    },
    "required": ["channel_id", "message_id"]
  }
}
```

#### Request Example

```json
{
  "jsonrpc": "2.0",
  "id": 30,
  "method": "tools/call",
  "params": {
    "name": "generate_message_link",
    "arguments": {
      "channel_id": "1087968824",
      "message_id": 12345
    }
  }
}
```

#### Response Schema

```json
{
  "type": "object",
  "properties": {
    "channel_id": { "type": "string" },
    "message_id": { "type": "integer" },
    "https_link": {
      "type": "string",
      "description": "HTTPS link format: https://t.me/c/{channel_id}/{message_id}?single"
    },
    "tg_protocol_link": {
      "type": "string",
      "description": "Native macOS protocol link: tg://resolve?channel={channel_id}&post={message_id}&single"
    },
    "markdown_link": {
      "type": "string",
      "description": "Markdown-formatted link for Comet UI"
    },
    "html_link": {
      "type": "string",
      "description": "HTML href ready for browser"
    },
    "macos_handler_info": {
      "type": "object",
      "properties": {
        "handler": {
          "type": "string",
          "enum": ["tg://", "https://t.me"],
          "description": "Recommended handler for this platform"
        },
        "opens_with": {
          "type": "string",
          "description": "Application name"
        },
        "requires_app_installed": { "type": "boolean" }
      }
    }
  }
}
```

#### Response Example

```json
{
  "jsonrpc": "2.0",
  "id": 30,
  "result": {
    "channel_id": "1087968824",
    "message_id": 12345,
    "https_link": "https://t.me/c/1087968824/12345?single",
    "tg_protocol_link": "tg://resolve?channel=1087968824&post=12345&single",
    "markdown_link": "[View in Telegram](tg://resolve?channel=1087968824&post=12345&single)",
    "html_link": "<a href=\"tg://resolve?channel=1087968824&post=12345&single\">View in Telegram</a>",
    "macos_handler_info": {
      "handler": "tg://",
      "opens_with": "Telegram Desktop",
      "requires_app_installed": true
    }
  }
}
```

---

### TOOL 4: `open_message_in_telegram`

**Purpose:** Directly open messages in Telegram Desktop (optional convenience tool)

#### Request Schema

```json
{
  "name": "open_message_in_telegram",
  "description": "Programmatically open a Telegram message in Telegram Desktop on macOS",
  "inputSchema": {
    "type": "object",
    "properties": {
      "channel_id": {
        "type": "string",
        "description": "Numeric channel ID"
      },
      "message_id": {
        "type": "integer",
        "description": "Message ID"
      },
      "use_tg_protocol": {
        "type": "boolean",
        "description": "Use tg:// protocol (more reliable on macOS, default: true)",
        "default": true
      }
    },
    "required": ["channel_id", "message_id"]
  }
}
```

#### Response Schema

```json
{
  "type": "object",
  "properties": {
    "success": { "type": "boolean" },
    "message": { "type": "string" },
    "link_used": { "type": "string" },
    "app_opened": { "type": "boolean" },
    "error": { "type": "string" }
  }
}
```

#### Response Example

```json
{
  "jsonrpc": "2.0",
  "id": 40,
  "result": {
    "success": true,
    "message": "Message opened in Telegram Desktop",
    "link_used": "tg://resolve?channel=1087968824&post=12345&single",
    "app_opened": true
  }
}
```

---

### TOOL 5: `get_subscribed_channels`

**Purpose:** List all subscribed channels for discovery

#### Request Schema

```json
{
  "name": "get_subscribed_channels",
  "description": "List all subscribed channels",
  "inputSchema": {
    "type": "object",
    "properties": {
      "limit": {
        "type": "integer",
        "default": 50,
        "minimum": 1,
        "maximum": 500
      },
      "offset": {
        "type": "integer",
        "default": 0
      }
    }
  }
}
```

#### Response Example

```json
{
  "jsonrpc": "2.0",
  "id": 50,
  "result": {
    "channels": [
      {
        "channel_id": "1087968824",
        "channel_name": "Habr - полезные статьи",
        "channel_username": "habrhabr",
        "member_count": 850000,
        "language_detected": "ru"
      }
    ],
    "total": 42,
    "has_more": false
  }
}
```

---

### TOOL 6: `check_mcp_status`

**Purpose:** Health check and diagnostics

#### Request & Response Example

```json
{
  "jsonrpc": "2.0",
  "id": 60,
  "method": "tools/call",
  "params": {
    "name": "check_mcp_status",
    "arguments": {}
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 60,
  "result": {
    "telegram_connected": true,
    "user_id": "1234567890",
    "user_phone": "+1234567890",
    "session_valid": true,
    "rate_limiter_status": {
      "tokens_available": 45.5,
      "max_tokens": 50,
      "refill_rate": 2.0,
      "next_refill_seconds": 2.25
    },
    "config": {
      "search_language": "ru",
      "search_window_hours": 48,
      "api_version": "0.1.0"
    },
    "last_search_time": "2025-12-14T16:45:00Z",
    "total_searches_today": 23
  }
}
```

---

## Configuration Guide

### Configuration File Structure

**Location:** `~/.config/telegram-connector/config.toml`

```toml
# Telegram MCP Service Configuration
# Version: 1.0.0

# SECURITY: Sensitive credentials (api_hash, phone_number) are protected
# using the `secrecy` crate and will not be exposed in debug logs or error messages.

[telegram]
# Telegram API credentials from https://my.telegram.org
api_id = "YOUR_API_ID"              # Required: numeric ID
api_hash = "YOUR_API_HASH"          # Required: hex string (SENSITIVE)

# Phone number for user authentication
phone_number = "+1234567890"        # Required: include country code (SENSITIVE)

# Session persistence
# Note: The session file path itself is not sensitive, but the file contents are.
session_file = "~/.config/telegram-connector/session.bin"  # Will be auto-created

[search]
# Default search parameters
language = "ru"                      # Search language (currently: ru)
default_hours_back = 48             # Default search window (1-72 hours)
max_results_default = 20            # Default result limit
max_results_limit = 100             # Maximum allowed results

[rate_limiting]
# Adaptive token bucket configuration
max_tokens = 50                      # Burst capacity
refill_rate = 2.0                    # Tokens per second
# Strategy: Option B (Burst-Friendly)
# - Allows rapid burst queries (up to 50 consecutive)
# - Refills at 2 tokens/second
# - Respects Telegram's global limits (30 req/sec)

[logging]
# Structured logging configuration
level = "info"                       # Options: trace, debug, info, warn, error
format = "json"                      # Options: json, compact, pretty
# Log output
output = "stderr"                    # Options: stderr, stdout, or file path

[server]
# MCP server configuration
protocol_version = "2025-06-18"     # MCP protocol version
```

### First-Run Setup

**Step 1: Create config directory**
```bash
mkdir -p ~/.config/telegram-connector
```

**Step 2: Create initial config file**
```bash
cat > ~/.config/telegram-connector/config.toml << 'EOF'
# SECURITY: Sensitive credentials (api_hash, phone_number) are protected
# using the `secrecy` crate and will not be exposed in debug logs or error messages.

[telegram]
api_id = "YOUR_API_ID"
api_hash = "YOUR_API_HASH"
phone_number = "+1234567890"
session_file = "~/.config/telegram-connector/session.bin"

[search]
language = "ru"
default_hours_back = 48
max_results_default = 20
max_results_limit = 100

[rate_limiting]
max_tokens = 50
refill_rate = 2.0

[logging]
level = "info"
format = "json"
output = "stderr"

[server]
protocol_version = "2025-06-18"
EOF
```

**Step 3: Get Telegram API credentials**
- Visit https://my.telegram.org/apps
- Create an application (if not done yet)
- Copy `api_id` and `api_hash` to config

**Step 4: First run - 2FA flow**
```bash
./telegram-mcp-service
# Service will:
# 1. Read config
# 2. Prompt: "Enter phone number: +1234567890"
# 3. Connect to Telegram
# 4. Display: "Telegram sent a code. Enter it: "
# 5. Accept code
# 6. Save session to session.bin
# 7. Ready to receive requests from Comet
```

**Step 5: Configure in Comet**
Add to Comet's MCP server list:
```json
{
  "name": "telegram-connector",
  "command": "/path/to/telegram-mcp",
  "args": ["--config", "~/.config/telegram-connector/config.toml"]
}
```

### Environment Variables (Optional)

```bash
# Override config file location
export TELEGRAM_MCP_CONFIG=~/.config/telegram-connector/config.toml

# Override log level (useful for debugging)
export RUST_LOG=telegram_mcp_service=debug

# Telegram session directory
export TELEGRAM_SESSION_DIR=~/.config/telegram-connector/
```

---

## Error Handling Strategy

### Error Type Hierarchy

```rust
// thiserror-based custom errors
#[derive(Debug, thiserror::Error)]
pub enum TelegramMcpError {
    // Authentication errors
    #[error("Authentication failed: {0}")]
    AuthenticationError(String),
    
    #[error("Session expired. Please re-authenticate")]
    SessionExpired,
    
    #[error("Invalid 2FA code")]
    InvalidTwoFactor,
    
    // Telegram API errors
    #[error("Telegram API error: {code} - {message}")]
    TelegramApiError { code: i32, message: String },
    
    #[error("Channel not found: {0}")]
    ChannelNotFound(String),
    
    #[error("Message not found: {channel_id}/{message_id}")]
    MessageNotFound { channel_id: String, message_id: i32 },
    
    #[error("User not subscribed to channel: {0}")]
    NotSubscribed(String),
    
    // Rate limiting errors
    #[error("Rate limit exceeded. Retry after {retry_after_seconds} seconds")]
    RateLimited { retry_after_seconds: u64 },
    
    #[error("Telegram server flood. Retry after {retry_after_seconds} seconds")]
    FloodWait { retry_after_seconds: u64 },
    
    // Configuration errors
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    #[error("Invalid Telegram credentials")]
    InvalidCredentials,
    
    // Network errors
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("Connection timeout")]
    Timeout,
    
    // MCP protocol errors
    #[error("Invalid tool arguments: {0}")]
    InvalidArguments(String),
    
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    
    // Generic/unknown
    #[error("Internal error: {0}")]
    InternalError(String),
}

// anyhow for context propagation
pub type Result<T> = anyhow::Result<T, TelegramMcpError>;
```

### Error Handling in Tools

#### Pattern 1: Graceful Degradation (Search)
```rust
// If one channel fails, continue with others
async fn search_messages(query: &str) -> Result<SearchResponse> {
    let channels = get_subscribed_channels().await?;
    let mut results = Vec::new();
    
    for channel in channels {
        match search_in_channel(&channel, query).await {
            Ok(msgs) => results.extend(msgs),
            Err(e) => {
                tracing::warn!(
                    channel_id = %channel.id,
                    error = %e,
                    "Failed to search channel, continuing..."
                );
                // Don't fail entire search
            }
        }
    }
    
    Ok(SearchResponse {
        results,
        total_found: results.len(),
        search_time_ms: elapsed.as_millis() as u32,
    })
}
```

#### Pattern 2: Exponential Backoff (Rate Limits)
```rust
async fn call_with_backoff<F, T>(mut f: F) -> Result<T>
where
    F: FnMut() -> BoxFuture<'static, Result<T>>,
{
    let mut backoff = Duration::from_millis(100);
    let max_retries = 3;
    
    for attempt in 0..max_retries {
        match f().await {
            Ok(result) => return Ok(result),
            Err(TelegramMcpError::RateLimited { retry_after_seconds }) => {
                let wait = Duration::from_secs(retry_after_seconds);
                tracing::info!(
                    attempt,
                    wait_secs = wait.as_secs(),
                    "Rate limited, backing off..."
                );
                tokio::time::sleep(wait).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
    
    Err(anyhow::anyhow!("Max retries exceeded"))
}
```

#### Pattern 3: User-Friendly Error Messages
```rust
// MCP error response format
{
  "jsonrpc": "2.0",
  "id": 10,
  "error": {
    "code": -32603,  // Internal error
    "message": "Search failed",
    "data": {
      "error_type": "TelegramApiError",
      "details": "Telegram server error 400",
      "retry_after_seconds": null,
      "user_message": "Could not search channels. Please try again."
    }
  }
}
```

### Edge Cases Handled

| Edge Case | Handling Strategy |
|-----------|-------------------|
| **Deleted messages** | Return empty result, don't fail search |
| **Private channels** | Skip silently if not subscribed |
| **Restricted channels** | Attempt fetch; skip on 403 error |
| **Network timeout** | Retry with exponential backoff (max 3 attempts) |
| **Rate limit (429)** | Return error with `retry_after_seconds` hint |
| **Flood wait** | Hold connection, exponential backoff |
| **Invalid session** | Return `SessionExpired` error (manual re-auth required) |
| **Message not found** | Return zero results for that message |
| **Channel not found** | Return error; suggest checking channel_id format |
| **Search text too short** | Validate minimum 3 characters, return error |
| **Search results > 10k** | Paginate, return first batch with `has_more` flag |

---

## Project Structure

### Expected Directory Layout

```
telegram-connector/
├── Cargo.toml                    # Package manifest
├── Cargo.lock                    # Dependency lock (committed)
├── README.md                     # Quick start guide
│
├── src/
│   ├── lib.rs                    # Library root, public API exports
│   ├── main.rs                   # Binary: CLI entry point
│   │
│   ├── config.rs                 # Configuration loading & validation
│   ├── error.rs                  # Custom error types (thiserror)
│   ├── logging.rs                # Tracing subscriber setup
│   ├── rate_limiter.rs           # Token bucket implementation
│   ├── link.rs                   # Deep link generation utility
│   │
│   ├── mcp.rs                    # MCP module declaration
│   ├── mcp/
│   │   ├── server.rs             # rmcp server setup & initialization
│   │   └── tools.rs              # All 6 MCP tools (split when >300 lines)
│   │
│   ├── telegram.rs               # Telegram module declaration
│   └── telegram/
│       ├── client.rs             # Grammers client wrapper
│       ├── auth.rs               # Authentication & 2FA flow
│       └── types.rs              # Message, Channel, SearchResult
│
├── tests/
│   ├── mcp_integration.rs        # MCP server integration tests
│   └── telegram_mock.rs          # Telegram client mock tests
│
└── .github/
    └── workflows/
        └── ci.yml                # CI/CD pipeline
```

---

### Module System (Rust 2018+ Style)

**No `mod.rs` files.** Use file-as-module pattern:

```rust
// src/lib.rs - Library root
pub mod config;
pub mod error;
pub mod link;
pub mod logging;
pub mod mcp;
pub mod rate_limiter;
pub mod telegram;

// Re-exports for convenient access
pub use config::Config;
pub use error::Error;
```

```rust
// src/mcp.rs - Module declaration
pub mod server;
pub mod tools;

pub use server::McpServer;
```

```rust
// src/telegram.rs - Module declaration
pub mod auth;
pub mod client;
pub mod types;

pub use client::TelegramClient;
pub use types::{Channel, Message, SearchResult};
```

### File Responsibilities

| File | Responsibility | Lines (estimate) |
|------|----------------|------------------|
| `lib.rs` | Public API exports, module declarations | ~30 |
| `main.rs` | CLI entry, server startup, signal handling | ~50 |
| `config.rs` | TOML loading, defaults, validation | ~100 |
| `error.rs` | Error enum with thiserror | ~80 |
| `logging.rs` | Tracing subscriber initialization | ~40 |
| `rate_limiter.rs` | Token bucket algorithm | ~100 |
| `link.rs` | `tg://` and `https://t.me` link generation | ~60 |
| `mcp/server.rs` | rmcp server setup, tool registration | ~80 |
| `mcp/tools.rs` | 6 MCP tool implementations | ~250 |
| `telegram/client.rs` | Grammers wrapper, search, channel ops | ~200 |
| `telegram/auth.rs` | Phone auth, 2FA, session persistence | ~150 |
| `telegram/types.rs` | Data structures with serde | ~100 |

**Total estimated:** ~1,200 lines (excluding tests)

---

### Scaling Strategy

When a file exceeds ~300 lines:

1. **`mcp/tools.rs`** → Split into individual tool files:
   ```
   mcp/
   ├── server.rs
   ├── tools.rs              # Re-exports only
   └── tools/
       ├── search_messages.rs
       ├── get_channel_info.rs
       └── ...
   ```

2. **`telegram/client.rs`** → Extract to:
   ```
   telegram/
   ├── client.rs             # Core client
   ├── search.rs             # Search operations
   └── channel.rs            # Channel operations
   ```

**Rule:** Split only when complexity demands it, not preemptively.

---

## Implementation Checklist

### Phase 1: Foundation (Week 1)

- [ ] **Project Setup**
  - [ ] Initialize Cargo project: `cargo new telegram-connector --lib`
  - [ ] Add all dependencies to Cargo.toml (see Development Stack)
  - [ ] Create git repository
  - [ ] Set up GitHub Actions CI/CD

- [ ] **Configuration**
  - [ ] Implement `config.rs` with TOML parsing
  - [ ] Test config loading with example config.toml
  - [ ] Support XDG config paths (~/.config/telegram-connector/)

- [ ] **Logging**
  - [ ] Implement `logging.rs` with tracing
  - [ ] Add file logging with rotation (tracing-appender)
  - [ ] JSON log format support for file logs
  - [ ] Configurable log levels

- [ ] **Error Handling**
  - [ ] Define `error.rs` with thiserror enums
  - [ ] Write Display implementations
  - [ ] Create Result<T> type alias

### Phase 2: Telegram Integration (Week 2)

- [ ] **Authentication**
  - [ ] Implement `telegram/auth.rs`:
    - [ ] Phone number input prompt
    - [ ] 2FA code handling
    - [ ] Session persistence
  - [ ] Test with real Telegram account
  - [ ] Write TDD tests for auth flow

- [ ] **Telegram Client**
  - [ ] Wrap grammers client in `telegram/client.rs`
  - [ ] Implement session lifecycle
  - [ ] Error handling for API calls
  - [ ] Connection stability

- [ ] **Channel Operations**
  - [ ] Implement channel operations in `telegram/client.rs`:
    - [ ] List subscribed channels
    - [ ] Get channel metadata
    - [ ] Language detection

### Phase 3: Search & Rate Limiting (Week 3)

- [ ] **Rate Limiter**
  - [ ] Implement adaptive token bucket in `rate_limiter.rs`
  - [ ] Option B: burst-friendly (max 50 tokens, 2 refill/sec)
  - [ ] Write property-based tests (proptest)
  - [ ] Handle edge cases (concurrent requests, overflow)

- [ ] **Message Search**
  - [ ] Implement search in `telegram/client.rs`:
    - [ ] Full-text search across channels
    - [ ] Time-window filtering (hours_back)
    - [ ] Result ordering by recency
    - [ ] Pagination support
  - [ ] TDD: write tests before implementation

- [ ] **Link Generation**
  - [ ] Implement `link.rs`:
    - [ ] Generate tg:// protocol links
    - [ ] Generate t.me HTTPS links
    - [ ] macOS handler detection

### Phase 4: MCP Server (Week 4)

- [ ] **MCP Infrastructure**
  - [ ] Set up rmcp server in `mcp/server.rs`
  - [ ] Implement tool registration
  - [ ] JSON-RPC request/response handling
  - [ ] stdio transport setup

- [ ] **Tool Implementations**
  - [ ] `search_messages` tool
    - [ ] Argument validation
    - [ ] Rate limiter integration
    - [ ] Result serialization
  - [ ] `get_channel_info` tool
  - [ ] `generate_message_link` tool
  - [ ] `open_message_in_telegram` tool (macOS `open` command)
  - [ ] `get_subscribed_channels` tool
  - [ ] `check_mcp_status` tool

- [ ] **MCP Testing**
  - [ ] Integration tests with mock requests
  - [ ] Test all error paths
  - [ ] Verify JSON-RPC compliance

### Phase 5: Testing & Documentation (Week 5)

- [ ] **Test Suite**
  - [ ] Unit tests for all modules
  - [ ] Integration tests for MCP server
  - [ ] Property-based tests for search & rate limiting
  - [ ] Error handling tests
  - [ ] Target: >80% code coverage

- [ ] **Documentation**
  - [ ] README.md with quick start
  - [ ] API.md with all tool schemas
  - [ ] SETUP.md with step-by-step guide
  - [ ] TROUBLESHOOTING.md with common issues
  - [ ] DEVELOPMENT.md for contributors
  - [ ] Code comments for complex logic

- [ ] **Examples**
  - [ ] Basic search example
  - [ ] List channels example
  - [ ] Link generation example

### Phase 6: Integration & Polish (Week 6)

- [ ] **Comet Integration**
  - [ ] Package binary for distribution
  - [ ] Test with actual Comet browser
  - [ ] Verify link clicks work (tg:// protocol)

- [ ] **Performance**
  - [ ] Profile memory usage
  - [ ] Benchmark search performance
  - [ ] Optimize hot paths

- [ ] **Security**
  - [ ] Session token protection (file permissions)
  - [ ] Input validation on all tools
  - [ ] No secrets in logs

- [ ] **Release**
  - [ ] Create v0.1.0 release
  - [ ] GitHub releases with binaries
  - [ ] Update documentation

---

## Testing Strategy (TDD)

### Test Structure

**Golden Rule:** Write tests BEFORE implementation. Tests drive design.

### Test Categories

#### 1. Unit Tests (Modules)

**Location:** `tests/` directory, same filename as module with `_tests.rs` suffix

**Example: Rate Limiter Tests**
```rust
// tests/rate_limiter_tests.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tokio;
    use proptest::prelude::*;
    
    #[tokio::test]
    async fn test_token_bucket_initialization() {
        let limiter = TokenBucket::new(50, 2.0);
        assert_eq!(limiter.available_tokens(), 50.0);
    }
    
    #[tokio::test]
    async fn test_acquire_single_token() {
        let limiter = TokenBucket::new(50, 2.0);
        limiter.acquire(1).await.unwrap();
        assert_eq!(limiter.available_tokens(), 49.0);
    }
    
    #[tokio::test]
    async fn test_acquire_exceeds_capacity() {
        let limiter = TokenBucket::new(50, 2.0);
        let result = limiter.acquire(51).await;
        assert!(result.is_err());  // Should return wait duration
    }
    
    // Property-based test: verify invariants
    proptest! {
        #[test]
        fn prop_tokens_never_exceed_max(
            requests in prop::collection::vec(1..100usize, 0..100)
        ) {
            // Implementation verifies token count never exceeds max
        }
    }
}
```

#### 2. Integration Tests

**Example: MCP Server Tests**
```rust
// tests/integration_test.rs
#[tokio::test]
async fn test_search_messages_tool() {
    let server = setup_test_mcp_server().await;
    
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "search_messages",
            "arguments": {
                "query": "test",
                "limit": 10
            }
        }
    });
    
    let response = server.handle_request(request).await;
    assert!(response["result"]["results"].is_array());
    assert!(response["result"]["total_found"].is_number());
}

#[tokio::test]
async fn test_error_handling_invalid_tool() {
    let server = setup_test_mcp_server().await;
    
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "nonexistent_tool",
            "arguments": {}
        }
    });
    
    let response = server.handle_request(request).await;
    assert!(response["error"].is_object());
    assert_eq!(response["error"]["code"], -32603);  // Method not found
}
```

#### 3. End-to-End Tests

**Example: Full Search Flow**
```rust
// tests/search_tests.rs
#[tokio::test]
#[ignore]  // Run separately with real Telegram connection
async fn test_full_search_flow() {
    let config = load_test_config().await;
    let client = TelegramClient::new(&config).await.unwrap();
    
    // Search
    let results = client.search_messages("test", 48, 10).await.unwrap();
    
    // Verify results structure
    assert!(!results.is_empty());
    for result in &results {
        assert!(!result.message_id.is_empty());
        assert!(!result.text.is_empty());
        assert!(!result.channel_username.is_empty());
        assert!(!result.message_link.is_empty());
    }
}
```

### Running Tests

```bash
# All unit tests
cargo test --lib

# All tests including integration
cargo test

# Run specific test
cargo test search_messages

# Run with output
cargo test -- --nocapture

# Run ignored tests (E2E with real Telegram)
cargo test -- --ignored

# Coverage report (requires tarpaulin)
cargo tarpaulin --out Html

# Benchmark (if applicable)
cargo bench
```

### Test Data & Fixtures

**Location:** `tests/fixtures/`

```json
// tests/fixtures/mock_responses.json
{
  "search_response": {
    "results": [
      {
        "message_id": 12345,
        "text": "Test message",
        "channel_name": "Test Channel",
        "channel_username": "test_channel",
        "channel_id": "1234567890",
        "timestamp": "2025-12-14T16:30:00Z",
        "message_link": "tg://resolve?channel=1234567890&post=12345",
        "sender_id": "0",
        "sender_name": "Test",
        "has_media": false,
        "media_type": "none"
      }
    ],
    "total_found": 1,
    "search_time_ms": 100
  }
}
```

### Continuous Integration

**GitHub Actions:** `.github/workflows/ci.yml`
```yaml
name: CI

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --all
      - run: cargo test --doc
      - run: cargo build --release

  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install tarpaulin
      - run: cargo tarpaulin --out Xml
      - uses: codecov/codecov-action@v3
```

---

## Documentation Examples

### Example 1: Basic Search

**Scenario:** Claude asks to search for recent AI discussions

**Comet/Claude:**
```
"Search for recent discussions about artificial intelligence in Russian channels"
```

**MCP Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "search_messages",
    "arguments": {
      "query": "искусственный интеллект",
      "hours_back": 24,
      "limit": 20
    }
  }
}
```

**MCP Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "results": [
      {
        "message_id": 99887,
        "text": "Новая модель GPT-4o показала впечатляющие результаты...",
        "channel_name": "AI News RU",
        "channel_username": "ai_news_ru",
        "channel_id": "1234567890",
        "timestamp": "2025-12-14T15:30:00Z",
        "message_link": "tg://resolve?channel=1234567890&post=99887&single",
        "sender_id": "0",
        "sender_name": "AI News RU",
        "has_media": true,
        "media_type": "photo"
      }
    ],
    "total_found": 47,
    "search_time_ms": 312,
    "query_metadata": {
      "query": "искусственный интеллект",
      "hours_back": 24,
      "channels_searched": 8,
      "language": "ru"
    }
  }
}
```

**Claude's Output to User:**
> Found 47 messages about AI in Russian channels (searched 8 channels in last 24 hours):
> 
> 1. **AI News RU** - "Новая модель GPT-4o показала впечатляющие результаты..." 
>    [View message](tg://resolve?channel=1234567890&post=99887&single) • 3 hours ago

### Example 2: Get Channel Information

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "get_channel_info",
    "arguments": {
      "channel_identifier": "durov"
    }
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "channel_id": "1003365139",
    "channel_name": "Pavel Durov",
    "channel_username": "durov",
    "description": "Telegram founder and CEO. Posts about technology, privacy, and freedom.",
    "member_count": 12500000,
    "language": "en",
    "is_verified": true,
    "is_public": true,
    "is_subscribed": true,
    "channel_link": "https://t.me/durov",
    "last_message_date": "2025-12-14T10:00:00Z",
    "profile_photo_url": "https://cdn4.telegram-cdn.org/..."
  }
}
```

### Example 3: Generate & Click Link

**Request:** (programmatic or via Claude)
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tools/call",
  "params": {
    "name": "generate_message_link",
    "arguments": {
      "channel_id": "1234567890",
      "message_id": 99887
    }
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "channel_id": "1234567890",
    "message_id": 99887,
    "https_link": "https://t.me/c/1234567890/99887?single",
    "tg_protocol_link": "tg://resolve?channel=1234567890&post=99887&single",
    "markdown_link": "[Open in Telegram](tg://resolve?channel=1234567890&post=99887&single)",
    "html_link": "<a href=\"tg://resolve?channel=1234567890&post=99887&single\">Open in Telegram</a>",
    "macos_handler_info": {
      "handler": "tg://",
      "opens_with": "Telegram Desktop",
      "requires_app_installed": true
    }
  }
}
```

**User clicks link:**
- macOS recognizes `tg://` protocol
- Launches Telegram Desktop
- Navigates to channel #1234567890, message #99887
- Message displayed in full context

### Example 4: Check MCP Health

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "tools/call",
  "params": {
    "name": "check_mcp_status",
    "arguments": {}
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "result": {
    "telegram_connected": true,
    "user_id": "1234567890",
    "user_phone": "+1234567890",
    "session_valid": true,
    "rate_limiter_status": {
      "tokens_available": 42.5,
      "max_tokens": 50,
      "refill_rate": 2.0,
      "next_refill_seconds": 1.25
    },
    "config": {
      "search_language": "ru",
      "search_window_hours": 48,
      "api_version": "0.1.0"
    },
    "last_search_time": "2025-12-14T16:45:00Z",
    "total_searches_today": 28
  }
}
```

---

## Future Roadmap

### V1.1 (Small Improvements)

- [ ] Better error messages in Comet UI
- [ ] Search result pagination
- [ ] Channel metadata caching (optional)
- [ ] Detailed logs in ~/.config/telegram-connector/logs/

### V2.0 (Major Features)

- [ ] **Channel Discovery**
  - [ ] `search_channels` tool for finding new channels
  - [ ] Channel recommendations based on subscribed channels
  - [ ] Trending channels by topic

- [ ] **Media Filtering**
  - [ ] Filter search by file type: documents, images, videos, audio
  - [ ] Media metadata extraction (size, duration, format)
  - [ ] Link to media downloads

- [ ] **Advanced Search**
  - [ ] Filter by sender/author
  - [ ] Filter by message reactions/engagement
  - [ ] Saved searches & subscriptions (alert on new matches)

- [ ] **Multi-Language Support**
  - [ ] Add English, Chinese, Spanish, etc.
  - [ ] Language auto-detection
  - [ ] Per-channel language override

- [ ] **Data Export**
  - [ ] Export search results to JSON, CSV, PDF
  - [ ] Archive conversations

- [ ] **Multi-Account Support**
  - [ ] Multiple Telegram accounts in one MCP
  - [ ] Account switching in requests

- [ ] **Session Management**
  - [ ] Automatic re-authentication on session expiry
  - [ ] Session refresh without 2FA code

### V3.0+ (Enterprise Features)

- [ ] Hosted MCP service (cloud deployment)
- [ ] Database caching for faster searches
- [ ] Full-text search indexing
- [ ] Real-time channel monitoring & webhooks
- [ ] Multi-language NLP search (semantic search)
- [ ] Analytics & insights dashboard
- [ ] Team collaboration features

---

## Appendix: JSON Schema Definition File

**File:** `schemas/mcp-tools.json`

This file contains complete MCP schema definitions in OpenAPI-compatible format, ready for import by MCP clients:

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "Telegram MCP Service Tools",
  "version": "1.0.0",
  "tools": [
    {
      "name": "search_messages",
      "description": "Search for messages in subscribed Russian Telegram channels",
      "inputSchema": {
        "type": "object",
        "properties": {
          "query": {
            "type": "string",
            "description": "Search text in Russian"
          },
          "channel_id": {
            "type": "string",
            "description": "Optional: Specific channel to search"
          },
          "hours_back": {
            "type": "integer",
            "minimum": 1,
            "maximum": 72,
            "default": 48
          },
          "limit": {
            "type": "integer",
            "minimum": 1,
            "maximum": 100,
            "default": 20
          }
        },
        "required": ["query"]
      }
    },
    {
      "name": "get_channel_info",
      "description": "Get metadata about a channel",
      "inputSchema": {
        "type": "object",
        "properties": {
          "channel_identifier": {
            "type": "string"
          }
        },
        "required": ["channel_identifier"]
      }
    },
    {
      "name": "generate_message_link",
      "description": "Generate deep link to open message in Telegram Desktop",
      "inputSchema": {
        "type": "object",
        "properties": {
          "channel_id": { "type": "string" },
          "message_id": { "type": "integer" },
          "include_tg_protocol": { "type": "boolean", "default": true }
        },
        "required": ["channel_id", "message_id"]
      }
    },
    {
      "name": "open_message_in_telegram",
      "description": "Open message directly in Telegram Desktop",
      "inputSchema": {
        "type": "object",
        "properties": {
          "channel_id": { "type": "string" },
          "message_id": { "type": "integer" },
          "use_tg_protocol": { "type": "boolean", "default": true }
        },
        "required": ["channel_id", "message_id"]
      }
    },
    {
      "name": "get_subscribed_channels",
      "description": "List subscribed channels",
      "inputSchema": {
        "type": "object",
        "properties": {
          "limit": { "type": "integer", "default": 50, "maximum": 500 },
          "offset": { "type": "integer", "default": 0 }
        }
      }
    },
    {
      "name": "check_mcp_status",
      "description": "Health check and diagnostics",
      "inputSchema": { "type": "object" }
    }
  ]
}
```

---

## Glossary

- **MCP:** Model Context Protocol - standardized protocol for LLM integration
- **rmcp:** Official Rust SDK for MCP (Model Context Protocol)
- **grammers:** Rust library for Telegram MTProto protocol
- **MTProto:** Telegram's encrypted protocol layer
- **2FA:** Two-Factor Authentication
- **tg://:** macOS URL scheme for Telegram Desktop
- **Token Bucket:** Rate limiting algorithm (adaptive in V1)
- **JSON-RPC:** JSON Remote Procedure Call (protocol used by MCP)
- **TDD:** Test-Driven Development (tests before code)
- **E2E:** End-to-End testing

---

## Support & Contributing

For questions, issues, or contributions:
- GitHub Issues: Report bugs and feature requests
- Pull Requests: Contribute improvements
- Documentation: Help improve guides and examples

---

**Document Version:** 1.0.0  
**Last Updated:** December 14, 2025  
**Status:** Ready for Implementation
