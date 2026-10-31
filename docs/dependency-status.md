# Dependency status — 2026-10-31

Fresh crates.io inventory completed at 2026-10-31T08:39:39.597680+00:00. This records the resolved dependency migration on nightly-2026-10-31, not just manifest requirements. Cargo.lock was updated in groups and retained throughout; incompatible upstream lines were not forced.

The tables below are the **Task 6 migration snapshot**, before the Task 7 schema
validator. Task 7 adds dev-only jsonschema 0.60.2 and 36 transitive package identities
without removing/upgrading an existing identity: the graph now has **341 packages,
340 external**, and 26 distinct direct registry dependencies. Its added older lines
and exact license exceptions are recorded in [the contract guide](tool-contract.md#validator-dependency).
Task 7 reran the full metadata policy and fresh source/advisory checks; the historical
304-package counts and duplicate tables below describe Task 6, not the expanded graph.

## Direct dependencies

All 25 distinct registry direct dependencies resolve to their current non-yanked stable release. The three direct grammers Git dependencies share the reviewed immutable Codeberg revision below. rmcp 3.8.0 was migrated in the preceding SDK work; this refresh preserves its runtime/protocol integration. Broad existing requirements that already admitted the latest lockfile release remain reviewed dispositions, not presumed stale versions.

| Registry dependency | Scope | Manifest requirement(s) | Resolved = current stable | Disposition |
| --- | --- | --- | --- | --- |
| [anyhow](https://crates.io/api/v1/crates/anyhow) | normal | 1.0.104 | 1.0.104 | Updated and validated |
| [async-trait](https://crates.io/api/v1/crates/async-trait) | normal | 0.1 | 0.1.92 | Current; retained |
| [base64](https://crates.io/api/v1/crates/base64) | normal | 0.23.1 | 0.23.1 | Updated and validated |
| [chrono](https://crates.io/api/v1/crates/chrono) | normal | 0.4.45 | 0.4.45 | Current; retained |
| [clap](https://crates.io/api/v1/crates/clap) | normal | 4.6.7 | 4.6.7 | Updated and validated |
| [dialoguer](https://crates.io/api/v1/crates/dialoguer) | normal | 0.12.0 | 0.12.0 | Current; retained |
| [directories](https://crates.io/api/v1/crates/directories) | normal | 6.0.0 | 6.0.0 | Current; retained |
| [filetime](https://crates.io/api/v1/crates/filetime) | dev | 0.2.29 | 0.2.29 | Current; retained |
| [futures](https://crates.io/api/v1/crates/futures) | normal | 0.3.34 | 0.3.34 | Updated and validated |
| [image](https://crates.io/api/v1/crates/image) | normal + dev | 0.25 | 0.25.10 | Current; retained |
| [libsql](https://crates.io/api/v1/crates/libsql) | dev | 0.10.0 | 0.10.0 | New dev declaration reuses sole production storage package; core only |
| [mockall](https://crates.io/api/v1/crates/mockall) | dev | 0.15.0 | 0.15.0 | Updated and validated |
| [proptest](https://crates.io/api/v1/crates/proptest) | dev | 1.12.0 | 1.12.0 | Updated and validated |
| [rmcp](https://crates.io/api/v1/crates/rmcp) | normal | 3.8 | 3.8.0 | Current; preceding SDK migration retained |
| [schemars](https://crates.io/api/v1/crates/schemars) | normal | 1.2.3 | 1.2.3 | Updated and validated |
| [secrecy](https://crates.io/api/v1/crates/secrecy) | normal | 0.10 | 0.10.3 | Current; retained |
| [serde](https://crates.io/api/v1/crates/serde) | normal | 1.0 | 1.0.229 | Current; retained |
| [serde_json](https://crates.io/api/v1/crates/serde_json) | normal | 1.0.154 | 1.0.154 | Updated and validated |
| [tempfile](https://crates.io/api/v1/crates/tempfile) | dev | 3.27.0 | 3.27.0 | Current; retained |
| [thiserror](https://crates.io/api/v1/crates/thiserror) | normal | 2.0.21 | 2.0.21 | Updated and validated |
| [tokio](https://crates.io/api/v1/crates/tokio) | normal + dev | 1.54.1 | 1.54.1 | Current; retained |
| [toml](https://crates.io/api/v1/crates/toml) | normal | 1.1.8 | 1.1.8+spec-1.1.0 | Updated and validated |
| [tracing](https://crates.io/api/v1/crates/tracing) | normal | 0.1.44 | 0.1.44 | Current; retained |
| [tracing-appender](https://crates.io/api/v1/crates/tracing-appender) | normal | 0.2.5 | 0.2.5 | Current; retained |
| [tracing-subscriber](https://crates.io/api/v1/crates/tracing-subscriber) | normal | 0.3 | 0.3.23 | Current; retained |

The image dev-only PNG codec and Tokio dev-only test-util features remain enabled; production image support remains JPEG-only. libsql is declared only as a new dev dependency for reproducing the previous SQLite schema without external executables or committed session databases. It resolves to the same 0.10.0 package already required by grammers-session; enabled features are only core and libsql-sys, with remote/TLS defaults disabled.

## Telegram revision and compatibility

All direct grammers-client, grammers-session and grammers-mtsender pins are **1e31e73cd9797790905bbea82cd75adcbe6f1242**, upstream 0.11.0, 2026-10-27. Fresh upstream HEAD matched that SHA immediately before migration. [Reviewed 41-commit comparison](https://codeberg.org/Lonami/grammers/compare/9fef0bae1e59b6138ae7777c783983934a80e129...1e31e73cd9797790905bbea82cd75adcbe6f1242). All eight resolved grammers Git packages use this same source revision (the TL parser retains its independent package version 1.2.2).

- Telegram layers through 232 add Channel in_news/pinned_msg_id fields, new service actions/entities and richer button/wallet types. Four raw Channel test fixtures were updated; no project production enum/error match needed an API change. Existing raw message, media, channel/Community and forward conversions passed the complete regression suite.
- InvocationError::Dropped now carries a diagnostic string, and ReadError::Transport was removed. Project transcription classifies only InvocationError::Rpc and otherwise preserves an opaque failure; no reason-string matching was introduced.
- Session cache_peer now consumes PeerInfo, cache_peers batches multiple peers, and newer non-None auth/kind data supersedes stale observations. A real cache-refresh behavior test failed on the old pin and passes on the new pin. Bulk-cache tests verify every user/basic-chat/channel entry, refresh, partial-field retention and reopen.
- The previous v1 SQLite schema is recreated from its old DDL into TempDir, with fake peer hashes and update-state rows. New grammers/libsql reopens it twice with IDs, peer namespaces, hashes, home DC and update state intact. This proves the tested offline schema path, not live account migration or lossless optional metadata.
- Message.peers remains pub(crate) in [the reviewed source](https://codeberg.org/Lonami/grammers/src/commit/1e31e73cd9797790905bbea82cd75adcbe6f1242/grammers-client/src/message/message.rs). Raw envelope-preserving pagers/fetches remain; no extra resolution RPC or media container migration was introduced.

### Explicit remaining identity/session limitations

**Bare numeric dialog lookup is ambiguous across peer kinds.** find_dialog_peer returns the first dialog whose PeerId.bare_id equals the requested number. Offline fixtures prove same-number user/chat/channel IDs occupy distinct typed namespaces and that raw envelope/message-batch handling preserves those namespaces. The numeric first-match lookup can nevertheless select a different peer kind depending on dialog order; no live collision was claimed. This migration preserves the identifier interface and RPC count. Use an unambiguous username where available; typed/canonical identity design and any ambiguity policy remain a follow-up, not a silent behavioral change.

**SQLite optional metadata is not lossless.** In both old and reviewed grammers revisions, [sqlite.rs](https://codeberg.org/Lonami/grammers/src/commit/1e31e73cd9797790905bbea82cd75adcbe6f1242/grammers-session/src/storages/sqlite.rs) writes Community subtype 16 but the reader does not restore Community kind; it returns kind=None while preserving ID/hash. User bot=false/is_self=false is encoded as NULL subtype and reads as None/None. Two passing pinned-revision characterization tests record these observations separately from the supported-kind refresh regression. Raw Community/megagroup conversion and envelope behavior remain covered and correct. No private-API or vendored upstream patch was applied.

## Security, sources and license declarations

The complete graph has **304 packages**: **295 registry package versions / 286 registry names**, **8 Codeberg Git packages**, and the workspace root. Fresh crates.io queries found **zero yanked locked registry versions**, no missing external license metadata, and **zero newer stable releases within any locked Cargo compatibility line**. Forty-four older incompatible package versions remain blocked by the exact parent requirements below.

Fresh cargo-audit 0.22.0 with --deny unsound --deny yanked returned zero vulnerabilities and no warnings, using RustSec DB **2295466c4e9df26647fd67d8fa1c705ac2fc7f81** (1,395 advisories, updated 2026-10-30T12:12:39+01:00). [Database snapshot](https://github.com/RustSec/advisory-db/tree/2295466c4e9df26647fd67d8fa1c705ac2fc7f81). No advisory ignore or target filtering was added. cargo-deny 0.20.2 reported advisories ok, sources ok.

Sources remain only crates.io and https://codeberg.org/Lonami/grammers at one full rev pin. The root license remains **unassigned**; its exact metadata identity is exempt from external dependency declaration checks, with its dependencies fully checked. No license grant, publish policy or blanket unknown-license approval was introduced.

The registry archive for newly introduced zlib-rs 0.6.8 declares Zlib, with the standard permissive origin/notice obligations. It is reached via flate2 1.1.10. The external metadata policy intentionally rejected the new declaration until reviewed; the parent policy task reviewed and added an exact zlib-rs 0.6.8 Zlib exception (owner nimec77, expiry 2027-01-31), retaining origin, altered-source and notice obligations. No global Zlib approval was added. The final full metadata policy now passes for all 303 external packages. Existing option-ext 0.2.0 retains its scoped MPL-2.0 exception. Standard declaration choices select MIT/Apache alternatives where available and retain combined Unicode obligations. This is declaration review, not a source-text distribution compliance audit.

## Distinct-version duplicates

| Name | Resolved versions |
| --- | --- |
| bitflags | 1.3.2, 2.13.2 |
| linux-raw-sys | 0.12.1, 0.4.15 |
| miniz_oxide | 0.8.9, 0.9.1 |
| rustix | 0.38.44, 1.1.6 |
| shlex | 1.3.0, 2.0.1 |
| syn | 2.0.119, 3.0.9 |
| thiserror | 1.0.69, 2.0.21 |
| thiserror-impl | 1.0.69, 2.0.21 |
| windows-sys | 0.59.0, 0.61.2 |

Host/target feature contexts can repeat an identical version in cargo tree -d; those are not additional multiple-version findings. The graph includes platform/build/dev branches.

## Every blocked latest line

All 44 entries below are outside the exact listed parent constraints. The removal path is an upstream parent release admitting the newer line, followed by a reviewed parent migration; do not force Cargo overrides. libsql 0.10 still retains its old bindgen/thiserror/parking_lot/zerocopy lines, and mockall 0.15 still retains fragile 2.

| Package | Resolved | Global latest stable | Current parent constraints | Removal path |
| --- | --- | --- | --- | --- |
| [bindgen](https://crates.io/api/v1/crates/bindgen) | 0.66.1 | 0.73.2 | libsql-ffi 0.10.0 (`^0.66.1`) | Migrate libsql-ffi when their upstream requirements advance |
| [bitflags](https://crates.io/api/v1/crates/bitflags) | 1.3.2 | 2.13.2 | defmt 1.2.0 (`^1`) | Migrate defmt when their upstream requirements advance |
| [fragile](https://crates.io/api/v1/crates/fragile) | 2.1.0 | 3.0.0 | mockall 0.15.0 (`^2.0`) | Migrate mockall when their upstream requirements advance |
| [libloading](https://crates.io/api/v1/crates/libloading) | 0.8.9 | 0.9.0 | clang-sys 1.9.1 (`^0.8`) | Migrate clang-sys when their upstream requirements advance |
| [libredox](https://crates.io/api/v1/crates/libredox) | 0.1.25 | 0.2.0 | redox_users 0.5.3 (`^0.1.3`) | Migrate redox_users when their upstream requirements advance |
| [linux-raw-sys](https://crates.io/api/v1/crates/linux-raw-sys) | 0.4.15 | 0.12.1 | rustix 0.38.44 (`^0.4.14`) | Migrate rustix when their upstream requirements advance |
| [lock_api](https://crates.io/api/v1/crates/lock_api) | 0.4.14 | 0.5.0 | parking_lot 0.12.5 (`^0.4.14`) | Migrate parking_lot when their upstream requirements advance |
| [miniz_oxide](https://crates.io/api/v1/crates/miniz_oxide) | 0.8.9 | 0.9.1 | png 0.18.1 (`^0.8`) | Migrate png when their upstream requirements advance |
| [moxcms](https://crates.io/api/v1/crates/moxcms) | 0.8.1 | 0.9.1 | image 0.25.10 (`^0.8.0`) | Migrate image when their upstream requirements advance |
| [nom](https://crates.io/api/v1/crates/nom) | 7.1.3 | 8.0.0 | cexpr 0.6.0 (`^7`) | Migrate cexpr when their upstream requirements advance |
| [parking_lot](https://crates.io/api/v1/crates/parking_lot) | 0.12.5 | 0.13.0 | libsql 0.10.0 (`^0.12.1`) | Migrate libsql when their upstream requirements advance |
| [parking_lot_core](https://crates.io/api/v1/crates/parking_lot_core) | 0.9.12 | 0.10.0 | parking_lot 0.12.5 (`^0.9.12`) | Migrate parking_lot when their upstream requirements advance |
| [peeking_take_while](https://crates.io/api/v1/crates/peeking_take_while) | 0.1.2 | 1.0.0 | bindgen 0.66.1 (`^0.1.2`) | Migrate bindgen when their upstream requirements advance |
| [prettyplease](https://crates.io/api/v1/crates/prettyplease) | 0.2.37 | 0.3.0 | bindgen 0.66.1 (`^0.2.0`) | Migrate bindgen when their upstream requirements advance |
| [quick-error](https://crates.io/api/v1/crates/quick-error) | 1.2.3 | 2.0.1 | rusty-fork 0.3.1 (`^1.2`) | Migrate rusty-fork when their upstream requirements advance |
| [r-efi](https://crates.io/api/v1/crates/r-efi) | 6.0.0 | 7.1.0 | getrandom 0.4.3 (`^6`) | Migrate getrandom when their upstream requirements advance |
| [redox_syscall](https://crates.io/api/v1/crates/redox_syscall) | 0.5.18 | 0.9.5 | parking_lot_core 0.9.12 (`^0.5`) | Migrate parking_lot_core when their upstream requirements advance |
| [rustc-hash](https://crates.io/api/v1/crates/rustc-hash) | 1.1.0 | 2.1.3 | bindgen 0.66.1 (`^1.0.1`) | Migrate bindgen when their upstream requirements advance |
| [rustix](https://crates.io/api/v1/crates/rustix) | 0.38.44 | 1.1.6 | which 4.4.2 (`^0.38.10`) | Migrate which when their upstream requirements advance |
| [shlex](https://crates.io/api/v1/crates/shlex) | 1.3.0 | 2.0.1 | bindgen 0.66.1 (`^1`) | Migrate bindgen when their upstream requirements advance |
| [syn](https://crates.io/api/v1/crates/syn) | 2.0.119 | 3.0.9 | bindgen 0.66.1 (`^2.0`); jiff-static 0.2.38 (`^2.0.98`); mockall_derive 0.15.0 (`^2.0.52`); prettyplease 0.2.37 (`^2.0.105`); thiserror-impl 1.0.69 (`^2.0.87`); tracing-attributes 0.1.31 (`^2.0`); windows-implement 0.60.2 (`^2.0`); windows-interface 0.59.3 (`^2.0`); zerocopy-derive 0.7.35 (`^2.0.31`) | Migrate bindgen, jiff-static, mockall_derive, prettyplease, thiserror-impl, tracing-attributes, windows-implement, windows-interface, zerocopy-derive when their upstream requirements advance |
| [termtree](https://crates.io/api/v1/crates/termtree) | 0.5.1 | 1.0.0 | predicates-tree 1.0.13 (`^0.5.0`) | Migrate predicates-tree when their upstream requirements advance |
| [thiserror](https://crates.io/api/v1/crates/thiserror) | 1.0.69 | 2.0.21 | libsql 0.10.0 (`^1.0.40`) | Migrate libsql when their upstream requirements advance |
| [thiserror-impl](https://crates.io/api/v1/crates/thiserror-impl) | 1.0.69 | 2.0.21 | thiserror 1.0.69 (`=1.0.69`) | Migrate thiserror when their upstream requirements advance |
| [wasi](https://crates.io/api/v1/crates/wasi) | 0.11.1+wasi-snapshot-preview1 | 0.14.7+wasi-0.2.4 | mio 1.2.4 (`^0.11.0`) | Migrate mio when their upstream requirements advance |
| [which](https://crates.io/api/v1/crates/which) | 4.4.2 | 8.0.6 | bindgen 0.66.1 (`^4.2.1`) | Migrate bindgen when their upstream requirements advance |
| [windows-core](https://crates.io/api/v1/crates/windows-core) | 0.62.2 | 0.100.0 | iana-time-zone 0.1.65 (`>=0.56, <=0.62`) | Migrate iana-time-zone when their upstream requirements advance |
| [windows-implement](https://crates.io/api/v1/crates/windows-implement) | 0.60.2 | 0.100.0 | windows-core 0.62.2 (`^0.60.2`) | Migrate windows-core when their upstream requirements advance |
| [windows-interface](https://crates.io/api/v1/crates/windows-interface) | 0.59.3 | 0.100.0 | windows-core 0.62.2 (`^0.59.3`) | Migrate windows-core when their upstream requirements advance |
| [windows-link](https://crates.io/api/v1/crates/windows-link) | 0.2.1 | 0.100.0 | chrono 0.4.45 (`^0.2`); jiff 0.2.38 (`^0.2.1`); libloading 0.8.9 (`^0.2`); parking_lot_core 0.9.12 (`^0.2.0`); windows-core 0.62.2 (`^0.2.1`); windows-result 0.4.1 (`^0.2.1`); windows-strings 0.5.1 (`^0.2.1`); windows-sys 0.61.2 (`^0.2.1`) | Migrate chrono, jiff, libloading, parking_lot_core, windows-core, windows-result, windows-strings, windows-sys when their upstream requirements advance |
| [windows-result](https://crates.io/api/v1/crates/windows-result) | 0.4.1 | 0.100.0 | windows-core 0.62.2 (`^0.4.1`) | Migrate windows-core when their upstream requirements advance |
| [windows-strings](https://crates.io/api/v1/crates/windows-strings) | 0.5.1 | 0.100.0 | windows-core 0.62.2 (`^0.5.1`) | Migrate windows-core when their upstream requirements advance |
| [windows-sys](https://crates.io/api/v1/crates/windows-sys) | 0.59.0 | 0.61.2 | rustix 0.38.44 (`>=0.52, <=0.59`) | Migrate rustix when their upstream requirements advance |
| [windows-targets](https://crates.io/api/v1/crates/windows-targets) | 0.52.6 | 0.53.5 | windows-sys 0.59.0 (`^0.52.6`) | Migrate windows-sys when their upstream requirements advance |
| [windows_aarch64_gnullvm](https://crates.io/api/v1/crates/windows_aarch64_gnullvm) | 0.52.6 | 0.53.1 | windows-targets 0.52.6 (`^0.52.6`) | Migrate windows-targets when their upstream requirements advance |
| [windows_aarch64_msvc](https://crates.io/api/v1/crates/windows_aarch64_msvc) | 0.52.6 | 0.53.1 | windows-targets 0.52.6 (`^0.52.6`) | Migrate windows-targets when their upstream requirements advance |
| [windows_i686_gnu](https://crates.io/api/v1/crates/windows_i686_gnu) | 0.52.6 | 0.53.1 | windows-targets 0.52.6 (`^0.52.6`) | Migrate windows-targets when their upstream requirements advance |
| [windows_i686_gnullvm](https://crates.io/api/v1/crates/windows_i686_gnullvm) | 0.52.6 | 0.53.1 | windows-targets 0.52.6 (`^0.52.6`) | Migrate windows-targets when their upstream requirements advance |
| [windows_i686_msvc](https://crates.io/api/v1/crates/windows_i686_msvc) | 0.52.6 | 0.53.1 | windows-targets 0.52.6 (`^0.52.6`) | Migrate windows-targets when their upstream requirements advance |
| [windows_x86_64_gnu](https://crates.io/api/v1/crates/windows_x86_64_gnu) | 0.52.6 | 0.53.1 | windows-targets 0.52.6 (`^0.52.6`) | Migrate windows-targets when their upstream requirements advance |
| [windows_x86_64_gnullvm](https://crates.io/api/v1/crates/windows_x86_64_gnullvm) | 0.52.6 | 0.53.1 | windows-targets 0.52.6 (`^0.52.6`) | Migrate windows-targets when their upstream requirements advance |
| [windows_x86_64_msvc](https://crates.io/api/v1/crates/windows_x86_64_msvc) | 0.52.6 | 0.53.1 | windows-targets 0.52.6 (`^0.52.6`) | Migrate windows-targets when their upstream requirements advance |
| [zerocopy](https://crates.io/api/v1/crates/zerocopy) | 0.7.35 | 0.8.63 | libsql-sys 0.10.0 (`^0.7.28`) | Migrate libsql-sys when their upstream requirements advance |
| [zerocopy-derive](https://crates.io/api/v1/crates/zerocopy-derive) | 0.7.35 | 0.8.63 | zerocopy 0.7.35 (`=0.7.35`) | Migrate zerocopy when their upstream requirements advance |

## Complete registry resolution and declarations

Each row links to the primary registry source. All listed resolved versions are non-yanked; source is crates.io throughout. A different latest version is covered by the blocker table above.

| Registry crate | Resolved version(s) | Global latest stable | Resolved license expression(s) |
| --- | --- | --- | --- |
| [adler2](https://crates.io/api/v1/crates/adler2) | 2.0.1 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| [aes](https://crates.io/api/v1/crates/aes) | 0.9.3 | 0.9.3 | MIT OR Apache-2.0 |
| [aho-corasick](https://crates.io/api/v1/crates/aho-corasick) | 1.1.5 | 1.1.5 | Unlicense OR MIT |
| [android_system_properties](https://crates.io/api/v1/crates/android_system_properties) | 0.1.6 | 0.1.6 | MIT OR Apache-2.0 |
| [anstream](https://crates.io/api/v1/crates/anstream) | 1.0.0 | 1.0.0 | MIT OR Apache-2.0 |
| [anstyle](https://crates.io/api/v1/crates/anstyle) | 1.0.14 | 1.0.14 | MIT OR Apache-2.0 |
| [anstyle-parse](https://crates.io/api/v1/crates/anstyle-parse) | 1.0.0 | 1.0.0 | MIT OR Apache-2.0 |
| [anstyle-query](https://crates.io/api/v1/crates/anstyle-query) | 1.1.5 | 1.1.5 | MIT OR Apache-2.0 |
| [anstyle-wincon](https://crates.io/api/v1/crates/anstyle-wincon) | 3.0.11 | 3.0.11 | MIT OR Apache-2.0 |
| [anyhow](https://crates.io/api/v1/crates/anyhow) | 1.0.104 | 1.0.104 | MIT OR Apache-2.0 |
| [async-trait](https://crates.io/api/v1/crates/async-trait) | 0.1.92 | 0.1.92 | MIT OR Apache-2.0 |
| [autocfg](https://crates.io/api/v1/crates/autocfg) | 1.5.1 | 1.5.1 | Apache-2.0 OR MIT |
| [base16ct](https://crates.io/api/v1/crates/base16ct) | 1.0.0 | 1.0.0 | Apache-2.0 OR MIT |
| [base64](https://crates.io/api/v1/crates/base64) | 0.23.1 | 0.23.1 | MIT OR Apache-2.0 |
| [bindgen](https://crates.io/api/v1/crates/bindgen) | 0.66.1 | 0.73.2 | BSD-3-Clause |
| [bit-set](https://crates.io/api/v1/crates/bit-set) | 0.11.1 | 0.11.1 | Apache-2.0 OR MIT |
| [bit-vec](https://crates.io/api/v1/crates/bit-vec) | 0.10.1 | 0.10.1 | Apache-2.0 OR MIT |
| [bitflags](https://crates.io/api/v1/crates/bitflags) | 1.3.2, 2.13.2 | 2.13.2 | MIT OR Apache-2.0; MIT/Apache-2.0 |
| [block-buffer](https://crates.io/api/v1/crates/block-buffer) | 0.12.1 | 0.12.1 | MIT OR Apache-2.0 |
| [block2](https://crates.io/api/v1/crates/block2) | 0.6.2 | 0.6.2 | MIT |
| [borsh](https://crates.io/api/v1/crates/borsh) | 1.8.1 | 1.8.1 | MIT OR Apache-2.0 |
| [borsh-derive](https://crates.io/api/v1/crates/borsh-derive) | 1.8.1 | 1.8.1 | Apache-2.0 |
| [bumpalo](https://crates.io/api/v1/crates/bumpalo) | 3.20.3 | 3.20.3 | MIT OR Apache-2.0 |
| [bytemuck](https://crates.io/api/v1/crates/bytemuck) | 1.25.2 | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| [byteorder](https://crates.io/api/v1/crates/byteorder) | 1.5.0 | 1.5.0 | Unlicense OR MIT |
| [byteorder-lite](https://crates.io/api/v1/crates/byteorder-lite) | 0.1.0 | 0.1.0 | Unlicense OR MIT |
| [bytes](https://crates.io/api/v1/crates/bytes) | 1.12.1 | 1.12.1 | MIT |
| [cc](https://crates.io/api/v1/crates/cc) | 1.10.0 | 1.10.0 | MIT OR Apache-2.0 |
| [cexpr](https://crates.io/api/v1/crates/cexpr) | 0.6.0 | 0.6.0 | Apache-2.0/MIT |
| [cfg-if](https://crates.io/api/v1/crates/cfg-if) | 1.0.5 | 1.0.5 | MIT OR Apache-2.0 |
| [cfg_aliases](https://crates.io/api/v1/crates/cfg_aliases) | 0.2.2 | 0.2.2 | MIT |
| [chacha20](https://crates.io/api/v1/crates/chacha20) | 0.10.2 | 0.10.2 | MIT OR Apache-2.0 |
| [chrono](https://crates.io/api/v1/crates/chrono) | 0.4.45 | 0.4.45 | MIT OR Apache-2.0 |
| [cipher](https://crates.io/api/v1/crates/cipher) | 0.5.2 | 0.5.2 | MIT OR Apache-2.0 |
| [clang-sys](https://crates.io/api/v1/crates/clang-sys) | 1.9.1 | 1.9.1 | Apache-2.0 |
| [clap](https://crates.io/api/v1/crates/clap) | 4.6.7 | 4.6.7 | MIT OR Apache-2.0 |
| [clap_builder](https://crates.io/api/v1/crates/clap_builder) | 4.6.7 | 4.6.7 | MIT OR Apache-2.0 |
| [clap_derive](https://crates.io/api/v1/crates/clap_derive) | 4.6.7 | 4.6.7 | MIT OR Apache-2.0 |
| [clap_lex](https://crates.io/api/v1/crates/clap_lex) | 1.1.1 | 1.1.1 | MIT OR Apache-2.0 |
| [cmake](https://crates.io/api/v1/crates/cmake) | 0.1.58 | 0.1.58 | MIT OR Apache-2.0 |
| [cmov](https://crates.io/api/v1/crates/cmov) | 0.5.4 | 0.5.4 | Apache-2.0 OR MIT |
| [colorchoice](https://crates.io/api/v1/crates/colorchoice) | 1.0.5 | 1.0.5 | MIT OR Apache-2.0 |
| [console](https://crates.io/api/v1/crates/console) | 0.16.6 | 0.16.6 | MIT |
| [const-oid](https://crates.io/api/v1/crates/const-oid) | 0.10.2 | 0.10.2 | Apache-2.0 OR MIT |
| [core-foundation-sys](https://crates.io/api/v1/crates/core-foundation-sys) | 0.8.7 | 0.8.7 | MIT OR Apache-2.0 |
| [core_detect](https://crates.io/api/v1/crates/core_detect) | 1.0.0 | 1.0.0 | MIT/Apache-2.0 |
| [cpubits](https://crates.io/api/v1/crates/cpubits) | 0.1.1 | 0.1.1 | MIT OR Apache-2.0 |
| [cpufeatures](https://crates.io/api/v1/crates/cpufeatures) | 0.3.1 | 0.3.1 | MIT OR Apache-2.0 |
| [crc32fast](https://crates.io/api/v1/crates/crc32fast) | 1.5.3 | 1.5.3 | MIT OR Apache-2.0 |
| [crossbeam-channel](https://crates.io/api/v1/crates/crossbeam-channel) | 0.5.17 | 0.5.17 | MIT OR Apache-2.0 |
| [crossbeam-utils](https://crates.io/api/v1/crates/crossbeam-utils) | 0.8.23 | 0.8.23 | MIT OR Apache-2.0 |
| [crypto-bigint](https://crates.io/api/v1/crates/crypto-bigint) | 0.7.10 | 0.7.10 | Apache-2.0 OR MIT |
| [crypto-common](https://crates.io/api/v1/crates/crypto-common) | 0.2.2 | 0.2.2 | MIT OR Apache-2.0 |
| [crypto-primes](https://crates.io/api/v1/crates/crypto-primes) | 0.7.2 | 0.7.2 | Apache-2.0 OR MIT |
| [ctr](https://crates.io/api/v1/crates/ctr) | 0.10.1 | 0.10.1 | MIT OR Apache-2.0 |
| [ctutils](https://crates.io/api/v1/crates/ctutils) | 0.4.7 | 0.4.7 | Apache-2.0 OR MIT |
| [darling](https://crates.io/api/v1/crates/darling) | 0.24.1 | 0.24.1 | MIT |
| [darling_core](https://crates.io/api/v1/crates/darling_core) | 0.24.1 | 0.24.1 | MIT |
| [darling_macro](https://crates.io/api/v1/crates/darling_macro) | 0.24.1 | 0.24.1 | MIT |
| [defmt](https://crates.io/api/v1/crates/defmt) | 1.2.0 | 1.2.0 | MIT OR Apache-2.0 |
| [defmt-macros](https://crates.io/api/v1/crates/defmt-macros) | 1.2.0 | 1.2.0 | MIT OR Apache-2.0 |
| [defmt-parser](https://crates.io/api/v1/crates/defmt-parser) | 1.1.0 | 1.1.0 | MIT OR Apache-2.0 |
| [deranged](https://crates.io/api/v1/crates/deranged) | 0.5.9 | 0.5.9 | MIT OR Apache-2.0 |
| [dialoguer](https://crates.io/api/v1/crates/dialoguer) | 0.12.0 | 0.12.0 | MIT |
| [digest](https://crates.io/api/v1/crates/digest) | 0.11.3 | 0.11.3 | MIT OR Apache-2.0 |
| [directories](https://crates.io/api/v1/crates/directories) | 6.0.0 | 6.0.0 | MIT OR Apache-2.0 |
| [dirs-sys](https://crates.io/api/v1/crates/dirs-sys) | 0.5.0 | 0.5.0 | MIT OR Apache-2.0 |
| [dispatch2](https://crates.io/api/v1/crates/dispatch2) | 0.3.1 | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| [downcast](https://crates.io/api/v1/crates/downcast) | 0.11.0 | 0.11.0 | MIT |
| [dyn-clone](https://crates.io/api/v1/crates/dyn-clone) | 1.0.20 | 1.0.20 | MIT OR Apache-2.0 |
| [either](https://crates.io/api/v1/crates/either) | 1.19.0 | 1.19.0 | MIT OR Apache-2.0 |
| [encode_unicode](https://crates.io/api/v1/crates/encode_unicode) | 1.0.0 | 1.0.0 | Apache-2.0 OR MIT |
| [equivalent](https://crates.io/api/v1/crates/equivalent) | 1.0.3 | 1.0.3 | Apache-2.0 OR MIT |
| [errno](https://crates.io/api/v1/crates/errno) | 0.3.14 | 0.3.14 | MIT OR Apache-2.0 |
| [fastrand](https://crates.io/api/v1/crates/fastrand) | 2.5.0 | 2.5.0 | Apache-2.0 OR MIT |
| [fdeflate](https://crates.io/api/v1/crates/fdeflate) | 0.3.7 | 0.3.7 | MIT OR Apache-2.0 |
| [filetime](https://crates.io/api/v1/crates/filetime) | 0.2.29 | 0.2.29 | MIT/Apache-2.0 |
| [find-msvc-tools](https://crates.io/api/v1/crates/find-msvc-tools) | 0.1.15 | 0.1.15 | MIT OR Apache-2.0 |
| [flate2](https://crates.io/api/v1/crates/flate2) | 1.1.10 | 1.1.10 | MIT OR Apache-2.0 |
| [fnv](https://crates.io/api/v1/crates/fnv) | 1.0.7 | 1.0.7 | Apache-2.0 / MIT |
| [fragile](https://crates.io/api/v1/crates/fragile) | 2.1.0 | 3.0.0 | Apache-2.0 |
| [futures](https://crates.io/api/v1/crates/futures) | 0.3.34 | 0.3.34 | MIT OR Apache-2.0 |
| [futures-channel](https://crates.io/api/v1/crates/futures-channel) | 0.3.34 | 0.3.34 | MIT OR Apache-2.0 |
| [futures-core](https://crates.io/api/v1/crates/futures-core) | 0.3.34 | 0.3.34 | MIT OR Apache-2.0 |
| [futures-executor](https://crates.io/api/v1/crates/futures-executor) | 0.3.34 | 0.3.34 | MIT OR Apache-2.0 |
| [futures-io](https://crates.io/api/v1/crates/futures-io) | 0.3.34 | 0.3.34 | MIT OR Apache-2.0 |
| [futures-macro](https://crates.io/api/v1/crates/futures-macro) | 0.3.34 | 0.3.34 | MIT OR Apache-2.0 |
| [futures-sink](https://crates.io/api/v1/crates/futures-sink) | 0.3.34 | 0.3.34 | MIT OR Apache-2.0 |
| [futures-task](https://crates.io/api/v1/crates/futures-task) | 0.3.34 | 0.3.34 | MIT OR Apache-2.0 |
| [futures-util](https://crates.io/api/v1/crates/futures-util) | 0.3.34 | 0.3.34 | MIT OR Apache-2.0 |
| [getrandom](https://crates.io/api/v1/crates/getrandom) | 0.4.3 | 0.4.3 | MIT OR Apache-2.0 |
| [glass_pumpkin](https://crates.io/api/v1/crates/glass_pumpkin) | 2.0.0 | 2.0.0 | MIT OR Apache-2.0 |
| [glob](https://crates.io/api/v1/crates/glob) | 0.3.4 | 0.3.4 | MIT OR Apache-2.0 |
| [hashbrown](https://crates.io/api/v1/crates/hashbrown) | 0.17.1 | 0.17.1 | MIT OR Apache-2.0 |
| [heck](https://crates.io/api/v1/crates/heck) | 0.5.0 | 0.5.0 | MIT OR Apache-2.0 |
| [hmac](https://crates.io/api/v1/crates/hmac) | 0.13.0 | 0.13.0 | MIT OR Apache-2.0 |
| [home](https://crates.io/api/v1/crates/home) | 0.5.12 | 0.5.12 | MIT OR Apache-2.0 |
| [hybrid-array](https://crates.io/api/v1/crates/hybrid-array) | 0.4.15 | 0.4.15 | MIT OR Apache-2.0 |
| [iana-time-zone](https://crates.io/api/v1/crates/iana-time-zone) | 0.1.65 | 0.1.65 | MIT OR Apache-2.0 |
| [iana-time-zone-haiku](https://crates.io/api/v1/crates/iana-time-zone-haiku) | 0.1.2 | 0.1.2 | MIT OR Apache-2.0 |
| [ident_case](https://crates.io/api/v1/crates/ident_case) | 1.0.1 | 1.0.1 | MIT/Apache-2.0 |
| [image](https://crates.io/api/v1/crates/image) | 0.25.10 | 0.25.10 | MIT OR Apache-2.0 |
| [indexmap](https://crates.io/api/v1/crates/indexmap) | 2.14.2 | 2.14.2 | Apache-2.0 OR MIT |
| [inout](https://crates.io/api/v1/crates/inout) | 0.2.5 | 0.2.5 | MIT OR Apache-2.0 |
| [is_terminal_polyfill](https://crates.io/api/v1/crates/is_terminal_polyfill) | 1.70.2 | 1.70.2 | MIT OR Apache-2.0 |
| [itoa](https://crates.io/api/v1/crates/itoa) | 1.0.18 | 1.0.18 | MIT OR Apache-2.0 |
| [jiff](https://crates.io/api/v1/crates/jiff) | 0.2.38 | 0.2.38 | Unlicense OR MIT |
| [jiff-core](https://crates.io/api/v1/crates/jiff-core) | 0.1.1 | 0.1.1 | Unlicense OR MIT |
| [jiff-static](https://crates.io/api/v1/crates/jiff-static) | 0.2.38 | 0.2.38 | Unlicense OR MIT |
| [jiff-tzdb](https://crates.io/api/v1/crates/jiff-tzdb) | 0.1.9 | 0.1.9 | Unlicense OR MIT |
| [jiff-tzdb-platform](https://crates.io/api/v1/crates/jiff-tzdb-platform) | 0.1.3 | 0.1.3 | Unlicense OR MIT |
| [js-sys](https://crates.io/api/v1/crates/js-sys) | 0.3.107 | 0.3.107 | MIT OR Apache-2.0 |
| [lazy_static](https://crates.io/api/v1/crates/lazy_static) | 1.5.1 | 1.5.1 | MIT OR Apache-2.0 |
| [lazycell](https://crates.io/api/v1/crates/lazycell) | 1.3.0 | 1.3.0 | MIT/Apache-2.0 |
| [libc](https://crates.io/api/v1/crates/libc) | 0.2.191 | 0.2.191 | MIT OR Apache-2.0 |
| [libloading](https://crates.io/api/v1/crates/libloading) | 0.8.9 | 0.9.0 | ISC |
| [libredox](https://crates.io/api/v1/crates/libredox) | 0.1.25 | 0.2.0 | MIT |
| [libsql](https://crates.io/api/v1/crates/libsql) | 0.10.0 | 0.10.0 | MIT |
| [libsql-ffi](https://crates.io/api/v1/crates/libsql-ffi) | 0.10.0 | 0.10.0 | MIT |
| [libsql-sys](https://crates.io/api/v1/crates/libsql-sys) | 0.10.0 | 0.10.0 | MIT |
| [linux-raw-sys](https://crates.io/api/v1/crates/linux-raw-sys) | 0.4.15, 0.12.1 | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [locate-locale](https://crates.io/api/v1/crates/locate-locale) | 0.2.0 | 0.2.0 | MIT OR Apache-2.0 |
| [lock_api](https://crates.io/api/v1/crates/lock_api) | 0.4.14 | 0.5.0 | MIT OR Apache-2.0 |
| [log](https://crates.io/api/v1/crates/log) | 0.4.35 | 0.4.35 | MIT OR Apache-2.0 |
| [matchers](https://crates.io/api/v1/crates/matchers) | 0.2.0 | 0.2.0 | MIT |
| [md5](https://crates.io/api/v1/crates/md5) | 0.8.1 | 0.8.1 | Apache-2.0 OR MIT |
| [memchr](https://crates.io/api/v1/crates/memchr) | 2.8.3 | 2.8.3 | Unlicense OR MIT |
| [mime](https://crates.io/api/v1/crates/mime) | 0.3.17 | 0.3.17 | MIT OR Apache-2.0 |
| [mime_guess](https://crates.io/api/v1/crates/mime_guess) | 2.0.5 | 2.0.5 | MIT |
| [minimal-lexical](https://crates.io/api/v1/crates/minimal-lexical) | 0.2.1 | 0.2.1 | MIT/Apache-2.0 |
| [miniz_oxide](https://crates.io/api/v1/crates/miniz_oxide) | 0.8.9, 0.9.1 | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| [mio](https://crates.io/api/v1/crates/mio) | 1.2.4 | 1.2.4 | MIT |
| [mockall](https://crates.io/api/v1/crates/mockall) | 0.15.0 | 0.15.0 | MIT OR Apache-2.0 |
| [mockall_derive](https://crates.io/api/v1/crates/mockall_derive) | 0.15.0 | 0.15.0 | MIT OR Apache-2.0 |
| [moxcms](https://crates.io/api/v1/crates/moxcms) | 0.8.1 | 0.9.1 | BSD-3-Clause OR Apache-2.0 |
| [nix](https://crates.io/api/v1/crates/nix) | 0.31.3 | 0.31.3 | MIT |
| [nom](https://crates.io/api/v1/crates/nom) | 7.1.3 | 8.0.0 | MIT |
| [nu-ansi-term](https://crates.io/api/v1/crates/nu-ansi-term) | 0.50.3 | 0.50.3 | MIT |
| [num-bigint](https://crates.io/api/v1/crates/num-bigint) | 0.5.1 | 0.5.1 | MIT OR Apache-2.0 |
| [num-conv](https://crates.io/api/v1/crates/num-conv) | 0.2.2 | 0.2.2 | MIT OR Apache-2.0 |
| [num-integer](https://crates.io/api/v1/crates/num-integer) | 0.1.47 | 0.1.47 | MIT OR Apache-2.0 |
| [num-traits](https://crates.io/api/v1/crates/num-traits) | 0.2.19 | 0.2.19 | MIT OR Apache-2.0 |
| [objc2](https://crates.io/api/v1/crates/objc2) | 0.6.5 | 0.6.5 | MIT |
| [objc2-cloud-kit](https://crates.io/api/v1/crates/objc2-cloud-kit) | 0.3.2 | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-core-data](https://crates.io/api/v1/crates/objc2-core-data) | 0.3.2 | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-core-foundation](https://crates.io/api/v1/crates/objc2-core-foundation) | 0.3.2 | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-core-graphics](https://crates.io/api/v1/crates/objc2-core-graphics) | 0.3.2 | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-core-image](https://crates.io/api/v1/crates/objc2-core-image) | 0.3.2 | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-core-location](https://crates.io/api/v1/crates/objc2-core-location) | 0.3.2 | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-core-text](https://crates.io/api/v1/crates/objc2-core-text) | 0.3.2 | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-encode](https://crates.io/api/v1/crates/objc2-encode) | 4.1.0 | 4.1.0 | MIT |
| [objc2-foundation](https://crates.io/api/v1/crates/objc2-foundation) | 0.3.2 | 0.3.2 | MIT |
| [objc2-io-surface](https://crates.io/api/v1/crates/objc2-io-surface) | 0.3.2 | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-quartz-core](https://crates.io/api/v1/crates/objc2-quartz-core) | 0.3.2 | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-ui-kit](https://crates.io/api/v1/crates/objc2-ui-kit) | 0.3.2 | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-user-notifications](https://crates.io/api/v1/crates/objc2-user-notifications) | 0.3.2 | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [once_cell](https://crates.io/api/v1/crates/once_cell) | 1.21.4 | 1.21.4 | MIT OR Apache-2.0 |
| [once_cell_polyfill](https://crates.io/api/v1/crates/once_cell_polyfill) | 1.70.2 | 1.70.2 | MIT OR Apache-2.0 |
| [option-ext](https://crates.io/api/v1/crates/option-ext) | 0.2.0 | 0.2.0 | MPL-2.0 |
| [os_info](https://crates.io/api/v1/crates/os_info) | 3.15.0 | 3.15.0 | MIT |
| [parking_lot](https://crates.io/api/v1/crates/parking_lot) | 0.12.5 | 0.13.0 | MIT OR Apache-2.0 |
| [parking_lot_core](https://crates.io/api/v1/crates/parking_lot_core) | 0.9.12 | 0.10.0 | MIT OR Apache-2.0 |
| [pastey](https://crates.io/api/v1/crates/pastey) | 0.2.3 | 0.2.3 | MIT OR Apache-2.0 |
| [pbkdf2](https://crates.io/api/v1/crates/pbkdf2) | 0.13.0 | 0.13.0 | MIT OR Apache-2.0 |
| [peeking_take_while](https://crates.io/api/v1/crates/peeking_take_while) | 0.1.2 | 1.0.0 | Apache-2.0/MIT |
| [pin-project-lite](https://crates.io/api/v1/crates/pin-project-lite) | 0.2.17 | 0.2.17 | Apache-2.0 OR MIT |
| [png](https://crates.io/api/v1/crates/png) | 0.18.1 | 0.18.1 | MIT OR Apache-2.0 |
| [portable-atomic](https://crates.io/api/v1/crates/portable-atomic) | 1.15.0 | 1.15.0 | Apache-2.0 OR MIT |
| [portable-atomic-util](https://crates.io/api/v1/crates/portable-atomic-util) | 0.2.8 | 0.2.8 | Apache-2.0 OR MIT |
| [powerfmt](https://crates.io/api/v1/crates/powerfmt) | 0.2.1 | 0.2.1 | MIT OR Apache-2.0 |
| [predicates](https://crates.io/api/v1/crates/predicates) | 3.1.4 | 3.1.4 | MIT OR Apache-2.0 |
| [predicates-core](https://crates.io/api/v1/crates/predicates-core) | 1.0.10 | 1.0.10 | MIT OR Apache-2.0 |
| [predicates-tree](https://crates.io/api/v1/crates/predicates-tree) | 1.0.13 | 1.0.13 | MIT OR Apache-2.0 |
| [prettyplease](https://crates.io/api/v1/crates/prettyplease) | 0.2.37 | 0.3.0 | MIT OR Apache-2.0 |
| [proc-macro-crate](https://crates.io/api/v1/crates/proc-macro-crate) | 3.5.0 | 3.5.0 | MIT OR Apache-2.0 |
| [proc-macro2](https://crates.io/api/v1/crates/proc-macro2) | 1.0.107 | 1.0.107 | MIT OR Apache-2.0 |
| [proptest](https://crates.io/api/v1/crates/proptest) | 1.12.0 | 1.12.0 | MIT OR Apache-2.0 |
| [pxfm](https://crates.io/api/v1/crates/pxfm) | 0.1.30 | 0.1.30 | BSD-3-Clause OR Apache-2.0 |
| [quick-error](https://crates.io/api/v1/crates/quick-error) | 1.2.3 | 2.0.1 | MIT/Apache-2.0 |
| [quote](https://crates.io/api/v1/crates/quote) | 1.0.47 | 1.0.47 | MIT OR Apache-2.0 |
| [r-efi](https://crates.io/api/v1/crates/r-efi) | 6.0.0 | 7.1.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| [rand](https://crates.io/api/v1/crates/rand) | 0.10.3 | 0.10.3 | MIT OR Apache-2.0 |
| [rand_core](https://crates.io/api/v1/crates/rand_core) | 0.10.1 | 0.10.1 | MIT OR Apache-2.0 |
| [rand_xorshift](https://crates.io/api/v1/crates/rand_xorshift) | 0.5.0 | 0.5.0 | MIT OR Apache-2.0 |
| [redox_syscall](https://crates.io/api/v1/crates/redox_syscall) | 0.5.18 | 0.9.5 | MIT |
| [redox_users](https://crates.io/api/v1/crates/redox_users) | 0.5.3 | 0.5.3 | MIT |
| [ref-cast](https://crates.io/api/v1/crates/ref-cast) | 1.0.27 | 1.0.27 | MIT OR Apache-2.0 |
| [ref-cast-impl](https://crates.io/api/v1/crates/ref-cast-impl) | 1.0.27 | 1.0.27 | MIT OR Apache-2.0 |
| [regex](https://crates.io/api/v1/crates/regex) | 1.13.1 | 1.13.1 | MIT OR Apache-2.0 |
| [regex-automata](https://crates.io/api/v1/crates/regex-automata) | 0.4.18 | 0.4.18 | MIT OR Apache-2.0 |
| [regex-syntax](https://crates.io/api/v1/crates/regex-syntax) | 0.8.11 | 0.8.11 | MIT OR Apache-2.0 |
| [rmcp](https://crates.io/api/v1/crates/rmcp) | 3.8.0 | 3.8.0 | Apache-2.0 |
| [rmcp-macros](https://crates.io/api/v1/crates/rmcp-macros) | 3.8.0 | 3.8.0 | Apache-2.0 |
| [rustc-hash](https://crates.io/api/v1/crates/rustc-hash) | 1.1.0 | 2.1.3 | Apache-2.0/MIT |
| [rustix](https://crates.io/api/v1/crates/rustix) | 0.38.44, 1.1.6 | 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [rustversion](https://crates.io/api/v1/crates/rustversion) | 1.0.23 | 1.0.23 | MIT OR Apache-2.0 |
| [rusty-fork](https://crates.io/api/v1/crates/rusty-fork) | 0.3.1 | 0.3.1 | MIT/Apache-2.0 |
| [schemars](https://crates.io/api/v1/crates/schemars) | 1.2.3 | 1.2.3 | MIT |
| [schemars_derive](https://crates.io/api/v1/crates/schemars_derive) | 1.2.3 | 1.2.3 | MIT |
| [scopeguard](https://crates.io/api/v1/crates/scopeguard) | 1.2.0 | 1.2.0 | MIT OR Apache-2.0 |
| [secrecy](https://crates.io/api/v1/crates/secrecy) | 0.10.3 | 0.10.3 | Apache-2.0 OR MIT |
| [serde](https://crates.io/api/v1/crates/serde) | 1.0.229 | 1.0.229 | MIT OR Apache-2.0 |
| [serde_core](https://crates.io/api/v1/crates/serde_core) | 1.0.229 | 1.0.229 | MIT OR Apache-2.0 |
| [serde_derive](https://crates.io/api/v1/crates/serde_derive) | 1.0.229 | 1.0.229 | MIT OR Apache-2.0 |
| [serde_derive_internals](https://crates.io/api/v1/crates/serde_derive_internals) | 0.30.0 | 0.30.0 | MIT OR Apache-2.0 |
| [serde_json](https://crates.io/api/v1/crates/serde_json) | 1.0.154 | 1.0.154 | MIT OR Apache-2.0 |
| [serde_spanned](https://crates.io/api/v1/crates/serde_spanned) | 1.1.2 | 1.1.2 | MIT OR Apache-2.0 |
| [serdect](https://crates.io/api/v1/crates/serdect) | 0.4.3 | 0.4.3 | Apache-2.0 OR MIT |
| [sha1](https://crates.io/api/v1/crates/sha1) | 0.11.0 | 0.11.0 | MIT OR Apache-2.0 |
| [sha2](https://crates.io/api/v1/crates/sha2) | 0.11.1 | 0.11.1 | MIT OR Apache-2.0 |
| [sharded-slab](https://crates.io/api/v1/crates/sharded-slab) | 0.1.7 | 0.1.7 | MIT |
| [shell-words](https://crates.io/api/v1/crates/shell-words) | 1.1.1 | 1.1.1 | MIT/Apache-2.0 |
| [shlex](https://crates.io/api/v1/crates/shlex) | 1.3.0, 2.0.1 | 2.0.1 | MIT OR Apache-2.0 |
| [signal-hook-registry](https://crates.io/api/v1/crates/signal-hook-registry) | 1.4.8 | 1.4.8 | MIT OR Apache-2.0 |
| [simd-adler32](https://crates.io/api/v1/crates/simd-adler32) | 0.3.10 | 0.3.10 | MIT |
| [slab](https://crates.io/api/v1/crates/slab) | 0.4.12 | 0.4.12 | MIT |
| [smallvec](https://crates.io/api/v1/crates/smallvec) | 1.16.4 | 1.16.4 | MIT OR Apache-2.0 |
| [socket2](https://crates.io/api/v1/crates/socket2) | 0.6.5 | 0.6.5 | MIT OR Apache-2.0 |
| [strsim](https://crates.io/api/v1/crates/strsim) | 0.11.1 | 0.11.1 | MIT |
| [symlink](https://crates.io/api/v1/crates/symlink) | 0.1.0 | 0.1.0 | MIT/Apache-2.0 |
| [syn](https://crates.io/api/v1/crates/syn) | 2.0.119, 3.0.9 | 3.0.9 | MIT OR Apache-2.0 |
| [tempfile](https://crates.io/api/v1/crates/tempfile) | 3.27.0 | 3.27.0 | MIT OR Apache-2.0 |
| [termtree](https://crates.io/api/v1/crates/termtree) | 0.5.1 | 1.0.0 | MIT |
| [thiserror](https://crates.io/api/v1/crates/thiserror) | 1.0.69, 2.0.21 | 2.0.21 | MIT OR Apache-2.0 |
| [thiserror-impl](https://crates.io/api/v1/crates/thiserror-impl) | 1.0.69, 2.0.21 | 2.0.21 | MIT OR Apache-2.0 |
| [thread_local](https://crates.io/api/v1/crates/thread_local) | 1.1.10 | 1.1.10 | MIT OR Apache-2.0 |
| [time](https://crates.io/api/v1/crates/time) | 0.3.55 | 0.3.55 | MIT OR Apache-2.0 |
| [time-core](https://crates.io/api/v1/crates/time-core) | 0.1.9 | 0.1.9 | MIT OR Apache-2.0 |
| [time-macros](https://crates.io/api/v1/crates/time-macros) | 0.2.32 | 0.2.32 | MIT OR Apache-2.0 |
| [tokio](https://crates.io/api/v1/crates/tokio) | 1.54.1 | 1.54.1 | MIT |
| [tokio-macros](https://crates.io/api/v1/crates/tokio-macros) | 2.7.2 | 2.7.2 | MIT |
| [tokio-util](https://crates.io/api/v1/crates/tokio-util) | 0.7.20 | 0.7.20 | MIT |
| [toml](https://crates.io/api/v1/crates/toml) | 1.1.8+spec-1.1.0 | 1.1.8+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_datetime](https://crates.io/api/v1/crates/toml_datetime) | 1.1.2+spec-1.1.0 | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_edit](https://crates.io/api/v1/crates/toml_edit) | 0.25.17+spec-1.1.0 | 0.25.17+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_parser](https://crates.io/api/v1/crates/toml_parser) | 1.1.5+spec-1.1.0 | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_writer](https://crates.io/api/v1/crates/toml_writer) | 1.1.3+spec-1.1.0 | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 |
| [tracing](https://crates.io/api/v1/crates/tracing) | 0.1.44 | 0.1.44 | MIT |
| [tracing-appender](https://crates.io/api/v1/crates/tracing-appender) | 0.2.5 | 0.2.5 | MIT |
| [tracing-attributes](https://crates.io/api/v1/crates/tracing-attributes) | 0.1.31 | 0.1.31 | MIT |
| [tracing-core](https://crates.io/api/v1/crates/tracing-core) | 0.1.36 | 0.1.36 | MIT |
| [tracing-log](https://crates.io/api/v1/crates/tracing-log) | 0.2.0 | 0.2.0 | MIT |
| [tracing-serde](https://crates.io/api/v1/crates/tracing-serde) | 0.2.0 | 0.2.0 | MIT |
| [tracing-subscriber](https://crates.io/api/v1/crates/tracing-subscriber) | 0.3.23 | 0.3.23 | MIT |
| [typenum](https://crates.io/api/v1/crates/typenum) | 1.20.1 | 1.20.1 | MIT OR Apache-2.0 |
| [unarray](https://crates.io/api/v1/crates/unarray) | 0.1.4 | 0.1.4 | MIT OR Apache-2.0 |
| [unicase](https://crates.io/api/v1/crates/unicase) | 2.10.0 | 2.10.0 | MIT OR Apache-2.0 |
| [unicode-ident](https://crates.io/api/v1/crates/unicode-ident) | 1.0.27 | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| [unicode-width](https://crates.io/api/v1/crates/unicode-width) | 0.2.3 | 0.2.3 | MIT OR Apache-2.0 |
| [utf8parse](https://crates.io/api/v1/crates/utf8parse) | 0.2.2 | 0.2.2 | Apache-2.0 OR MIT |
| [uuid](https://crates.io/api/v1/crates/uuid) | 1.29.0 | 1.29.0 | Apache-2.0 OR MIT |
| [valuable](https://crates.io/api/v1/crates/valuable) | 0.1.1 | 0.1.1 | MIT |
| [wait-timeout](https://crates.io/api/v1/crates/wait-timeout) | 0.2.1 | 0.2.1 | MIT/Apache-2.0 |
| [wasi](https://crates.io/api/v1/crates/wasi) | 0.11.1+wasi-snapshot-preview1 | 0.14.7+wasi-0.2.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [wasm-bindgen](https://crates.io/api/v1/crates/wasm-bindgen) | 0.2.130 | 0.2.130 | MIT OR Apache-2.0 |
| [wasm-bindgen-macro](https://crates.io/api/v1/crates/wasm-bindgen-macro) | 0.2.130 | 0.2.130 | MIT OR Apache-2.0 |
| [wasm-bindgen-macro-support](https://crates.io/api/v1/crates/wasm-bindgen-macro-support) | 0.2.130 | 0.2.130 | MIT OR Apache-2.0 |
| [wasm-bindgen-shared](https://crates.io/api/v1/crates/wasm-bindgen-shared) | 0.2.130 | 0.2.130 | MIT OR Apache-2.0 |
| [which](https://crates.io/api/v1/crates/which) | 4.4.2 | 8.0.6 | MIT |
| [winapi](https://crates.io/api/v1/crates/winapi) | 0.3.9 | 0.3.9 | MIT/Apache-2.0 |
| [winapi-i686-pc-windows-gnu](https://crates.io/api/v1/crates/winapi-i686-pc-windows-gnu) | 0.4.0 | 0.4.0 | MIT/Apache-2.0 |
| [winapi-x86_64-pc-windows-gnu](https://crates.io/api/v1/crates/winapi-x86_64-pc-windows-gnu) | 0.4.0 | 0.4.0 | MIT/Apache-2.0 |
| [windows-core](https://crates.io/api/v1/crates/windows-core) | 0.62.2 | 0.100.0 | MIT OR Apache-2.0 |
| [windows-implement](https://crates.io/api/v1/crates/windows-implement) | 0.60.2 | 0.100.0 | MIT OR Apache-2.0 |
| [windows-interface](https://crates.io/api/v1/crates/windows-interface) | 0.59.3 | 0.100.0 | MIT OR Apache-2.0 |
| [windows-link](https://crates.io/api/v1/crates/windows-link) | 0.2.1 | 0.100.0 | MIT OR Apache-2.0 |
| [windows-result](https://crates.io/api/v1/crates/windows-result) | 0.4.1 | 0.100.0 | MIT OR Apache-2.0 |
| [windows-strings](https://crates.io/api/v1/crates/windows-strings) | 0.5.1 | 0.100.0 | MIT OR Apache-2.0 |
| [windows-sys](https://crates.io/api/v1/crates/windows-sys) | 0.59.0, 0.61.2 | 0.61.2 | MIT OR Apache-2.0 |
| [windows-targets](https://crates.io/api/v1/crates/windows-targets) | 0.52.6 | 0.53.5 | MIT OR Apache-2.0 |
| [windows_aarch64_gnullvm](https://crates.io/api/v1/crates/windows_aarch64_gnullvm) | 0.52.6 | 0.53.1 | MIT OR Apache-2.0 |
| [windows_aarch64_msvc](https://crates.io/api/v1/crates/windows_aarch64_msvc) | 0.52.6 | 0.53.1 | MIT OR Apache-2.0 |
| [windows_i686_gnu](https://crates.io/api/v1/crates/windows_i686_gnu) | 0.52.6 | 0.53.1 | MIT OR Apache-2.0 |
| [windows_i686_gnullvm](https://crates.io/api/v1/crates/windows_i686_gnullvm) | 0.52.6 | 0.53.1 | MIT OR Apache-2.0 |
| [windows_i686_msvc](https://crates.io/api/v1/crates/windows_i686_msvc) | 0.52.6 | 0.53.1 | MIT OR Apache-2.0 |
| [windows_x86_64_gnu](https://crates.io/api/v1/crates/windows_x86_64_gnu) | 0.52.6 | 0.53.1 | MIT OR Apache-2.0 |
| [windows_x86_64_gnullvm](https://crates.io/api/v1/crates/windows_x86_64_gnullvm) | 0.52.6 | 0.53.1 | MIT OR Apache-2.0 |
| [windows_x86_64_msvc](https://crates.io/api/v1/crates/windows_x86_64_msvc) | 0.52.6 | 0.53.1 | MIT OR Apache-2.0 |
| [winnow](https://crates.io/api/v1/crates/winnow) | 1.0.4 | 1.0.4 | MIT |
| [zerocopy](https://crates.io/api/v1/crates/zerocopy) | 0.7.35 | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| [zerocopy-derive](https://crates.io/api/v1/crates/zerocopy-derive) | 0.7.35 | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| [zeroize](https://crates.io/api/v1/crates/zeroize) | 1.9.1 | 1.9.1 | Apache-2.0 OR MIT |
| [zlib-rs](https://crates.io/api/v1/crates/zlib-rs) | 0.6.8 | 0.6.8 | Zlib |
| [zmij](https://crates.io/api/v1/crates/zmij) | 1.0.23 | 1.0.23 | MIT |
| [zune-core](https://crates.io/api/v1/crates/zune-core) | 0.5.3 | 0.5.3 | MIT OR Apache-2.0 OR Zlib |
| [zune-jpeg](https://crates.io/api/v1/crates/zune-jpeg) | 0.5.15 | 0.5.15 | MIT OR Apache-2.0 OR Zlib |

## Verification scope

On aarch64 macOS with rustc 1.101.0-nightly (92b5b8c87 2026-10-29), selected by nightly-2026-10-31: formatting, all-target Clippy with warnings denied, all 848 library tests (5 pre-existing ignored), binary tests, release build, and fresh advisory/source/yank checks passed. The modern/legacy wire regressions ran in the full suite; real offline process lifecycle and the separate probe client also passed. No real Telegram session or credentials were accessed.

Linux/macOS CI matrix remains configured, but this local migration did not execute Linux or Windows tests; only the aarch64-apple-darwin target is installed and no Linux container runtime is available in this environment. Hosted Linux verification remains a platform acceptance limitation.
