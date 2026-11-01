# Media artifacts implementation verification — 2026-11-01

M2 cached processed-JPEG resources and M3 explicit local file export are implemented on
`feat/media-artifact-delivery`. This records macOS offline verification of the actual
connector, following the [preimplementation client investigation](2026-11-01-deepseek-media-compatibility.md).
The permanent [media guide](../media-artifacts.md) describes activation and operational
limits. Final independent task/whole-branch review and integration are tracked separately.
No version bump, push or publication is included.

## Environment and scope

- Darwin arm64, macOS 27.2; `nightly-2026-10-31`, rustc
  `1.101.0-nightly (92b5b8c87 2026-10-29)`.
- Production release reports `telegram-mcp 0.24.1`.
- Task 3 base: `adf6344c9911a23c8728a40ea580384c33e3c948`, containing the reviewed store
  and integration commits. Task 3 adds the disposable fixture opt-in, independent client
  matrix, exact wire error assertions and current documentation; no production Rust changes.
- All Telegram operations are replaced at the existing client seam. No credentials,
  authenticated session, live Telegram, model request, desktop UI, user/client configuration
  changes or Linux setup were used.

## Reproducible repository checks

All commands below passed from the project root with cached dependencies:

```sh
cargo fmt --all --check
cargo clippy --offline --locked --all-targets -- -D warnings
cargo test --offline --locked
RUSTDOCFLAGS='-D warnings' cargo doc --offline --locked --no-deps
cargo build --offline --locked --release
target/release/telegram-mcp --version
cargo build --offline --locked --example offline_mcp_fixture
python3 -B scripts/test_artifact_clients.py target/debug/examples/offline_mcp_fixture
python3 -B scripts/test_media_clients.py target/debug/examples/offline_mcp_fixture
python3 -B scripts/test_stdio_process.py target/debug/examples/offline_mcp_fixture
python3 -B scripts/test_mcp_probe.py target/debug/examples/offline_mcp_fixture
python3 -B -m unittest discover -s scripts -p test_dependency_policy.py
python3 -B scripts/check_dependency_policy.py --offline
```

The Rust gate passed **988 library tests, 0 failures, 7 ignored**, plus **3 doctests**
and 0 binary tests. Five ignored subprocess entrypoints are exercised by parent tests;
the two pre-existing opt-in latency/RSS measurements were not run. Formatting, strict
all-target Clippy and warning-free rustdoc passed. The new protocol assertions pin malformed
URI `artifact_invalid_uri` / -32602 and missing `artifact_missing` / legacy -32002,
modern -32602, rather than merely checking that strings differ.

The existing Python media client passed both output modes across both protocol revisions,
previews, keyed replay and mixed batches. Native process checks passed idle/pending EOF,
signal shutdown and saturated bootstrap. The probe client passed modern discovery,
modern first-request and legacy handshake modes.

The **25 dependency policy fixtures** and complete policy over **340 external packages**
passed. Cargo.lock package identities match the Task 3 base exactly (341 including root),
and all three direct grammers revisions remain
`1e31e73cd9797790905bbea82cd75adcbe6f1242`. The new direct getrandom declaration
reuses an existing identity. No dependency/pin/version files changed in Task 3.
Current guide links resolve locally, example TOML parses with disabled artifact defaults,
and `git diff --check` passes. Historical reports remain unchanged.

## Independent artifact process matrix

`scripts/test_artifact_clients.py` uses actual NDJSON and only disposable explicit roots.
The fixture accepts `--typed-tools --artifact-root /absolute/private/empty/root`, with
optional `--artifact-ttl-seconds 1` for expiry tests. Without an explicit root it initializes
no artifact store, preserving the original fixture behavior and avoiding user AppSupport.

| Behavior | Independent process coverage |
| --- | --- |
| Four delivery modes | Both protocol revisions, single and mixed three-ID batch: 16 successful tool responses. |
| Inline content | Every mode retains JPEG preview; descriptor absence for inline and requested-handle presence/absence checked. |
| Bytes and indices | Base64 resource bytes and raw file bytes equal each corresponding preview; image/link indices follow actual positions around the missing middle item. |
| Metadata | Processed-JPEG kind, MIME, dimensions, size, future UTC expiry, canonical URI shape, private file mode and absolute root-contained paths. |
| Discovery/cache | Empty resources/templates, modern private bounded TTL hints, legacy omission. |
| Safe errors | Exact malformed/missing numeric codes and discriminators, actual one-second expiry, file-size tampering produces `artifact_unavailable` / -32603, private paths absent from errors. |
| Compatibility/lifecycle | Disabled and legacy-output non-inline rejection; ordinary EOF removes owned artifacts/lock. |

