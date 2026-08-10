# telegram-mcp v0.13.0 — Engineering Work Order

**Audience:** implementation agent with access to the telegram-mcp source tree.
**Author:** black-box audit, 2026-08-10.
**Target version under test:** `server_version: 0.13.0`.

---

## 0. How to read this document

Every finding below was **reproduced against a live server**. Each entry gives the exact call, the observed response, why it is wrong, a proposed fix, and acceptance criteria.

Two things this document does **not** have, and you should not assume otherwise:

- **No source access.** The audit was black-box. File paths, function names, and internal types are not referenced anywhere below. Where a cause is guessed, it is labelled *Hypothesis* — verify before acting on it.
- **No changelog / git history.** The "what's new" delta in §1 was reconstructed by diffing the published JSON schemas against the API surface documented in a downstream consumer (a `news-digest` skill, last synced 2026-07-02). A tool listed as "new" may simply have been unused by that consumer. Treat §1 as context, not as a spec.

Confidence labels used: **CONFIRMED** (reproduced ≥2×, or verified against independently computed ground truth) and **OBSERVED** (seen once, consistent with the model, not adversarially retested).

### Test environment

| | |
|---|---|
| Server version | 0.13.0 |
| Account | Telegram Premium enabled (`premium: true`) |
| Subscriptions | 186 chats (channels + groups) |
| Rate limiter | capacity 30, refill ≈0.36 tokens/sec (measured: 23.23 → 30.0 over 19 s) |
| Coverage | 12/12 tools exercised, ~50 calls |

### Reference fixtures used throughout

| Fixture | Value |
|---|---|
| Public channel | `@swodki`, numeric id `1144180066` |
| Live message (photo) | `610119` |
| Live message (video) | `610121` |
| Deleted message | `609784` |
| Non-existent message | `999999999` |
| Album (8 parts, one timestamp) | `610047`–`610054`, all `2026-08-10T05:55:12Z` |
| Private group (magic username) | `Семейный чатик`, id `521440428` |
| High-album channel | `🔥Full-Time Trading`, id `1292964247` |

---

## 1. API surface as of 0.13.0 (context)

### 1.1 Tools present

| Tool | Purpose |
|---|---|
| `check_mcp_status` | connection state, rate limiter, session counters |
| `get_subscribed_channels` | list subscriptions, paginated |
| `get_channel_info` | channel detail by username or id; `include_full` adds description + member_count |
| `get_recent_messages` | messages from one channel by time window |
| `search_messages` | keyword search, one channel or global |
| `search_public_channels` | Telegram public directory search |
| `get_message_by_link` | fetch one message by t.me link |
| `generate_message_link` | build `tg://` and `https://t.me` deep links |
| `get_message_media` | photo / video thumbnail as an image + metadata |
| `transcribe_voice_message` | server-side transcription (requires Premium) |
| `open_message_in_telegram` | open a message in Telegram Desktop (macOS) |
| `get_last_responses` | replay buffer of recent tool responses |

### 1.2 Delta vs the documented consumer API (2026-07-02)

New tools: `get_channel_info`, `search_public_channels`, `get_message_by_link`, `generate_message_link`, `open_message_in_telegram`, `get_last_responses`.

New fields and parameters:

- `from_date` / `to_date` (RFC 3339 UTC) on `get_recent_messages` and `search_messages`, overriding `hours_back`
- message field `video_info`: `duration_seconds`, `width`, `height`, `file_size_bytes`, `kind`, `has_thumbnail`, `mime_type`
- message field `forwarded_from`: `{channel_id, original_date, original_message_id}`
- message field `link_preview`: `{url, site_name, title, description}`
- message field `reply_to_message_id` (optional)
- `get_message_media`: `max_dimension`, best-variant selection, `is_thumbnail`, video/video_note thumbnail support
- `check_mcp_status`: `server_version`, `requests_received`, `responses_written`, `last_response_write_age_secs`, `session_started_at`, `session_uptime_secs`
- `get_subscribed_channels`: `limit` / `offset` pagination

### 1.3 What already works correctly — do not regress

