# Correctness Core (B1, B2, B3, D9) — Design

**Date:** 2026-08-10
**Source:** `docs/telegram-mcp-0.13.0-work-order.md` (black-box audit of v0.13.0)
**Release:** v0.13.1, branch `fix/correctness-core`, single PR
**Status:** APPROVED

## Context: work-order decomposition

The full v0.13.0 work order (10 bugs, 10 improvements, 8 feature requests) is split
into five sub-projects, each with its own spec → plan → implementation → release
cycle, in dependency order:

1. **Correctness core** — B1, B2+D9, B3 *(this spec)* → v0.13.1
2. **Albums & response size** — B5+A2, B4+A4, D1 → next release
3. **Metadata honesty** — B6, B7, B8, B9, B10
4. **Polish** — D2–D8, D10
5. **New surface** — A1, A3, A5–A8

One release ships per sub-project. §1.3 of the work order lists verified-correct
behaviour (clamping, date windows, media guards, identifier flexibility) that must
not regress.

## Approach

Targeted fixes at existing seams (approved over a trait-level
`MessageFetchOutcome` redesign, which would ripple through the client trait, both
mocks, and much of the 456-test suite for no added correctness).

## B1 — stop fabricating deleted/missing messages

**Observed defect:** `get_message_by_link` on a deleted id (`609784`) or a
never-existed id (`999999999`) returns a success object with `text: ""`,
`timestamp: "1970-01-01T00:00:00Z"`, and no `views`/`forwards`.

**Cause (code-level):** `get_message_by_id_impl`
(`src/telegram/client/ops_message.rs`) already errors when grammers returns
`None`, but grammers wraps deleted ids in a `MessageEmpty`-backed message object,
which `convert_message` (`src/telegram/converters/message.rs`) maps blindly —
producing the epoch-timestamp fabrication.

**Fix:**

- Detect the empty/deleted variant at the grammers boundary in `convert_message`
  or immediately before it: the raw `MessageEmpty` TL variant if grammers exposes
  it, otherwise the empty-text + epoch-`date()` signature. Exact detection
  mechanism is verified at implementation time against the pinned grammers rev.
- Return a distinct "empty" signal (not a fabricated `Message`).
  `get_message_by_id_impl` maps it to the existing clean error style:
  `Message {id} not found or deleted in channel {channel}` via
  `Error::InvalidInput` (no new error variant unless implementation shows one is
  needed).
- Per the work order's stated preference, this is an **error**, not a
  `deleted: true` response object.

**Call-site audit** (same fallthrough pattern):

- Iteration paths (`ops_search`, `ops_history`) skip unconvertible messages —
  verify the empty signal is likewise skipped, never emitted.
- Direct-fetch paths — `get_message_media` (`ops_media`) and
  `transcribe_voice_message` (`ops_transcribe`) — fetch by id and must get the
  same guard; each gets a regression test against a deleted id (work order §5
  flags these as untested and possibly sharing B1's root cause).

**Acceptance:** no tool response ever contains `timestamp:
"1970-01-01T00:00:00Z"`. Regression tests cover a known-deleted id and an id far
above the channel's max, across every tool taking a `message_id`.

## B2 + D9 — shared link builder, public links for public channels

**Observed defects:**

- `generate_message_link(channel_id="1144180066", message_id=610121)` emits
  `https://t.me/c/1144180066/610121?single` + `tg://privatepost?…` for a public
  channel (`@swodki`). The `t.me/c/` form only resolves for members.
- `open_message_in_telegram` builds the same wrong `tg://privatepost` form.
- D9: `generate_message_link` rejects usernames (`'swodki' is not a valid
  number`) — the only tool with a strictly numeric `channel_id`.

**Fix:**

- **Builder:** `MessageLink::new` in `src/link.rs` gains a
  `username: Option<&str>` parameter and becomes the single link builder used by
  both tools (and by sub-project 2's D1 later). Output fields:
  - `https_link` — `https://t.me/<username>/<id>` when a username exists;
    `https://t.me/c/<channel_id>/<id>` otherwise.
  - `tg_protocol_link` — `tg://resolve?domain=<username>&post=<id>` when public;
    `tg://privatepost?channel=<channel_id>&post=<id>` otherwise.
  - `internal_link` — always the `https://t.me/c/…` form (new field).
  - `is_public` — `bool` (new field).

  Existing field names are retained (their values switch to the public form
  when the channel has a username); the two new fields are additive, so the
  response schema change is non-breaking. The current `?single` /
  `&single` suffix is dropped from generated links — it is a media-group hint,
  not part of a canonical message link.
- **Resolution:** `generate_message_link_impl` accepts username *or* numeric id
  (same flexible identifier convention as every other tool — fixes D9) and
  resolves the channel once via the existing `resolve_peer` to learn its
  username. This costs one rate-limiter token; the tool moves from purely
  offline to one lookup, which is unavoidable to know whether a public form
  exists.
- `open_message_in_telegram` uses the same builder and prefers the public
  `tg://resolve` form when available.

**Acceptance:** `generate_message_link` on `1144180066` returns
`https://t.me/swodki/610121`; a genuinely private chat still returns the
`t.me/c/` form; `generate_message_link(channel_id="swodki", …)` works; both
tools share one builder.

## B3 — resolvable schemas: inline `MediaFilter`, guard against dangling `$ref`

**Observed defect:** the published `inputSchema` for `get_recent_messages` and
`search_messages` references `#/$defs/MediaFilter`, but no `$defs` block is
emitted. Schema-following clients cannot construct a valid `media_filter` call —
the feature is dead.

**Fix:**

- Inline the enum into the schema. First choice: `#[schemars(inline)]` on the
  `MediaFilter` enum (`src/telegram/types/media.rs`). Fallback if the attribute
  doesn't survive rmcp's schema flattening: configure inlining at the
  schema-generator level (rmcp/schemars generator settings).
- **Recurrence guard:** a test iterates all 12 tools' `inputSchema` from the tool
  router and asserts that every `$ref` anywhere in a schema has a resolvable
  target in an accompanying `$defs`. Runs in `cargo test`, so the existing
  pre-merge gate (`fmt` + `clippy` + `test`) enforces it in CI.
- Post-fix QA note: exercise `media_filter: "voice"` / `"video_note"` end to end
  against the live server (work order §5 — runtime behaviour was untestable
  while the schema was broken).

**Acceptance:** published schemas for both tools contain no dangling `$ref`;
`media_filter: "voice"` validates and executes; the schema-walk test fails if a
future type reintroduces an unresolvable `$ref`.

## Error handling

New and changed error text follows the §1.3 template (`invalid input: Channel
not found: @…`) — lowercase category prefix, actionable message, no double
prefixes (D7's full audit stays in sub-project 4, but no new text introduced
here may double-wrap).

## Testing (TDD — failing tests first)

- Deleted-id and out-of-range-id tests for `get_message_by_link`,
  `get_message_media`, `transcribe_voice_message` (mock returns an empty-variant
  message; assert error, not fabricated object).
- Assertion helper: no serialized response contains an epoch-0 timestamp.
- Link-builder unit tests: public channel → `t.me/<username>` + `tg://resolve`;
  private → `t.me/c/` + `tg://privatepost`; `internal_link` and `is_public`
  populated in both cases.
- `generate_message_link` accepts `"swodki"`, `"@swodki"`, `"1144180066"`.
- `open_message_in_telegram` uses the public form for public channels.
- Schema-walk test: every `$ref` in every published tool schema resolves.
- All existing 456 tests keep passing (§1.3 no-regress list).
