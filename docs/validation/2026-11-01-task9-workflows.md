# Task 9 workflow and dependency-maintenance acceptance

Validated on 2026-11-01 in the `chore/final-modernization-acceptance` worktree on
Darwin arm64. This records configuration review and native macOS validation.
It does not claim a hosted workflow run. Linux execution remains owner-managed;
no dispatch, Linux tooling, VM/container or live Telegram operation was used.

## Acceptance evidence

| Task 9 item | Configuration and evidence | Result |
| --- | --- | --- |
| Reviewed action pins, least privilege, credentials and runtime | Every `uses` in all three workflows is a full 40-character SHA. `actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1` resolves to official `v7.0.1`; `dtolnay/rust-toolchain@683b588c487a69d01f6c6ddf40f036ff682cc2c8` resolves to the upstream commit dated 2026-10-16. Each workflow grants exactly `contents: read`, with no job override; each checkout sets `persist-credentials: false`. Runtime/source review below verifies Node24 and the runner floor. | Configuration and upstream identities verified. |
| Required dated nightly and separate floating schedule | `rust-toolchain.toml`, `ci.yml` and `security.yml` select `nightly-2026-10-31`; `floating-nightly.yml` selects `nightly` only in its separate scheduled/manual workflow, weekly Monday 06:43 UTC. Native `rustup show active-toolchain` returned `nightly-2026-10-31-aarch64-apple-darwin`; `rustc --version` returned `1.101.0-nightly (92b5b8c87 2026-10-29)`. | Exact configuration parity verified; tested toolchain evidence belongs to the combined quality gate. |
| Fresh advisories, unsoundness and exceptions | `security.yml` runs on ordinary PRs, pushes and Monday 06:17 UTC. Pinned cargo-audit 0.22.0 runs `cargo audit --deny unsound --deny yanked`, without `--offline`, `--no-fetch` or advisory ignores. Vulnerabilities fail by default; unsoundness and yanks explicitly fail. `cargo deny --locked check sources advisories` independently checks advisories. `deny.toml` has `ignore = []`, `yanked = "deny"`. `docs/maintenance.md` requires advisory, affected scope, rationale, owner, expiry and tracking issue for any future exception; none is approved. | Policy and scan wiring verified; fresh scan execution is recorded with the combined security gate. |
| Dependency updates separated by risk | Weekly Cargo and GitHub Actions update entries are separate. rmcp/rmcp-macros minor and patch updates share the rmcp group. `grammers-*` is ignored at ecosystem scope, preserving manual joint revision review. Generic patch and explicitly stable-major minor groups exclude incompatible updates by update type; ungrouped majors and unlisted 0.x minor updates remain individual. No auto-merge or write-token workflow exists. Schema validation and representative routing cases below passed. | Verified. |
| Explicit sources, licenses and yanks; fast normal gate | `deny.toml` denies unknown registries/Git and yanks, permits only crates.io and the official Codeberg grammers URL, and requires `rev`. `dependency-policy.json` plus `check_dependency_policy.py` checks the complete locked metadata graph, including dev/build/platform packages, exact Git revisions and reviewed full license expressions. Only the exact local application root is exempt; missing/unknown licenses fail. Four exact package/version exceptions carry owner, rationale and 2027-01-31 expiry. Network security checks/tool installs stay in `security.yml`; ordinary CI runs only fast local policy fixtures. | Explicit policy and 25 regression fixtures verified; whole-graph execution belongs to the combined security gate. |
| Syntax and macOS quality checks | Official actionlint 1.7.12 macOS arm64 binary passed all workflows; parsed YAML, shell syntax and semantic checks passed. Dependabot passed versioned JSON Schema validation, checked against authoritative GitHub options documentation. The only CI change adds the existing independent Python media client to the built offline fixture checks. | Config validation passed. Full Rust/macOS quality, release and client results are recorded by the final acceptance report. Automation is already tracked; this work adds one CI command and this evidence record. |

## Upstream action and runner review

Read-only GitHub commands used the `gh` skill (native release commands, API for
ref/commit/content fields not exposed by native commands):

```sh
gh release view v7.0.1 --repo actions/checkout --json tagName,targetCommitish,url
gh api repos/actions/checkout/git/ref/tags/v7.0.1
gh api repos/dtolnay/rust-toolchain/commits/683b588c487a69d01f6c6ddf40f036ff682cc2c8
```

