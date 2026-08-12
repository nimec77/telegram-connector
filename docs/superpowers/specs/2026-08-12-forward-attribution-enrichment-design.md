# Forward Attribution Enrichment — Design

**Date:** 2026-08-12
**Status:** APPROVED (requirements supplied pre-approved in the work order; the
mechanism decision below follows the work order's explicit fallback clause)

## Problem

`forwarded_from` on messages returned by `search_messages` and
`get_recent_messages` emits only `channel_id`, `original_date`, and
`original_message_id`. The specced `channel_name`, `channel_username`, and
`sender_name` fields exist on the domain type but are never populated
(`converters/message.rs` hardcodes them to `None`). Attribution is therefore an
opaque integer precisely when it matters: a forward from a channel the account
does not subscribe to cannot be resolved afterwards (`resolve_channels` returns
"Channel not found" for unsubscribed sources — verified against live Telegram).

The data has been in hand all along: every MTProto history/search response
envelope (`messages.Messages` / `messages.ChannelMessages`) carries `chats` and
`users` arrays containing the entities referenced by each message's `fwd_from`
header. No extra network call is needed — the fix is to stop discarding the
envelope.

## Constraint discovered during design

The work order's primary mechanism — read the entity map off the high-level
grammers `Message` — is **impossible in the pinned rev** (`9fef0bae`, Codeberg):

- `Message.peers: PeerMap` is `pub(crate)`; the only public lookups are
  `peer()` and `sender()`, neither of which can reach a forward's source peer.
- No public constructor or accessor for `PeerMap` exists anywhere in the crate
  (`build_peer_map` and `empty_peer_map` are `pub(crate)`).
- The pinned rev **is upstream HEAD** (verified 2026-08-12 by shallow-cloning
  `https://codeberg.org/Lonami/grammers`; HEAD = `9fef0ba`). A rev bump cannot
  provide the accessor.

The work order pre-authorizes the applicable fallback: *"if the high-level API
does not expose the entity map for a given code path, drop to the raw TL
response rather than skipping the field."*

## Approaches considered

1. **Raw TL fetch for the affected paths (CHOSEN).** In `TelegramClient`,
   replace the three grammers iterators used by `get_recent_messages` and
   `search_messages` with direct `client.invoke()` calls of the *same* TL
   requests the iterators issue internally (`messages.GetHistory`,
   `messages.Search`, `messages.SearchGlobal`), keeping the response envelope.
   Same request count — zero additional network calls. Everything needed is
   public API: `Client::invoke`, `tl::functions::messages::*`,
   `Media::from_raw(tl::enums::MessageMedia)`, `Peer::from_raw(client, Chat)`.
2. **Bump the grammers rev.** Dead end — pinned rev is upstream HEAD; no newer
   API exists to bump to.
3. **Fork/patch grammers to expose `peers`.** Rejected: the repo deliberately
   pins Codeberg upstream by rev; carrying a fork for one accessor taxes every
   future bump, and the work order prefers the raw-TL fallback.
4. **Rely on grammers' session peer cache.** Rejected: `auto_cache_peers`
   skips peers without auth (`peer.auth().is_some()` gate) — min peers, which
   is exactly what unsubscribed forward sources arrive as — and the session
   lookup returns id/hash refs, not titles/usernames.

## Design

### 1. `EntityLookup` — pure envelope entity map (new: `src/telegram/envelope.rs`)

A client-free map built from a response envelope's raw `chats` + `users`:

```rust
pub(crate) struct EntityInfo { name: Option<String>, username: Option<String> }
pub(crate) struct EntityLookup { /* HashMap<(PeerKind, i64), EntityInfo> */ }
```

- Keyed by peer *kind* + bare id (Telegram bare ids are per-namespace;
  `PeerUser(5)` and `PeerChannel(5)` must not collide).
- Built from `Vec<tl::enums::User>` and `Vec<tl::enums::Chat>`:
  - `Chat::Channel` → title + `username` field (no collectible-username
    fallback — mirrors grammers `Channel::username()`),
  - `Chat::ChannelForbidden` / `Chat::Community` / `Chat::CommunityForbidden`
    → title only,
  - `User::User` → name = first + " " + last (grammers `full_name()`
    semantics), username; `User::Empty` skipped,
  - `Chat::Empty` skipped.
- `get(&tl::enums::Peer) -> Option<&EntityInfo>` resolves a raw `from_id`.
- Also carries `first_name` separately so message-level `sender_name`
  (historically first-name-only via `Peer::name()`) keeps byte-identical
  output while forward-level `sender_name` uses the full display name.
- `EntityLookup::empty()` for call paths without an envelope.
- Pure data → fully offline-testable; structurally incapable of network I/O.

### 2. Raw pagers (new: `src/telegram/client/raw_pager.rs`, crate-internal)

Three thin pagers replicating grammers' iterator pagination byte-for-byte
(verified against `client/messages.rs` in the pinned rev), yielding
`(tl::enums::Message, Arc<EntityLookup>)` per message — entities are **per
page**, so each message pairs with its own page's envelope:

- **History** (`messages.GetHistory`): page limit 100 (grammers `MAX_LIMIT`);
  advance `offset_id`/`offset_date` from the last message of the page; last
  chunk when the page is empty, the response is non-`Slice` `Messages`, or
  `messages[0].id() <= limit`.
- **Channel search** (`messages.Search`): same, advancing `offset_id` +
  `max_date`; carries `q` and the media `filter`; supports the `before_id`
  cursor as the initial `offset_id`.
- **Global search** (`messages.SearchGlobal`): advances `offset_rate` (from
  `next_rate`), `offset_id`, and `offset_peer` — the latter built as an
  `InputPeer` from the last message's `peer_id` looked up in the *page's* raw
  chats/users for the access hash (`InputPeer::Empty` when absent, as grammers
  does). Also exposes each message's owning chat as a high-level `Peer`
  (`Peer::from_raw`) since the ops layer needs it for identity/link building.

Offset-advancement and last-chunk logic live in pure functions over the
decoded response so pagination parity is unit-testable offline.

### 3. Converter split (`src/telegram/converters/message.rs`)

- Core: `convert_raw_message(raw: &tl::enums::Message, peer: &Peer,
  entities: &EntityLookup) -> Option<Message>` — all existing conversion logic
  moves here; already raw-oriented except sender and media:
  - sender: from raw `from_id` + `entities` (message-level `sender_name`
    stays first-name-only for users — no output change),
  - media: `Media::from_raw(message.media)` (public, client-free) feeds the
    existing `convert_media_to_type` / video / audio / link-preview helpers.
- `extract_forward_info(header: &tl::types::MessageFwdHeader, entities:
  &EntityLookup) -> ForwardInfo`:
  - `from_id = PeerChannel` → `channel_id` (unchanged) + `channel_name`,
    `channel_username` from `entities`,
  - `from_id = PeerUser` → `sender_name` = full display name from `entities`;
    channel fields absent,
  - `from_id = PeerChat` → `channel_name` = group title (id fields stay
    absent, as today),
  - `from_id` absent + `from_name` set (hidden sender) → `sender_name` =
    `from_name` (unchanged),
  - `post_author` → new pass-through when set (signed channel posts),
  - entity-map miss → ids only, exactly today's output; never an error, never
    a fabricated name, never a resolution call.
- Compat wrapper `convert_message(msg: &grammers::Message, peer)` for the
  unchanged call paths (`get_message_by_id`, `get_messages_batch`,
  `get_channel_stats`): seeds an `EntityLookup` from what *is* public on the
  message (`msg.sender()`, `msg.peer()`) so message-level `sender_name`
  output is preserved; forwards on those paths remain ids-only (their
  envelopes are unreachable behind `pub(crate)` — out of scope per the work
  order, which names only the two search tools).

### 4. Ops changes (`ops_history.rs`, `ops_search.rs`)

Swap the grammers iterator loops for the raw pagers. All surrounding logic —
timeout budgets, cutoff/window checks, `before_id`/`after_id` cursor
semantics (A8), album collapse (post-level limits), `has_more`, fan-out — is
untouched; loops read message id/date via existing raw seams
(`timestamp_from_raw`). `matches_media_filter` gains a raw-message variant
sharing the same core. The `channel_ids` fan-out (`tools/fanout.rs`) and
album-collapse paths sit above this seam and inherit enrichment automatically.

### 5. Domain + DTO

`ForwardInfo` (in `types/entities.rs`, embedded directly by the response DTO)
already declares `channel_name`/`channel_username`/`sender_name` — all
`Option` + `skip_serializing_if`. One additive field: `post_author:
Option<String>` (same serde attrs). Serde/schemars derives update the JSON
schema automatically; the stale "intentionally never populated" doc comment is
rewritten. Existing field names, types, and JSON shape unchanged — strictly
additive.

## Error handling

- One unresolvable forward degrades that one object to ids-only; conversion
  never fails a message, a message never fails a batch.
- Raw invoke errors map to the existing `Error::TelegramApi` path, same as
  iterator errors today.
- `MessageEmpty` placeholders are skipped as today (B1).

## Testing

Per the work-order matrix, all offline in converter/envelope tests using raw
TL fixtures (test_helpers style):

1. forward from a channel in the envelope → name + username populated
2. forward from a no-username channel → name only
3. forward from a user → `sender_name` (full name), channel fields absent
4. hidden sender (`from_name` only) → `sender_name` only
5. signed post → `post_author`
6. entity-map miss → ids only, no error
7. non-forwarded message → `forwarded_from` absent from serialized JSON

Plus: `EntityLookup` construction (all Chat/User variants, kind/id keying);
pager offset-advancement and last-chunk pure-function tests; a server-level
mockall test with forwards present proving no resolve/get-entity calls
(the primary enforcement is structural: `convert_raw_message` and
`EntityLookup` take no client and are synchronous — network is impossible by
type signature).

Gate: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`.

## Manual acceptance

Against live Telegram: a forwarded post whose source channel is not in the
subscription list returns a human-readable `channel_name`.

## Docs

README `forwarded_from` response example; CHANGELOG entry; `docs/memory.md`
note (envelope capture pattern + why raw TL was required); tasklist tick.

## Non-goals

No resolution/caching in conversion; no `resolve_channels` changes; no new
tools; no media/transcription changes; `get_message_by_id` /
`get_messages_batch` / `get_channel_stats` forward enrichment (envelope
unreachable on those grammers-high-level paths; can ride a future rev bump if
upstream ever exposes the peer map).
