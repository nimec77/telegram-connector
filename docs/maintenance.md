# Dependency and toolchain maintenance

Normal CI checks Linux and macOS with `nightly-2026-10-31`, locked dependency
fetching, offline formatting/Clippy/tests/release builds, and the offline stdio
process/probe checks. The floating-nightly workflow is scheduled/manual and separate
from the reproducible required gate. Windows runtime support remains unverified.
Action revisions are immutable and reviewed; checkout uses Node24 and requires
runner >=2.327.1 (hosted runners provide the runtime). Credentials are not persisted,
and workflows use only `contents: read` with ordinary pull_request events.

Security runs on pushes, PRs, a weekly schedule and manual dispatch. It installs
cargo-audit 0.22.0 and cargo-deny 0.20.2 with locked tool dependencies. Fresh audit
fails vulnerabilities, unsoundness and yanks; no advisory exception is currently
approved. Any future exception must identify the advisory, affected scope, reason,
named owner, expiry and tracking issue. Offline compile checks do not replace fresh
advisory data.

`cargo deny --locked check sources advisories` checks the entire graph. Sources
are restricted to crates.io and immutable revisions of the official Codeberg
grammers repository. All three direct grammers dependencies move together after
review of upstream commits, TL changes, peer/session fixtures and wire tests.

`python3 scripts/check_dependency_policy.py` reads complete locked Cargo metadata
(including dev/build/platform packages). The metadata-declaration check exempts
only the exact local resolve.root named telegram-connector: its own licensing is
unassigned. No license grant or publish policy is invented. Every other package,
including path/workspace dependencies, needs a reviewed full declaration in
`dependency-policy.json`; unknown sources, missing or unreviewed licenses fail.
This is a declaration policy, not a source-text license or distribution-compliance
scanner. Full expressions are reviewed as a whole, including Unicode obligations;
the helper does not split AND/OR expressions. Selected alternatives are recorded
for review. Existing option-ext 0.2.0 has a version-scoped MPL-2.0 exception owned by
nimec77, expiring 2027-01-31; it does not authorize arbitrary copyleft dependencies.
The flate2 backend zlib-rs 0.6.8 has an equally exact Zlib declaration exception,
with the same owner/expiry and retained origin/notice obligations. This does not
approve Zlib for arbitrary packages. All packages from an allowed Git repository
must use one full revision.

Run policy fixtures with `python3 -m unittest discover -s scripts -p
test_dependency_policy.py`. They reject unlicensed external/path packages,
unreviewed combined obligations, wrong roots/incomplete graphs, unpinned or
unexpected Git sources, mismatched revisions and out-of-scope/expired exceptions.
`--metadata PATH` checks previously captured complete metadata; `--offline` is for
cached local checks and does not make an advisory scan fresh.

Dependabot separates rmcp, compatible registry patches, explicitly stable-major
minor updates and GitHub Actions. Incompatible updates stay individual; grammers
rev updates are manual and never follow master automatically. No auto-merge or
write-token update workflow is enabled.

The workflow linter is actionlint 1.7.12; its Linux release archive SHA256 is pinned
in CI. Primary references: [checkout v7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1),
[rust-toolchain pin](https://github.com/dtolnay/rust-toolchain/commit/683b588c487a69d01f6c6ddf40f036ff682cc2c8),
[dated nightly manifest](https://static.rust-lang.org/dist/2026-10-31/channel-rust-nightly.toml),
[cargo-deny](https://github.com/EmbarkStudios/cargo-deny/releases/tag/0.20.2),
[actionlint](https://github.com/rhysd/actionlint/releases/tag/v1.7.12).

Local tool/config validation is separate from hosted execution. Record the actual
Linux/macOS CI run and installed versions at final acceptance; configuration alone
does not prove a workflow ran or a live Telegram session migrated.
