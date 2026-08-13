# telegram-connector — Work Orders (v0.19.0)

**Repository:** https://github.com/nimec77/telegram-connector
**Baseline version:** 0.19.0
**Stack:** Rust 2024 nightly, `rmcp` SDK, `grammers` Telegram client
**Findings verified:** 2026-08-13, by live calls against the deployed MCP server

---

## How to use this document

Three independent work orders. Each is self-contained — hand them to the agent
one at a time, in the order below.

| Order | Scope | Subsystem | Why this position |
|---|---|---|---|
| **A** | Converter parity: forward attribution + document/poll metadata | `converters.rs`, response DTOs | Same files as recent work; smallest diff; structural fix that prevents recurrence |
| **B** | Global search latency | search path, MTProto call layer | Independent; requires measurement before any change |
| **C** | Media retrieval throughput | media path, rate limiter | Independent; touches config defaults |

**Do not run these in parallel.** A and C both touch the rate limiter config
surface, and B and C both reuse the existing `channel_ids` fan-out concurrency
approach.

### Values that are estimates, not measurements

Flagged inline in each work order. Summary:

- **C:** `max_tokens` 60, `media_download` cost 3, payload cap 8 MB — estimated,
  not calibrated against Telegram's real flood thresholds
- **B:** `search_deadline_seconds` default 20 — chosen to sit below typical MCP
  client timeouts, not derived from measurement
- **A:** no estimated values

Tune these against real channel data rather than accepting them as given.

---

# Work Order A — Converter parity

Fix inconsistent message enrichment across code paths in this repository
(telegram-connector v0.19.0: Rust 2024 nightly, rmcp SDK, grammers).

## Problem 1 — forwarded_from enrichment is path-dependent

v0.19.0 added `channel_name` / `channel_username` to `forwarded_from`, but
only on the history/search path. Verified against ONE message (channel
1912881684, message 298716, forwarded from channel 1783384254):

```
get_recent_messages -> {"channel_id":1783384254,
                        "channel_name":"Pavel Zloi",
                        "channel_username":"evilfreelancer", ...}   CORRECT
get_message_by_link -> {"channel_id":1783384254, ...}               IDs only
get_messages_batch  -> {"channel_id":1783384254, ...}               IDs only
```

Same message, same envelope availability, three different outputs. This is a
structural problem, not three separate bugs: the enrichment lives in a code
path that some tools bypass.

`get_messages_batch` is the documented re-fetch path for truncated text, so a
workflow that finds a forward in search results and then re-fetches it for
full text SILENTLY LOSES attribution it already had. That is the worst
possible failure shape — a downgrade that looks like success.

## Problem 2 — document and poll media have no metadata object

`video_info` and `audio_info` are rich (duration, dimensions, size, mime,
has_thumbnail). Documents get nothing. Verified: a post of meetup slides
(channel 2246801752, message 198, a 4-item album of documents) returns only
`"media_type":"document"` — no filename, no size, no mime type. A caller
cannot distinguish a 2 MB PDF from a 500 MB archive, and the filename often
carries the entire meaning of the post.

Check whether `poll` media has the same gap and fix it if so.

## Required work

### 1. Single shared conversion path (the actual fix)

Audit EVERY tool that returns messages — currently search_messages,
get_recent_messages, get_message_by_link, get_messages_batch, plus the
multi-channel fan-out (`channel_ids`) and album-collapse code paths — and
route them all through ONE conversion function that produces the complete
enriched domain Message.

Do not patch the two broken tools individually. If a path cannot reuse the
shared function (e.g. a different TL response envelope type), extract the
enrichment into a function taking the entity map as a parameter so every
envelope type can supply it. Leave a code comment on any path that needs
special handling and why.

Add a test that FAILS if a message-returning tool bypasses the shared
converter, so tools added later inherit enrichment by default rather than by
remembering to. A trait-level or type-level constraint is preferable to a
test that enumerates tool names, since an enumeration has the same failure
mode as the current bug.

### 2. document_info object

Add optional `document_info`, omitted when absent (`skip_serializing_if`):

