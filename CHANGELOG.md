# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
