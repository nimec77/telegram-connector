# Media delivery: binary processing, portable MCP previews, optional files

Date: 2026-10-31. Status: analysis and proposal only; no media implementation changed.
Scope: local stdio MCP 2026-07-28. This supplements the
[development audit](2026-10-31-development-audit-design.md) and
[roadmap](../plans/2026-10-31-development-roadmap.md).

## Recommendation

Keep standard inline image blocks as the default. They use base64 because MCP stdio
carries JSON. Improve the internal binary pipeline so a batch does not retain all raw
downloads before processing. Offer cached resource links only as an optional feature;
offer direct local files only through a verified shared-filesystem client integration.
Do not replace usable image previews with paths the host cannot read.

This separates three different requirements: showing an image to the model, avoiding
repeated downloads, and giving another program a binary file. One delivery mechanism
need not serve all three. Base64 is an encoding, not compression, encryption, or by itself
a sign that media is being handled incorrectly.

## What the protocol supports

The [stdio transport](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio)
uses newline-delimited JSON-RPC on one shared channel. It provides no per-request raw
byte stream. Writing JPEG bytes, file descriptors or multipart content to stdout would
break interoperable framing. Incrementally writing a JSON message is an implementation
optimization; it does not let the receiving client use a new binary MCP message type.

| Choice | Binary encoding | Fit |
| --- | --- | --- |
| Inline image block | Base64 in `data` | Portable preview for image-capable clients; current default. |
| Embedded binary resource | Base64 in `blob` | Different content semantics, no encoding savings. |
| Resource link then `resources/read` | Initial URI only; later blob still base64 | Deferred retrieval/reuse when the client supports resources. |
| Private file plus a client filesystem adapter | Raw bytes in a file | Avoids base64 for that copy; requires actual host access to the file. |
| Custom binary/chunk protocol | Client-specific | No portable core stdio contract; not recommended for this server. |

