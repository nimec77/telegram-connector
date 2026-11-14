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

## Runtime, recovery and privacy

- Native run_io wire tests validate every content-kind receipt against its actual advertised schema under MCP 2026-07-28 and the 2025-11-25 handshake. Structured/text representations match.
- Tests recover completed receipts, reject duplicate keys before a second mutation, and show that pending/cancelled/evicted/expired/restarted recovery is inconclusive even after a simulated committed submission. Zero-grace and saturated-output shutdown never invent not-sent delivery; completed receipts survive output loss under existing retention.
- A deterministically queued local copy can be cancelled before account acquisition or client work. Actual uploader cancellation and read/copy/cleanup lease tests remain part of the suite.
- Full/combined acknowledgements must match requested reply/topic metadata; malformed forum headers cannot fall back to General. Direct short acknowledgements retain the approved ID/date rule.
- Native logging now suppresses payload-bearing rmcp/grammers events even in spawned workers and under explicit RUST_LOG trace overrides. Safe connector stage/category/duration logs remain. This deliberately removes low-level vendor diagnostics; read retry policy and wire outputs remain supported.
- Gate after integration: 1,075 library tests, 3 doctests, strict all-target Clippy, format and warning-free rustdoc passed on macOS. Of nine ignored tests in the top-level harness, seven subprocess child entrypoints are exercised by normal parent tests; two pre-existing explicit measurement benchmarks remain outside the routine gate.

## Final pre-review checks

- Final locked offline macOS gate passed: 1,075 library tests, 9 ignored harness entries and 3 doctests; strict all-target Clippy, format and warning-free rustdoc. Seven ignored subprocess children are exercised by parents; two pre-existing measurement benchmarks are intentionally explicit.
- `cargo run --offline --locked --example inert_server` passed. `python3 scripts/test_mcp_probe.py target/debug/examples/offline_mcp_fixture` passed modern, discovery-skip and legacy echo checks. The integration script takes a binary argument; unittest discovery is not its runner.
- Additional inert modern/legacy CLI probes validated 18 tools, text/image receipts, targeted recovery and completely closed disposable staging roots. TOML/JSON examples, relative documentation links and whitespace checks passed. Source pins and Cargo.lock are unchanged.
- Independent whole-branch review follows these checks. Live account delivery/playback and Linux/Windows validation are not claimed.

## Independent review and verified correction

One fresh independent senior review covered `52a12d7..0de48a2` against the approved
specification, plan, source/pinned APIs and validation evidence. It found no Critical or
Minor findings and one Important issue: a queued upload-pool Quit cannot interrupt an
inline stalled InitConnection, so background retirement could retain admission forever.
The earlier parked-runner fixture proved ownership, not bounded stalled-setup recovery.

The regression `sending_media_stalled_setup_cannot_hold_capacity_after_caller_cancellation`
failed against that implementation. Upload supervision now owns a current-thread Tokio
runtime on its supervising thread. Caller cancellation or the original operation deadline
interrupts the driver; runtime shutdown destroys its owned async task set and queues
before the snapshot lease is released on the original filesystem executor. No original
read/final-send pool policy or authentication changes.

Additional regressions prove deadline-driven teardown without caller Drop and hold a child
buffer destructor to verify admission stays occupied through complete child teardown.
All eleven focused media checks pass. The Important finding was corrected in one
RED→GREEN fix pass and verified by the author; no second independent review was requested.
There are no deferred Minor findings.

Final post-fix macOS validation: **1,078 library tests, 3 doctests**, strict all-target Clippy,
format check, warning-free rustdoc and optimized locked offline release build passed.
Nine main-harness ignores remain seven parent-exercised subprocess children and two
pre-existing explicit benchmarks. The updated standalone two-near-cap public-uploader
fixture measured `ru_maxrss = 37,175,296` bytes (about 35.5 MiB); each still read four
512 KiB parts before its unpolled driver stalled. This remains a bounded-buffer/teardown
fixture, not a full network transfer, valid-video playback test or hard RSS guarantee.

The review accepted per-store budgets shared across clones (native startup creates one),
the pinned forbidden-channel sentinel handling, owned filesystem readers, dormant-cost
compatibility and the documented routing/logging/probe choices. It did not judge live
account behavior/dedup retention, Linux/Windows, hostile same-UID mutation, full codecs or
headers beyond the bounded probe, or an embedder's independently installed subscriber;
these remain the explicit limits described in the operation/API guides.
