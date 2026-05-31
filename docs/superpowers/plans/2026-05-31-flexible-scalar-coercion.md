# Flexible Scalar Coercion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every cross-type scalar field on the MCP request structs accept the alternate JSON form (number↔string↔bool), so messy clients stop getting rejected before tool code runs.

**Architecture:** Add five reusable `serde` `deserialize_with` functions in `src/mcp/tools/types/serde_helpers.rs` (the file that already hosts `deserialize_optional_media_filter`). Apply them via `#[serde(...)]` attributes on the request fields. Field types and the derived `JsonSchema` stay unchanged; tool bodies and the domain layer are untouched. Leniency is a deserialization anti-corruption layer at the transport boundary.

**Tech Stack:** Rust (2024 edition, nightly), `serde` + `serde_json` (untagged enums), `schemars` v1, `rmcp` v1.6.

> **Repo rule (CLAUDE.md):** NEVER create git commits — the user manages all git. Each task therefore ends with the pre-commit quality gate, **not** a `git commit`. After every code change run `cargo fmt --all`. Staging/committing is the user's call.

> **TDD note for Rust:** A test that references a not-yet-defined function fails by *not compiling* — that compile error is the "red" state. Implement the function, and the test goes "green". This is the standard Rust TDD red→green.

---

## File Structure

| File | Change | Responsibility |
|------|--------|----------------|
| `src/mcp/tools/types/serde_helpers.rs` | Modify | Add 5 `flexible_*` deserializers + unit tests. One import line added. |
| `src/mcp/tools/types/requests.rs` | Modify | Annotate fields with `deserialize_with`; extend struct-level tests. |
| `src/mcp/server.rs` | **No change** | Tool bodies already read `request.field` at unchanged types. |
| `src/telegram/types/params.rs` | **No change** | Domain layer stays strict. |

All five deserializers use the same shape: deserialize through a small
`#[serde(untagged)]` enum, then coerce. Required fields use
`#[serde(deserialize_with = "…")]` (stays required); optional fields use
`#[serde(default, deserialize_with = "…")]` (missing → `None`).

---

## Task 1: `flexible_opt_u32` deserializer

**Files:**
- Modify: `src/mcp/tools/types/serde_helpers.rs`

- [ ] **Step 1: Add the `serde::de::Error` import**

At the top of `src/mcp/tools/types/serde_helpers.rs`, change the imports to:

```rust
//! Custom serde deserializers for MCP tool types.

use crate::telegram::types::MediaFilter;
use serde::de::Error;
use serde::{Deserialize, Deserializer};
```

- [ ] **Step 2: Write the failing tests**

In the `#[cfg(test)] mod tests` block at the bottom of the file, add a reusable
test struct and tests (place after the existing `use super::*;` line):

```rust
#[derive(Deserialize)]
struct OptU32T {
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    v: Option<u32>,
}

#[test]
fn opt_u32_accepts_number() {
    let t: OptU32T = serde_json::from_str(r#"{"v": 10}"#).unwrap();
    assert_eq!(t.v, Some(10));
}

#[test]
fn opt_u32_accepts_numeric_string() {
    let t: OptU32T = serde_json::from_str(r#"{"v": "10"}"#).unwrap();
    assert_eq!(t.v, Some(10));
}

#[test]
fn opt_u32_trims_whitespace() {
    let t: OptU32T = serde_json::from_str(r#"{"v": " 10 "}"#).unwrap();
    assert_eq!(t.v, Some(10));
}

#[test]
fn opt_u32_empty_string_is_none() {
    let t: OptU32T = serde_json::from_str(r#"{"v": ""}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_u32_missing_is_none() {
    let t: OptU32T = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_u32_null_is_none() {
    let t: OptU32T = serde_json::from_str(r#"{"v": null}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_u32_float_string_errors() {
    assert!(serde_json::from_str::<OptU32T>(r#"{"v": "1.5"}"#).is_err());
}

#[test]
fn opt_u32_garbage_string_errors() {
    assert!(serde_json::from_str::<OptU32T>(r#"{"v": "abc"}"#).is_err());
}

#[test]
fn opt_u32_negative_number_errors() {
    assert!(serde_json::from_str::<OptU32T>(r#"{"v": -5}"#).is_err());
}

#[test]
fn opt_u32_float_number_errors() {
    assert!(serde_json::from_str::<OptU32T>(r#"{"v": 10.0}"#).is_err());
}
```