These were explicitly verified and are **working as documented**. Any change made for the findings below must keep them true.

- **Parameter clamping.** `limit=500` → 100; `hours_back=1000` → 168 (recent); `hours_back=500` → 72 (search); `max_dimension=10` → 64. All clamp silently and correctly.
- **Date-window accuracy.** `get_recent_messages(@swodki, from_date=2026-08-10T05:00:00Z, to_date=2026-08-10T07:00:00Z)` returned exactly 21 messages, matching ground truth computed independently from a full 100-message dump of the same channel. Boundaries are inclusive as documented.
- **No silent deep-window truncation.** A 37-hour-old 2-hour window returned 1 message. This was suspected to be truncation and was **disproved**: neighbouring ids `609783` (`2026-08-08T23:42`) and `609786` (`2026-08-09T02:16`) fall outside the window, so the window was genuinely near-empty. Do not "fix" this.
- **Window validation messages.** `to_date` before the `hours_back` window produces a clear, actionable error naming both the conflict and the remedy.
- **Media guards.** `get_message_media` on a text-only message → `message has no visual media (media type: none)`. `transcribe_voice_message` on a video → `message is not transcribable (media type: video); only voice and video_note are supported`.
- **Identifier flexibility.** `channel_id` accepts `@swodki`, `swodki`, and `1144180066` on `get_recent_messages`, `search_messages`, `get_channel_info`, `get_message_media`.
- **Unknown-channel error.** `@no_such_channel_zzz_9182` → `invalid input: Channel not found: @no_such_channel_zzz_9182`. Already normalized; use as the template for D8.

---

## 2. Bugs

### B1 — `get_message_by_link` fabricates a message instead of erroring — CONFIRMED

**Severity:** critical. Silent data corruption.

**Reproduce:**
```
get_message_by_link(link="https://t.me/swodki/609784")     # deleted message
get_message_by_link(link="https://t.me/swodki/999999999")  # never existed
```

**Observed** — both calls return success:
```json
{
  "id": 609784,
  "channel_id": 1144180066,
  "channel_name": "…",
  "channel_username": "swodki",
  "text": "",
  "timestamp": "1970-01-01T00:00:00Z",
  "sender_id": null,
  "sender_name": null,
  "has_media": false,
  "media_type": "none"
}
```
Note `views` and `forwards` are absent entirely, unlike a real message.

**Why it is wrong:** the caller cannot distinguish "message exists and is empty" from "message does not exist". A Unix-epoch timestamp is a plausible-looking value that flows straight into date filters, sort orders, and digests. This is the worst failure mode in the codebase: it corrupts downstream data rather than failing.

*Hypothesis:* MTProto returns `MessageEmpty` for deleted/out-of-range ids and the mapping layer falls through to a `Default` value rather than matching the variant.

**Fix:** match the empty/absent case explicitly. Either
- return an error: `Message {id} not found or deleted in {channel}`, or
- return `{"id": …, "channel_id": …, "deleted": true}` with no synthesized timestamp.

Prefer the error unless a caller has a demonstrated need to distinguish deleted-from-missing.

**Also audit:** every other path that constructs a message object from an MTProto response, for the same `Default`-fallthrough pattern. `get_message_media` and `transcribe_voice_message` take `(channel_id, message_id)` directly and were not tested against a deleted id — check them.

**Acceptance:** no response from any tool ever contains `timestamp: "1970-01-01T00:00:00Z"`. Add a regression test for a known-deleted id and for an id far above the channel's max.

---

### B2 — `generate_message_link` emits a private link for a public channel — CONFIRMED

**Severity:** critical. Every generated link is unusable for attribution.

**Reproduce:**
```
generate_message_link(channel_id="1144180066", message_id=610121)
```

**Observed:**
```json
{
  "channel_id": "1144180066",
  "message_id": 610121,
  "https_link": "https://t.me/c/1144180066/610121?single",
  "tg_protocol_link": "tg://privatepost?channel=1144180066&post=610121&single"
}
```

