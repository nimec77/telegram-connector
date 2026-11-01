# Versioned tool output and migration

MCP calls default to `[server] output_format = "json_v1"`. This application contract is
independent of MCP 2026-07-28 versus the retained 2025-11-25 handshake, and of a search
request’s `format = "full" | "compact"`. The default changes the application wire format;
existing clients that parse the historical text can explicitly select `output_format =
"legacy"` while migrating. No release/version change or publication is implied by this
implementation stage.

## Envelope and errors

All sixteen registered tools return one versioned JSON object in `structuredContent` and
the same JSON value in the first text content block. Success has required
`{"schema_version":"1","data":{...},"error":null}`; failure has required
`{"schema_version":"1","data":null,"error":{"code":"invalid_input","message":"...",
"retryable":false}}`. The version is a string, and exactly one of data/error is non-null.
Tool failures set MCP `isError:true`; successful partial batches set it false and retain
per-item failures in data. Each advertised outputSchema is the complete JSON Schema
2020-12 success/error union, including locally resolvable definitions.

Error codes are stable snake_case: invalid_input, rate_limited, timeout, unauthenticated,
unavailable, message_not_found, channel_not_found, ambiguous_channel, unsupported_peer,
unsupported_platform, desktop_open_failed, internal_error, all_channels_failed,
telegram_error, download_failed, no_visual_media, media_too_large, payload_cap_reached,
premium_required, transcription_failed, voice_too_long, not_transcribable, response_too_large, duplicate_recovery_key,
recovery_capacity_reached, response_pending and response_unavailable. A rate-limit
error includes integer retry_after_seconds and is retryable; timeouts are retryable. Other
categories conservatively report false, without claiming arbitrary provider failures are
transient. No automatic retry occurs. Canonical messages omit raw provider/auth/download
payloads. Typed argument decoding uses the input schema rather than exposing parser values.

Valid argument objects with missing required or invalid typed fields produce tool errors.
Missing/null arguments behave as an empty object: optional-only requests may succeed,
required-field requests fail. Non-object arguments, malformed call envelopes and unknown
tools remain JSON-RPC errors. Existing scalar coercions and unknown-field acceptance remain
at the request boundary. Uncorrelated protocol errors retain the SDK’s permitted omitted id.

## IDs, dates and partial data

Channel/user IDs are positive decimal strings; message IDs, reply IDs, album message IDs
and channel cursors remain integers. IDs are never recursively rewritten by JSON key name.
Message data carries channel_id even in compact mode and a neutral peer_namespace
(user/chat/channel); bare numbers are not globally unique. Full results retain distinct
same-number namespaces. Compact headers that would conflate identities or conflicting
name/username observations return ambiguous_channel in both modes. Full output avoids
header hoisting; an unambiguous public username can identify an eligible channel when
available. Neither workaround makes a private ambiguous bare ID globally unique.

Numeric resolution excludes user peers, deduplicates repeated typed identities and scans
the entire bounded dialog traversal before accepting a match. Different eligible namespaces
with the same bare number fail closed. This may read more pages than the old first-match
lookup, under the same resolve deadline and account charge. No extra resolution RPC is
performed merely to fill output IDs. Search retains a username-resolved namespace through
SearchParams and applies it to the existing dialog traversal; numeric-only requests remain
unqualified. An incomplete search dialog traversal keeps the existing deadline partial
semantics and never starts message fetches from a provisional identity. Source identity remains available for all-missing
message batches and all-failed media batches.

Dates serialize as RFC3339 UTC. Unknown counts and nullable base fields remain null;
optional enrichment fields are omitted when absent. Envelope data/error keys are always
present. Optional retry_after_seconds is omitted. Successful media entries carry an image
index; failed entries never do. Search retains partial/timed_out metadata, per-channel
errors, the absence of global/fan-out message cursors, text truncation and album metadata.
Transcription includes its resolved channel/namespace/message identity and preserves partial
text. Unavailable status observations remain explicit degraded data; unavailable replay or
corrupt stored JSON is a tool failure, not an empty/nullable success.

