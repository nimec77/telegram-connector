# 02 — Architecture Adherence & Duplication

The architecture itself is sound: clean MCP → application → Telegram layering,
trait-based DI (`McpServer<T, R>`), typed errors, and a consistent
wrapper → `*_impl` shape. The findings here are duplication and small drifts from
that shape, not structural problems.

---

## AD-1 — Peer resolution is implemented three different ways 🔴

**Effort:** M · **Risk:** Med · **Impact:** High · **Pair with LM-2**

### Evidence
A dedicated helper exists:

```rust
// client.rs:176 — resolve a channel ref (numeric ID via dialog walk, or username)
async fn resolve_peer(&self, channel_ref: &str) -> Result<Peer, Error> { … }
```

It is used by `get_message_by_id` (`:644`), `download_message_media` (`:707`),
and `transcribe_audio` (`:871`). But **two other methods re-implement the same
logic inline instead of calling it**:

- `get_channel_info` (`client.rs:293–339`) — hand-rolls the
  username-strip / `resolve_username` / numeric-ID dialog-walk branches.
- `get_recent_messages` (`client.rs:501–562`) — hand-rolls a `resolve_username`
  attempt plus a dialog-walk fallback.

So the "resolve a channel reference to a peer" concept has three slightly
different copies. They differ in subtle ways (e.g. `get_channel_info` tries
username-without-`@` as a last resort; `get_recent_messages` only resolves
usernames it can prove are non-numeric), which is exactly the danger: behavior
that *should* be identical has quietly diverged.

### Why it matters
- This is past the Rule of Three — the project's own bar for extraction
  (`docs/conventions.md`).
- A fix or timeout-budget change to resolution must be made in three places;
  miss one and behavior diverges further.
- It's the main reason `client.rs` is 923 lines.

### Fix sketch
Promote `resolve_peer` into the canonical entry point and express the variants as
explicit options rather than copies. Land this in the new `client/resolve.rs`
(LM-2):

```rust
// One resolver, with the documented behaviors as flags/return shape.
async fn resolve_peer(&self, channel_ref: &str) -> Result<Peer, Error>;

// get_channel_info delegates:
let peer = self.resolve_peer(identifier).await?;
convert_peer_to_channel(&peer).ok_or(/* not a channel/group */)

// get_recent_messages delegates, keeping its "prefer username, fall back to
// dialog by id" intent by passing the already-known ChannelId for the fallback.
```

Capture the union of the three behaviors deliberately (don't silently drop the
"username without @" last-resort path) and add a test per branch so the merge is
provably behavior-preserving. This is where the **Med risk** comes from — the
copies aren't byte-identical, so the consolidation needs test coverage of each
prior branch.

---

## AD-2 — `get_recent_messages` resolves a username channel twice over the network 🟡

**Effort:** S · **Risk:** Low · **Impact:** Medium (latency + extra MTProto call)

### Evidence
For a username (non-numeric `channel_id`), the server layer resolves the channel
just to obtain its numeric ID:

```rust
// server.rs:333–341 (get_recent_messages_impl)
let channel = self.telegram_client
    .get_channel_info(&request.channel_id)   // network: resolve_username
    .await?;
(channel.id, Some(original_identifier))       // pass BOTH id and the username
```

Then the client layer resolves the **same username again**:

```rust
// client.rs:501–511 (get_recent_messages)
let username = identifier.strip_prefix('@').unwrap_or(identifier);
if !username.chars().all(|c| c.is_ascii_digit()) {
    … self.client.resolve_username(username).await …   // network: resolve again
}
```

The `channel.id` obtained by the first call is then only used as a fallback /
for logging. So the username path performs **two** resolve round-trips where one
suffices.

### Why it matters
Extra latency and an extra MTProto call on every username-based
`get_recent_messages`, against an API that flood-limits. It also muddies
ownership: both layers think they own resolution.

### Fix sketch
Pick one layer to own resolution. Cleanest: let the **client** own it (it already
re-resolves) and have the server pass the raw identifier through without calling
`get_channel_info` first:

```rust
// server: don't pre-resolve; hand the identifier to the client
let params = HistoryParams::new_from_ref(&request.channel_id, …);
```

`HistoryParams` already carries `channel_identifier: Option<String>`
(`params.rs:56`) precisely for direct resolution — lean on it and drop the
server-side `get_channel_info` pre-call. After AD-1, the client's single
`resolve_peer` handles both numeric and username forms, so the server doesn't
need the numeric `channel_id` up front at all (it's used today mainly for logging
and the dialog fallback).

> Verify the `ChannelId` requirement: `HistoryParams.channel_id` is currently
> non-optional. If the numeric id is genuinely needed for logging, derive it from
> the resolved peer in the client rather than pre-resolving in the server.

---

## AD-3 — 11× near-identical `#[tool]` logging-wrapper boilerplate 🟡

**Effort:** M · **Risk:** Med · **Impact:** Medium

### Evidence
Every one of the 11 tools is the same shape (~20 lines each, ~220 lines total):

```rust
pub async fn search_messages(&self, Parameters(request): …, id: RequestId)
    -> Result<String, String>
{
    let request_id = id.0.to_string();
    let started = Instant::now();
    tracing::info!(tool = "search_messages", request_id = %request_id,
        /* per-tool fields */, "Tool invocation started");
    let result = self.search_messages_impl(request).await;
    log_tool_outcome("search_messages", &request_id, started, &result);
    result
}
```

Only the tool name, the `*_impl` call, and the per-tool log fields vary.
`log_tool_outcome` (`server.rs:854`) already factors out the *completion* log;
the *start* log + timing scaffold is still copy-pasted 11 times.

### Why it matters
Adding a tool means copying the ritual exactly; a divergence (a forgotten
`log_tool_outcome`, a mismatched `tool` string) is easy and silent. This is the
single most-repeated block in the codebase.

### Fix sketch (two options, in order of preference)
1. **Declarative macro** that emits the wrapper, given name + impl + fields:

   ```rust
   tool_wrapper! {
       #[tool(description = "…")]
       search_messages(request: SearchRequest) => search_messages_impl
       log { query = %request.query, channel_id = ?request.channel_id, … }
   }
   ```

   Caveat (**Med risk**): the wrapper must still expand to a method carrying the
   `#[tool(description=…)]` attribute *inside* the `#[tool_router]` block, so the
   macro has to interleave with rmcp's macro. Prototype one tool end-to-end and
   confirm `list_tools`/`call_tool` still see it before converting all 11.

2. **Minimal helper** if the macro interplay proves fragile: keep the wrappers but
   collapse start-log + timing into a small guard object:

   ```rust
   let _span = ToolInvocation::start("search_messages", &id /* + fields */);
   let result = self.search_messages_impl(request).await;
   _span.finish(&result)   // logs outcome on drop/finish
   ```

   This removes the timing/`log_tool_outcome` repetition without fighting the
   proc-macro, at the cost of leaving the per-tool field list inline.

> If neither lands cleanly, this is a fine **accept-as-is** (see `04-roadmap.md`):
> the boilerplate is verbose but uniform and low-risk. Don't add a fragile macro
> just to save lines.

---

## AD-4 — Peer → (id, name, username) extraction is duplicated 🟢

**Effort:** S · **Risk:** Low · **Impact:** Low

### Evidence
`convert_peer_to_channel` (`converters.rs:220`) and `convert_message`
(`converters.rs:367`) both match on `Peer::{Channel,Group,User}` and extract the
same `(ChannelId, ChannelName, Username)` triple, including the repeated fallback
idiom:

```rust
ch.username()
    .and_then(|u| Username::new(u).ok())
    .unwrap_or_else(|| Username::new("unknown").unwrap())   // repeated 5×
```

### Why it matters
Minor, but it's the same extraction twice with the same magic fallbacks
("unknown", "group", "user"). A change to how a peer's display identity is derived
must touch both. (Also intersects CQ-1: those `.unwrap()`s.)

### Fix sketch
Extract one helper into the new `converters/channel.rs` (LM-4):