**Expected:** `1144180066` is `@swodki`, a public channel. The correct links are
`https://t.me/swodki/610121` and `tg://resolve?domain=swodki&post=610121`.

**Why it is wrong:** the `t.me/c/…` form only resolves for members and cannot be shared. Any consumer citing sources publicly produces broken links for 100% of public-channel messages.

The resolver clearly exists — `get_message_by_link` accepts and correctly resolves `https://t.me/swodki/609784`. It is simply not used on the generation path.

**Same defect in `open_message_in_telegram`** — CONFIRMED:
```
open_message_in_telegram(channel_id="1144180066", message_id=610119)
→ {"success": true, "link_used": "tg://privatepost?channel=1144180066&post=610119&single", "app_opened": true}
```
Functionally it still opens (the account is a member), but the construction is wrong and will fail for non-member public channels.

**Fix:** resolve `channel_id` → username before building the link. If a username exists, emit the public form; fall back to `t.me/c/` only for genuinely private chats. Extract this into one shared link-builder used by both tools, so they cannot diverge again.

Consider returning both forms when the channel is public, e.g. `public_link` plus `internal_link`.

**Acceptance:** `generate_message_link` on `1144180066` returns `https://t.me/swodki/610121`. A private-chat id still returns the `t.me/c/` form. Both tools use the same builder.

---

### B3 — `media_filter` is uncallable: dangling `$ref` in the published schema — CONFIRMED

**Severity:** critical. An entire documented feature is unreachable.

**Reproduce:** fetch the tool definition for `get_recent_messages` or `search_messages` and inspect `inputSchema`.

**Observed:**
```json
"media_filter": {
  "anyOf": [ { "$ref": "#/$defs/MediaFilter" }, { "type": "null" } ],
  "default": null,
  "description": "Optional: Filter by media type. Applied client-side. …"
}
```
The top-level schema object contains only `properties`, `required`, and `type`. **There is no `$defs` key.** The `$ref` resolves to nothing.

**Why it is wrong:** the allowed enum values are never transmitted. A schema-following client cannot construct a valid call — six attempts across both tools failed, every one rejected at the client validation layer before reaching the server. A strict client would reject the tool definition outright.

Downstream impact: the `news-digest` consumer depends on `media_filter='voice'` and `'video_note'` for its audio-verification pass. That path is dead in 0.13.0.

*Hypothesis:* the schema generator (`schemars` or equivalent) emits enums into a `$defs` block that is dropped when the schema is flattened into the MCP tool definition. This affects **any** `$ref` in any tool — grep the generated schemas for `$ref` and confirm `$defs` accompanies each one.

**Fix:** inline the enum rather than referencing it:
```json
"media_filter": {
  "type": ["string", "null"],
  "enum": ["photo", "video", "voice", "video_note", "document", null],
  "default": null,
  "description": "…"
}
```
In Rust this is typically an inline-schema attribute on the field or type, or hand-built schema JSON. Whichever route, the fix must be structural — emitting `$defs` alongside the tool schema is also acceptable if the MCP client resolves it, but inlining is safer.

Replace the enum list above with the actual variants; it is inferred from documentation strings, not from the type.

**Acceptance:** the published `inputSchema` for both tools contains no `$ref`. A call with `media_filter: "voice"` validates and executes. Assert in CI that no generated tool schema contains `$ref` without a resolvable `$defs`.

---

### B4 — the documented maximum `limit` produces an unusable response — CONFIRMED

**Severity:** high.

**Reproduce:**
```
get_recent_messages(channel_id="@swodki", hours_back=1000, limit=500)
```
Clamps correctly to `hours_back=168, limit=100` — then returns **85,241 characters** in a single JSON line. This exceeded the MCP client's token budget; the response was diverted to a file and never reached the model.

**Why it is wrong:** `limit: 100` is the documented maximum, so a caller following the schema will hit this. On any verbose channel the max is unreachable in practice. ~850 bytes/message average on this channel; text-heavy channels are worse.