| Tool | Canonical data |
| --- | --- |
| check_mcp_status | Connection/cache observations, rates, media limits, counters and UTC session time |
| get_subscribed_channels, search_public_channels | channels, returned, nullable total/has_more |
| get_channel_info | Typed channel data |
| generate_message_link | Resolved channel identity, integer message_id and links |
| open_message_in_telegram | Successful launch result; real launch failure is a tool error |
| search_messages, get_recent_messages | Messages, headers, pagination/query metadata and typed channel errors |
| get_message_by_link | One message with explicit channel association |
| get_last_responses | Typed diagnostic entries with UTC dates and opaque stored JSON-RPC payloads |
| get_message_media, get_messages_media_batch | Ordered items plus requested/returned/failed and base64-budget summary |
| transcribe_voice_message | Resolved identity, text, partial flag, media type and optional duration |
| get_messages_batch | Resolved source, messages, typed missing entries and optional omitted IDs |
| resolve_channels | Ordered identifier/channel-or-error outcomes and counts |
| get_channel_stats | String channel_id, numeric statistics and UTC sample bounds |

Media’s first block is the JSON envelope; image blocks follow. `content_index` is a zero-based
index into the actual MCP content array, so the first image is index 1. JSON contains metadata
only; base64 occurs once in standard image blocks. Request order and every-ID accounting
remain intact. Existing no-image/shared-failure refund behavior and cancellation charging are
preserved. The two shared image CPU permits and nonblocking token-bound progress are reused.
Desktop launch is bounded by ten seconds and tested with a fake launcher, not the real app.
Its annotation declares side effects; transcription is not advertised as harmless/idempotent
because it may consume Telegram quota. Other tools accurately advertise read-only behavior.

## Legacy and Rust compatibility

Legacy mode retains historical JSON text shapes, numeric legacy domain IDs and image/text
pair ordering; its tool metadata has no output schema or conflicting structured content.
Desktop execution failure retains historical success:false data in legacy mode. Correctness
fixes still apply: constructor-invalid domain deserialization, ambiguous numeric resolution,
invalid local references, corrupt replay and conflicting compact attribution are rejected.

Public direct McpServer tool methods remain legacy convenience adapters regardless of the
configured wire mode. The registered *_wire entrypoints use typed operations directly;
there is no stringify/parse conversion between the two modes. `with_output_format` rebuilds
instance metadata in both directions. Explicit ServerConfig controls serving; default
run_stdio preserves the builder’s mode. Other existing constructors/builders remain.

Intentional Rust API changes: Message/Channel and response headers retain optional internal
peer_namespace (excluded from legacy JSON); ChannelIdentity carries the known namespace;
SearchParams adds optional channel_namespace metadata from prior username resolution;
MessageBatch and MediaDownload carry resolved channel/namespace; media batches return an
outer MediaBatch with ordered outcomes; TranscriptionOutcome carries source/message IDs.
Resolution trait results use ChannelResolution<Error>, and typed SearchResponse/related DTOs
support generic per-item errors while retaining String defaults for legacy readers. The
ordinary From<SearchResult> path remains available. External trait implementations and
struct-literal fixtures must populate truthful identity metadata; absent identity is never
invented for canonical output. New Error variants affect exhaustive matches. Domain ID/name
Deserialize now delegates to the original constructors, preserving valid JSON types while
rejecting invalid values and normalizing channel-name whitespace. See [Rust API](rust-api.md).

## Serialized budgets and targeted recovery

Search/history (including fan-out) and message batches count the full serialized JSON-RPC
response, including identity metadata and the duplicated structured JSON/text fallback,
against `[limits] response_byte_budget` (default 40,000). Trimming preserves request order,
oldest-first removal, continuation metadata and the one-message floor. The floor permits
one oversized message only when metadata itself fits; metadata overflow returns
`response_too_large`. This accounting can return fewer messages than earlier releases.

Media uses independent per-image/batch base64 limits and a separate 65,536-byte cap on
serialized non-binary overhead. Exceeding it returns a small error, no images, and refunds
all no-image charges. Image data appears once in image blocks, outside JSON metadata.