- `file_name` (string, optional) — from DocumentAttributeFilename
- `file_size_bytes` (u64)
- `mime_type` (string, optional)
- `title` / `performer` (string, optional) — audio documents

Source: the raw document attributes already present on the message. Zero
additional API calls.

### 3. poll_info object (verify first)

If poll messages currently return bare `"media_type":"poll"`, add optional
`poll_info`: `question` (string), `options` (array of strings),
`total_voters` (u64, optional), `closed` (bool), `multiple_choice` (bool).
Poll results are on the message media where available — do not make a
separate API call to fetch results.

## Hard constraints

- ZERO additional network calls for all of the above. Enforce in tests
  (mockall: assert no resolve/get-entity/download calls during conversion).
- Backward compatible: no existing field renamed, retyped, or removed.
- Graceful degradation: a missing entity or attribute emits fewer fields,
  never an error, and never fails a batch of 100 messages.

## Quality gates

- `cargo fmt --check && cargo clippy -- -D warnings && cargo test` passes.
- Tests: forwarded_from identical across ALL message-returning tools for the
  same fixture; document with and without filename; audio document with
  title/performer; poll with and without results; non-document message ->
  `document_info` absent from JSON.
- Update README.md response examples and CHANGELOG.md.

## Manual acceptance check

Fetch channel 1912881684 message 298716 via get_recent_messages,
get_message_by_link, AND get_messages_batch. All three must return
`"channel_name":"Pavel Zloi"`. Then fetch channel 2246801752 message 198 and
confirm `document_info.file_name` is populated.

## Non-goals

- No resolution/caching layer, no calls to resolve_channels during conversion.
- No document download, no content parsing — metadata only.

---

# Work Order B — Global search latency

Investigate and bound global search latency in this repository
(telegram-connector v0.19.0: Rust 2024 nightly, rmcp SDK, grammers).

## Problem

`search_messages` without `channel_id`/`channel_ids` has wildly unpredictable
latency. Measured in one session, same tool, comparable limits:

```
channel-scoped, no filter              0.5 - 0.8 s
global, media_filter=url,   24h        0.6 s
global, media_filter=video, 72h        0.6 s
global, media_filter=voice, 72h       10.1 s
global, media_filter=video_note, 24h  12.0 s
global, media_filter=document, 24h    35.3 s   <-- worst observed
```

A 35-second call risks exceeding MCP client timeouts, which fails an entire
workflow rather than degrading. The spread (60x on the same code path) matters
more than the peak: callers cannot predict cost, so they cannot budget calls.

## Task

1. INSTRUMENT FIRST, do not guess. Add tracing spans around the global search
   path measuring: time in the MTProto call(s), number of round trips issued,
   whether the path fans out across dialogs client-side or issues a single
   searchGlobal, and time spent in conversion. Reproduce the document-filter
   case and report where the 35 s actually goes before changing behavior.

2. Based on the measurement, apply the appropriate fix:
   - If the path fans out per-dialog: bound concurrency and cap the number of
     dialogs swept, reusing whatever concurrency approach the existing
     `channel_ids` fan-out already uses rather than inventing a second one.
   - If a single upstream call is slow: this is Telegram-side; the fix is
     deadline handling, not optimization.

3. Regardless of cause, add a deadline. Introduce a configurable
   `search_deadline_seconds` (default 20 — ESTIMATE, chosen to sit below
   typical MCP client timeouts) in `[search]`. On expiry, return the results
   gathered so far with `"timed_out": true` and `"partial": true` in
   `query_metadata`, plus `next_cursor` where the shape allows resuming.
   Never return an error for a slow-but-working search: partial results are
   strictly more useful than a failure.

4. Report actual timing in the response. `search_time_ms` already exists;
   add `query_metadata.dialogs_scanned` (or equivalent) so a caller can see
   the work performed and understand why a call was expensive.

5. Document in the tool description that unscoped searches with a media
   filter are the expensive shape, and that scoping via `channel_ids` is
   dramatically faster. Callers cannot avoid a cost they cannot see.

## Quality gates

- `cargo fmt --check && cargo clippy -- -D warnings && cargo test` passes.
- Test the deadline path with a mocked slow client: assert partial results
  are returned with the flags set, and that no error propagates.