**Fix — layered:**
1. **Response byte budget.** Cap the serialized response (suggest ~40 KB, configurable). When the cap would be exceeded, return fewer messages and set `has_more: true` plus a cursor (see A8).
2. **Per-message text truncation.** Add `max_text_length` (default ~2000). When truncated, set `text_truncated: true` and `text_full_length: <n>` so the caller knows to fetch the full message.
3. **Reduce per-message overhead** — see A4. `channel_name` and `channel_username` are repeated verbatim in all 100 messages.

Do not simply lower the documented max; that trades one problem for another. Budget by bytes, not by count.

**Acceptance:** `limit=100` against `@swodki` returns a response under the byte cap, with `has_more` set truthfully.

---

### B5 — albums fragment the result set; `limit` counts raw records, not posts — CONFIRMED

**Severity:** high. This is the single most damaging defect for any digest/summarization consumer.

Message objects carry no `grouped_id`. A Telegram album arrives as N sibling messages sharing a timestamp, with N−1 of them carrying empty `text`.

**Reproduce:**
```
get_recent_messages(channel_id="1292964247", hours_back=72, limit=10)
```

**Observed:** 10 messages returned, comprising **2 actual posts**:
- `412591`–`412598` — 8 messages, all `2026-08-10T13:24:04Z`, 7 with `text: ""`
- `412599`, `412600` — 2 messages, all `2026-08-10T13:34:08Z`, 1 with `text: ""`

Second instance on a different channel: in the verified `@swodki` 05:00–07:00 window, 9 of 21 messages (`610046`–`610054`) were a single album.

**Why it is wrong:** three separate consequences.
1. `limit=10` does not mean 10 posts. Callers cannot size a request.
2. Albums silently consume the time window, pushing real posts out of the result.
3. One post counts as N for any deduplication or volume heuristic downstream.

**Fix:**
1. **Expose `grouped_id`** on every message that has one. This is the minimum and unblocks callers to group client-side.
2. **Add `collapse_albums: bool`** (suggest default `true`; if the consumer contract cannot change, default `false` and document it). When set, emit one object per album:
   - `text` from whichever sibling carries it
   - `media_count: N`
   - `media_types: [...]`
   - `message_ids: [...]` (all siblings, so callers can still reach each part)
   - `id` = the lowest sibling id, for stable referencing
3. When collapsing, `limit` counts collapsed posts.

**Acceptance:** `get_recent_messages(1292964247, limit=10)` with collapsing on returns 10 distinct posts. `grouped_id` is present on all album members when collapsing is off.

---

### B6 — `total_found` and `channels_searched` report the wrong quantity — CONFIRMED

**Severity:** medium. Misleads coverage reasoning.

**6a. `total` / `total_found` is the returned page size, not the match count.**
```
get_subscribed_channels(limit=3, offset=2) → {"total": 3, "has_more": true}
get_subscribed_channels(limit=5)          → {"total": 5, "has_more": true}
get_subscribed_channels(limit=500)        → {"total": 186, "has_more": false}
```
It coincides with the true total only when everything fits in one page.

**6b. `channels_searched` counts distinct channels in the *output*, not channels scanned.**

Decisive test — the account has **186 subscriptions**:
```
search_messages(query="США", hours_back=24, limit=2)
→ {"total_found": 2, "channels_searched": 2}

search_messages(query="нейросеть", hours_back=48, limit=4)
→ {"total_found": 4, "channels_searched": 4}
```
`channels_searched` tracks `limit` exactly, because it is derived from the result set.

**Why it is wrong:** a consumer reading `channels_searched: 2` concludes the sweep was narrow and may re-query, or reports false coverage. Both fields are named as scope metrics but computed as output metrics.

**Fix:**
- Rename to `returned` (both tools). Keep `total`/`total_found` only if populated with a genuine match count from Telegram; otherwise drop them.
- Rename `channels_searched` → `channels_in_results`, and add a real `channels_scanned` if the search path knows it.
- Ensure `has_more` reflects actual remaining results, not `returned == limit`.

**Acceptance:** with 186 subscriptions and `limit=2`, the response distinguishes "2 results, from 2 channels, scanned N".

---

