# Optional media resource links and file export

Date: 2026-10-31. Updated 2026-11-01: M1 ships with 0.24.0; M2/M3 below remain
separate, unimplemented product proposals. The completed M1 checklist is archived in Git history; the modernization
[roadmap](../plans/2026-10-31-development-roadmap.md) remains active for Tasks 9–10.
Current inline behavior is documented in the
[tool contract](../../tool-contract.md), with [macOS validation](../../validation/2026-11-01-task8.md).
Scope: local stdio MCP 2026-07-28; inline image previews remain the default.

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

## Next decision

Select a target client and establish its resource/shared-filesystem capabilities before
planning either optional mode. Neither mode is required for the shipped inline pipeline.