- Include the before/after measurements for the document-filter case in the
  PR description or CHANGELOG.

## Non-goals

- No caching of search results.
- No change to result ordering or to the filter semantics themselves.
- Do not "fix" this by silently lowering the default limit.

---

# Work Order C — Media throughput

Improve media retrieval throughput in this repository (telegram-connector
v0.19.0: Rust 2024 nightly, rmcp SDK, grammers).

## Problem

`get_message_media` fetches exactly one image per call at `media_download` = 5
tokens, against a bucket of capacity 30 refilling at 1 token/sec. Effective
throughput: 6 images immediately, then one per 5 seconds.

A realistic digest run covering 10-15 visual posts therefore spends over a
minute blocked on refill, plus 10-15 separate round trips. The rate limiter
exists to avoid Telegram-side flood limits, but downloading 15 photos is not
close to those limits — the current settings are stricter than the constraint
they model.

## Task

### 1. Batch media tool

Add `get_messages_media_batch`:

- `channel_id` (string, required)
- `message_ids` (array, 1-10)
- `max_dimension` (integer, optional, default 1280, clamped 64-2048)

Returns one MCP image content block per message that has visual media (photo,
or thumbnail for video/animation/video_note), each paired with its metadata,
following the existing single-message tool's output shape and its
photo-vs-thumbnail rules exactly. Reuse that code path; do not fork it.

Per-id failures (deleted message, no visual media, oversize) are reported in a
`failed` array with a reason per id — never fail the whole batch, matching the
`missing` convention already used by `get_messages_batch`.

Downloads within a batch run concurrently with bounded concurrency, reusing
the existing `channel_ids` fan-out approach.

### 2. Total payload cap

Enforce a configurable `media_batch_max_total_bytes` (default 8 MB —
ESTIMATE) across the whole batch. When the cap would be exceeded,
progressively reduce `max_dimension` for remaining images; if still exceeded,
stop and report the un-returned ids in `failed` with reason
`payload_cap_reached`. Never return a response that could overflow the
client's context.

### 3. Rate limiter retune

- Charge a batch `media_download_cost x number_of_images_actually_returned`
  (failed ids cost nothing).
- Raise `[rate_limiting] max_tokens` default from 30 to 60 and lower
  `media_download` from 5 to 3, keeping both configurable.
- These two numbers are ESTIMATES, NOT measured against Telegram's real flood
  thresholds. Implement them as defaults, note in config.example.toml that
  they are conservative starting points, and do not hardcode them anywhere.
- Add a `retry_after_seconds` hint to the rate-limit error so a caller can
  wait precisely instead of guessing.

### 4. Status reporting

Extend `check_mcp_status` with the media-relevant limits (batch max ids,
payload cap) alongside the existing cost table, so a caller can plan a run
instead of discovering limits by hitting them.

## Quality gates

- `cargo fmt --check && cargo clippy -- -D warnings && cargo test` passes.
- Tests: mixed batch (photos + video thumbnails + one no-media id) returns
  correct blocks plus a populated `failed`; payload cap triggers progressive
  downscaling then `payload_cap_reached`; rate limiter charges only for
  returned images; batch of 1 matches single-tool output exactly.
- Update README.md (tool reference), config.example.toml, and CHANGELOG.md.

## Non-goals

- No full video download. Thumbnails only, same rule as the existing tool.
- No disk caching of downloaded media.
- No changes to transcription.

---

## Appendix — Out of scope, deliberately

Observed during verification, judged not worth code changes:

| Observation | Why not a fix |
|---|---|
| `views: 1` on freshly-posted messages | Real behavior — view counts need time to accumulate. A caveat for consumers, not a bug. |
| Unpunctuated transcription output | Telegram's own server-side model produces this. Not the connector's to fix. |
| `channels_scanned: null` on global searches | Honest reporting of an unknown. Work Order B adds `dialogs_scanned` where a real count exists. |
| No `forwarded_from` on copy-paste aggregators (e.g. `@swodki`) | Those channels copy text and re-upload media rather than using native forwards — there is no forward header to read. Verified: 0 of 25 recent posts carried one. Content-level deduplication belongs in the consuming workflow, not the connector. |