### B7 — `query_metadata.hours_back` echoes an overridden parameter — CONFIRMED

**Severity:** low.

**Reproduce:**
```
get_recent_messages(channel_id="@swodki",
                    from_date="2026-08-10T05:00:00Z",
                    to_date="2026-08-10T07:00:00Z", limit=50)
→ "query_metadata": {"query": "", "hours_back": 48, "channels_searched": 1}
```
`hours_back` was never supplied and was overridden by the date window, yet the default `48` is reported. The actual window (2 hours) appears nowhere.

**Fix:** report the window actually applied — `window_from` and `window_to` as RFC 3339 — and omit `hours_back` when dates were used. Same change in `search_messages`.

**Acceptance:** metadata always describes the window that was executed.

---

### B8 — `last_message_date` is declared but never populated — CONFIRMED

**Severity:** low.

Always `null`, in all three channel-returning tools: `get_subscribed_channels`, `get_channel_info` (**including** `include_full: true`), `search_public_channels`.

**Fix:** populate it, or remove the field. A permanently-null field is worse than an absent one — callers write logic against it. If populating costs an extra RPC per channel, gate it behind `include_full`.

**Acceptance:** either the field is gone, or `get_channel_info(@swodki, include_full=true)` returns a real timestamp.

---

### B9 — magic strings instead of `null` in `username` — CONFIRMED

**Severity:** low, but a correctness trap.

```json
{"id": 521440428,  "name": "Семейный чатик",     "username": "group"}
{"id": 1292964247, "name": "🔥Full-Time Trading", "username": "unknown"}
{"id": 1798673537, "name": "Telegram Premium",    "username": "premium"}
```

Private groups get `"group"`; private channels get `"unknown"`. Meanwhile `description` and `member_count` correctly use `null` in the same object — the convention is inconsistent within a single struct.

Both sentinels are syntactically valid Telegram usernames, so they can collide with a real channel. The third row above is not hypothetical: `@premium` is a real channel in this account's subscriptions with an equally short, generic username.

**Fix:** `username: null` when there is none. Move chat kind to its own field: `chat_type: "channel" | "group" | "supergroup"`. Apply across all three channel-returning tools.

**Acceptance:** no response contains `"username": "unknown"` or `"username": "group"` as a sentinel. `chat_type` is present and correct for all 186 subscriptions.

---

### B10 — `forwarded_from` carries only a numeric id — CONFIRMED

**Severity:** medium for attribution-dependent consumers.

```json
"forwarded_from": {
  "channel_id": 1036362176,
  "original_date": "2026-08-10T06:01:07Z",
  "original_message_id": 288696
}
```

Labelling "reposted from X" requires a separate `get_channel_info` per forward. On aggregator channels forwards dominate: all 6 sampled messages from `@AptiAlaudinovAKHMAT` were forwards, from 6 distinct source channels — 6 extra round trips for one page of results.

**Fix:** add `original_channel_name` and `original_channel_username`. Both are normally present in the entity set attached to the same MTProto response — no extra RPC needed. Verify this before implementing.

**Acceptance:** a page of forwarded messages is fully attributable with zero follow-up calls.

---

## 3. Improvements

### D1 — put the permalink on the message object

**Priority: highest in this section.** No message-returning tool includes a link. Obtaining one means calling `generate_message_link` per message — which currently produces a wrong link (B2) and costs N extra calls.

Add `link` (public form when available) to every message object in `get_recent_messages`, `search_messages`, `get_message_by_link`. Once done, `generate_message_link` becomes a rarely-needed convenience.

Do B2 first; this depends on the shared link builder.

### D2 — expose reactions

`views` and `forwards` are present; reactions are not. Reactions are the strongest available resonance signal for ranking. Add `reactions: [{emoji, count}]` and/or `reactions_total`.

### D3 — `original_*` in `get_message_media` is misnamed

Same message, two calls:
```
get_message_media(@swodki, 610119, max_dimension=10)   → original_width: 320,  returned: 64×36
get_message_media(@swodki, 610119, max_dimension=1280) → original_width: 1280, returned: 1280×711
```
`original_*` changes with the request, so it describes the **selected variant**, not the original.

