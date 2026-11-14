# Telegram sending validation

Implementation evidence is added as each task completes. No live sends are authorized
or performed by these checks.

## Transport feasibility

- The pinned public `SenderPoolFatHandle::thin` can be cloned before moving the fat
  handle into `Client::new`. `SenderPoolHandle::raw_invoke_in_dc` accepts serialized
  requests and returns bytes without entering `do_invoke_in_dc`'s `AutoSleep` loop.
- An unpolled retained sender runner proves the real raw API compiles and can be
  cancelled without polling the network runner. Injected final responses exercise
  the connector's actual correlation, deadline and refusal classifier.
- Source inspection of pinned `client/files.rs`: `upload_stream` consumes Tokio
  `AsyncRead`, uses 512 KiB parts and four workers for files larger than 10 MiB.
  This is not a measured account/network buffering result or a hard RSS guarantee.
- The raw one-shot seam does not regenerate requests or random IDs. Lower-level
  MTProto retransmission and Telegram's deduplication retention remain unverified
  by these offline fixtures.

## Recipient and thread validation

- Qualified dialog selection includes users and distinguishes colliding user/chat/channel IDs. The real stalled client proves resolve/topic stage caps and the shared deadline.
- Deleted/migrated peers, missing authority, inaccessible broadcast metadata and incomplete/deleted/closed/hidden or wrong-peer topic states are rejected before subsequent stages.
- [Telegram forum semantics](https://core.telegram.org/api/forum) identify General as topic 1 without an ordinary message root. Non-General replies retain the selected top thread; direct topic-root sends use the root itself. Offline TL fixtures check both forms and cross-topic/cross-peer refusal.
- Community containers and channel direct-message monoforums are refused; the initial namespace/options contract does not supply their separate routing identities. No live topic placement was tested.

## Canonical text route

- Actual framed modern and legacy-handshake requests send through the canonical contract. Disabled catalogs retain 16 tools; enabled catalogs expose 18 with strict self-contained write input schemas and non-idempotent annotations.
- Offline tests prove one charge before the client seam, no refunds, no charge for local/budget refusal, matching JSON text/structured receipts, scoped upstream diagnostic suppression and cached path-free status.
- Forced serializer failure and invalid receipt metadata produce nonretryable unknown delivery; duplicate recovery keys reject before a second send. Full macOS locked offline gate passed after integration. Media remains a pre-dispatch unavailable stub until the staging/upload tasks land.

## Private staging

- Real disposable macOS filesystem fixtures verify private snapshots, descriptor identity, source preservation, symlink/hardlink/FIFO/path refusal, sensitive directory overlap, ACL/ancestry policy, byte/slot admission and bounded header probes.
- Cancellation tests retain reservations in started workers; detached read jobs and unconsumed completed buffers retain the snapshot lease. Failed deletion retains quota and does not delete replacement objects. Runtime cleanup reuses the existing shutdown deadline.
- The bounded reader uses owned blocking jobs rather than Tokio filesystem internals, so no Tokio fs feature was added. Explicit io-util does not change Cargo.lock. All 1,055 library tests and 3 doctests, strict all-target Clippy, format and warning-free docs passed.

## Media submission and working memory

- JPEG/PNG photos, MP3/M4A/AAC audio, Ogg/Opus voice and MP4 video use controlled MIME/filename/attribute mappings, one caption and one raw final mutation. Wire mocks prove staging precedes the one three-token charge and failures never refund it.
- Each uploader owns a temporary public SenderPool sharing the existing authenticated session/API ID. Retirement joins that pool before releasing its snapshot lease, preventing queued binary parts from escaping media admission after cancellation. Existing read/final-send pools are unchanged. Extra connection setup consumes the same operation deadline.
- The actual pinned public upload_stream, with two retained unpolled runners, read exactly 2,097,152 bytes per 104,857,600-byte near-cap fixture before stalling: four 524,288-byte parts each. No third media operation was admitted after caller cancellation until those queues were dropped. This measures initial in-flight buffering and retirement, not complete network transfer or playback.
- Standalone macOS child peak RSS (`resource.getrusage(RUSAGE_CHILDREN).ru_maxrss`) was 36,487,168 bytes (about 34.8 MiB). The fixture includes 200 MiB of staged logical file data, not valid encoded videos. It is an observation, not a hard RSS ceiling. `/usr/bin/time -l` could not read a sandboxed kernel counter, so Python resource collected the measurement.
- Paused time verifies that target validation and upload share one 120-second budget. Known [sendMedia validation refusals](https://core.telegram.org/method/messages.sendMedia) are not-sent failures; unrecognized post-dispatch errors stay unknown. Full gate: 1,063 library tests and 3 doctests passed.
