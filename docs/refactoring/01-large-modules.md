# 01 — Large Modules

Splitting oversized modules into single-purpose units. Read alongside
`02-architecture-and-duplication.md` — LM-2 (split `client.rs`) and AD-1 (share
peer resolution) are best executed together.

> **Ordering note:** Do **LM-1 first**. Extracting inline tests is mechanical and
> test-backed, and it shrinks four files before any production code moves, making
> the production-only splits (LM-2/4/5) easier to review.

---

## LM-1 — Inline tests inflate modules; apply the existing `#[path]` convention 🟡

**Effort:** M · **Risk:** Low · **Impact:** High (removes the bulk of the "large
file" problem with zero production change)

### Evidence
The repo already extracts tests in three places:
- `config.rs` → `#[path = "config/tests.rs"]` (`config.rs:485`)
- `mcp/server.rs` → `#[path = "tests.rs"]` → `src/mcp/tests/*` (`server.rs:878`)
- `telegram/client.rs` → `src/telegram/tests/*`

But these equally-large files keep tests **inline**:

| File | Inline test lines (approx.) | First test line |
|------|----------------------------:|-----------------|
| `telegram/converters.rs` | ~371 | `:458` |
| `mcp/observability.rs` | ~381 | `:349` |
| `mcp/tools/types/serde_helpers.rs` | ~328 | `:178` |
| `rate_limiter.rs` | ~319 | `:105` |
| `mcp/tools/types/responses.rs` | ~196 | `:267` |
| `mcp/tools/types/requests.rs` | ~259 | `:167` |

### Why it matters
The convention is applied inconsistently, so "how big is this module's actual
logic?" can't be answered from the line count. Extracting tests is the
lowest-risk way to address the stated goal ("split large modules") because the
compiler proves equivalence — no behavior changes.

### Fix sketch
For each file above, move the `#[cfg(test)] mod tests { … }` block into a sibling
file and reference it with the established pattern:

```rust
// end of src/telegram/converters.rs
#[cfg(test)]
#[path = "tests/converters_tests.rs"]
mod tests;
```

Target layout:
- `src/telegram/tests/converters_tests.rs`
- `src/mcp/observability/tests.rs` (pairs with the LM-5 split below)
- `src/mcp/tools/types/tests/{serde_helpers,responses,requests}_tests.rs`
- `src/rate_limiter/tests.rs` (or `src/tests/rate_limiter_tests.rs`)

Keep `use super::*;` at the top of each extracted module. No test bodies change.

### After
`converters.rs` ~457 → ~457 prod-only (now clearly the real size), and the other
files drop to their production cores (e.g. `rate_limiter.rs` 423 → ~104).

---

## LM-2 — `telegram/client.rs` (923 lines, all production) 🔴

**Effort:** L · **Risk:** Med · **Impact:** High

The single largest production module, and the only one with no test-extraction
relief. It bundles five distinct responsibilities:

1. **Client lifecycle** — struct, `new()`, session/pool wiring, accessors
   (`client.rs:49–123`).
2. **Authentication** — `request_login_code`, `sign_in`, `check_password`
   (`client.rs:126–170`).
3. **Peer resolution** — `resolve_peer` + two *inlined* duplicates (see AD-1).
4. **The trait operations** — each large and self-contained:
   - `search_messages` ~135 lines (`client.rs:350–485`)
   - `get_recent_messages` ~140 lines (`client.rs:487–627`)
   - `download_message_media` ~145 lines (`client.rs:692–835`)
   - `get_message_by_id`, `get_channel_info`, `get_subscribed_channels`,
     `transcribe_audio`, `is_premium`
5. **The `with_timeout` utility** (`client.rs:35–46`) — a generic helper that has
   nothing to do with `TelegramClient` specifically.

### Why it matters
A 900-line file where one operation spans 145 lines is hard to navigate and
review, and it concentrates merge contention. Each operation is independently
testable and changes for different reasons (Single Responsibility).

### Constraint
You can only have **one** `impl TelegramClientTrait for TelegramClient` block, so
the trait methods can't literally live in different files *as trait methods*. The
idiomatic Rust split keeps the trait impl thin and delegates to **inherent
methods** (or free functions) defined in submodules.

### Fix sketch
Convert `client.rs` into a module directory (file-as-module is preserved — the
parent stays `client.rs` only if no dir; here introduce `src/telegram/client/`):

```
src/telegram/
  client.rs            -> declares: mod lifecycle; mod auth; mod resolve;
                          mod ops_search; mod ops_history; mod ops_media;
                          mod ops_transcribe;  + the trait impl (thin delegators)
  timeout.rs           -> with_timeout()  (moved out; pub(crate))
  client/
    lifecycle.rs       -> struct fields live here? (see note)
    auth.rs            -> request_login_code / sign_in / check_password (inherent)
    resolve.rs         -> resolve_peer + shared resolution helpers (AD-1)
    ops_search.rs      -> fn search_messages_impl(&self, ..)
    ops_history.rs     -> fn get_recent_messages_impl(&self, ..)
    ops_media.rs       -> fn download_message_media_impl(&self, ..)
    ops_transcribe.rs  -> fn transcribe_audio_impl(&self, ..) + invoke_transcribe
```

> **Note on the no-`mod.rs` rule:** `docs/conventions.md` forbids `mod.rs`. The
> directory form above is compatible — `client.rs` is the module file and
> `client/` holds its children, exactly like the existing `config.rs` +
> `config/tests.rs` and `mcp.rs` + `mcp/` pairs. No `mod.rs` is introduced.

The trait impl then reads as thin delegators:

```rust
#[async_trait::async_trait]
impl TelegramClientTrait for TelegramClient {
    async fn search_messages(&self, p: &SearchParams) -> Result<SearchResult, Error> {
        self.search_messages_impl(p).await
    }
    // …one line per method…
}
```

This mirrors the pattern already used in `server.rs` (public `#[tool]` wrapper →
private `*_impl`), so it's consistent with the house style.

### Sequencing
Pair with **AD-1**: when you create `resolve.rs`, fold the three resolution
copies into one helper at the same time (don't move duplicated code, then
de-duplicate it in a second pass — do both in the move).

---

## LM-3 — `mcp/server.rs` (877 lines production) 🟡

**Effort:** M · **Risk:** Low · **Impact:** Medium

The file holds three things: the `*_impl` methods (real logic, lines ~107–585),
the `#[tool_router]` block of 11 `#[tool]` wrappers (~587–835), and the
`ServerHandler` impl + `log_tool_outcome` (~839–875).

### Constraint (from CLAUDE.md)
The `#[tool]` methods **must** stay together in the `#[tool_router] impl` block —
the macro generates the router from that single block. That's a hard constraint
and should not be fought.

### Opportunity
The `*_impl` methods are **plain inherent methods** with no macro constraint.
Rust allows multiple inherent `impl McpServer<T, R>` blocks across files. Move
the impls into themed sibling files, leaving `server.rs` as the router + handler:

```
src/mcp/
  server.rs              -> struct, builders, run_stdio, #[tool_router] wrappers,
                            ServerHandler, log_tool_outcome
  server/
    impl_status.rs       -> check_mcp_status_impl, get_last_responses_impl
    impl_channels.rs     -> get_subscribed_channels_impl, get_channel_info_impl
    impl_links.rs        -> generate_message_link_impl, open_message_in_telegram_impl
    impl_search.rs       -> search_messages_impl, get_recent_messages_impl,
                            get_message_by_link_impl
    impl_media.rs        -> get_message_media_impl, transcribe_voice_message_impl
```

Each file is `impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static>
McpServer<T, R> { … }`. The wrappers in `server.rs` keep calling
`self.xxx_impl(..)` unchanged.

### Why it matters
`server.rs` drops to roughly the router + handler (~300 lines) — the part that's
genuinely coupled to the macro — and each group of related tool logic lives with
its peers. See AD-3 for tackling the wrapper boilerplate itself.

---

## LM-4 — `telegram/converters.rs` (~457 production) 🟡

**Effort:** M · **Risk:** Low · **Impact:** Medium

A flat bag of conversion functions spanning three sub-domains:

- **Media classification & sizing:** `convert_media_filter`,
  `convert_media_to_type`, `detect_document_type`, `extract_audio_duration`,
  `extract_video_info`, `extract_audio_info`, `matches_media_filter`,
  `select_size_candidate`, `size_candidates` (`:12–319`).
- **Message assembly:** `extract_forward_info`, `extract_link_preview`,
  `convert_message` (`:327–456`).
- **Channel/peer:** `convert_peer_to_channel` (`:220–273`).

### Fix sketch
After LM-1 extracts the tests, split production into:

```
src/telegram/converters.rs        -> declares submodules + re-exports
src/telegram/converters/
  media.rs      -> filter/type detection, video/audio info, size candidates
  message.rs    -> convert_message, forward header, link preview
  channel.rs    -> convert_peer_to_channel (+ shared peer identity, see AD-4)
```

Keep the public function names and re-export them from `converters.rs` so
`client.rs`'s import list is unchanged.

### Why it matters
Each sub-domain changes for different reasons (media handling vs message shape vs
channel metadata). This split also gives AD-4 (shared peer-identity extraction) a
natural home.

---

## LM-5 — `mcp/observability.rs` (~348 production) 🟡

**Effort:** M · **Risk:** Low · **Impact:** Medium

Three independent units in one file:

- `SessionMetrics` + `InFlightRequest` (counters, uptime) — `:20–133`
- `ResponseBuffer` + `BufferedResponse` + `OVERSIZED_PAYLOAD_STUB` (ring buffer)
  — `:135–207`
- `InstrumentedTransport` (the `Transport` wrapper) + `is_slow_write` — `:210–347`

These share only small constants. They're separately testable (the inline tests
already cluster by unit).