The storage test restores its test-owned original bytes after verifying read refusal so
normal cleanup can safely complete. A development run left the tampered size and correctly
made shutdown refuse unsafe cleanup; the test was corrected without changing production.
The first new Python run failed at `resources/list` with -32601 because the fixture did
not yet enable the store. Wiring the explicit opt-in turned the same matrix green.

**Coverage boundary:** oversize resource reads, admission timeout/cancellation, exact
completion-time expiry/deadline and quota boundaries are covered by direct/internal Rust
tests, including paused-time tests. They were not forced through the independent process
matrix. This report does not claim a full wire error matrix for those boundaries.

## Actual connector through installed DeepSeek adapters

The inspected installed Harness is **0.2.1-rc.5**, build
`d4db512a969fccf0c5cc7ec8fe38e9c1d27af916`. Its extracted bundled adapters and temporary
probe remained outside the repository. The actual Rust fixture was launched by the
installed MCP client, with a fresh private root and isolated attachment home:

```sh
'/Applications/DeepSeek Harness.app/Contents/Resources/runtime/primary-runtime/dependencies/node/bin/node' \
  /private/tmp/deepseek-inspect/dsh/connector-media-smoke.mjs \
  /Users/comrade77/RustroverProjects/telegram-connector/target/debug/examples/offline_mcp_fixture \
  --typed-tools --artifact-root '{run_dir}/artifacts'
```

This is a conditional inspection rerun, requiring that installed build and the preserved
temporary extraction; those machine-specific dependencies are not portable repository
tests. The probe expands literal `{run_dir}` to a fresh `/private/tmp` root and refuses
exported paths outside it. A fake local capability resolver throws if any model stream
is requested. It mounts the actual installed MCP, resource, local filesystem, attachment
and tool services without changing the user's configuration.

The implementing agent ran the positive probe, then the root agent independently reran
it and confirmed **PASS**, with **0 model calls**. The committed full transcript is the
root agent's final rerun (the earlier positive transcript was replaced). The fixture's 64×64 JPEG was **1256 bytes**, SHA-256
`5d6ba0a5b326e869fea51ab264e2f3408305eec7eec5ae78762baf663c910959`.
The canonical resource bytes equaled the exported file; resource rendering produced only
text with a binary placeholder and omitted base64 from projection. The media result kept
its native inline image and link. `read_image({file_path})` read the exported JPEG outside
the temporary workspace, produced text plus native image content, and retained identical
durable attachment bytes. A text-only model route refused the read.

The [full combined adapter output](2026-11-01-media-artifacts-deepseek.txt) preserves
**46 external validator warnings** about ignored Rust numeric formats (`uint`, `uint32`,
`uint64`). These client-side warnings were not suppressed. They are separate from the
clean Rust gate and did not prevent successful adapter validation/execution. A real
pre-wiring connector probe failed with canonical `invalid_input`, as expected while
artifact mode was disabled; no fabricated result stood in for connector output.

This establishes installed-adapter execution/rendering and stored bytes, not delivery to
the user's chosen live model, account or desktop UI. Those remain unverified. Harness
normalization may resize larger images, and client copies persist under client policy.
Linux and Windows validation were not arranged. No new advisory-database freshness or
coverage percentage is claimed by this offline verification.

## Local evidence logs

Temporary exact transcripts are retained under `/private/tmp/media-artifacts-`:
`python-red.log`, `python-green.log`, `connector-smoke-red.log`,
`connector-smoke-green.log`, `fixture-build.log`, `fmt.log`, `clippy.log`, `tests.log`,
`rustdoc.log`, `release.log`, `version.log`, `existing-media.log`, `process.log`,
`probe.log`, `policy-fixtures.log` and `policy.log`. The complete successful installed
adapter output is additionally checked in at the link above. Temporary probe code and
third-party bundled sources are not committed.
