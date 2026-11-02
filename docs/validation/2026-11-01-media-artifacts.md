# Media artifacts implementation verification — 2026-11-01

**Subsequent local integration — 2026-11-02:** version **0.25.0** was committed and
fast-forward merged into `master` at `84e170c`. The fresh version-bump gate passed
995 library tests + 3 doctests, formatting, strict all-target Clippy and warning-free
rustdoc. The production release build also passed and its binary reports
`telegram-mcp 0.25.0`. Completed guide/media plans and specs were archived after the merge; their
accepted content remains in Git history. Earlier sections below record the original
0.24.1 feature-branch verification. This integration does not push or publish a release.

M2 cached processed-JPEG resources and M3 explicit local file export are implemented on
`feat/media-artifact-delivery`. This records macOS offline verification of the actual
connector, following the [preimplementation client investigation](2026-11-01-deepseek-media-compatibility.md).
The permanent [media guide](../media-artifacts.md) describes activation and operational
limits. Final independent review closure is recorded below; integration remains separate.
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


## Final review fixes and fresh validation — 2026-11-02

The final fix wave starts at `38e6fc60535b4ce1a66f01e42e6a017c4a7d907c`.
The earlier sections and their 988-test count describe the prior dated validation.
This section records new evidence after correcting final review I1, I2 and M1.

**Current-response delivery:** response-owned pins protect artifacts from quota eviction
until complete tool-result assembly. If protected entries leave insufficient entry/byte
capacity, the new item receives the existing typed `unavailable` cache error, its batch
image charge is refunded, and the store remains usable. Pin destruction takes no store
mutex or filesystem action and cancellation releases completed pins. Pins do not extend
logical TTL or keep the store alive. Canonical publication rejects expired artifacts and
handles from a known failed/closed store, with late batch refunds. Later independent
requests may still evict artifacts after assembly; no minimum retention is promised.

A private transparent canonical wrapper retains pins through structured JSON, matching
text and content-block assembly. Existing public `MediaData` fields and wire shape remain
unchanged. Overhead shadows retain descriptors and link metadata but neither pins nor
refund authority. Original media/read/shutdown deadlines and started-worker ownership
are unchanged, and metadata-overflow rollback still removes owned pinned entries.

The entry-quota and byte-quota regressions failed against the original implementation
because the first reported successful artifact could not be immediately read. Both now
pass for `resource`, `file`, and `resource_and_file`, checking all reported URI/path bytes,
actual indices, typed partial failures and refunds. Delayed-batch expiry and later storage
failure likewise failed with an earlier success descriptor and now become explicit item
failures. Further tests cover transient capacity recovery, release after reply/cancellation,
canonical guard lifetime, shadow independence and expiry after data preparation.

The queued-periodic-cleanup Weak/held-worker scenario now runs in an ignored isolated
child. Its ordinary parent checks successful execution and enforces a 15-second bound
with child termination on timeout. The behavioral body and assertions are preserved;
no deterministic reproduction of the former losing parallel schedule is claimed.
No suite-wide serialization or production test hook was introduced. The three factual
resource-handler, canonical ancestry and internal-DTO guide statements were corrected.

The final macOS offline gate passed **995 library tests, 0 failures, 8 ignored**, plus
**3 doctests**. Six ignored subprocess entrypoints are exercised by ordinary parents;
the other two are the existing optional latency/RSS measurements. This adds seven behavior
tests and one isolated child entrypoint. Formatting, strict all-target Clippy and
warning-free rustdoc passed. Release and fixture builds passed; the release remains
`telegram-mcp 0.24.1`. All four repository Python client/process/probe scripts above passed,
as did 25 dependency-policy fixtures and the 340-package policy. Package, lockfile,
dependency pins and toolchain are unchanged. Current guide relative links and
`git diff --check` passed.