- [ ] **Step 3: Run tests to verify they fail (do not compile)**

Run: `cargo test --lib opt_u32`
Expected: compilation error — `cannot find function 'flexible_opt_u32' in this scope`.

- [ ] **Step 4: Write the implementation**

Add this function to `serde_helpers.rs` (above the `#[cfg(test)]` block):

```rust
/// Deserialize `Option<u32>` accepting either a JSON number or a numeric string.
///
/// The string form is trimmed before parsing. An empty/whitespace string or a
/// JSON `null` becomes `None`. Floats, negatives, out-of-range, and non-numeric
/// values produce an error.
pub fn flexible_opt_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(u32),
        Str(String),
    }

    match Option::<NumOrStr>::deserialize(deserializer)? {
        None => Ok(None),
        Some(NumOrStr::Num(n)) => Ok(Some(n)),
        Some(NumOrStr::Str(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed
                .parse::<u32>()
                .map(Some)
                .map_err(|_| Error::custom(format!("expected an integer, got '{}'", s)))
        }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib opt_u32`
Expected: all 10 `opt_u32_*` tests PASS.

- [ ] **Step 6: Quality gate**

```bash
cargo fmt --all
cargo clippy -- -D warnings
```
Expected: no warnings, no errors.

---

## Task 2: `flexible_i64` deserializer

**Files:**
- Modify: `src/mcp/tools/types/serde_helpers.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block:

```rust
#[derive(Deserialize)]
struct I64T {
    #[serde(deserialize_with = "flexible_i64")]
    v: i64,
}

