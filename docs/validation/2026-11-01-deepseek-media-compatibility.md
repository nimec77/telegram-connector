# DeepSeek Harness media compatibility — 2026-11-01

This is **preimplementation client compatibility evidence** for optional M2 cached
resources and M3 local-file export. It does not demonstrate that Telegram MCP
Connector implements either mode. The existing inline preview remains necessary:
M2 provides programmatic access to cached image bytes; M3 can additionally expose a local
image to an image-capable model route.

## Installed target and source provenance

- Application: `/Applications/DeepSeek Harness.app`, macOS arm64,
  **0.2.1-rc.5** (`Contents/Info.plist`, `CFBundleShortVersionString` and
  `CFBundleVersion`; runtime `primary-runtime/runtime.json` also records arm64).
- `Contents/Resources/app.asar/package.json` records clean build commit
  **`d4db512a969fccf0c5cc7ec8fe38e9c1d27af916`**, `dshBuildDirty: false`.
  Extracted metadata: `/private/tmp/deepseek-inspect/package.json:25–26`.
- Runtime packages inspected below are bundled version 0.2.1-rc.5. Installed
  package sources were extracted to
  `/private/tmp/deepseek-inspect/dsh/node_modules/@deepseek-ai/`. Their original
  archive prefix is `Contents/Resources/app.asar/dsh/node_modules/@deepseek-ai/`.
  Source references below use package-relative paths under those prefixes.
- Official public master resolved to
  **`ce7073d51254c26cd63b22a60835b8f948deb447`** during inspection. GitHub returned
  HTTP 422 when queried for the installed `d4db512…` commit. Public links below
  are pinned primary-source counterparts, **not proof that the installed build
  corresponds to that public revision**. Installed source and execution are the
  compatibility evidence.

## Verified behavior

| Area | Installed evidence and result |
| --- | --- |
| M2 custom URI transport | `dsh-mcp-resources/lib/index.js:75–90` accepts explicit `server` and string `uri`; `dsh-mcp-client/lib/index.js:685–696` forwards the URI to SDK `readResource`. The offline stdio fixture successfully served `telegram-media://artifact/test-image`. |
| M2 binary rendering | `dsh-mcp-resources/lib/index.js:16–23` renders text only and replaces each string `blob` with a binary-length description. Canonical results retain base64 for programmatic callers. A resource read therefore does **not** produce a native model image. |
| Resource links | `dsh-mcp-client/lib/index.js:396–404` renders resource links as name/URI text and embedded resources as an unsupported-resource placeholder. Neither automatically retrieves or promotes the referenced image. |
| M3 schema | `dsh-tool-fs/lib/index.js:984–1007` registers `read_image` with required string argument `file_path`: `{"file_path":"/absolute/path/image.jpg"}`. JPEG, PNG, WebP and GIF are supported; mismatched extensions/content are refused. |
| M3 prerequisites | Lines 976–980 require a mounted durable attachment store for registration. Lines 909–916 require the exact resolved provider/model route to explicitly declare `inputModalities` containing `image`; unknown and text-only routes fail before filesystem reading. Scoped tool restrictions may also hide the reader. |
| M3 image delivery | Lines 1010–1057 resolve a regular file, read bounded bytes and save an attachment; lines 966–973 render text plus a native image attachment block. The offline JPEG read produced that block and readable durable stored bytes. |
| Filesystem access | `dsh-fs-local/lib/index.js:154–175, 720–723` accepts absolute paths, resolves relative paths against cwd and uses realpath identity; cwd is not a containment boundary. `dsh-fs-sandbox/lib/index.js:69–75` passes reads through in every mode. No read-root allowlist exists in these bundled backends. An absolute JPEG outside the fixture workspace was successfully read. OS permissions and any separately composed permission plugin still apply. |
| Model consumption | `dsh-llm-deepseek/lib/index.js:1430–1440` consumes tool-result images through `attachments.readImageRequest`, requiring a vision route and attachment service. Lines 2182–2207 prepare Files API references with inline fallback; lines 1619–1629 serialize image sources as base64 or file IDs. This later network/model stage was inspected, not invoked. |

M3 must return a raw absolute filesystem path for `file_path`, rather than a
`file://` or custom resource URI. Attachment admission and normalization still
apply: bundled defaults are 20 MiB per source image, 64 million pixels and an
8192px side limit; normalized images have a 4 MiB/2048×2048 pixel budget
(`dsh-attachment-local/lib/index.js:998–1014`). Large originals can be downscaled,
so original-resolution delivery to the model is not established. The current
user-selected model route and user configuration were not inspected.

Official source counterparts at public revision `ce7073d…`:
[resource renderer](https://github.com/deepseek-ai/deepseek-harness/blob/ce7073d51254c26cd63b22a60835b8f948deb447/packages/mcp/mcp-resources/src/render.ts),
[image reader](https://github.com/deepseek-ai/deepseek-harness/blob/ce7073d51254c26cd63b22a60835b8f948deb447/packages/fs/tool-fs/src/read-image.ts),
[offline image-reader tests](https://github.com/deepseek-ai/deepseek-harness/blob/ce7073d51254c26cd63b22a60835b8f948deb447/packages/fs/tool-fs/tests/read-image.spec.ts).

## Offline smoke and limits

Preserved temporary inspection artifacts:

- `/private/tmp/deepseek-inspect/dsh/smoke.mjs`: installed adapter/service smoke.
- `/private/tmp/deepseek-inspect/dsh/fixture.py`: local MCP stdio fixture.
- `/private/tmp/deepseek-inspect/dsh/smoke.log`: passing assertion output.
- `/private/tmp/deepseek-media-offline/`: JPEG fixture, fixture workspace and
  explicit isolated attachment home.

Both the investigating agent and the root agent independently ran the smoke and
confirmed all assertions pass. It verifies custom-URI transport and decoded blob
equality, M2 text-only projection, M3 outside-workspace JPEG reading and native
image projection, stored-byte equality for the small fixture, and text-only route
refusal. It calls tool execution/rendering directly, with a fake model capability
resolver; it does not execute a model request or the desktop UI.

Conditional macOS rerun while the temporary extraction remains available:

```sh
deepseek_node='/Applications/DeepSeek Harness.app/Contents/Resources/runtime/primary-runtime/dependencies/node/bin/node'
deepseek_smoke='/private/tmp/deepseek-inspect/dsh/smoke.mjs'
if [ "$(uname -s)" = Darwin ] && [ -x "$deepseek_node" ] && [ -f "$deepseek_smoke" ]; then
  "$deepseek_node" "$deepseek_smoke"
else
  echo 'Requires the inspected macOS installation and preserved temporary smoke artifacts.' >&2
  exit 1
fi
```

The smoke writes only the two temporary trees and spawns the local Python fixture.
It makes no network/model/Telegram calls and reads or modifies no user/client
configuration. Separate official-source research used read-only GitHub requests.
The scripts and extracted bundled third-party code are temporary inspection
artifacts, **not checked into this repository**; this command is not a permanent
repository test. No live Telegram behavior, paid model behavior, desktop rendering
or connector M2/M3 implementation was verified.
