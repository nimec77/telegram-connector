# Converter Parity — Design

**Date:** 2026-08-13
**Status:** APPROVED
**Work order:** `docs/telegram-connector-work-orders.md` — Work Order A
**Baseline:** v0.19.0

## Problem

Two defects, one shared root, plus a metadata gap.

### Problem 1 — `forwarded_from` enrichment is path-dependent

v0.19.0 (Phase 33) populated `channel_name` / `channel_username` on
`forwarded_from`, but only on the history/search path. Verified live against
channel `1912881684`, message `298716` (forwarded from channel `1783384254`):

```
get_recent_messages -> {"channel_id":1783384254,
                        "channel_name":"Pavel Zloi",
                        "channel_username":"evilfreelancer", ...}   CORRECT
get_message_by_link -> {"channel_id":1783384254, ...}               IDs only
get_messages_batch  -> {"channel_id":1783384254, ...}               IDs only
```

`get_messages_batch` is the documented re-fetch path for text truncated by
`max_text_length`. A workflow that finds a forward in search results and
re-fetches it for full text therefore **silently loses attribution it already
had** — a downgrade that looks like success.

### Problem 2 — documents and polls carry no metadata object

`video_info` and `audio_info` are rich (duration, dimensions, size, mime,
`has_thumbnail`). Documents get nothing: a 4-item album of meetup slides
(channel `2246801752`, message `198`) returns only `"media_type":"document"`.
A caller cannot distinguish a 2 MB PDF from a 500 MB archive, and the filename
often carries the entire meaning of the post. Polls have the same gap — bare
`"media_type":"poll"`, no question, no options, no results.

## Root cause — the converter is starved, not bypassed

The work order's stated diagnosis is that "the enrichment lives in a code path
that some tools bypass." **This is not what the code does.** There is already
exactly one conversion function, and all five message-returning routes reach
it:

```rust
// src/telegram/converters/message.rs:228
pub(crate) fn convert_raw_message(
    raw: &tl::enums::Message,
    peer: &grammers_client::peer::Peer,
    entities: &EntityLookup,
) -> Option<Message>
```

The defect is in the **fetch** layer, not the conversion layer. Enrichment
quality is a function of the `EntityLookup` a caller supplies, and the two
broken tools cannot supply a real one:

| Route | Fetch call | Envelope |
|---|---|---|
| `get_recent_messages` | raw `messages.GetHistory` (`raw_pager.rs`) | full `chats`+`users` |
| `search_messages` | raw `messages.Search` / `SearchGlobal` | full `chats`+`users` |
| `get_message_by_link` | grammers `client.get_messages_by_id()` | **discarded** |
| `get_messages_batch` | grammers `client.get_messages_by_id()` | **discarded** |
| `get_channel_stats` | grammers `client.iter_messages()` | **discarded** |

`convert_message` (`message.rs:320`) is a compatibility wrapper that
synthesizes a two-entry `EntityLookup` from the only peers the high-level API
exposes — `msg.sender()` and `msg.peer()` — because `Message.peers` is
`pub(crate)` in the pinned grammers rev (`9fef0bae`; see
`docs/superpowers/specs/2026-08-12-forward-attribution-enrichment-design.md`
for why a rev bump cannot fix that). A forward source the account does not
subscribe to is by construction neither the sender nor the chat, so it is never
in that pair, and attribution degrades to ids-only.

Phase 33 already solved this exact shape for history/search by dropping to raw
TL invocations that preserve the response envelope. This design applies the
same, now-proven pattern to the `getMessages` and `iter_messages` paths, and
then **removes the envelope-less entry point entirely** so the failure mode
cannot recur.

## Design

### 1. Envelope-preserving `getMessages`

Add a raw twin of grammers' `get_messages_by_id` to
`src/telegram/client/raw_pager.rs`, alongside the existing pagers (same module,
same pattern, same justification):

```rust
pub(super) async fn fetch_messages_by_id(
    client: &Client,
    peer: PeerRef,
    ids: &[i32],
) -> Result<(HashMap<i32, tl::enums::Message>, Arc<EntityLookup>), InvocationError>
```