#[test]
fn i64_accepts_number() {
    let t: I64T = serde_json::from_str(r#"{"v": 575403}"#).unwrap();
    assert_eq!(t.v, 575403);
}

#[test]
fn i64_accepts_numeric_string() {
    let t: I64T = serde_json::from_str(r#"{"v": "575403"}"#).unwrap();
    assert_eq!(t.v, 575403);
}

#[test]
fn i64_accepts_negative_string() {
    let t: I64T = serde_json::from_str(r#"{"v": " -42 "}"#).unwrap();
    assert_eq!(t.v, -42);
}

#[test]
fn i64_empty_string_errors() {
    assert!(serde_json::from_str::<I64T>(r#"{"v": ""}"#).is_err());
}

#[test]
fn i64_garbage_errors() {
    assert!(serde_json::from_str::<I64T>(r#"{"v": "abc"}"#).is_err());
}

#[test]
fn i64_missing_errors() {
    assert!(serde_json::from_str::<I64T>(r#"{}"#).is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail (do not compile)**

Run: `cargo test --lib i64_`
Expected: compilation error — `cannot find function 'flexible_i64'`.

- [ ] **Step 3: Write the implementation**

Add to `serde_helpers.rs`:

```rust
/// Deserialize `i64` accepting either a JSON number or a numeric string.
///
/// The string form is trimmed before parsing. Empty, non-numeric, or float
/// values produce an error.
pub fn flexible_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(i64),
        Str(String),
    }

    match NumOrStr::deserialize(deserializer)? {
        NumOrStr::Num(n) => Ok(n),
        NumOrStr::Str(s) => s
            .trim()
            .parse::<i64>()
            .map_err(|_| Error::custom(format!("expected an integer, got '{}'", s))),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib i64_`
Expected: all 6 `i64_*` tests PASS.

- [ ] **Step 5: Quality gate**

```bash
cargo fmt --all
cargo clippy -- -D warnings
```
Expected: clean.

---

## Task 3: `flexible_string` deserializer

**Files:**
- Modify: `src/mcp/tools/types/serde_helpers.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block:

```rust
#[derive(Deserialize)]
struct StrT {
    #[serde(deserialize_with = "flexible_string")]
    v: String,
}

#[test]
fn string_accepts_string() {
    let t: StrT = serde_json::from_str(r#"{"v": "swodki"}"#).unwrap();
    assert_eq!(t.v, "swodki");
}

#[test]
fn string_accepts_integer_number() {
    let t: StrT = serde_json::from_str(r#"{"v": 123}"#).unwrap();
    assert_eq!(t.v, "123");
}

#[test]
fn string_accepts_negative_integer() {
    let t: StrT = serde_json::from_str(r#"{"v": -1001234}"#).unwrap();
    assert_eq!(t.v, "-1001234");
}

#[test]
fn string_passes_empty_through() {
    let t: StrT = serde_json::from_str(r#"{"v": ""}"#).unwrap();
    assert_eq!(t.v, "");
}

#[test]
fn string_float_number_errors() {
    assert!(serde_json::from_str::<StrT>(r#"{"v": 1.5}"#).is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail (do not compile)**

Run: `cargo test --lib string_`
Expected: compilation error — `cannot find function 'flexible_string'`.

- [ ] **Step 3: Write the implementation**

Add to `serde_helpers.rs`:

```rust
/// Deserialize `String` accepting either a JSON string or an integer JSON number.
///
/// Integer numbers are stringified (`123` -> `"123"`). Float numbers error.
pub fn flexible_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StrOrInt {
        Str(String),
        Int(i64),
    }

    match StrOrInt::deserialize(deserializer)? {
        StrOrInt::Str(s) => Ok(s),
        StrOrInt::Int(n) => Ok(n.to_string()),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib string_`
Expected: all 5 `string_*` tests PASS.

- [ ] **Step 5: Quality gate**

```bash
cargo fmt --all
cargo clippy -- -D warnings
```
Expected: clean.

---

## Task 4: `flexible_opt_string` deserializer

**Files:**
- Modify: `src/mcp/tools/types/serde_helpers.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block:

```rust
#[derive(Deserialize)]
struct OptStrT {
    #[serde(default, deserialize_with = "flexible_opt_string")]
    v: Option<String>,
}

#[test]
fn opt_string_accepts_string() {
    let t: OptStrT = serde_json::from_str(r#"{"v": "tech_news"}"#).unwrap();
    assert_eq!(t.v, Some("tech_news".to_string()));
}

#[test]
fn opt_string_accepts_integer_number() {
    let t: OptStrT = serde_json::from_str(r#"{"v": 123}"#).unwrap();
    assert_eq!(t.v, Some("123".to_string()));
}

#[test]
fn opt_string_empty_is_none() {
    let t: OptStrT = serde_json::from_str(r#"{"v": ""}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_string_missing_is_none() {
    let t: OptStrT = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_string_null_is_none() {
    let t: OptStrT = serde_json::from_str(r#"{"v": null}"#).unwrap();
    assert_eq!(t.v, None);
}
```

- [ ] **Step 2: Run tests to verify they fail (do not compile)**

Run: `cargo test --lib opt_string_`
Expected: compilation error — `cannot find function 'flexible_opt_string'`.

- [ ] **Step 3: Write the implementation**

Add to `serde_helpers.rs`:

```rust
/// Deserialize `Option<String>` accepting a JSON string or an integer number.
///
/// Integer numbers are stringified. An empty/whitespace string or JSON `null`
/// becomes `None`. Float numbers error.
pub fn flexible_opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StrOrInt {
        Str(String),
        Int(i64),
    }

    match Option::<StrOrInt>::deserialize(deserializer)? {
        None => Ok(None),
        Some(StrOrInt::Str(s)) if s.trim().is_empty() => Ok(None),
        Some(StrOrInt::Str(s)) => Ok(Some(s)),
        Some(StrOrInt::Int(n)) => Ok(Some(n.to_string())),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib opt_string_`
Expected: all 5 `opt_string_*` tests PASS.

- [ ] **Step 5: Quality gate**

```bash
cargo fmt --all
cargo clippy -- -D warnings
```
Expected: clean.

---

## Task 5: `flexible_opt_bool` deserializer

**Files:**
- Modify: `src/mcp/tools/types/serde_helpers.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block:

```rust
#[derive(Deserialize)]
struct OptBoolT {
    #[serde(default, deserialize_with = "flexible_opt_bool")]
    v: Option<bool>,
}

#[test]
fn opt_bool_accepts_bool() {
    let t: OptBoolT = serde_json::from_str(r#"{"v": true}"#).unwrap();
    assert_eq!(t.v, Some(true));
}

#[test]
fn opt_bool_accepts_true_string() {
    let t: OptBoolT = serde_json::from_str(r#"{"v": "true"}"#).unwrap();
    assert_eq!(t.v, Some(true));
}

#[test]
fn opt_bool_accepts_false_string_case_insensitive() {
    let t: OptBoolT = serde_json::from_str(r#"{"v": "FALSE"}"#).unwrap();
    assert_eq!(t.v, Some(false));
}

#[test]
fn opt_bool_accepts_numeric_one_and_zero() {
    let one: OptBoolT = serde_json::from_str(r#"{"v": 1}"#).unwrap();
    let zero: OptBoolT = serde_json::from_str(r#"{"v": 0}"#).unwrap();
    assert_eq!(one.v, Some(true));
    assert_eq!(zero.v, Some(false));
}

#[test]
fn opt_bool_accepts_string_one_and_zero() {
    let one: OptBoolT = serde_json::from_str(r#"{"v": "1"}"#).unwrap();
    let zero: OptBoolT = serde_json::from_str(r#"{"v": "0"}"#).unwrap();
    assert_eq!(one.v, Some(true));
    assert_eq!(zero.v, Some(false));
}

#[test]
fn opt_bool_empty_is_none() {
    let t: OptBoolT = serde_json::from_str(r#"{"v": ""}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_bool_missing_is_none() {
    let t: OptBoolT = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(t.v, None);
}

#[test]
fn opt_bool_invalid_number_errors() {
    assert!(serde_json::from_str::<OptBoolT>(r#"{"v": 2}"#).is_err());
}

#[test]
fn opt_bool_invalid_string_errors() {
    assert!(serde_json::from_str::<OptBoolT>(r#"{"v": "yes"}"#).is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail (do not compile)**

Run: `cargo test --lib opt_bool_`
Expected: compilation error — `cannot find function 'flexible_opt_bool'`.

- [ ] **Step 3: Write the implementation**

Add to `serde_helpers.rs`:

```rust
/// Deserialize `Option<bool>` accepting a JSON bool, the numbers `0`/`1`, or the
/// strings `"true"`/`"false"`/`"1"`/`"0"` (case-insensitive, trimmed).
///
/// An empty/whitespace string or JSON `null` becomes `None`. Anything else errors.
pub fn flexible_opt_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BoolOrIntOrStr {
        Bool(bool),
        Int(i64),
        Str(String),
    }

    match Option::<BoolOrIntOrStr>::deserialize(deserializer)? {
        None => Ok(None),
        Some(BoolOrIntOrStr::Bool(b)) => Ok(Some(b)),
        Some(BoolOrIntOrStr::Int(n)) => match n {
            0 => Ok(Some(false)),
            1 => Ok(Some(true)),
            other => Err(Error::custom(format!("expected a boolean, got '{}'", other))),
        },
        Some(BoolOrIntOrStr::Str(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            match trimmed.to_ascii_lowercase().as_str() {
                "true" | "1" => Ok(Some(true)),
                "false" | "0" => Ok(Some(false)),
                _ => Err(Error::custom(format!("expected a boolean, got '{}'", s))),
            }
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib opt_bool_`
Expected: all 9 `opt_bool_*` tests PASS.

- [ ] **Step 5: Quality gate**

```bash
cargo fmt --all
cargo clippy -- -D warnings
```
Expected: clean.

---

## Task 6: Wire the deserializers into the request structs

**Files:**
- Modify: `src/mcp/tools/types/requests.rs`

- [ ] **Step 1: Write the failing struct-level tests**

Add these tests inside the existing `#[cfg(test)] mod tests` block in `requests.rs`:

```rust
#[test]
fn get_channels_request_accepts_string_numbers() {
    let json = r#"{"limit": "10", "offset": "5"}"#;
    let request: GetChannelsRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.limit, Some(10));
    assert_eq!(request.offset, Some(5));
}

#[test]
fn get_channels_request_empty_string_limit_is_none() {
    let json = r#"{"limit": ""}"#;
    let request: GetChannelsRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.limit, None);
}

#[test]
fn search_request_accepts_string_numbers() {
    let json = r#"{"query": "ai", "hours_back": "72", "limit": "50"}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.hours_back, Some(72));
    assert_eq!(request.limit, Some(50));
}

#[test]
fn search_request_channel_id_accepts_number() {
    let json = r#"{"query": "ai", "channel_id": 123456}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.channel_id, Some("123456".to_string()));
}

#[test]
fn search_request_query_accepts_number() {
    let json = r#"{"query": 42}"#;
    let request: SearchRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.query, "42");
}

#[test]
fn generate_link_request_message_id_accepts_string() {
    let json = r#"{"channel_id": "123", "message_id": "575403"}"#;
    let request: GenerateLinkRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.channel_id, "123");
    assert_eq!(request.message_id, 575403);
}

#[test]
fn generate_link_request_channel_id_accepts_number() {
    let json = r#"{"channel_id": 456, "message_id": 1}"#;
    let request: GenerateLinkRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.channel_id, "456");
}

#[test]
fn open_message_request_bool_accepts_string() {
    let json = r#"{"channel_id": "1", "message_id": "2", "use_tg_protocol": "false"}"#;
    let request: OpenMessageRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.use_tg_protocol, Some(false));
}

#[test]
fn get_recent_messages_request_channel_id_accepts_number() {
    let json = r#"{"channel_id": 123456}"#;
    let request: GetRecentMessagesRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.channel_id, "123456");
}

#[test]
fn get_message_by_link_request_link_accepts_number() {
    let json = r#"{"link": 575403}"#;
    let request: GetMessageByLinkRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.link, "575403");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib requests`
Expected: the new `*_accepts_*` tests FAIL — e.g. `get_channels_request_accepts_string_numbers` fails to deserialize `"10"` into `Option<u32>` (invalid type: string). The pre-existing `requests` tests still pass.

- [ ] **Step 3: Update the import line**

At the top of `requests.rs`, replace:

```rust
use super::serde_helpers::deserialize_optional_media_filter;
```

with:

```rust
use super::serde_helpers::{
    deserialize_optional_media_filter, flexible_i64, flexible_opt_bool, flexible_opt_string,
    flexible_opt_u32, flexible_string,
};
```

- [ ] **Step 4: Annotate the fields**

Apply the `#[serde(...)]` attributes below. Each `#[schemars(description = ...)]`
already present stays; add the `#[serde(...)]` line directly beneath it. The full
target field definitions:

`GetChannelsRequest`:
```rust
    #[schemars(description = "Maximum number of channels to return (default: 50, max: 500)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub limit: Option<u32>,

    #[schemars(description = "Offset for pagination (default: 0)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub offset: Option<u32>,
```

`GetChannelInfoRequest`:
```rust
    #[schemars(description = "Channel username (@channel) or numeric ID")]
    #[serde(deserialize_with = "flexible_string")]
    pub channel_identifier: String,
```

`GenerateLinkRequest`:
```rust
    #[schemars(description = "Numeric channel ID")]
    #[serde(deserialize_with = "flexible_string")]
    pub channel_id: String,

    #[schemars(description = "Message ID within the channel")]
    #[serde(deserialize_with = "flexible_i64")]
    pub message_id: i64,

    #[schemars(description = "Also return tg:// protocol link (default: true)")]
    #[serde(default, deserialize_with = "flexible_opt_bool")]
    pub include_tg_protocol: Option<bool>,
```

`OpenMessageRequest`:
```rust
    #[schemars(description = "Numeric channel ID")]
    #[serde(deserialize_with = "flexible_string")]
    pub channel_id: String,

    #[schemars(description = "Message ID within the channel")]
    #[serde(deserialize_with = "flexible_i64")]
    pub message_id: i64,

    #[schemars(description = "Use tg:// protocol (default: true). If false, uses https")]
    #[serde(default, deserialize_with = "flexible_opt_bool")]
    pub use_tg_protocol: Option<bool>,
```

`SearchRequest` (leave `media_filter` exactly as it is):
```rust
    #[schemars(
        description = "Search query. Required unless media_filter is set. Can be empty when filtering by media type only."
    )]
    #[serde(deserialize_with = "flexible_string")]
    pub query: String,

    #[schemars(description = "Optional: Filter by specific channel ID")]
    #[serde(default, deserialize_with = "flexible_opt_string")]
    pub channel_id: Option<String>,

    #[schemars(description = "How many hours back to search (default: 48, max: 168)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub hours_back: Option<u32>,

    #[schemars(description = "Maximum results to return (default: 20, max: 100)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub limit: Option<u32>,
```

`GetRecentMessagesRequest` (leave `media_filter` exactly as it is):
```rust
    #[schemars(description = "Channel ID or username (required)")]
    #[serde(deserialize_with = "flexible_string")]
    pub channel_id: String,

    #[schemars(description = "Hours of history to retrieve (default: 48, max: 168)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub hours_back: Option<u32>,

    #[schemars(description = "Maximum messages to return (default: 20, max: 100)")]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub limit: Option<u32>,
```

`GetMessageByLinkRequest`:
```rust
    #[schemars(
        description = "Telegram message link. Supported formats: https://t.me/username/12345, https://t.me/c/channel_id/12345, t.me/username/12345"
    )]
    #[serde(deserialize_with = "flexible_string")]
    pub link: String,
```

> **Note on `SearchRequest`'s `#[derive(... Default)]`:** adding `deserialize_with`
> does not affect the derived `Default` impl. Leave the derive list unchanged.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib requests`
Expected: all `requests` tests PASS, including both the new `*_accepts_*` tests
and every pre-existing test (which use the plain number/string forms).

- [ ] **Step 6: Quality gate**

```bash
cargo fmt --all
cargo clippy -- -D warnings
```
Expected: clean.

---

## Task 7: Full regression + schema sanity check

**Files:**
- None (verification only)

- [ ] **Step 1: Run the serde_helpers + requests suites together**

Run: `cargo test --lib serde_helpers requests`
Expected: all deserializer unit tests and all request struct tests PASS.

- [ ] **Step 2: Run the full pre-commit gate**

Run:
```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test -- --test-threads=1
```
Expected: formatting clean, no clippy warnings, all tests pass. (Using
`--test-threads=1` keeps the env-var-mutating config tests serial, per CLAUDE.md.)

- [ ] **Step 3: Confirm the advertised schema is unchanged**

The `JsonSchema` derive reads field *types*, not `deserialize_with`. Numeric
fields must still advertise `integer`, string fields `string`, bool fields
`boolean`. Spot-check by searching the generated tool schemas — confirm no field
turned into a `string`-typed integer or a union:

Run: `cargo test --lib mcp`
Expected: all MCP tool tests PASS (these exercise the tool round-trip and would
break if a schema/param type regressed).

- [ ] **Step 4: Hand off**

Report results to the user. Per CLAUDE.md, do **not** commit — the user stages
and commits. Update `docs/tasklist.md` / `docs/memory.md` per the project
workflow if directed.

---

## Self-Review (completed during planning)

- **Spec coverage:** All five deserializers (Tasks 1–5) and every field in the
  spec's application map (Task 6) are covered. Pragmatic leniency semantics
  (trim, empty→None for optional, empty→error for required, bool string/number
  forms, number→string for String fields, float→error) each have a dedicated
  test.
- **Placeholder scan:** No TBD/TODO; every code step contains complete code.
- **Type consistency:** Function names (`flexible_opt_u32`, `flexible_i64`,
  `flexible_string`, `flexible_opt_string`, `flexible_opt_bool`) and signatures
  are identical across the definition tasks, the import line, and the field
  annotations. `Error::custom` relies on the `use serde::de::Error;` added in
  Task 1 Step 1.
