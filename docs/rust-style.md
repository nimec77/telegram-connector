# Rust coding manual for Telegram MCP Connector

Version: proposed 2026-10-31. This manual describes the target standard and adoption
exceptions; it does not claim the existing code already complies. The accompanying
[alignment plan](superpowers/plans/2026-10-31-rust-standards-alignment.md) schedules adoption.
Until that migration updates the project instructions, existing AGENTS.md and conventions
remain authoritative. This is a Cargo/Tokio application with an exported library surface.

## External guide and applicability

[Google's Rust guide](https://google.github.io/styleguide/rustguide.html) prioritizes
maintainability and applies stronger API requirements to reusable libraries. Its
Chromium/Fuchsia-specific build, async, threading and test rules are not this project's
platform policy. This manual combines selected Google conventions, the Rust API
Guidelines, and the repository's operational invariants; it is not a reproduction.

Google-specific adoption map:

| Topic | Project decision |
| --- | --- |
| Imports | Explicit production imports; module-grouped imports; qualify external free functions. |
| Trait scope | Local imports for locally used traits; shared imports when used throughout a file. |
| Modules | Keep implementation focused; document necessary parent-module glue. |
| Formatting | Adopt module import grouping, lowercase hex and macro-matcher formatting. |
| Poison | Never resume using potentially inconsistent poisoned state. |
| Constructors | Prefer `try_new` for new fallible APIs; migrate existing names compatibly. |
| Unstable language features | No new feature gates without a documented need; nightly tooling remains. |
| Specialized platforms | No Chromium crate flattening, async prohibition, special imports or Fuchsia mutex/test framework. |

## Rule levels and exceptions

**Required** rules protect correctness or establish an agreed format. **Preferred** rules
allow a documented local alternative when it makes this application simpler. An exception
records its scope, reason, and test/review evidence; it is not a crate-wide lint suppression.
Generated dependency code is outside this manual. Project-owned generated schemas remain
subject to contract tests. A migration task must distinguish Rust API compatibility from
MCP wire compatibility: one can change without the other.

## 1. Formatting and imports

Required: format with the selected toolchain's `cargo fmt --all`; CI checks formatting.
Four-space indentation and a 100-column target are formatter decisions, not manual
alignment exercises. Proposed `rustfmt.toml`:

```toml
edition = "2024"
style_edition = "2024"
max_width = 100
imports_granularity = "Module"
hex_literal_case = "Lower"
format_macro_matchers = true
```

Some options require nightly rustfmt; verify support in the pinned toolchain. Escape
invisible characters in ordinary source literals. A format skip needs a local reason,
such as a readable fixture table. Keep formatting-only commits separate from behavior.
[Rust formatting baseline](https://doc.rust-lang.org/style-guide/).

Use explicit names in production. Test-local `super::*` and deliberate facade reexports
are permitted. Example project style: `use crate::mcp::tools::helpers;` followed by
`helpers::parse_message_id(raw)` makes the operation's origin visible. Avoid importing
every helper through a broad parent prelude. Import commonly used type names directly;
retain qualified names such as `tl::enums::Peer` where the generated type family is clearer.

## 2. File structure and visibility

Required: keep file-as-module organization, no `mod.rs`, thin MCP entry points, operation
bodies in `server/impl_*.rs`, and Telegram RPC bodies in `client/ops_*.rs`.
Prefer private or `pub(crate)` implementation items. Public APIs must be intentional.
Facade modules should explain their purpose and expose a selected surface.

The entire `#[tool_router]` impl must remain visible to rmcp's attribute macro. Do not
hide tool methods inside a declarative macro or split that impl to satisfy a style count.
The existing server's coordination role is a documented structural exception. Named
responsibilities, rather than arbitrary line limits, determine module splits.

Before reducing visibility, list external paths used by `src/main.rs` (a separate crate)
and any supported Rust consumers. Preserve needed reexports; schedule public removals as
API migrations. Do not add `#[non_exhaustive]` to every DTO: use it for genuinely evolving
external Rust APIs after providing construction methods, and retain exhaustive internal
enums and explicit known upstream variants. Passive serialized DTOs may have public fields.
[API evolution guidance](https://rust-lang.github.io/api-guidelines/future-proofing.html).

## 3. Names, constructors and conversions

Types/traits use UpperCamelCase; modules/functions use snake_case; constants use
SCREAMING_SNAKE_CASE. Names describe domain behavior: `ChannelId`, `MessageWalk`, and
`retry_after_seconds` carry more information than generic identifiers. Distinguish a cheap
borrow (`as_*`), a conversion that may allocate (`to_*`), and consuming conversion (`into_*`).
Avoid mechanical renames of established MCP tools or serialized fields for Rust aesthetics.
[Naming conventions](https://rust-lang.github.io/api-guidelines/naming.html).

Required: conversions that can reject input use `TryFrom`/`FromStr` or a fallible named
constructor; infallible value conversion uses `From`. Derive traits when their semantics
match the type. `Default` must represent a valid useful state, not a fabricated positive
ID or a placeholder credential. Do not add `Copy`/`Clone` to resources merely because a
lint suggests it. Preserve error causes where safe; errors containing strings cannot be
made `Copy` without changing their meaning.
[Standard trait guidance](https://rust-lang.github.io/api-guidelines/interoperability.html).

## 4. Parameters, ownership and domain types

Required: keep distinct `ChannelId`, `MessageId`, `UserId`, and name types. Validate at
construction and at external deserialization boundaries; no conversion may bypass the
invariant. Keep flexible scalar coercion in MCP request parsing, not in the domain.
Use named option structs or enums when positional flags are ambiguous. JSON booleans
with explicit field names need not become JSON enums merely for internal Rust style.
[Type safety](https://rust-lang.github.io/api-guidelines/type-safety.html).

Prefer `&str`, `&[T]`, `&Path` and `&T` for reading. Take owned values when retaining or
consuming them; avoid accepting a borrow merely to clone it immediately. A deep clone of
message text/image bytes needs a lifetime or ownership reason. Owned `Vec` outputs are
appropriate for bounded Telegram batches and serialized responses; do not introduce
generic iterators that every caller immediately collects.
[Ownership-oriented API design](https://rust-lang.github.io/api-guidelines/flexibility.html).

Keep `Arc<T>` for shared server/client/limiter state and asynchronous task lifetimes.
This is an intentional architecture choice; use `Arc::clone` to make pointer sharing
visible. Do not replace it with complex borrowing or runtime dispatch for appearance.
Retain the client/limiter traits as real test seams. Add a new trait only for a concrete
substitution or shared contract, not to split an impl cosmetically.

## 5. Errors, panics and secrets

Required: typed `thiserror` errors in the library; `anyhow` context at application edges.
Preserve distinctions such as timeout, unauthenticated, unavailable and not found.
`Option` means absence/unknown, not a discarded transport failure. Production `unwrap`
is prohibited; `expect` requires a provable invariant. OS signal registration and network
calls are fallible external operations, not invariants.

Keep error variants intact until the MCP boundary, where the JSON contract maps them to
stable codes and `isError`. Protocol faults use protocol errors. Never attach a parser
source excerpt, credential, phone, session token or raw secret-bearing error chain to logs
or model-visible errors. Use `SecretString` for sensitive configuration. Stdout belongs
exclusively to MCP; operational diagnostics go to tracing/stderr.

An explicit degraded result may omit an unavailable field, but it must retain the
documented partial/error meaning. No `unwrap_or_default` or `filter_map(Result::ok)`
may erase a failure without a reviewed domain rationale. Tests may use `expect` with
meaningful fixture context; no mass rewrite of harmless existing test assertions.

## 6. Concurrency and bounded work

Required: local validation precedes token acquisition, which precedes every tool RPC.
An operation's timeout covers its complete traversal, not each page independently.
Limit fan-out, downloaded bytes, decoded allocation, encoded output and in-flight work
separately. Keep CPU-heavy image processing off async executor threads. A blocking task
that has begun cannot be assumed cancelled just because its awaiting future is dropped.

Mutex guards must not cross `.await` unless an async lock and invariant analysis require
it. Keep synchronous critical sections short. Select lock types by access pattern, not
by a blanket substitution to another mutex crate. Poison indicates an interrupted critical
section; Rust permits inspecting its state, but doing so is not this project's default
recovery policy. [Mutex semantics](https://doc.rust-lang.org/std/sync/struct.Mutex.html).

Target failure policy: a poisoned admission-control bucket refuses new RPCs with a typed
internal error; a poisoned metrics/replay store becomes unavailable while transport still
works. Do not reset the limiter to a full bucket, inspect poisoned contents, or falsely
report failed replay as a successfully empty history. Tests need an explicit policy too.
This replaces current poison recovery only during the scheduled behavior-tested migration.

## 7. Unsafe code, toolchains and macros

Required: keep product code safe. Any unavoidable unsafe block needs a local `SAFETY:`
argument identifying all callers, lifetimes and synchronization obligations. Performance
justifications need measurements. Current environment-mutating tests are a migration item:
a mutex serializing cooperating tests does not prove every library/thread avoids environment
reads. Prefer subprocess tests configured through `Command::env` or pure expansion helpers.
Do not add more mutation outside the existing EnvGuard while it is still the project policy.

Nightly selection is independent from using unstable language features. Preserve the
required toolchain; do not introduce `#![feature]` simply because nightly is available.
Use ordinary functions for new application logic. Existing serde/schemars/thiserror/rmcp
and mockall macros are justified integrations; no macro-driven mini-language for handlers.
Do not access third-party hidden/private API as a shortcut around the grammers seam.

## 8. Documentation and return values

Required: document retained public APIs and the crate's purpose. Explain failure cases,
timeouts, rate charges, pagination exclusivity, side effects, and partial results where
relevant. Use `///` for an item's contract and `//!` for a module; implementation rationale
belongs in normal comments. Add executable examples for the main supported workflows.
Use `# Errors`, `# Panics` and `# Safety` only when applicable; avoid comments that restate
the identifier. [Documentation guidance](https://rust-lang.github.io/api-guidelines/documentation.html).

Mark consuming builders whose return must be used, such as `McpServer::with_observability`,
with `#[must_use]`. Examine other pure functions individually. `Result` already warns
when discarded; duplicate annotations with no useful message add little. `len` APIs keep
their `is_empty` companion. Public `const fn` and exhaustive shapes are compatibility
commitments, not automatic performance improvements.

## 9. Tests, checks and review

Required: a failing observable behavior test before production behavior changes. Keep
mock-based offline tests, pure pagination/conversion tests, paused Tokio time for deadlines,
and property tests for invariants. Test real production decisions beneath a mocked network
boundary; a successful mock expectation alone proves little. No sleep-based property tests.

Run the existing gate after code changes:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Use `--offline` when dependencies are cached. The adoption plan adds focused lint and
rustdoc enforcement gradually. Do not enable all pedantic/restriction lints, apply all
automatic fixes, or add source-text tests that merely count annotations.

Review each change for observable behavior, error meaning, ownership, lock scope, public
surface, wire compatibility, tests and bounds. Keep style-only diffs separate from security
fixes. Document legitimate exceptions at the narrowest scope with a reason. Update this
manual when an exception becomes a durable architectural decision.