Rename to `source_variant_width` / `source_variant_height` / `source_variant_size_bytes`, and add `largest_available_width` / `largest_available_height` so a caller can tell whether a better variant exists — important when reading text in an image and the first attempt is illegible.

### D4 — skip re-encoding when dimensions are unchanged

Same call as above at `max_dimension=1280`: `source 79,252 bytes → returned 102,438 bytes`. JPEG re-encoding at identical dimensions made the payload **29% larger**.

When `returned_dims == source_dims` and the source is already JPEG, pass the original bytes through.

### D5 — make the rate-limit budget legible

`check_mcp_status` reports `rate_limiter_tokens` alone. Capacity, refill rate, and per-operation costs are all invisible, so a caller cannot budget — one number without a scale is not a budget.

Measured externally: capacity 30, refill ≈0.36 tokens/sec, ≈2.26 tokens per media download (from 3 concurrent downloads: 30.0 → 23.23).

Add to `check_mcp_status`:
```json
"rate_limiter": {
  "tokens": 23.23,
  "capacity": 30,
  "refill_per_sec": 0.36,
  "costs": {"search": 1, "media_download": 2.26, "transcription": 5}
}
```
Use the real values from the implementation. Keep `rate_limiter_tokens` as a deprecated alias for one release if callers depend on it.

Also: when a call is rejected for rate limiting, the error should state the deficit and the wait time.

### D6 — `get_last_responses` should not replay binary payloads

Called with `n=1` after a `get_message_media` call, it returned the **full base64 image**. This tool exists to recover from a response lost in transit — precisely when context is already damaged. Replaying the blob defeats the purpose.

Add `include_binary: bool` defaulting to `false`. When false, replace binary content blocks with a stub: `{"type": "image", "omitted": true, "mime_type": "…", "size_bytes": …}`.

### D7 — double-wrapped error prefixes

```
generate_message_link(channel_id="1144180066", message_id=-5)
→ "Invalid message_id: invalid input: Message ID must be positive, got -5"
```
The error is wrapped twice. Audit the error-wrapping layer for repeated prefixing.

### D8 — normalize raw MTProto errors, validate cheaply client-side

```
get_channel_info(channel_identifier="not a valid identifier!!!")
→ "telegram API error: Failed to resolve username: request error: rpc error 400: USERNAME_INVALID caused by contacts.resolveUsername"
```
Compare with the already-clean unknown-channel error (§1.3). Bring the rest to that standard.

Additionally, reject malformed usernames locally before spending an RPC: Telegram usernames are 5–32 characters, `[A-Za-z0-9_]`, not starting with a digit.

### D9 — `generate_message_link` should accept usernames

```
generate_message_link(channel_id="swodki", message_id=610121)
→ "Invalid channel_id: 'swodki' is not a valid number"
```
It is the only tool requiring a strictly numeric `channel_id`; every other tool documents "Channel ID or username". Accept both. (Falls out naturally from the B2 fix.)

### D10 — `has_more` in `search_public_channels` asserts more than it knows

`search_public_channels(query="rust programming", limit=5)` returned 5 results and `has_more: false`. When the result count equals the limit, "no more" is almost certainly unknown rather than false. Either determine it truthfully or make the field nullable.

---

## 4. Feature requests

Ordered by impact on a digest/summarization consumer.

### A1 — `get_messages_batch(channel_id, message_ids[])`

Verifying or re-fetching N specific messages currently costs N calls to `get_message_by_link`. This is the hot path for deduplication and quote-checking. One call, one round trip.

### A2 — album collapsing and post-level `limit`

See B5. Beyond the bug fix, `limit` should be expressible in posts rather than records, so callers can size requests meaningfully.

### A3 — multi-channel `get_recent_messages(channel_ids[], …)`

Consumers sweep 20–60 curated channels one at a time. A single call taking a channel list, fanning out server-side with bounded concurrency, cuts both latency and repeated per-channel metadata. Pairs with A4.

### A4 — compact response mode

