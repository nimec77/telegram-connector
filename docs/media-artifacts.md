# Media artifacts and DeepSeek Harness

The two media tools can retain their processed JPEG previews in a private, bounded
per-process store. Every delivery mode still returns the inline preview. Artifacts
contain the selected photo variant or video thumbnail after preview processing;
they do not download original attachments, videos, audio or documents.

## Enable the store

Merge this optional table into the connector configuration, then restart the server:

```toml
[media_artifacts]
enabled = true
file_export_enabled = true
# Optional: a dedicated, empty, absolute private directory.
# root = "/absolute/private/path/media-artifacts"
ttl_seconds = 600
max_entries = 100
max_total_bytes = 268435456
```

Both flags default to false. Disabled artifacts cause no store initialization or disk
writes. File export requires the store and explicit file delivery. If `root` is omitted,
the store uses `media-artifacts` beneath the platform configuration directory documented
in [README](../README.md#configuration). A custom config file does not relocate it.
Enabling artifacts requires verified Unix ownership/mode checks; other platforms refuse
the opt-in while retaining default inline behavior. Validation has run on macOS.

Limits are per server process: TTL 1–3600 seconds, 1–1000 entries, and 1–1073741824
total JPEG bytes. The defaults above are 10 minutes, 100 entries and 256 MiB. Each stored
JPEG is limited to 20 MiB. Existing preview/download/response budgets still apply.
`check_mcp_status` reports configuration, read limits and reserved usage, including
unavailable-state information; it exposes neither paths nor a cache catalog.

## Request delivery

Use canonical `[server] output_format = "json_v1"` (the default). Both
`get_message_media` and `get_messages_media_batch` accept these four choices:

| `delivery` | Successful content and descriptor |
| --- | --- |
| `inline` (default) | Inline JPEG only; no artifact descriptor. |
| `resource` | Inline JPEG, then a resource link; descriptor contains resource URI/index. |
| `file` | Inline JPEG; descriptor contains an absolute raw file path. |
| `resource_and_file` | Inline JPEG and resource link; descriptor contains both handles. |

For example, these are four complete `tools/call` parameter objects:

```json
{"name":"get_message_media","arguments":{"channel_id":"100","message_id":1,"delivery":"inline"}}
{"name":"get_message_media","arguments":{"channel_id":"100","message_id":1,"delivery":"resource"}}
{"name":"get_message_media","arguments":{"channel_id":"100","message_id":1,"delivery":"file"}}
{"name":"get_message_media","arguments":{"channel_id":"100","message_id":1,"delivery":"resource_and_file"}}
```

For a batch, use `name: "get_messages_media_batch"` and `message_ids: [1,2,3]`
instead of `message_id`. Unavailable delivery modes and non-inline requests in legacy
output mode fail before account charges or Telegram RPCs. Inline legacy behavior stays
available independently of the negotiated MCP revision.

Read the canonical envelope from `structuredContent` or its matching JSON text.
Each successful `data.items[]` has `content_index` for its image. Its optional `artifact`
contains `kind: "processed_jpeg"`, `mime_type: "image/jpeg"`, `size_bytes`, `width`,
`height`, UTC `expires_at`, and only the requested handles: `resource_uri`,
`resource_content_index`, and/or `file_path`. Resource indices point into the actual
content array after each item's image. Failed batch items retain their own errors and
have no image/artifact indices. See [the tool contract](tool-contract.md).

## Resource URI versus file path

Use `resources/read` with the exact `telegram-media://artifact/<48 lowercase hex>` URI.
For example, a client's resource adapter receives `{"server":"telegram","uri":"<returned resource_uri>"}`.
The URI is an opaque cache handle, not a filesystem address. Reads return JPEG base64
in `contents[].blob`, without a Telegram RPC or account charge. Discovery lists are
empty; no subscriptions or change notifications are advertised.

Resource reads allow at most 6 MiB of JPEG (8 MiB base64), with a five-second deadline
including blocking-worker admission. Larger cached previews require an explicit file
mode or a new, explicitly requested smaller preview. Modern clients receive private
cache hints no longer than remaining TTL; legacy clients receive no modern hints.
Malformed URI, missing/expired artifact, oversized read, timeout and unavailable storage
have distinct safe `error.data.code` values: `artifact_invalid_uri`, `artifact_missing`,
`artifact_too_large`, `artifact_timeout`, and `artifact_unavailable`. Missing uses numeric
-32002 for legacy and -32602 for modern protocol; malformed uses -32602 for both.
Expiry/missing reads never silently re-download or spend another Telegram token.

Use `file_path` only with an authorized client filesystem/image tool on the same host.
It is a raw absolute path, not `file://` and not the resource URI. Remote clients and
sandboxed hosts need their own verified shared-filesystem access.

## DeepSeek Harness activation

Register the built connector as a stdio MCP server in DeepSeek Harness using your
private connector config and authenticated session. Configure the optional table above
and restart that server. This repository does not edit your Harness configuration.
Choose `resource_and_file` when you want both programmatic retrieval and image reading.

The inspected macOS Harness **0.2.1-rc.5**, build
`d4db512a969fccf0c5cc7ec8fe38e9c1d27af916`, preserves canonical resource bytes, but its
resource renderer projects binary blobs as text placeholders. Resource links are text
references and do not automatically retrieve images. For native image inspection, enable
the Harness filesystem `read_image` tool and call:

```json
{"file_path":"<returned artifact.file_path>"}
```

The tool requires a mounted durable attachment service, tool permission/visibility, OS
read access, and the exact resolved provider/model route declaring image input.
Text-only/unknown routes refuse the image. In the inspected local backend, absolute
paths outside its workspace are readable subject to those permissions. Client attachment
admission/normalization can resize larger images; exporting a preview does not promise
original resolution at the model. Native image attachments copied by the client persist
under the client's own retention policy, independently of server TTL and cleanup.

The [initial compatibility investigation](validation/2026-11-01-deepseek-media-compatibility.md)
and [actual connector fixture verification](validation/2026-11-01-media-artifacts.md)
record installed-adapter execution. No selected live model, desktop UI or Telegram account
was exercised; those remain separate operational checks.

## Ownership, lifetime and cleanup

Use a dedicated empty private root: directories are 0700, generated JPEGs and the process
lock are 0600. Existing permissions are verified without widening them. The server rejects
symlinked/unsafe ancestry, granting macOS directory ACLs and linked/nonregular entries.
The private root cannot itself be a shared sticky directory. Only one process can own a
root; a create-new lock and empty-root requirement prevent cross-process reuse.
Treat both URIs and paths as sensitive handles; they are not multi-user authentication.
Hostile mutation by another process with the same user identity is outside this boundary.

TTL is a maximum lifetime, never refreshed by reads. Oldest entries may be evicted
earlier to admit an entry within byte/entry quotas. Too-large entries do not evict healthy
ones. Failed deletion keeps its quota reservation; unrecoverable storage failures refuse
further mutations rather than accumulating unbounded orphan files.

Expired URIs are refused immediately. Physical deletion runs on operations and periodically
at intervals of at most 60 seconds, plus blocking-worker admission delay. A path is not
a revocable handle: logical URI expiry does not guarantee immediate physical deletion or
removal of copies already retained by a client.

Ordinary EOF/signal shutdown awaits owned artifact/lock cleanup within the **original**
absolute drain deadline shared with requests/stdout. It does not start a second grace
period. Zero grace, exhausted deadlines, failed cleanup or a crash can leave bounded
files and a stale lock. Drop cleanup during runtime teardown is best effort.

Startup refuses a nonempty/stale root without deleting its contents. First stop every
process using that root and inspect ownership and contents. Then manually remove only
the known artifacts/lock you intend to discard, or configure a fresh dedicated empty
private root. Do not delete an active process's lock. No persistent recovery manifest,
automatic stale-lock removal or cross-process artifact reuse is provided.