It mirrors grammers `client/messages.rs:1064-1104` in the pinned rev:

- **Request routing.** `peer.id.kind() == PeerKind::Channel` →
  `channels::GetMessages { channel: peer.into(), id }`; otherwise
  `messages::GetMessages { id }`. `PeerRef` converts directly to
  `InputChannel`, so no manual access-hash plumbing is needed. Ids are wrapped
  as `InputMessage::Id(InputMessageId { id })`.
- **Decode.** Reuse the existing `unpack_page` helper, which already handles
  all four `messages.Messages` variants. It treats `NotModified` as an empty
  final page rather than `panic!`-ing as grammers does — the repo's never-unwrap
  rule, and unreachable anyway since our `hash` is always 0.
- **Envelope.** Build one `Arc<EntityLookup>` via `EntityLookup::from_envelope`
  and share it across every message in the response.
- **Peer-match guard.** Replicate grammers' `filter(|m| m.peer_id() == peer.id)`
  using the existing `raw_peer_id` helper. This matters on the non-channel
  branch: `messages.getMessages` resolves bare ids across all of the account's
  dialogs, so without the filter a message from an unrelated chat could be
  returned for a requested id.
- **Result shape.** Key by message id rather than zipping positionally, exactly
  as grammers does (`map.remove(id)`). The API is not required to return
  results in request order.

`ops_message.rs` then converts through `convert_raw_message(raw, &peer,
&entities)`. Both `get_message_by_id_impl` and `get_messages_batch_impl` keep
their present semantics unchanged:

- `require_found` still turns an absent single message into the not-found error.
- `is_empty_variant` still routes a `MessageEmpty` placeholder to `missing_ids`
  rather than into `messages` (work-order B1 guard).
- Every requested id still lands in exactly one of `messages` / `missing_ids`.

**RPC count is unchanged** — one `channels.GetMessages` before, one after.

### 2. `get_channel_stats` onto the raw pager

`ops_stats.rs:38` iterates via `client.iter_messages(peer_ref)`. Replace with
the existing `RawHistoryPager` (already proven on `get_recent_messages`),
reading dates through `timestamp_from_raw` instead of `message_timestamp`.

Stats does not return messages to the caller — it aggregates. Enrichment is
therefore irrelevant to its output, and this migration is **not** a behavior
fix. Its purpose is to retire the last envelope-less caller so that step 3 can
delete the envelope-less converter. The sweep semantics (window cutoff,
`MAX_MESSAGES_SCANNED` cap, `complete` flag, oldest-timestamp tracking) are
preserved exactly; `RawHistoryPager` already replicates `iter_messages`
pagination request-for-request.

### 3. Structural guard — make starvation unrepresentable

With no envelope-less callers left:

| Symbol | Fate | Rationale |
|---|---|---|
| `convert_message` (`message.rs:320`) | **deleted** | Its only purpose was to satisfy the converter's signature without an envelope |
| `EntityLookup::insert_peer` (`envelope.rs:118`) | **deleted** | Zero callers outside `convert_message` (verified) |
| `EntityLookup::empty` (`envelope.rs:54`) | gated `#[cfg(test)]` | Still needed by degradation tests |
| `EntityLookup::from_envelope` | sole production constructor | — |

This satisfies the work order's requirement for a type-level constraint over an
enumerating test. Conversion requires an `EntityLookup`; outside `#[cfg(test)]`
the only way to obtain one is from a real MTProto response envelope. A tool
added later cannot starve the converter, because after this change there is
nothing left to starve it with — the failure mode is removed from the type
system rather than merely detected by a test that a future author must
remember to extend.

`EntityLookup`'s module doc records the invariant explicitly, so the reason for
the `#[cfg(test)]` gate survives future edits.

A behavioral test complements the type-level guarantee: one forwarded-message
fixture, converted through every message-returning route, asserting identical
`forwarded_from` output. It is a regression net, not the primary defense.

### 4. `document_info`

New type in `src/telegram/types/media.rs`:

```rust
pub struct DocumentInfo {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub file_name: Option<String>,
    pub file_size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mime_type: Option<String>,
}
```

`file_name` comes from `DocumentAttribute::Filename`; `file_size_bytes` and
`mime_type` from the raw document itself. `extract_document_info(media)` lands
in `converters/media.rs` beside `extract_video_info` / `extract_audio_info` and
follows their structure: match the media, read `doc.raw.document`, walk
attributes, return `None` for anything else.

**Emitted only when `convert_media_to_type(media) == MediaType::Document`.**
Video, audio, voice, animation, and sticker media are all document-backed at
the TL level, but they already have dedicated info objects; emitting
`document_info` for them too would duplicate `file_size_bytes` and `mime_type`
on every media message and consume the 40 KB `[limits] response_byte_budget`
for no new information. One info object per media class.

### 5. `audio_info` gains `title` / `performer`

The work order lists `title` / `performer` under `document_info` for "audio
documents". They belong on `AudioInfo`, which already exists for exactly that
media class — putting them on `document_info` would force audio messages to
carry two overlapping objects.

`AudioInfo` gains two optional fields, both read from the
`DocumentAttribute::Audio` attribute already being walked for `duration`:

```rust
#[serde(skip_serializing_if = "Option::is_none", default)]
pub title: Option<String>,
#[serde(skip_serializing_if = "Option::is_none", default)]
pub performer: Option<String>,
```

Backward compatible: existing fields are untouched, and both new fields are
omitted from JSON when Telegram supplies no ID3 metadata.

### 6. `poll_info`

Confirmed available with zero calls: grammers' `Media::Poll` exposes public
`raw: tl::types::Poll` and `raw_results: tl::types::PollResults`. Per the
pinned TL schema:

```
poll#58747131 id:long flags:# closed:flags.0?true public_voters:flags.1?true
  multiple_choice:flags.2?true quiz:flags.3?true question:TextWithEntities
  answers:Vector<PollAnswer> ...
pollResults#7adf2420 flags:# min:flags.0?true
  results:flags.1?Vector<PollAnswerVoters> total_voters:flags.2?int ...
pollAnswerVoters#3b6ddad2 flags:# chosen:flags.0?true correct:flags.1?true
  option:bytes voters:int = PollAnswerVoters;
```

New types:

```rust
pub struct PollInfo {
    pub question: String,
    pub options: Vec<PollOption>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub total_voters: Option<u64>,
    pub closed: bool,
    pub multiple_choice: bool,
    pub quiz: bool,
}

pub struct PollOption {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub voters: Option<u64>,
}
```

Serialized shape:

```json
"poll_info": {
  "question": "Какой стек выбрать?",
  "options": [{"text": "Rust", "voters": 287}, {"text": "Go", "voters": 125}],
  "total_voters": 412, "closed": true, "multiple_choice": false, "quiz": false
}
```

**Deviation from the work order, approved during design:** the work order
specifies `options` as an array of strings. Per-option vote counts are free on
`PollAnswerVoters`, and `total_voters` without a per-option breakdown does not
tell a caller what the poll concluded — which is the whole point of reading a
poll. `voters` is omitted per-option when `PollResults.results` is absent
(an unvoted poll, or one whose results Telegram withholds), so the
strings-only case is the degenerate form of this shape rather than a second
code path. `quiz` is a free flag off the same struct and distinguishes a graded
quiz from an opinion poll.

Options are matched to their vote counts by the `option: bytes` key, which is
the identifier both `PollAnswer` and `PollAnswerVoters` carry. Poll answer text
is `TextWithEntities`; only its `.text` is read — entity markup is out of scope,
consistent with how message text is handled elsewhere.

No separate API call is made to fetch results: only `raw_results`, as delivered
on the message media, is read.

### 7. Wiring

`document_info` and `poll_info` are added to:

- `Message` (`src/telegram/types/entities.rs`), both
  `#[serde(skip_serializing_if = "Option::is_none", default)]`
- `MessageResponse` (`src/mcp/tools/types/responses.rs`) and its
  `From<Message>` impl

