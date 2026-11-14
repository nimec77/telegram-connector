# Sending Telegram messages and media

Date: 2026-11-14

Status: Draft specification for review; no implementation is authorized by this document.

Baseline: Telegram MCP Connector 0.25.0; grammers revision
`1e31e73cd9797790905bbea82cd75adcbe6f1242`; rmcp 3.8.

## 1. Intent and decisions

Extend the existing authenticated MTProto connector so MCP clients can send text,
images, audio and video using the connected Telegram account. The user explicitly
confirmed destinations: channels, groups, private chats and Saved Messages.

All other product choices below are **proposals**, not recorded user approvals:

- Two tools: `send_message` and `send_media`.
- Sending is opt-in through configuration. An enabled tool call performs the send;
  there is no connector-owned confirmation dialog or prepare/commit ceremony.
- Media comes from configured local upload folders. Audio includes regular audio
  attachments and prepared voice notes. These are proposed defaults awaiting
  confirmation.
- One destination and at most one attachment per call; optional caption, reply,
  forum topic and silent delivery. Plain text only in this version.
- No application-level resend or durable outgoing queue. An uncertain final send
  is reported as uncertain, never as a safely retryable failure.

Success means Telegram acknowledged a newly created message and the connector can
return its actual destination and positive message ID. It does not mean a recipient
read the message, received a notification, or finished downloading a video.

## 2. Current implementation and feasibility

| Existing component | Consequence for this change |
| --- | --- |
| `src/mcp/server.rs`: 16 tools, one `#[tool_router]` impl | Add thin routes here; put sending operations in a sibling implementation module. |
| `src/telegram/trait_def.rs` and `client/ops_*.rs` | Extend the existing client seam and keep production decisions testable beneath whole-client mocks. |
| `client/resolve.rs` excludes user peers, including numeric dialog matches | Add a separate recipient resolver; changing the read resolver would alter established contracts. |
| Domain IDs and `PeerNamespace` distinguish user/chat/channel | Reuse these invariants; never treat a bare number as a globally unique recipient. |
| Canonical `json_v1`, safe typed errors, self-contained schemas | Extend the canonical contract rather than returning upstream responses or display strings. |
| Input frames default to 1 MiB; 16 requests can be in flight | Large inline base64 is a poor default. Upload byte/concurrency limits must be separate. |
| Existing media artifacts are processed JPEG previews | They are not an upload API or originals. Keep outgoing staging separate. |
| Recovery keys reserve bounded, process-local response slots | Useful after lost responses, but not a durable idempotency ledger. |

The pinned dependency already provides `Client::upload_stream`, photo/document
builders, audio/voice/video attributes and the requisite generated TL requests.
No grammers upgrade or Bot API integration is needed for the proposed scope.

Two details require deliberate handling:

1. `grammers-client/src/client/messages.rs::send_message` generates a fresh
   `random_id` each invocation. When response mapping fails, it can return a
   `MessageEmpty` placeholder with ID zero and log raw updates. The connector must
   not treat this as success or introduce that logging path for outgoing content.
2. `SenderPool::new` uses default connection parameters. At this revision,
   `AutoSleep` retries qualifying flood/slow-mode and I/O failures. “No retries in
   connector operation code” does not imply “no retries in the dependency.”

Use the public upload API and a small raw TL adapter for the final send and receipt
mapping. Retain a clone of the pool's public thin handle at construction, and use
`raw_invoke_in_dc` for the final serialized send. It bypasses the pool's application
error retry loop without changing existing read behavior. Protocol-level resend of
the same serialized request can still occur; retain its original `random_id`.
This design uses public APIs only, with an offline adapter feasibility check as an
implementation prerequisite.

