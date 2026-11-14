# Sending messages and media

Sending is an explicit opt-in feature of the authenticated MTProto account. Calls send
immediately, using Telegram's ordinary account/channel posting identity; there is no
connector confirmation dialog or per-call sender override. The default catalog has 16
tools. Enabling sending with `output_format = "json_v1"` adds `send_message` and
`send_media`. Both supported MCP protocol versions use the same canonical receipts.
Enabling sending with legacy output is a configuration error; existing legacy reads
remain supported.

## Enable text or local media

Text-only sending needs no filesystem configuration:

```toml
[sending]
enabled = true
```

For media, create an existing prepared-attachment directory and a **separate, empty,
private staging directory** owned by the connector user. Keep configuration, sessions,
logs and media-artifact caches outside upload roots. A root must not contain those
locations, even when a protected file does not exist yet. Do not grant other users write
access through modes or ACLs. Staging requires mode 0700 and creates mode-0600 files.
No upload root defaults to the home directory or project.

```toml
[server]
output_format = "json_v1"

[sending]
enabled = true
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

`source.root` selects an alias; `source.relative_path` is relative to that directory
on the **connector host**, not the MCP client's computer. Aliases contain 1–64 ASCII
letters, digits, underscores or hyphens. Source roots must exist and be absolute.
Paths cannot escape a root, traverse submitted symlinks, or select hardlinked or
nonregular files. The connector checks ownership/ancestry and takes a verified private
snapshot before account charging or Telegram RPCs. It uploads the snapshot unchanged;
it never edits the source. Avoid changing a source during preparation. Hostile mutation
by the same operating-system user is outside the storage boundary.

Image caps can be lowered from 10 MiB; audio/voice/video caps can be lowered from
100 MiB. They cannot be raised above those initial maxima. Media concurrency is one or
two, with nonwaiting capacity admission; staging quota must fit at least one largest
permitted file. These limits apply to the shared staging store, including server clones.
Upload parts use separate working memory from the on-disk quota and MCP response/image
budgets. They are streamed, not embedded as base64 in tool arguments.

Use client budgets above the configured operation totals. The example's 180-second tool
timeout covers these defaults; its 120-second startup budget also leaves room for
authorization, Premium observation and optional media-root initialization. Increase client
budgets if increasing server deadlines.

## Choose an exact recipient and thread

Accepted recipient objects are:

```json
{"kind":"saved_messages"}
```

```json
{"kind":"username","username":"@example_user","expected_namespace":"user"}
```

```json
{"kind":"peer","namespace":"channel","id":"1234567890"}
```

Username input requires `expected_namespace: "user" | "channel"`. Qualified numeric IDs
require `namespace: "user" | "chat" | "channel"`; `chat` means a basic group, while
supergroups and broadcast channels use `channel`. IDs are positive decimal strings;
existing safe scalar coercion also accepts integer ID scalars at this boundary. Bare
numbers, display names, phone numbers, invitation links and Bot API `-100...` notation
are not destinations. Numeric destinations need usable session authority or a bounded
dialog lookup. No contact import or chat join is performed, and migrated groups do not
redirect automatically. Community containers and channel direct-message monoforums are
not supported by this initial routing contract.

An optional `reply_to_message_id` must be a positive Telegram message ID in the same
resolved destination. A forum destination always requires an explicit `topic_id`, even
for General (`1`); nonforums forbid it. Replies must belong to the selected topic.
Missing, inaccessible, closed, deleted or inconsistent targets fail before upload and
never fall back to another chat/thread. General is validated as a topic, not by assuming
message 1 exists. Telegram remains authoritative for permission/topic races after checks.
These rules follow [Telegram's forum semantics](https://core.telegram.org/api/forum).

## Content and request examples

All new write objects reject unknown fields, including nested recipient/media/source
objects. `silent` defaults to false. Text and captions are plain text, preserved verbatim;
no HTML/Markdown/entities are parsed, and content is not split or truncated.

| Content | Input and metadata | Connector limit |
| --- | --- | --- |
| Text | Nonblank `text`; `link_preview` defaults to false | 4,096 UTF-16 units |
| Image | JPEG or PNG; sent as a Telegram photo | 10 MiB; Telegram may compress/convert |
| Audio | MP3, M4A/AAC; positive integer `duration_seconds` up to `i32::MAX`; optional `title`/`performer` | 100 MiB; each tag at most 256 UTF-16 units |
| Voice | Prepared Ogg/Opus; the same positive integer duration range | 100 MiB; no waveform |
| Video | MP4 prepared with H.264 and optional AAC; finite positive duration, integer width/height `1..=8192` | 100 MiB; ordinary video, conservative streaming flag |

Captions default to empty and allow at most 1,024 UTF-16 units. An emoji outside the
basic multilingual plane counts as two units. Metadata is supplied by the caller; no
FFmpeg, recording, tag extraction or transcoding is performed. Bounded signatures and
container headers (up to 64 KiB) reject obvious mismatches; they do not establish codec
validity. Telegram can reject a plausible but invalid file. No generic-file fallback is
performed. Only a sanitized basename with the detected kind's extension is submitted.

Text to Saved Messages:

```json
{"recipient":{"kind":"saved_messages"},"text":"Build completed.","silent":true,"recovery_key":"build-unique-01"}
```

Image to a channel:

```json
{"recipient":{"kind":"peer","namespace":"channel","id":"1234567890"},"source":{"root":"outgoing","relative_path":"reports/chart.png"},"media":{"kind":"image"},"caption":"Current results.","recovery_key":"chart-unique-01"}
```

Audio to a private chat:

```json
{"recipient":{"kind":"username","username":"@example_user","expected_namespace":"user"},"source":{"root":"outgoing","relative_path":"audio/demo.m4a"},"media":{"kind":"audio","duration_seconds":25,"title":"Demo","performer":"Example"},"recovery_key":"audio-unique-01"}
```

Prepared voice note:

```json
{"recipient":{"kind":"saved_messages"},"source":{"root":"outgoing","relative_path":"audio/note.ogg"},"media":{"kind":"voice","duration_seconds":8},"recovery_key":"voice-unique-01"}
```

Video replying within a forum topic:

```json
{"recipient":{"kind":"peer","namespace":"channel","id":"1234567890"},"source":{"root":"outgoing","relative_path":"video/demo.mp4"},"media":{"kind":"video","duration_seconds":12.5,"width":1280,"height":720},"caption":"Demonstration.","topic_id":50,"reply_to_message_id":60,"recovery_key":"video-unique-01"}
```

Examples describe payload shapes, not executed sends. For ordered messages, await each
result before starting the next call. Albums, bulk/fan-out delivery, scheduling,
editing/deleting/forwarding, arbitrary documents, remote URLs, inline binaries, custom
thumbnails, round video notes, paid delivery and alternate senders are outside this version.
The read tools do not gain private-chat history access.

## Receipts, charges and uncertain delivery

Success means Telegram acknowledged a real positive message identity. It does not prove
reading, notification delivery or completed video download. Receipts omit text, captions,
source paths, access hashes and bytes:

```json
{"schema_version":"1","data":{"delivery_state":"sent","recipient":{"peer_namespace":"user","peer_id":"123456789","saved_messages":true},"message_id":42,"date":"2026-11-14T08:30:00Z","content_kind":"text","reply_to_message_id":null,"topic_id":null,"message_link":null},"error":null}
```

Media adds `media: {"mime_type":"video/mp4","size_bytes":12345}` for the submitted
snapshot, not Telegram's compressed/stored size. Links are optional and follow existing
namespace/link rules; private users and Saved Messages have no message link.

| Outcome | Meaning | Automatic resend |
| --- | --- | --- |
| `sent` | Matched acknowledgement with actual ID/date | Would create another message |
| `not_sent` | Final send never dispatched, or an allowlisted definitive refusal | Only consider the typed `retryable` policy |
| `unknown` | Final dispatch may have committed; `code="delivery_unknown"` | Never: `retryable=false` |

One total deadline starts before local staging: text 30 seconds, media 120 by default.
Resolution, reply/topic checks, upload, dependency waits, final send and receipt mapping
share it. Known pre-dispatch timeouts can be retryable; ambiguous final-send timeout,
connection loss or acknowledgement/serialization failure is unknown. A provider wait
hint does not cause a final sleep/resend. Lower-level retransmission can preserve the same
serialized request/random ID; this is not cross-call or cross-restart exactly-once delivery.

Local validation, staging/capacity checks and worst-case complete wire budgeting precede
charging. Text costs one account token, media three by default. Once acquired, the charge
is never refunded for resolution, upload, rejection, timeout or cancellation. `not_sent`
does not imply the attachment was never uploaded to Telegram.

Use a unique `recovery_key` per intended call and recover a lost reply with:

```json
{"recovery_keys":["video-unique-01"]}
```

`get_last_responses` is local/free. Pending/retained duplicate keys reject before another
send; they do not replay a mutation. Retention is bounded and process-local for ten minutes
after completion. Missing, cancelled, expired, oversized, evicted or restarted recovery
is **not evidence that nothing was sent**. Reusing a key after eviction, restarting, or
making a new call can duplicate a message. Inspect Telegram before choosing to repeat an
uncertain send. There is no durable outbox or application-level resend queue.

## Cleanup, diagnostics and validation

Cancellation stops future stages, but cannot retract already enqueued requests or started
filesystem work. MCP cancellation/shutdown may suppress a response entirely. Started jobs,
queued upload buffers and their results retain admission until they actually finish.
Capacity can therefore remain occupied after a cancelled call. Staging deletion is owned
and asynchronous; cleanup failure retains quota and refuses conflicting reuse. Shutdown
uses the original drain deadline, not a second grace period.

After a crash or incomplete cleanup, inspect the old private directory yourself and use a
fresh empty root. Startup never sweeps old directories. Copies retained by Telegram follow
Telegram's own policies. Native logs contain safe tool/stage/category/duration metadata;
payload-bearing SDK/MTProto diagnostics are suppressed even under `RUST_LOG=trace`.

See the [offline validation report](validation/2026-11-14-telegram-sending.md) for filesystem,
wire, cancellation, memory and delivery-truth evidence. macOS was validated locally;
Linux remains owner-managed, and Windows file-staging ownership/ACL support is unavailable.
Account permissions, actual media playback and live Telegram behavior remain a separate
owner-authorized acceptance step with named destinations.

For credential-free protocol experiments, the fixture can simulate sends:

```bash
cargo run --offline --locked --example offline_mcp_fixture -- --typed-tools --sending-enabled true
```

Add `--upload-root /absolute/disposable/prepared` and
`--staging-root /absolute/disposable/empty-staging` for media. This **inert fixture synthesizes
receipts and never connects to Telegram**; it is not a delivery test or a production client.