Add `fields` (projection) or `format: "compact"`. `channel_name` and `channel_username` are repeated in every message of every response — at `limit=100` on a channel with a 60-character name, that is kilobytes of pure duplication. Hoist channel metadata into a response header and emit it once. Directly mitigates B4.

### A5 — `get_channel_stats(channel_id)`

Posting rate, median views, promo/ad share, typical post length. Consumers currently hand-maintain tier lists and hand-filter promotional posts. Basic statistics would let that be computed.

### A6 — search scoped to a channel subset

`search_messages` supports one channel or all 186. The common need sits in between: keyword search across a named subset. Accept `channel_ids[]` alongside the existing `channel_id`.

### A7 — `resolve_channels(identifiers[])`

Batch-resolve usernames/ids/titles to channel entities. Solves title-only private channels (consumers resolve those by numeric id today) and eliminates the per-forward lookup behind B10.

### A8 — cursor pagination

`offset` drifts on active channels: new posts arrive at the head while paging, shifting every subsequent page. Add `before_id` / `after_id` cursors keyed on `message_id`. Also gives callers a clean way to resume a window that hit the `limit` or the byte budget from B4.

---

## 5. Not verified — do not assume either way

| Item | Status |
|---|---|
| `media_filter` runtime behaviour | **Untested.** Blocked by B3 — no valid value could be constructed. After fixing B3, test `photo`, `video`, `voice`, `video_note` end to end. |
| `transcribe_voice_message` happy path | **Untested.** No `voice` or `video_note` message was found in any sampled window (`@oper_goblin`, `@AptiAlaudinovAKHMAT`, `@swodki`, `1292964247`, up to 168 h back). The error path is correct. `premium: true` on this account, so the Premium gate itself is also unverified. Needs a known voice-message fixture. |
| `get_message_media` / `transcribe_voice_message` against a **deleted** message id | **Untested.** Both take `(channel_id, message_id)` directly and may share B1's root cause. Test explicitly. |
| `open_message_in_telegram` on non-macOS | **Untested.** Documented as macOS-only; behaviour elsewhere unknown. |
| Concurrency / rate-limit rejection path | **Partially tested.** 3 concurrent media downloads succeeded and drew the bucket down correctly. Exhaustion and the resulting error were never triggered. |

---

## 6. Suggested execution order

Each step is independently shippable.

1. **B1** — stop fabricating messages. Audit all MTProto→message mappings for the same fallthrough. *Silent data corruption; everything else is visible.*
2. **B2** + **D9** — shared link builder, public form for public channels, username input. *Unblocks D1.*
3. **B3** — inline enum schemas; add a CI assertion that no tool schema contains an unresolvable `$ref`. *Restores a dead feature and prevents recurrence.*
4. **B5** + **A2** — `grouped_id`, album collapsing, post-level `limit`. *Largest correctness win for digest consumers.*
5. **B4** + **A4** — response byte budget, text truncation flags, compact mode. *Makes the documented max usable.*
6. **D1** — `link` on every message object. *Removes N calls per digest.*
7. **B6**, **B7**, **B8**, **B9**, **B10** — metadata honesty pass. Cheap, mostly mechanical, and they compound.
8. **D2**–**D10** — polish.
9. **A1**, **A3**, **A5**–**A8** — new surface, once the above is stable.

## 7. Regression tests worth adding

- Deleted and out-of-range message ids across **every** tool taking `message_id` — assert an error, and assert no epoch-0 timestamp appears in any response.
- Every generated tool schema: no `$ref` without a resolvable `$defs`.
- `generate_message_link` for a public channel returns the `t.me/<username>/` form; for a private chat, the `t.me/c/` form.
- `limit=100` on a text-heavy channel stays under the response byte cap.
- An album-heavy channel with `limit=N` returns N distinct posts when collapsing is enabled.
- Golden test for date-window accuracy: fixed `from_date`/`to_date` against a recorded fixture, asserting the exact message-id set. Locks in the §1.3 behaviour.
- `username` is `null`, never `"unknown"` or `"group"`.