Telegram documents separate text/media methods and a `random_id` used for resend
deduplication. This does not establish an unlimited or cross-restart exactly-once
guarantee. See [sendMessage](https://core.telegram.org/method/messages.sendMessage),
[sendMedia](https://core.telegram.org/method/messages.sendMedia) and
[update mapping](https://core.telegram.org/api/updates).

## 3. Approaches considered

| Approach | Tradeoff | Disposition |
| --- | --- | --- |
| One `send_message` with a tagged text/media payload | Small catalog, but all calls inherit a larger conditional schema. | Viable alternative. |
| `send_message` plus `send_media` with image/audio/voice/video variants | Simple text calls, shared upload/error policy, explicit media requirements. | Recommended; not yet user-approved. |
| Four or five separate content-specific tools | Very simple individual schemas; duplicates destination/options and expands the catalog. | Unnecessary for the initial scope. |

Continue using the existing user session and MTProto. A separate bot changes the
sender, permissions and authentication workflow. A prepare/send protocol or durable
outbox could support stronger retry guarantees later, but adds lifecycle and storage
requirements beyond sending a single message.

## 4. Scope and exclusions

Required: text, photo-style images, playable audio attachments, prepared voice notes,
ordinary videos, captions, replies, explicit forum-topic routing, Saved Messages,
typed receipts, bounded uploads, rate charging, cancellation semantics and offline
behavior tests.

Excluded: albums, bulk/fan-out delivery, scheduling, editing/deleting/forwarding,
contact import, joining chats, secret chats, stories, round video notes, recording,
transcoding, arbitrary documents, URL fetching, inline binary input, custom thumbnails,
HTML/Markdown/entities, alternate `send_as` identities, paid messages and paid flood
bypass. The existing read tools do not expand to private-chat history in this change.

Do not silently split long text, truncate captions, change a media kind into a generic
file, substitute another recipient, fall back from a topic to the main chat, or report
an upload as a sent message. Do not clear Telegram drafts.

## 5. MCP requests

### Common destination and options

`recipient` is a tagged object with exactly one of these forms:

```json
{"kind":"saved_messages"}
```

```json
{"kind":"username","username":"@example","expected_namespace":"user"}
```

```json
{"kind":"peer","namespace":"channel","id":"1234567890"}
```

`expected_namespace` is required for username input and accepts `user` or `channel`.
Qualified IDs accept `user`, `chat` or `channel`; supergroups and broadcast channels
use `channel`, basic groups use `chat`. IDs are positive decimal strings and retain
existing safe scalar coercion at the boundary. Reject bare numbers, display names,
phone numbers, invitation links and Bot API `-100...` notation.

Username resolution must match the requested namespace. Qualified IDs require a
known usable peer reference from the session or one bounded dialog traversal,
including user dialogs. Never synthesize access hashes. Do not redirect a migrated
group automatically: return a safe recipient error so the caller can choose its new
identity. Saved Messages resolves to the connected user's own identity before sending.

| Common field | Contract |
| --- | --- |
| `recipient` | Required tagged object above. |
| `reply_to_message_id` | Optional positive integer; same resolved destination. |
| `topic_id` | Optional positive integer identifying a forum topic root. Required for forum destinations, including explicit General-topic selection; forbidden for other destinations. |
| `silent` | Boolean, default false; requests Telegram silent delivery. |
| `recovery_key` | Existing optional wire-only recovery key; strongly recommended. Its existing retention/duplicate semantics remain unchanged. |

Resolve once and retain the typed recipient through upload and final send. Username
changes during the call must not redirect it. Validate replies/topic roots with
bounded reads after acquisition and before any upload. A reply must exist in that
destination and belong to the selected topic when applicable. Deleted, inaccessible,
closed or inconsistent targets fail without a final send. Telegram remains the final
authority on permissions and races after validation. General-topic validation must
use Telegram's actual topic semantics, not assume a normal message exists at its ID.

New write request objects reject unknown fields, unlike compatibility-preserved read
requests. This prevents misspelled delivery options being silently ignored. Inline
all request unions/enums needed by the actual advertised schema.

### `send_message`

Additional fields:

| Field | Contract |
| --- | --- |
| `text` | Required nonempty string, preserved verbatim. Whitespace-only content is invalid. |
| `link_preview` | Boolean, default false. |

Use a conservative connector maximum of 4,096 UTF-16 code units. Do not normalize,
parse formatting or split automatically. UTF-16 counting deliberately bounds emoji
as well as ordinary text; this is a connector policy, not a claim that all Telegram
limits use that counting rule. Provider limits can still be stricter.

```json
{
  "recipient":{"kind":"saved_messages"},
  "text":"Build completed successfully.",
  "silent":true,
  "recovery_key":"release-report-unique-call-01"
}
```

### `send_media`

Additional fields are `source`, `media` and optional `caption` (default empty; maximum
1,024 UTF-16 code units). The caption is sent with the attachment in one message.

`source` is `{"root":"outgoing","relative_path":"reports/demo.mp4"}`. `root` is a
configured alias, and `relative_path` identifies a file on the **connector host**.
No client filesystem, arbitrary resource URI or remote URL is implicitly accessible.

`media` is one of these tagged variants:

| Kind | Initially supported input | Required metadata | Optional metadata |
| --- | --- | --- | --- |
| `image` | JPEG, PNG | None | None |
| `audio` | MP3, M4A/AAC | `duration_seconds`: positive integer, at most signed 32-bit maximum | `title`, `performer`: at most 256 UTF-16 units each |
| `voice` | Ogg/Opus | `duration_seconds`, same range | None; waveform omitted |
| `video` | MP4 with H.264 and optional AAC audio | Finite positive `duration_seconds`; positive integer `width`, `height`, each at most 8,192 | None |

Metadata is caller-supplied in v1; the connector does not install or launch FFmpeg.
Document this requirement in tool descriptions. Inspect bounded signatures/container
headers to reject obvious kind mismatches and use controlled MIME/filename mappings.
Do not claim full codec validation from a header check. Telegram's parser may reject
a structurally plausible but invalid file. That is a typed failure, with no fallback.

Images are sent as Telegram photos, so Telegram may compress/convert them. Audio uses
an audio document attribute; voice uses its voice flag; video uses a video attribute
with `round_message=false` and conservative `supports_streaming=false`. Optional
title/performer are explicit caller values rather than extracted tags. These attributes
are described in [audio](https://core.telegram.org/constructor/documentAttributeAudio)
and [video](https://core.telegram.org/constructor/documentAttributeVideo).

```json
{
  "recipient":{"kind":"peer","namespace":"channel","id":"1234567890"},
  "source":{"root":"outgoing","relative_path":"reports/demo.mp4"},
  "media":{"kind":"video","duration_seconds":12.5,"width":1280,"height":720},
  "caption":"Demonstration of the new workflow.",
  "recovery_key":"demo-upload-unique-call-01"
}
```

## 6. Local upload contract

Use configured source roots and a separate private per-process staging directory.
Validate roots at startup and require existing absolute directories. Root aliases are
unique, nonempty, bounded strings. Empty source-root configuration permits text sends
but rejects media before acquisition. Never default to the home directory or project.

Resolve paths component-wise beneath the selected root. Reject absolute paths, `..`,
symlinks in the submitted path, nonregular files and multiply-linked files. Use an
opened descriptor with verified file identity, rather than validate then reopen an
unchecked path. Trust assumptions match existing local storage policy: hostile
same-UID mutation is outside the boundary; do not claim protection against it.
Protect roots/ancestry against other-user replacement using the existing storage
policy, and reject a root that contains connector credentials, session files or logs.

Copy into a create-new private staging file before any Telegram RPC. Enforce actual
streamed bytes, not just `stat`; detect truncation/growth and source identity changes.
Inspect the staged snapshot, then upload that same snapshot. Never modify the source.
Use a sanitized basename with a verified-kind extension as Telegram's filename;
never send a local directory path as a filename.

Proposed limits: images 10 MiB; audio/voice/video 100 MiB each; two concurrent media
operations; 200 MiB aggregate staging reservations; 512 KiB copy buffers. Reserve
declared size before staging and enforce actual limits while copying. Reject capacity
exhaustion promptly before account charging. All allocations, probing and started
blocking work retain their resource permits until they actually finish. Use bounded
image header/dimension inspection; no full image decoding is needed for uploading.

Stream staging files through `upload_stream`; do not load complete videos into RAM.
The pinned uploader uses bounded parts and internal parallel workers. Account for its
working memory separately from staging bytes; verify observed bounds under two
concurrent uploads. Upload data does not enter the MCP request/response binary budgets.
Telegram's upload protocol is documented [here](https://core.telegram.org/api/files).

Delete staging files on completion, rejection, failure, cancellation and shutdown.
An active cleanup operation shares the original shutdown deadline. On cleanup failure,
retain the occupied quota and refuse conflicting reuse. Use a fresh empty private
root after a crash; never sweep arbitrary prior directories. Reuse the existing
0700/0600 and macOS ACL rules. No outgoing queue or source bytes survive intentionally.

## 7. Admission, deadlines and send state

Each invocation follows this order:

1. Existing recovery reservation and request decode; reject disabled sending,
   unsupported output mode, malformed fields and locally invalid recipient shapes.
2. Preflight worst-case receipt/error serialization against the real wire budget,
   including request identity. Admit bounded local work, validate/stage media and
   prepare all purely local request data.
3. Acquire the configured account cost exactly once, nonblocking.
4. Resolve recipient, validate reply/topic and perform any required bounded reads.
5. Upload staged media, if present; build final send request with one nonzero random
   64-bit ID generated using the existing CSPRNG dependency.
6. Mark final-send dispatch before enqueueing the mutation; submit one final request.
7. Match its acknowledgement and render the receipt; release resources.

No resolve, account refresh, reply lookup or upload RPC occurs before acquisition.
Charge text at one token and media at three by default. These are costs per operation,
not per upload part. Failed resolution, upload, provider rejection, timeout and
cancellation after acquisition do not refund the charge. All upload parts run under
that prior admission. Existing read/media-download refund rules are unchanged.

Start one absolute operation deadline after basic decoding, before staging/admission.
Proposed totals: text 30 seconds; media 120 seconds. Remaining time bounds staging,
resolution, dependency waits, upload, final send and receipt mapping. Existing per-RPC
resolve/history caps are additionally bounded by that same remaining time. Neither
moving to upload nor completing a part restarts the deadline. There is no queue that
silently waits beyond it. Client tool timeouts must exceed the configured total with
transport headroom; the current 180-second example covers these defaults.

Use three observable outcomes:

| State at completion | Result |
| --- | --- |
| Final send was never dispatched, or Telegram definitively rejected it | Error with `delivery_state:"not_sent"`. Media may already have been uploaded to Telegram. |
| Acknowledgement contains a matched real message identity | Success with `delivery_state:"sent"`. |
| Final send was dispatched, but acceptance cannot be established or ruled out | `delivery_unknown`, `retryable:false`, `delivery_state:"unknown"`. |

Upload completion alone is not final dispatch. Timeouts before final dispatch can be
retryable; timeouts, I/O loss, response decoding/mapping failure and cancellation
after dispatch must never be translated through the existing generic retryable timeout
mapping. A definitive flood/slow-mode refusal supplies `retry_after_seconds`; do not
sleep and resend the final mutation. Payment flags remain unset; payment-required
responses fail rather than spending Stars.

The upload helper's existing bounded retries are allowed only within the same total
deadline, before the final send. The final adapter bypasses `AutoSleep`; lower-level
MTProto retransmission must preserve the serialized request and its random ID. Tests
must distinguish these layers. Do not change the pool-wide policy for existing reads.

Cancellation stops launching further work, drops awaited futures and schedules no
resend. Already enqueued requests and started filesystem work cannot be assumed
retracted. Existing MCP cancellation may suppress a result entirely; a caller that
loses a response after dispatch must treat the outcome as unknown. Shutdown follows
the same rule. There is no ordering guarantee between concurrent calls; clients that
need ordering must await each result before starting the next call.

## 8. Receipts, errors and recovery

Canonical success uses the existing versioned envelope and matching JSON text block:

```json
{
  "schema_version":"1",
  "data":{
    "delivery_state":"sent",
    "recipient":{"peer_namespace":"user","peer_id":"123456789","saved_messages":true},
    "message_id":42,
    "date":"2026-11-14T08:30:00Z",
    "content_kind":"text",
    "reply_to_message_id":null,
    "topic_id":null,
    "message_link":null
  },
  "error":null
}
```

`content_kind` is `text|image|audio|voice|video`. Media receipts additionally contain
`media:{"mime_type":"video/mp4","size_bytes":12345}` describing submitted bytes,
not a claim about Telegram's final compressed size. Optional public/private links
are created only when existing link rules support the resolved namespace; private
users and Saved Messages may have null links. No extra RPC is made just for a link.
Do not echo message bodies, captions, source paths, access hashes or binary data.

Accept `updateShortSentMessage` only as the direct response to this invocation with
a positive ID/date. For full/combined Updates, correlate the original random ID via
`updateMessageID` and match a new message's typed destination. Never take the first
unrelated update or a zero/empty message. A positive ID mapping without enough receipt
information is conservatively unknown; do not fabricate dates or send a second time.

Errors retain `data:null`, safe `ToolError` fields and `isError:true`. Add an optional
`delivery_state` field to `ToolError`, omitted for existing tools, with write failures
limited to `not_sent|unknown`. Add stable codes as needed:

| Code | Meaning / retry policy |
| --- | --- |
| `sending_disabled` | Configuration refuses writes; not retryable. |
| `recipient_not_found`, `recipient_mismatch` | Missing usable reference or wrong namespace; not retryable. |
| `send_forbidden` | Permissions, block, media restriction or paid delivery requirement; not retryable. |
| `invalid_reply_target` | Missing/wrong reply or topic; not retryable. |
| `upload_source_invalid`, `unsupported_media` | Local source/type/metadata validation failed; not retryable. |
| `upload_failed` | Upload failed before dispatch; conservatively nonretryable unless a typed transient cause is established. |
| `send_capacity_reached` | Bounded local admission refused before sending; retryable. |
| `delivery_unknown` | Final dispatch may have committed; never automatically retryable. |

Reuse `invalid_input`, `media_too_large`, `rate_limited`, `unauthenticated`, `timeout`
and safe internal errors where their meanings fit. Unknown upstream errors after
dispatch default to `delivery_unknown`; only allowlisted definitive rejections can
claim `not_sent`. Do not derive typed categories by parsing display strings. Probe
and upstream stderr/payloads must not leak through error source chains or tracing.

Recommend a unique `recovery_key` per intended call and `get_last_responses` if its
response is lost. Retained duplicate keys continue to reject without sending again;
they do not replay a new mutation. A missing, expired, cancelled, oversized or evicted
recovery entry is **not evidence that nothing was sent**. Restart clears recovery.
The key and Telegram random ID are different mechanisms. Reusing a key after eviction
or issuing a new call can create a duplicate. No cross-call or cross-restart exactly-once
guarantee is part of v1. Inspect Telegram before choosing to repeat an uncertain send.

## 9. Configuration, compatibility and observability

Proposed configuration (values shown are defaults, except illustrative paths):

```toml
[sending]
enabled = false
max_image_bytes = 10485760
max_media_bytes = 104857600
max_concurrent_media = 2
max_staging_bytes = 209715200
staging_root = "/absolute/private/empty-outgoing-staging"

[sending.upload_roots]
outgoing = "/absolute/private/prepared-attachments"

[telegram.timeouts]
send_secs = 30
upload_send_secs = 120

[rate_limiting]
send_message_cost = 1
send_media_cost = 3
```

The new `[sending]` table defaults to disabled with no roots/staging path; do not create
directories while disabled. Enabling text-only sending needs no staging directory.
Configuring media roots requires a separate private empty staging root. Validate
nonzero finite timeouts/costs, costs at most bucket capacity, nonzero caps and enough
staging quota for at least one maximum-sized permitted file. Per-file caps may be
lowered, not raised above the stated initial limits, until larger-file behavior is
separately validated. Media concurrency may be configured as one or two.

Expose effective sending availability and limits in cached `check_mcp_status` without
new RPCs or filesystem paths. Sending enabled with `output_format="legacy"` is a
configuration error in v1. Keep existing legacy reads fully supported; new writes
require the canonical contract. This avoids introducing mutation outcomes that old
text parsers might interpret as success. Disabled catalogs remain at 16 tools;
enabled canonical catalogs have 18. Both negotiated protocol versions are supported.

Omit disabled send tools from `tools/list`, and independently enforce the enable flag
at dispatch and operation boundaries so a direct name invocation cannot bypass it.
No response caching hints imply send idempotency. Tool annotations are
`read_only_hint=false`, `destructive_hint=false` (additive creation),
`idempotent_hint=false`, `open_world_hint=true`. Descriptions state that calls send
immediately as the authenticated account, under Telegram's ordinary sender rules,
including channel posting identity. No per-call sender override is offered.

Record tool outcome, stage, duration, bounded byte counts and error category. Do not
log text, captions, attachment contents, paths, usernames, raw Updates or raw provider
errors. Recovery retains receipt/error metadata under the existing bounded policy;
it does not become an attachment cache. Update tool-count, schema, annotation, fixture
and both-protocol tests deliberately rather than weakening their assertions.

## 10. Architecture and implementation boundaries

| Area | Proposed change |
| --- | --- |
| `src/mcp/server.rs` | Two thin wire entries, tool visibility and enable enforcement. |
| `src/mcp/server/impl_send.rs` | Decode, local preparation, charging, operation invocation and typed rendering. |
| `src/mcp/tools/types/sending.rs` | Request unions and receipt DTOs; canonical error/schema integration. |
| `src/telegram/types/` | Validated recipient/send options and sent-message result; keep boundary coercion out. |
| `src/telegram/trait_def.rs`, `client.rs` | Typed send methods and thin dispatch; update every mock/example implementer. |
| `src/telegram/client/resolve_recipient.rs` | General write-specific resolution, preserving existing channel resolver. |
| `src/telegram/client/ops_send.rs` | Deadline/state transitions, raw final adapter and safe acknowledgement mapping. |
| Focused outgoing-file module | Root validation, snapshot staging, permits, cleanup and streaming. Share proven storage helpers only where contracts match. |
| Config/defaults/validation, `main.rs` | Validated settings and startup wiring. |
| Error/contract/status/observability | Safe outcome classification and visibility. |

Use named option structs, file-as-module layout and documented fallible APIs. Keep
pure recipient selection, media request construction, acknowledgement parsing and
error classification independently testable. Introduce a narrow transport adapter
for final submission only where needed to exercise dispatch/unknown outcomes beneath
`MockTelegramClientTrait`. Do not add an alternate general Telegram client framework.

No new third-party dependency is anticipated. Streaming may require explicitly
enabling the existing Tokio `fs`/`io-util` features; document actual use and inspect
any lockfile diff. Keep all grammers pins unchanged. Do not hand-roll full media
decoders, use hidden dependency APIs, or invoke external programs for metadata.
An inability to implement the safe public-API adapter is a finding to resolve before
implementation proceeds, not permission to silently weaken send semantics.

## 11. Acceptance and validation

Write failing observable tests before production behavior changes. Required coverage:

1. **Catalog/compatibility:** 16 disabled and 18 enabled tools; direct disabled-call
   refusal; output-mode configuration rejection; correct annotations; self-contained
   schemas and actual envelopes under both supported protocol versions. Existing read
   outputs, recovery behavior and rate/refund rules remain unchanged.
2. **Validation/admission:** malformed recipient, empty/overlong Unicode text/caption,
   invalid IDs, unknown fields, impossible metadata, unsupported type, source failure,
   insufficient quota and response-overhead overflow all produce zero Telegram RPCs
   and no account charge. Include emoji/surrogate-pair boundaries.
3. **Destinations:** users/basic groups/supergroups/channels/Saved Messages; colliding
   bare IDs; username namespace mismatch; unknown access reference; migrated/deleted
   peers; bounded dialog traversal; no contact import or join.
4. **Content:** one real send request per admitted call, correct text/caption, photo,
   audio, voice and video TL attributes; original content preserved; silent and preview
   flags; no clear-draft, paid, schedule or send-as override flags. Missing reply/topic
   never falls back to a different destination/thread.
5. **Files/resources:** escaping paths, symlinks, hardlinks, nonregular files, source
   mutation, lying size, boundary-sized streams, staged snapshots, MIME mismatches,
   concurrency/quota, failure cleanup and slow blocking work retaining permits. Use
   disposable fixtures only; verify upload chunking without buffering entire files.
6. **Charging/deadlines:** one configured charge before every network path; no refunds
   after admission; paused-time coverage through staging, resolution, upload parts,
   dependency retry waits and final send. No deadline restarts or extra call at expiry.
7. **Delivery truth:** short/full/combined acknowledgements, unrelated updates,
   random-ID mismatch, empty/zero-ID results, missing dates, explicit rejection,
   before/after-dispatch timeout and connection loss. Unknown never becomes generic
   retryable timeout, success or automatic resend.
8. **Recovery/cancellation:** recover a completed receipt; duplicate key rejected before
   send; lost/cancelled/expired/restarted recovery remains explicitly inconclusive;
   cancellation prevents further stages; shutdown does not claim unsent after dispatch.
9. **Privacy:** hostile filenames, text and provider errors do not appear in logs or
   model-visible error chains; serialized responses contain no binary or local paths.

Run the normal macOS nightly gate, using cached dependencies with `--offline --locked`:
format check, all-target Clippy with warnings denied, tests/doctests and warning-free
rustdoc. Update offline MCP fixtures, examples, README, tool contract, Rust API guide,
configuration example and durable memory facts when implementation lands. No production
credentials or live sends are part of offline verification.

Live acceptance is a separate owner-authorized step: send each content kind to Saved
Messages, then selected test chats with posting permission; inspect actual playback,
photo rendering, caption/reply/topic placement and returned IDs. Record account-dependent
restrictions and platform limitations. Linux remains owner-managed. Live tests must
name their destinations before execution; this specification does not authorize them.

## 12. Delivery outline and remaining choices

Suggested order for a later implementation plan:

1. Validate the pinned public raw-send adapter, response mapping and upload behavior
   with offline fixtures; settle the transport and staging seams.
2. Add destination/options/configuration contracts and text sends with outcome truth.
3. Add bounded file staging and all four media variants.
4. Complete wire compatibility, cancellation/recovery/limits tests and documentation;
   obtain independent code review, then owner-authorized live acceptance if requested.

The draft is ready to plan against with its proposed defaults. Product choices to
confirm before treating it as an approved implementation specification are media
source policy, voice-note inclusion, the canonical-only write requirement and the
limited retry guarantee. If durable safe resubmission across restarts is required,
design a persisted outbox and query/reconciliation contract as additional scope;
do not relabel the current recovery buffer as idempotency.

Exact internal type names, helper placement and fixture structure are left to the
builder within the architecture and behavioral requirements above. This document is
a specification, not an implementation plan. Remove it when its eventual change is
merged, following the repository's design-document lifecycle.