The schema explicitly defines both image and blob payloads as base64 strings.
[ImageContent](https://modelcontextprotocol.io/specification/2026-07-28/schema#imagecontent),
[BlobResourceContents](https://modelcontextprotocol.io/specification/2026-07-28/schema#blobresourcecontents).

Resource handling is controlled by the host. A resource URI is not proof of access to
the server's physical filesystem; core resource reads have no standard byte-range or
resumable-chunk parameters. Replacing an image with a link can remove model vision if
the host does not retrieve/render it. A linked artifact need not appear in resource
enumeration. [Resources specification](https://modelcontextprotocol.io/specification/2026-07-28/server/resources).

## Current code and costs

The project already handles binary data internally:

1. `src/telegram/client/ops_media.rs` selects/downloads a photo variant or a video-like
   item's thumbnail. It does not fetch full video/audio/document attachments.
2. Downloaded chunks are accumulated into a bounded `Vec<u8>` (default cap 20 MiB,
   configured download timeout default 120 seconds).
3. `src/mcp/tools/image.rs` decodes/resizes JPEG and creates base64. A JPEG already within
   dimensions and size passes through without lossy re-encoding.
4. `src/mcp/server/impl_media.rs` returns MCP image blocks plus JSON metadata/summary.

Base64 output length is `4 × ceil(binary_bytes / 3)`: approximately one third larger than
the binary. The per-image cap of 1,572,864 base64 bytes is 1.5 MiB, corresponding to at most
1.125 MiB JPEG; the batch cap is 8 MiB of base64. Metadata and framing are additional.
These are transfer/allocation costs, not a direct conversion to language-model text tokens:
a conforming host treats an image content block as an image.

Two concrete optimization opportunities:

- `ops_media.rs:108–130` uses concurrency four and then collects all results into a Vec.
  Up to ten near-cap downloads can retain roughly **200 MiB of compressed bytes**, before
  decoded pixels, base64 and serialization copies. This is a calculated bound, not measured
  resident memory. Limiting active downloads does not bound retained completed downloads.
- `image.rs:70–75` encodes each shrink candidate to base64 before checking its length.
  Calculate the length with checked arithmetic first; encode only the accepted candidate.
  `observability/transport.rs:63` also intentionally serializes a recovery/metrics copy.
  Optimize that only with equivalent byte accounting and recovery behavior demonstrated.

## Phase M1: Optimize existing delivery, preserving its contract

This is the recommended next media work, folded into roadmap Task 8.

**Files:** `src/telegram/trait_def.rs`, `src/telegram/client/ops_media.rs`,
`src/telegram/types/media.rs`, `src/mcp/server/impl_media.rs`,
`src/mcp/tools/{image,media_budget}.rs`, media/transport tests.

- [ ] Add regressions for base64 length at 0/1/2/3-byte boundaries and overflow, unchanged
  JPEG passthrough, per-image limits and batch summaries. Calculate candidate size before
  encoding; retain the exact existing base64 format and cap definitions.
- [ ] Introduce backpressure between downloading and processing. Resolve/fetch once per
  batch as today; consume each completed outcome and release its raw bytes promptly.
  The Telegram layer produces binary outcomes; the MCP layer retains JPEG/base64 shaping.
  Do not make Telegram code import MCP types to implement the pipeline.
- [ ] Proposed baseline: at most four raw-buffer slots per process, each capped at the
  configured per-download maximum; a slot remains charged while a completed result waits
  or is being processed. At most two image decode/encode jobs per process. Avoid per-call
  limits that multiply without bound under concurrent tool calls. Reject configurations
  whose global memory allowance cannot admit one permitted input.
- [ ] Hold raw-buffer and processing permits inside any started `spawn_blocking` closure
  until it exits. Cancelling its awaiting request must not release capacity while that
  closure still owns bytes or computes. Test cancellation followed immediately by a
  replacement request and assert the same process-wide limits still hold.
- [ ] Include raw-slot, processing-slot and pipeline-capacity waits in one absolute
  operation deadline established before queuing, using the configured download timeout
  (default 120 seconds). Cancellation stops waiting immediately; deadline expiry returns
  the defined timeout/partial-item outcome and existing refunds. Never start a fresh
  timeout after waiting for capacity or leave an unbounded permit wait.
- [ ] Preserve original request order and item identity. A bounded ordered window must
  not accumulate an unbounded reorder queue. Release raw data before admitting more work;
  account for final encoded outputs separately, because stdio still returns one result.
- [ ] Add explicit decoded pixel/allocation limits and cancellation checks before queued
  work. Select exact limits from representative thumbnail/photo fixtures and measured
  peak memory; record the values in config/status/docs before merging. This is an
  implementation measurement decision, not permission to claim a hard RSS bound now.
- [ ] Test saturated backpressure, slow first item, ten near-limit downloads, two concurrent
  calls, mixed failures and cancellation. Preserve media token refunds and no repeated
  channel resolution. Measure peak retained raw bytes and RSS separately.
- [ ] Preserve per-image/batch base64 budgets and Task 8's separate non-binary overhead
  budget. Never apply the 40,000-byte search/history budget to image payloads.
- [ ] Verify preview rendering with actual target MCP clients after offline tests pass;
  benchmark using local fixtures, not repeated account-throttling Telegram calls.

Started blocking computations cannot be forcibly aborted by dropping their Tokio handles;
bounded concurrency and finite decoding work are necessary even with cancellation.
[Tokio spawn_blocking documentation](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).

## Phase M2: Optional artifact references

Implement only if deferred/repeated media access is wanted after M1. This adds an optional
core Resources capability, not a requirement for the original 16-tool stdio server.

Proposed result: inline preview plus a `resource_link` such as
`telegram-media://artifact/<opaque-id>`. JSON metadata records the image item, MIME type,
size, dimensions, expiry, and whether the artifact is a processed JPEG, downloaded photo
variant or thumbnail. Do not label it the original full attachment.

Store bytes once under a private server-controlled cache root. Proposed starting limits:
10-minute TTL, 100 entries, 256 MiB total disk, and the existing 20 MiB per-download cap;
these are new proposed limits, not current configuration. Use atomic writes, restrictive
permissions, opaque random identifiers, strict URI/manifest lookup, and cleanup limited
to owned entries. Never accept arbitrary source/destination paths from tool arguments.
Bind entries to the configured account/process, avoid account identifiers in names, and
treat returned links as sensitive. A handle alone is not authenticated multi-user isolation.

`resources/read` must return private cache hints with TTL no longer than remaining lifetime,
safe missing/expired-resource protocol errors, and a bounded blob. Limit an inline resource
read to 8 MiB of base64; a larger cached artifact needs a verified file adapter or a smaller
preview, not an output-limit bypass. Expiry must not silently trigger another paid download.
Empty resource listing is acceptable; avoid a mutable per-connection catalog of cached
private artifacts. Advertise no subscription/listChanged feature without implementing it.

**Future files:** new artifact-store module with no Telegram credentials, resource handlers
in `src/mcp/server.rs`, metadata/config/status types and expiry/quota/path-rejection tests.
Use the same canonical JSON metadata convention; media itself remains an MCP content block.
Test host retrieval/rendering before relying on link-only output. The default remains inline.

## Phase M3: Optional direct file export

This is the only proposed way to avoid base64 for an exported copy while retaining local
stdio as the control protocol. It needs a client that can read the same filesystem under
its own sandbox permissions. A locally launched server does not prove that capability.

Use a configured private export root and server-generated filenames; return a generated
artifact descriptor/path for the host's authorized filesystem/image tool. Reuse M2's
quotas, ownership, expiry and cleanup. Explain that files are temporary, and let the caller
copy them through an authorized client operation if persistence is required. Keep previews
unless a verified client explicitly selects file-only delivery.

No automatic localhost HTTP server, arbitrary file-writing tool, full attachment download,
custom binary stdout framing or public hosting is proposed. Remote HTTP/object storage
requires a separate scope and security design. Streamable HTTP alone would not turn MCP
ImageContent into raw binary.

## Effort and acceptance

M1: roughly 2–4 engineering days, partly overlapping roadmap Task 8; new concurrency/type
interfaces and memory measurement dominate uncertainty. M2/M3 are separately sized future
features after a target client is selected. Completion of M1 means stable previews and
per-item semantics, demonstrated memory/backpressure bounds, unchanged encoding compatibility,
and no unbounded work after cancellation—not elimination of base64 from standard MCP.