Every tool except `get_last_responses` accepts an optional wire-only `recovery_key` of
1–128 printable ASCII characters. Use unique random keys. A key is reserved before
dispatch; duplicate pending/retained keys fail without account charges or Telegram RPCs.
Canonical recovery requires `recovery_keys: ["your-key"]` and returns only those selections,
in selection order. Pending keys return per-key `response_pending`; missing, expired,
evicted, cancelled or restarted results return `response_unavailable`. Key errors are data
inside a successful recovery call. Results remain for ten minutes after completion, capacity
eviction or restart. Oversized payloads remain explicit stubs with the original wire size.
All pending slots reject new reservations rather than evicting active work. Cancellation
makes its slot unavailable and evictable. Recovery responses are never retained.

Keys prevent accidental selection mistakes; they provide no authenticated isolation. Calls
without keys have no targeted guarantee. Unfiltered last-N remains available in legacy
wire mode and through the direct Rust diagnostic adapter. Binary is omitted from replay
by default; `include_binary:true` requests retained bytes explicitly. The diagnostic ring
and keyed reservations are each bounded by `[observability] response_buffer_size`, and each
stored payload uses `max_buffered_payload_bytes` plus fixed omission-stub overhead.
Shared `recovery_key` fields are advertised at the wire schema boundary, preserving direct
Rust request literals. `GetLastResponsesRequest` adds optional `recovery_keys`.

Production MCP media uses a lazy ordered pipeline with four process-wide raw slots sharing
80 MiB. Its configured download timeout is one deadline through resolution, fetch, capacity
waits, transfers and CPU work. Started workers retain raw and processing permits until exit,
even when their request is cancelled. Deadline partial batches keep completed images and
refund unsuccessful IDs. Source images are limited to 8192 pixels per side, 8,388,608 pixels
and a 64 MiB decoder allowance. The decoder allowance is not a process RSS bound: resize,
encoded output, allocator and codec overhead remain separate. Status exposes each limit.

Rust consumers may use `download_messages_media_pipeline` and `MediaBatch::into_stream`.
The existing collected batch method retains its signature and behavior; its caller owns
retained raw memory. MediaDownload adds optional `lease`; MediaBatch adds optional `stream`
and `deadline`. External struct-literal fixtures supply None for these fields. Production
consumers move leases with bytes into blocking work, rather than cloning retained inputs.
`MediaDownload` value equality ignores operational lease identity. The new trait method has
a compatibility default for third-party clients, which retain responsibility for I/O/memory
bounds. No dependency or lockfile update is required.

On macOS, storage opens create new directories at 0700 and establish database/sidecar mode
0600 before SQLite can write credentials. Symlinks, hardlinks and nonregular entries are
refused. Canonical ancestry must be owned by the current user or root and exclude unprotected
group/other writes; root-owned sticky temporary roots are allowed. Access-granting directory
ACLs are rejected and file ACLs are cleared. Existing directory modes/ACLs are preserved.
Ownership/ACL utilities use captured output and bounded execution. Disposable tests cover
these cases and valid DB/WAL/SHM files; no real session was inspected or changed. Windows
ACL behavior remains unverified. Keyed recovery's historical written_at field records result
completion before delivery, while legacy diagnostic entries record observed stdout writes.

See [macOS validation](validation/2026-11-01-task8.md) for measured results and limitations.
Optional resource/file media delivery remains outside this change.

## Validator dependency

jsonschema 0.60.2 (MIT, declared MSRV 1.85) is dev-only with default features disabled.
Tests build Draft202012 validators with explicit format checks and offline reference handling.
The exact lock addition introduces 37 package identities and removes/updates none of the
previous resolved identities. All existing grammers revisions remain unchanged.

Older compatible lines are upstream constraints, not overrides: fancy-regex 0.19.2 requires
bit-set 0.8 (which requires bit-vec 0.8); fraction 0.17/num 0.4/num-rational 0.4 require
num-bigint 0.4; ahash 0.8 plus jsonschema/jsonschema-value/referencing 0.60 require getrandom
0.3; ahash 0.8 requires zerocopy 0.8. Moving these across incompatible ranges requires an
upstream migration. The highest declared MSRV among additions is 1.87, supported by the
selected nightly. Parent-reviewed package-specific MIT-0 and Zlib declarations cover
borrow-or-share 0.2.4 and foldhash 0.2.0; root licensing remains unassigned. Full graph policy,
source/advisory checks and offline schema tests validate the actual locked graph.