Populated in `convert_raw_message` beside the existing `video_info` /
`audio_info` calls, from the `media` value already computed there. No new
argument, no new dependency, no client access.

### 8. Paths deliberately not touched

- **Album collapse** (`src/telegram/albums.rs`) operates on already-converted
  domain `Message` values and selects a representative sibling. It inherits
  every enrichment field automatically.
- **`channel_ids` fan-out** (`src/mcp/tools/fanout.rs`) calls the same client
  ops per channel and merges converted results.
- **`get_message_media`** returns image content blocks plus `MediaMetadata`,
  not `Message` values, and is outside this work order's scope.

Both of the first two are audited and require no change; the spec records this
so the audit result is not re-derived later.

## Constraints

- **Zero additional network calls.** Structural, not merely tested:
  `convert_raw_message` is a pure function of `(raw, peer, entities)` and holds
  no client handle. Every new and extended field derives from data already in
  the response. Enforced in tests via mockall expectations asserting no
  resolve/get-entity/download calls occur during conversion.
- **Backward compatible.** Every change is either a new optional field
  (omitted when absent) or deletion of an internal, non-`pub` symbol. No
  existing field is renamed, retyped, or removed. The `MessageResponse` wire
  format gains fields only.
- **Graceful degradation.** A missing entity, attribute, or poll result emits
  fewer fields — never an error, and never a failed batch. An envelope miss
  still yields the ids-only `forwarded_from`, exactly as today.

## Testing

Per the repo's TDD convention, each item below is a failing test written before
its implementation.

**Parity (the core regression net)**

- One forwarded-message fixture converted through `get_recent_messages`,
  `search_messages`, `get_message_by_link`, and `get_messages_batch` yields
  byte-identical `forwarded_from`.
- Batch re-fetch of a message first seen in search results preserves
  `channel_name` — the exact workflow the bug silently broke.

**Raw fetch**

- Channel peer routes to `channels.GetMessages`; non-channel peer routes to
  `messages.GetMessages`.
- Results are keyed by message id, not position: a response returning ids out
  of request order maps each id to the correct message.
- The peer-match filter drops a message whose `peer_id` differs from the
  requested peer.
- `MessageEmpty` in a slot reports the id as missing, not as a converted
  message.
- `NotModified` decodes as an empty page, not a panic.

**Media metadata**

- Document with a filename; document without one (`file_name` absent from JSON).
- Audio document with `title` / `performer`; without them (both absent).
- Non-document message: `document_info` absent from serialized JSON.
- Poll with results (per-option `voters` present); poll without results
  (per-option `voters` absent, `total_voters` absent); quiz poll sets `quiz`.
- Poll option text matched to the correct vote count by `option` bytes key.

**Zero-call invariant**

- mockall expectations assert no resolve / get-entity / download call fires
  during conversion of a 100-message batch containing forwards, documents, and
  polls.

**Stats migration**

- Window cutoff, `MAX_MESSAGES_SCANNED` cap, and the `complete` flag behave
  identically before and after the `RawHistoryPager` switch.

## Quality gates

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

Docs updated: `README.md` response examples (`document_info`, `poll_info`,
extended `audio_info`), `CHANGELOG.md`, `docs/tasklist.md` (new phase row),
`docs/memory.md` (the starved-converter lesson and the type-level guard).

## Manual acceptance

Per the work order:

1. Fetch channel `1912881684` message `298716` via `get_recent_messages`,
   `get_message_by_link`, **and** `get_messages_batch`. All three must return
   `"channel_name":"Pavel Zloi"`.
2. Fetch channel `2246801752` message `198` and confirm
   `document_info.file_name` is populated.

Requires a live authenticated session; run against the deployed MCP server
after the automated gates pass.

## Non-goals

- No resolution or caching layer; no `resolve_channels` call during conversion.
- No document download and no content parsing — metadata only.
- No changes to Work Order B (search latency) or C (media throughput). They are
  independent work orders with their own specs; the work-orders doc explicitly
  requires they not run in parallel with this one.