### Fix sketch
```
src/mcp/observability.rs    -> re-exports + shared consts
src/mcp/observability/
  metrics.rs                -> SessionMetrics, InFlightRequest
  buffer.rs                 -> ResponseBuffer, BufferedResponse
  transport.rs              -> InstrumentedTransport, is_slow_write
  tests/…                   -> per-unit (pairs with LM-1)
```

Re-export from `observability.rs` so `server.rs`'s
`use crate::mcp::observability::{InstrumentedTransport, ResponseBuffer,
SessionMetrics}` keeps working.

---

## LM-6 — `config.rs` (~485 production) 🟢

**Effort:** S · **Risk:** Low · **Impact:** Low–Medium

Two clearly separable concerns clutter the file:

1. **27 `default_*` free functions** (`config.rs:5–137`) — ~130 lines of tiny
   serde default providers ahead of the actual types.
2. **Env-var expansion** — `expand_env_vars` / `expand_env_vars_in_line`
   (`config.rs:423–482`), a self-contained string-processing concern with its own
   test surface.

### Fix sketch
```
src/config.rs            -> structs, impls, load/load_from, validation
src/config/defaults.rs   -> the default_* fns (pub(crate))
src/config/env.rs        -> expand_env_vars[_in_line]
src/config/tests.rs      -> already exists
```

The `#[serde(default = "…")]` attributes reference `defaults::default_xxx` after
the move.

### Why it matters (lower priority)
This is cosmetic relative to LM-2/LM-3 — `config.rs` is mostly declarative struct
definitions, which are easy to scan even at 485 lines. Schedule it only when
touching config for another reason. Note CLAUDE.md requires config tests to run
single-threaded (`--test-threads=1`); the split must not change that.

---

## Summary

| ID | File | Sev | Effort | Risk | First? |
|----|------|-----|--------|------|--------|
| LM-1 | (4 files) test extraction | 🟡 | M | Low | ✅ do first |
| LM-2 | `client.rs` | 🔴 | L | Med | with AD-1 |
| LM-3 | `server.rs` | 🟡 | M | Low | |
| LM-4 | `converters.rs` | 🟡 | M | Low | after LM-1 |
| LM-5 | `observability.rs` | 🟡 | M | Low | after LM-1 |
| LM-6 | `config.rs` | 🟢 | S | Low | opportunistic |