Root regenerated the temporary extractor/probe from the same installed Harness
0.2.1-rc.5 build `d4db512a969fccf0c5cc7ec8fe38e9c1d27af916` and independently ran it
against the fixed fixture. The fresh [full installed-adapter transcript](2026-11-02-media-artifacts-deepseek.txt)
records **PASS**, **0 model calls**, and **46 external numeric-format warnings** (counted
from this new log; retained unsuppressed). Resource/file/preview/durable-attachment bytes
match, native image projection remains available, the text-only route refuses reading,
and the 1256-byte JPEG hash remains
`5d6ba0a5b326e869fea51ab264e2f3408305eec7eec5ae78762baf663c910959`.
The original November 1 transcript remains unchanged. This still establishes installed
adapter execution, not chosen live model, desktop UI, account or Telegram behavior.

New exact command logs and the detailed RED/GREEN report are retained in the local ignored
`.superpowers/sdd/2026-11-01-media-artifacts/` workspace as `final-fix-*.log` and
`final-fix-report.md`; the older `/private/tmp` logs named above are historical locations.
The new installed-adapter transcript is committed separately at the link above. Final
scoped re-review and integration remain root-owned; no push, merge, version bump, user/client
configuration edit, live call or Linux setup is part of this fix wave.

## Independent review closure — 2026-11-02

The full branch review covered `1a53ea0..38e6fc6`, including the preceding current-guide
cleanup. It found no Critical or production security-boundary issue, but identified
unusable same-batch artifact handles and a parallel-test circular wait, plus three
factual guide corrections. Fix `c811191df63fd05f0ba3b4c630854dde506b584b` addressed
all findings. A fresh scoped re-review of `38e6fc6..c811191` judged I1, I2 and M1
**ADDRESSED**, with no new breakage or out-of-scope observations.

The root inspected the final 995-library/3-doctest, strict Clippy, rustdoc, release and
policy evidence and independently executed the corrected fixture through installed
DeepSeek components. The original dated gates/transcript remain historical evidence;
the new [November 2 transcript](2026-11-02-media-artifacts-deepseek.txt) records the
post-fix success. No live chosen-model/UI/account/Telegram or non-macOS execution is
claimed. The feature remains on its local branch; no merge, version bump, push or
publication occurred. Subsequent review closeout changes only Markdown.

Implementation decisions and their tradeoffs:

1. Used the clean existing checkout on a feature branch, adding no worktree; unrelated
   simultaneous work would require renewed isolation. Earlier documentation branches remain.
2. Retained inline previews in every delivery mode because this DeepSeek resource reader
   does not project blobs as images; the initial image payload therefore remains.
3. Refuse nonempty/stale roots without automatic deletion; forced failure/exit can require
   inspected cleanup or a fresh private root.
4. Promoted the already-locked getrandom 0.4.3 identity for safe OS randomness; this adds
   one direct declaration without expanding the resolved package graph.
5. Use exact `telegram-media://artifact/<48 lowercase hex>` identifiers; alternate URI
   spellings are rejected rather than normalized.
6. Cache reads have five seconds including admission; slow reads fail while started work
   retains its worker ownership.
7. Explicit closure shares the original shutdown deadline; zero/exhausted budgets can
   leave the documented bounded stale-root state.
8. Corrected terminal root-symlink spellings during integration, preserving physical
   parent traversal; this required a narrow store-helper change and regressions.
9. Preserved SDK protocol-specific numeric error mapping with stable safe discriminators;
   clients use `error.data.code` where modern numeric codes converge.
10. Protect current-response artifacts until assembly and fail insufficient/expired
    delivery explicitly; temporary per-item capacity failures replace unusable success.
11. Corrected factual resource/privacy/type documentation without widening public Rust
    visibility or claiming support for an inaccessible type.

The review explicitly left hostile same-UID races outside the established trust boundary,
actual model/UI/account/client retention and other-platform acceptance unverified, and full
original attachment/video/audio/document downloading outside this feature. Logical expiry,
cleanup lag, cancellation ownership and exhausted-grace leftovers were reviewed within
scope and match the documented behavior. Detailed session logs/review packages were
inspected in the ignored plan workspace; the checked-in reports/transcripts and commands
above are the durable record.
