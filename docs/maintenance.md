# Dependency and toolchain maintenance

Normal CI checks Linux and macOS with `nightly-2026-10-31`. It fetches the locked
workspace graph, then runs formatting, workspace Clippy, tests, rustdoc, release
builds, isolated member builds, and the gateway dependency check. It also runs
the offline stdio process and client probes and the hosted account page fixture.
The scheduled and manually triggered floating-nightly workflow runs separately.
Windows runtime support remains unverified. [Check commands](conventions.md#formatting-and-quality-gate)
Action revisions are immutable and reviewed; checkout uses Node24 and requires
runner >=2.327.1 (hosted runners provide the runtime). Credentials are not persisted,
and workflows use only `contents: read` with ordinary pull_request events.

Security runs on pushes, PRs, a weekly schedule and manual dispatch. It installs
cargo-audit 0.22.0 and cargo-deny 0.20.2 with locked tool dependencies. Fresh audit
fails vulnerabilities, unsoundness and yanks; no advisory exception is currently
approved. Any future exception must identify the advisory, affected scope, reason,
named owner, expiry and tracking issue. Offline compile checks do not replace fresh
advisory data.

The workspace pins an unreleased rmcp revision based on 3.11.0 that makes Chrono
optional. Workspace defaults are disabled. The production stdio connector selects
`base64`, `macros`, and `server`. Its loopback fixture adds
`transport-streamable-http-server` in dev dependencies. The production gateway
selects `server` and `transport-streamable-http-server` directly. Both use the shared
workspace SDK pin, with `chrono` disabled.
[Dependency status](dependency-status.md#temporary-rmcp-pin--2026-11-20)

Return to a crates.io rmcp release once a stable version can resolve the whole
workspace without Chrono. Verify the complete graph and repeat the stdio and
HTTP conformance checks, then remove the Rust SDK repository from the source
allowlists in `deny.toml` and `dependency-policy.json`.

`cargo deny --locked check sources advisories` checks the entire graph.
The source policy covers crates.io and exact revisions of the official
grammers and Rust MCP SDK repositories. All three direct grammers dependencies
move together after review of upstream commits, TL changes, peer/session
fixtures and wire tests.

`python3 scripts/check_dependency_policy.py` reads complete locked Cargo metadata
(including dev/build/platform packages). The checker recognizes first-party packages
by exact reviewed names, manifest paths under `crates/`, and membership in this
checkout's virtual workspace. First-party licensing remains unassigned. Every
external package needs a reviewed source and full license declaration in
`dependency-policy.json`.
This is a declaration policy, not a source-text license or distribution-compliance
scanner. Full expressions are reviewed as a whole, including Unicode obligations;
the helper does not split AND/OR expressions. Selected alternatives are recorded
for review. Existing option-ext 0.2.0 has a version-scoped MPL-2.0 exception owned by
nimec77, expiring 2027-01-31; it does not authorize arbitrary copyleft dependencies.
The flate2 backend zlib-rs 0.6.8 has an equally exact Zlib declaration exception,
with the same owner/expiry and retained origin/notice obligations. This does not
approve Zlib for arbitrary packages. The jsonschema development validator adds exact
borrow-or-share 0.2.4 MIT-0 and foldhash 0.2.0 Zlib exceptions with the same owner/expiry;
their published notices were reviewed and do not broaden global license choices.
Hosted HTTPS verifies certificates with the reviewed Rustls dependency chain.
The pinned aws-lc-sys 0.47.0 and webpki-root-certs 1.0.12 declarations have exact
exceptions expiring 2027-01-31. Retain the bundled crypto source notices and
make the CDLA agreement text available with shared trust-anchor data.

All packages from an allowed Git repository must use one full revision.

Run policy fixtures with `python3 -m unittest discover -s scripts -p
test_dependency_policy.py`. They reject unlicensed external/path packages,
unreviewed combined obligations, wrong roots/incomplete graphs, unpinned or
unexpected Git sources, mismatched revisions and out-of-scope/expired exceptions.
`--metadata PATH` checks previously captured complete metadata; `--offline` is for
cached local checks and does not make an advisory scan fresh.

Dependabot groups rmcp and rmcp-macros updates, compatible registry patches, and
minor updates for selected stable-major dependencies. GitHub Actions have a
separate weekly schedule. Review the temporary SDK pin and grammers revision
changes manually. Incompatible updates stay individual. Auto-merge and write-token
update workflows are disabled.

The workflow linter is actionlint 1.7.12; its Linux release archive SHA256 is pinned
in CI. Primary references: [checkout v7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1),
[rust-toolchain pin](https://github.com/dtolnay/rust-toolchain/commit/683b588c487a69d01f6c6ddf40f036ff682cc2c8),
[dated nightly manifest](https://static.rust-lang.org/dist/2026-10-31/channel-rust-nightly.toml),
[cargo-deny](https://github.com/EmbarkStudios/cargo-deny/releases/tag/0.20.2),
[actionlint](https://github.com/rhysd/actionlint/releases/tag/v1.7.12).

Local tool/config validation is separate from hosted execution. Agent acceptance uses
native macOS checks; Linux setup/execution is owner-managed and does not block it.
The existing CI matrix remains configured for both platforms. If a hosted run is
performed later, record its actual platform and installed versions separately;
configuration alone does not prove a workflow ran or a live session migrated.