```rust
/// (id, display name, username-or-fallback) for a channel/group/user peer.
fn peer_identity(peer: &Peer) -> Option<(ChannelId, ChannelName, Username)> { … }
```

`convert_peer_to_channel` and `convert_message` both call it. Replace the
`.unwrap()` fallbacks with a single const-backed default (see CQ-1).

---

## AD-5 — `serde_json::to_string(..).map_err(|e| e.to_string())` repeated ~13× 🟢

**Effort:** S · **Risk:** Low · **Impact:** Low

### Evidence
Nearly every `*_impl` ends with the identical serialize-and-stringify-error tail
(`server.rs` lines 123, 148, 161, 189, 246, 315, 395, 432, 457, 513, 583, …).

### Why it matters
Low — it's idiomatic given the rmcp `Result<String, String>` contract. But a tiny
helper documents intent and removes the repeated `map_err`.

### Fix sketch
```rust
// in mcp/tools/helpers.rs
pub fn json_response<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}
```

Then `Ok(json_response(&response)?)` / `json_response(&SearchResponse::from(result))`.
Purely a readability win; skip if the team prefers the explicit form.

---

## AD-6 — Hard limits are hardcoded while their siblings are config-driven 🟡

**Effort:** M · **Risk:** Low · **Impact:** Medium

### Evidence
The project has a strong, consistent config story (`[timeouts]`, `[rate_limiting]`,
`[observability]`, `[search]`). Yet several operational limits of the same nature
are hardcoded constants scattered across modules:

| Constant | Location | Configurable sibling exists? |
|----------|----------|------------------------------|
| `MAX_DOWNLOAD_BYTES = 20 MB` | `client.rs:699` | yes — `media_download_cost` *is* in `[rate_limiting]` |
| `DEFAULT_MAX_DIMENSION/MIN/MAX` (image) | `server.rs:464–466` | no |
| `DEFAULT_TIMEOUT/MAX_TIMEOUT` (transcription) | `server.rs:525–526` | yes — other timeouts in `[timeouts]` |
| `MAX_BASE64_LEN`, `JPEG_QUALITY` | `image.rs:14,16` | no |
| link-preview truncation `500` | `converters.rs:360` | no |
| `POLL_INTERVAL_SECS = 2` | `transcription.rs:13` | no |

### Why it matters
Inconsistent: an operator can tune the *cost* of a media download but not its
*size cap*; can tune search/history/resolve timeouts but not the transcription
timeout bounds. It also scatters "magic numbers" that callers can't discover.

### Fix sketch (graduated — don't over-config)
1. **Centralize first, configure second.** Move the operational limits into a
   single `limits` module (or the relevant config table) so they're discoverable,
   even if some stay `const`.
2. Promote to config **only** the ones an operator plausibly tunes:
   - `media.max_download_bytes` → `[media]` or `[rate_limiting]`
   - `transcription.{default,max}_timeout_seconds` → `[timeouts]` or `[transcription]`
3. Leave genuinely-internal constants (`JPEG_QUALITY`, `POLL_INTERVAL_SECS`,
   the base64 cap, the 500-char preview cap) as named `const`s with a doc comment
   — promoting these would be premature configuration (KISS).

Keep all new fields `#[serde(default)]` so existing config files keep working
(matches the `TimeoutConfig` pattern in `config.rs`).

---

## Summary

| ID | Finding | Sev | Effort | Risk |
|----|---------|-----|--------|------|
| AD-1 | Consolidate 3 peer-resolution copies onto `resolve_peer` | 🔴 | M | Med |
| AD-2 | Stop double-resolving the username path of `get_recent_messages` | 🟡 | S | Low |
| AD-3 | Collapse the 11× tool-wrapper boilerplate (macro or guard) | 🟡 | M | Med |
| AD-4 | Share peer→identity extraction | 🟢 | S | Low |
| AD-5 | `json_response` helper for the serialize tail | 🟢 | S | Low |
| AD-6 | Centralize (and selectively config-ize) scattered limits | 🟡 | M | Low |