The checkout tag is a direct commit ref to
`3d3c42e5aac5ba805825da76410c181273ba90b1`. Pinned `action.yml` declares
`runs.using: node24`. The [pinned upstream README](https://github.com/actions/checkout/blob/3d3c42e5aac5ba805825da76410c181273ba90b1/README.md)
requires Actions Runner **2.327.1 or newer**. Its separate **2.329.0** floor applies
to authenticated Git inside Docker container actions, which these workflows do
not use. Git 2.18 or newer supports normal checkout; upstream documents REST
fallback when it is unavailable. The workflows use hosted `ubuntu-latest` and
`macos-latest` labels, and no self-hosted runner is being qualified here. The
actual version of a future hosted runner remains execution-time evidence.

The [pinned Rust installer](https://github.com/dtolnay/rust-toolchain/blob/683b588c487a69d01f6c6ddf40f036ff682cc2c8/action.yml)
is a composite Bash action, with no nested `uses` action or Node requirement.
The explicit `toolchain` input overrides the action revision's default toolchain
selection. It invokes rustup with a minimal profile and requested components,
installing rustup through curl when absent. Its special component-availability
`--allow-downgrade` behavior applies only to the separate floating `nightly`
input, not the exact required dated nightly. The action source contains bounded
checksum-race retries; jobs themselves have 45-minute timeouts.

All workflows use ordinary `pull_request` where applicable, never
`pull_request_target` or `workflow_run`; no unsafe fork checkout opt-in is set.

## Native workflow/config validation

Downloaded only the official **Darwin arm64** asset and checksum manifest into
`/private/tmp/task9-actionlint`, without a global installation:

```sh
gh release view v1.7.12 --repo rhysd/actionlint --json tagName,url,assets
gh release download v1.7.12 --repo rhysd/actionlint \
  --pattern actionlint_1.7.12_darwin_arm64.tar.gz \
  --pattern actionlint_1.7.12_checksums.txt --dir /private/tmp/task9-actionlint
```

From that temporary directory, `shasum -a 256 -c
 actionlint_1.7.12_checksums.txt --ignore-missing` returned
`actionlint_1.7.12_darwin_arm64.tar.gz: OK`. SHA256:
`aba9ced2dee8d27fecca3dc7feb1a7f9a52caefa1eb46f3271ea66b6e0e6953f`,
matching both the upstream checksum file and GitHub release asset digest.
`file actionlint` reported `Mach-O 64-bit executable arm64`;
`actionlint -version` reported 1.7.12, built with Go 1.26.1 for darwin/arm64.
The Linux archive digest already pinned in `security.yml`,
`8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8`,
also matches the official manifest/release metadata; it was neither downloaded
nor executed during this work.

```sh
/private/tmp/task9-actionlint/actionlint -shellcheck= -pyflakes= .github/workflows/*.yml
```

Passed before and after the CI edit (exit 0, no diagnostics). Shellcheck and
Pyflakes integration were explicitly disabled, matching existing CI; this is not
a claim those tools ran. A separate local validator parsed all workflow YAML,
ran `bash -n` on each actual `run` script, and checked action pin structure,
permissions, checkout credentials, required/floating toolchain selection,
positive job timeouts, scheduled triggers and the security PR trigger. All passed.

Dependabot was parsed with PyYAML 6.0.3 and validated with jsonschema 4.26.0,
isolated under `/private/tmp/task9-actionlint/venv`. A YAML loader retained `on`
as a string and interpreted explicit `true`/`false` booleans. The supplemental
[SchemaStore schema](https://github.com/SchemaStore/schemastore/blob/1f9ebc8379f842319663f776f9767fdf6ca5e4a5/src/schemas/json/dependabot-2.0.json)
was fixed to commit `1f9ebc8379f842319663f776f9767fdf6ca5e4a5`, SHA256
`f310bfb4960eea6a5fb7d75de54ab49ba36f947757c8f4d5de88d3ea8ac51d3f`.
It is community-maintained; normative semantics were checked against the
[official GitHub options reference](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference)
and [official grouping examples](https://docs.github.com/en/code-security/tutorials/secure-your-dependencies/optimizing-pr-creation-version-updates).
GitHub documents wildcard exclusions, first matching group behavior,
`major`/`minor`/`patch` grouping, and individual updates outside groups.

The local command
`/private/tmp/task9-actionlint/venv/bin/python /private/tmp/task9-actionlint/validate_configs.py`
passed schema validation and rejected an invalid schedule-interval negative
control. It exercised parsed policy routing with these results:

| Dependency/update | Disposition |
| --- | --- |
| rmcp minor; rmcp-macros patch | rmcp group |
| rmcp major; toml major | individual |
| grammers-client patch; grammers-session major; grammers-mtsender minor | ignored |
| tokio minor | stable-compatible-minors |
| serde patch | compatible-patches |
| image minor; base64 minor | individual |

Every member of the stable-compatible-minors allowlist currently has major >=1
in Cargo.toml. Thus numeric minor grouping does not inadvertently group current
0.x breaking minor updates. These semantic examples supplement schema validation;
they do not claim a hosted Dependabot update job was executed.

`python3 -m unittest discover -s scripts -p test_dependency_policy.py` passed
**25 tests**, covering incomplete metadata, exact root scope, license obligations,
Git pin/source/revision restrictions and expired/out-of-scope exceptions.
`git diff --check` passed after the CI change.

## Change and limits

CI previously built the offline fixture and ran the Python stdio/probe scripts,
but omitted `scripts/test_media_clients.py`. Added that existing script to the
same step. It lists all 16 real tools and exercises media images, keyed replay,
and mixed-success batches across typed/legacy output and modern/legacy protocol.
This is a configuration coverage addition: validation used actionlint and parsed
config semantics rather than a test that matches exact YAML text. Its native
execution and the full Rust/security gate are integrated in the final acceptance
record; they were not duplicated in this scoped configuration audit.

Source/license metadata checks do not constitute source-text license or
distribution-compliance scanning. Root package licensing remains unassigned.
Windows runtime/ACL support and live account behavior remain unverified.
Optional media M2/M3 and release publication are outside this workstream.

## Combined native execution and closure

Parent integration completed all previously deferred command evidence, then refreshed it
after the combined security review: format, explicit selected/all-target Clippy, 942 library
tests + 3 doctests, warning-free rustdoc, production release, both protocol clients, native
process checks and inert composition all exit 0. Exact pinned macOS cargo-deny sources/
advisories, fresh cargo-audit, complete 340-external-identity license/source policy and
25 policy fixtures pass. Native actionlint/config validation was also rerun by the parent.
See [final acceptance](2026-11-01-final-acceptance.md) for commands and scope. Task 9's
six-item checklist is complete. The media-client output path was additionally corrected
and tested to honor TMPDIR on macOS, preserving existing cross-platform CI configuration
without claiming a Linux execution. No workflow dispatch was performed.
