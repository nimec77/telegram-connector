# Design: Flexible Scalar Coercion at the MCP Request Boundary

## Summary

Some MCP clients send scalar arguments in the "wrong" JSON type — a numeric
string `"10"` where we expect an integer, a JSON number `123` where we expect a
string, or `"true"`/`1` where we expect a boolean. Today the request structs in
`src/mcp/tools/types/requests.rs` deserialize strictly, so serde rejects these
payloads *before* any tool code runs and the client gets an opaque
invalid-params error.

This change makes every cross-type scalar field on the request structs tolerant
of the alternate JSON form, while keeping the advertised JSON Schema and the
entire domain layer unchanged.

## Approach

**Boundary deserializers (selected).** Add a small set of reusable
`serde` `deserialize_with` functions and apply them via `#[serde(...)]`
attributes on the request fields. This is the same technique already used by
`deserialize_optional_media_filter` in
`src/mcp/tools/types/serde_helpers.rs`.

Field *types* stay exactly as they are (`Option<u32>`, `i64`, `String`,
`Option<bool>`), which means:

- `#[derive(JsonSchema)]` keeps advertising the correct types to Claude — we
  *tolerate* strings/numbers without *inviting* them.
- Tool bodies in `server.rs` and the domain layer (`params.rs`, `ChannelId`,
  `parse_channel_id`, `parse_message_id`) are **untouched**.

This is a deserialization anti-corruption layer: leniency is localized at the
transport boundary, the domain stays strict.

### Approaches considered

- **A — Boundary deserializers (selected):** smallest change, matches the
  existing `serde_helpers.rs` precedent, JSON Schema and domain untouched.
- **B — DDD newtype wrapper (`Flexible<T>`):** type-encodes the leniency and is
  reusable, but changes field types, needs a hand-written `JsonSchema` impl per
  wrapper, and forces edits to every tool body and test that reads the field.
  Rejected: these values (`limit`, `offset`, `hours_back`, `message_id`) are
  transport parameters that are immediately unwrapped and validated downstream
  in `params.rs`; the real domain types (`ChannelId`, `MessageId`) already exist
  deeper in and are constructed in the tool body.
- **C — `serde_with` crate (`PickFirst<(_, DisplayFromStr)>`):** declarative and
  battle-tested, but adds a dependency and the `serde_as` + `schemars`
  interaction needs verification. Rejected to keep dependencies minimal.

## Deserializers

All five functions live in `src/mcp/tools/types/serde_helpers.rs`. Each
deserializes through a small `#[serde(untagged)]` enum, then coerces.

| Function | Target | Accepts | Empty `""` | Invalid input |
|----------|--------|---------|-----------|---------------|
| `flexible_opt_u32` | `Option<u32>` | JSON number, trimmed numeric string | → `None` | `"1.5"`, `-5`, `10.0`, garbage → error |
| `flexible_i64` | `i64` (required) | JSON number, trimmed numeric string | → error | float / garbage → error |
| `flexible_string` | `String` (required) | string, integer number → stringified | passes through `""` | float JSON → error |
| `flexible_opt_string` | `Option<String>` | string, integer number → stringified | → `None` | float JSON → error |
| `flexible_opt_bool` | `Option<bool>` | bool, `1`/`0`, `"true"`/`"false"`/`"1"`/`"0"` (case-insensitive, trimmed) | → `None` | other → error |

### Coercion semantics (pragmatic)

```
"limit": "10"      -> Some(10)
"limit": " 10 "    -> Some(10)      (trimmed)
"limit": ""        -> None          (optional field)
"limit": "1.5"     -> error
"limit": 10.0      -> error         (float for integer field)
"message_id": ""   -> error         (required field)
"channel_id": 123  -> "123"         (number stringified)
"use_tg_protocol": "true" -> true
"use_tg_protocol": 1      -> true
```

### Required vs optional attribute usage

- Optional fields use `#[serde(default, deserialize_with = "…")]`. The `default`
  handles a *missing* field (→ `None`); the deserializer handles present values
  including explicit `null` and empty string. schemars marks the field optional.
- Required fields use `#[serde(deserialize_with = "…")]` with **no** `default`,
  so a missing field still errors and schemars keeps the field required.

## Field application map (`requests.rs`)

| Field(s) | Deserializer | Attribute |
|----------|--------------|-----------|
| `GetChannelsRequest.{limit, offset}` | `flexible_opt_u32` | `default` |
| `SearchRequest.{hours_back, limit}` | `flexible_opt_u32` | `default` |
| `GetRecentMessagesRequest.{hours_back, limit}` | `flexible_opt_u32` | `default` |
| `GenerateLinkRequest.message_id` | `flexible_i64` | required |
| `OpenMessageRequest.message_id` | `flexible_i64` | required |
| `GetChannelInfoRequest.channel_identifier` | `flexible_string` | required |
| `GenerateLinkRequest.channel_id` | `flexible_string` | required |
| `OpenMessageRequest.channel_id` | `flexible_string` | required |
| `GetRecentMessagesRequest.channel_id` | `flexible_string` | required |
| `SearchRequest.query` | `flexible_string` | required |
| `GetMessageByLinkRequest.link` | `flexible_string` | required |
| `SearchRequest.channel_id` | `flexible_opt_string` | `default` |
| `GenerateLinkRequest.include_tg_protocol` | `flexible_opt_bool` | `default` |
| `OpenMessageRequest.use_tg_protocol` | `flexible_opt_bool` | `default` |
| `*.media_filter` | (unchanged) | already `deserialize_optional_media_filter` |

Tool bodies in `server.rs` and the domain layer are not modified.

## Error handling

Unparseable input still fails, but with a clearer message produced via
`serde::de::Error::custom` (e.g. `expected an integer, got '1.5'`). Because
these are deserialization errors, rmcp surfaces them as a JSON-RPC
invalid-params error to the client — the same failure channel as today, with a
better message.

## Testing (TDD)

- **Unit tests** in `serde_helpers.rs`, per function: number form, string form,
  whitespace trimming, empty string (optional → `None` / required → error),
  invalid input → error, all boolean variants, float-string → error, and u32
  negative / overflow → error.
- **Struct-level tests** in `requests.rs`: extend the existing tests so each
  request deserializes correctly from the swapped forms — `"limit": "10"`,
  `"channel_id": 123`, `"use_tg_protocol": "true"`, `"message_id": "575403"`.

## Known limitation

A JSON **float** for an integer field (`10.0`) is treated as invalid (→ error),
consistent with the advertised `integer` schema. Only string ↔ number ↔ bool
scalar swaps are coerced; structural mismatches (arrays, objects) and float →
integer are not.
